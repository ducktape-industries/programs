//! The loop consensus runs for a thread: read the agent's configuration and its model, pick the
//! account to ask (`routes.rs`), learn the window, summarize when the next answer would not fit,
//! ask for a streamed reply, build the turn, run its calls, and ask again once they are back.
use ::program::{
    BlobId, Ctx, Error, ExecCtx, HashKind, ProgramId, QueryCtx, Range, all, unexpected_reply,
    wrong_state,
};
use abi::role::egress::{self, http};

use crate::agents::{Agent, configured_pinned, manual, running, thread};
use crate::calls::{Placed, place, refusal, run_call, take_back_showing};
use crate::conversations::{binding, retire};
use crate::listening::seed;
use crate::pictures::{self, Looks, Picture};
use crate::providers::{self, Reach};
use crate::resume::{self, Failure};
use crate::routes::{self, Picked};
use crate::state::{
    ANSWERED, ASKED, ENDING, FLIGHTS, FRAMES, HELD, HELD_AT, LANES, LIVE, Lane, Live, PARTIALS,
    PASSES, PICKS, RETRIES, SEALED, SEALED_FOR, SHOWN, THREADS, WITHHELD,
};
use crate::tallies;
use crate::waiting::settling;
use crate::wire::{self, Answered, Foreign, Header, Lanes, Summarized, Summary};
use crate::{
    Asking, Body, Configured, Context, Forming, Frame, PART, Partial, Role, Said, Sealed, Stop,
    ThreadRow, Window, Wire, streamed_as,
};

struct Asked {
    wire: Wire,
    provider: String,
    service: String,
    account: Option<String>,
    thread: u64,
    wait: Option<u64>,
    config: BlobId,
}

impl Asked {
    fn target(&self) -> model_wire::Target {
        match &self.account {
            Some(account) => model_wire::Target::Pinned {
                service: self.service.clone(),
                account: account.clone(),
            },
            None => model_wire::Target::Unpinned,
        }
    }
}

struct Turn {
    asked: Asked,
    lane: Lane,
    model: String,
    max_tokens: u32,
    header: BlobId,
}

pub(crate) fn append(ctx: &ExecCtx, thread: &mut ThreadRow, body: Body) {
    thread.frames += 1;
    let seq = thread.frames;
    if body.role() == Some(Role::User) {
        thread.last_input = seq;
    }
    let frame = Frame {
        seq,
        height: ctx.env().height,
        time: ctx.env().time,
        body,
    };
    thread.last_height = frame.height;
    thread.last_time = frame.time;
    FRAMES.put(ctx, &(thread.id, seq), &frame);
}

pub(crate) fn say(ctx: &ExecCtx, thread: &mut ThreadRow, body: Body) {
    say_with(ctx, thread, body, Vec::new());
}

pub(crate) fn say_with(ctx: &ExecCtx, thread: &mut ThreadRow, body: Body, pictures: Vec<Picture>) {
    say_from(ctx, thread, body, pictures, None);
}

pub(crate) fn say_from(
    ctx: &ExecCtx,
    thread: &mut ThreadRow,
    body: Body,
    pictures: Vec<Picture>,
    place: Option<String>,
) {
    let looping = thread.asking != Asking::Idle || thread.paused();
    if !looping {
        append(ctx, thread, body);
        pictures::keep(ctx, thread.id, thread.frames, pictures);
        return;
    }
    pictures::hold(ctx, thread.id, thread.held, pictures);
    HELD.put(ctx, &(thread.id, thread.held), &body);
    if let Some(place) = place {
        HELD_AT.put(ctx, &(thread.id, thread.held), &place);
    }
    thread.held += 1;
}

pub(crate) fn settle(ctx: &ExecCtx, thread: &mut ThreadRow) -> Result<(), Error> {
    let armed_a_wait = ENDING.has(ctx, &thread.id);
    if armed_a_wait && thread.asking == Asking::Running {
        thread.seen = thread.frames;
    }
    ENDING.remove(ctx, &thread.id);
    // input held behind a paused turn would sit between a server tool's use and its result, which
    // the continuation carries: it waits until the continuation is answered
    if !thread.paused() {
        unhold(ctx, thread)?;
    }
    thread.asking = Asking::Idle;
    FLIGHTS.remove(ctx, &thread.id);
    if thread.closed {
        let_go(ctx, thread)?;
    }
    Ok(())
}

fn unhold(ctx: &ExecCtx, thread: &mut ThreadRow) -> Result<(), Error> {
    for index in 0..thread.held {
        let key = (thread.id, index);
        if let Some(body) = HELD.get(ctx, &key)? {
            append(ctx, thread, body);
            pictures::unhold(ctx, thread.id, index, thread.frames)?;
        }
        HELD.remove(ctx, &key);
        HELD_AT.remove(ctx, &key);
    }
    thread.held = 0;
    Ok(())
}

pub(crate) fn give_up(ctx: &ExecCtx, thread: &mut ThreadRow, text: String) -> Result<(), Error> {
    if thread.paused() {
        unpause(ctx, thread)?;
    }
    append(ctx, thread, Body::Notice { text: text.clone() });
    crate::failures::failed(ctx, thread, text)?;
    thread.seen = thread.seen.max(thread.last_input);
    thread.last_stop = None;
    RETRIES.remove(ctx, &thread.id);
    routes::untried(ctx, thread.id);
    Ok(())
}

fn unpause(ctx: &ExecCtx, thread: &mut ThreadRow) -> Result<(), Error> {
    let mut paused = None;
    for seq in (thread.base..=thread.frames).rev() {
        if let Some(frame) = FRAMES.get(ctx, &(thread.id, seq))?
            && matches!(frame.body, Body::Turn { .. })
        {
            paused = Some(frame);
            break;
        }
    }
    let Some(mut frame) = paused else {
        return Ok(());
    };
    let Body::Turn { said, .. } = &mut frame.body else {
        return Ok(());
    };
    let answered: std::collections::BTreeSet<String> = said
        .iter()
        .filter_map(|said| {
            let Said::ServerToolResult(block) = said else {
                return None;
            };
            wire::answers(block)
        })
        .collect();
    said.retain(|said| !matches!(said, Said::ServerToolUse { id, .. } if !answered.contains(id)));
    let spoken = said
        .iter()
        .any(|said| !matches!(said, Said::Thinking { .. } | Said::RedactedThinking(_)));
    if !spoken {
        let text = "the model paused its turn mid server tool and the turn was given up before the tool answered, so it is left out".into();
        frame.body = Body::Notice { text };
    }
    if !spoken || frame.seq < thread.tail_from {
        unseal(ctx, thread)?;
    }
    FRAMES.put(ctx, &(thread.id, frame.seq), &frame);
    Ok(())
}

pub(crate) fn bound(ctx: &QueryCtx) -> Result<ProgramId, Error> {
    let Some(program) = ctx.env().roles.get(abi::role::EGRESS).cloned() else {
        return Err(wrong_state(
            "this network binds no program to the egress role",
        ));
    };
    Ok(program)
}

pub(crate) fn serves(ctx: &QueryCtx, service: &str) -> Result<bool, Error> {
    let program = bound(ctx)?;
    let asked = egress::Query::Serving {
        service: service.into(),
    };
    match ctx.query(program.clone(), &asked)? {
        egress::Reply::Serving(keys) => Ok(!keys.is_empty()),
        other => Err(unexpected_reply(&program, "Serving", &other)),
    }
}

pub(crate) async fn request(
    ctx: &Ctx,
    request: egress::Request,
    delivery: egress::Delivery,
    wait: Option<u64>,
) -> Result<Vec<u8>, Error> {
    let op = egress::Op::Request {
        request,
        delivery,
        wait,
    };
    ctx.call_raw(bound(ctx)?, abi::encode(&op)).await
}

async fn call_model(
    ctx: &Ctx,
    asked: &Asked,
    head: http::Head,
    parts: Vec<BlobId>,
    delivery: egress::Delivery,
) -> Result<http::Response, Failure> {
    let prompted = !parts.is_empty();
    let request_ = egress::Request::Http {
        service: asked.service.clone(),
        head,
        body: parts,
    };
    let answer = request(ctx, request_, delivery, asked.wait).await;
    let bytes = answer.map_err(|error| Failure::unreached(&error))?;
    let response: http::Response = abi::decode(&bytes)
        .map_err(|_| Failure::last("the egress answer is not an HTTP response"))?;
    crate::budget::seen(ctx, &asked.service, &asked.provider, &response);
    routes::heard(ctx, asked.thread, &response);
    let answered_a_prompt = prompted && response.status == 200;
    if answered_a_prompt {
        resume::answered_on(ctx, &asked.service, asked.thread).await;
    }
    Ok(response)
}

fn put_constant(
    ctx: &ExecCtx,
    name: &[u8],
    wire: Wire,
    target: &model_wire::Target,
    text: String,
) -> Result<BlobId, Error> {
    let aimed = match target {
        model_wire::Target::Unnamed => vec![0],
        model_wire::Target::Unpinned => vec![1],
        model_wire::Target::Pinned { service, account } => {
            [vec![2], abi::encode(&(service, account))].concat()
        }
    };
    let line = [
        b"const/".as_slice(),
        name,
        b"/",
        &abi::encode(&wire),
        &aimed,
    ]
    .concat();
    ctx.blob_put_on(HashKind::Sha256, PART, text.into_bytes(), line)
}

fn put_on(ctx: &ExecCtx, thread: u64, line: &[u8], text: String) -> Result<BlobId, Error> {
    ctx.blob_put_on(
        HashKind::Sha256,
        PART,
        text.into_bytes(),
        held_on(thread, line),
    )
}

pub(crate) fn held_on(thread: u64, line: &[u8]) -> Vec<u8> {
    [thread.to_be_bytes().as_slice(), b"/", line].concat()
}

fn grouped(frames: impl IntoIterator<Item = Frame>) -> Vec<(Role, Vec<Frame>)> {
    let mut groups: Vec<(Role, Vec<Frame>)> = Vec::new();
    for frame in frames {
        let Some(role) = frame.body.role() else {
            continue;
        };
        match groups.last_mut() {
            Some((last, frames)) if *last == role => frames.push(frame),
            _ => groups.push((role, vec![frame])),
        }
    }
    groups
}

fn summary_part(ctx: &ExecCtx, thread: &ThreadRow, wire: Wire) -> Result<Option<BlobId>, Error> {
    let Some(seq) = thread.summary else {
        return Ok(None);
    };
    let Some(Frame {
        body: Body::Compaction {
            wire: written,
            block,
        },
        ..
    }) = FRAMES.get(ctx, &(thread.id, seq))?
    else {
        let key = FRAMES.key(&(thread.id, seq));
        return Err(::program::corrupt(
            "frame",
            &key,
            "is not the thread's summary",
        ));
    };
    let text = wire::summary_words(written, &block);
    let summary = wire::summary(wire, written, &block, &text);
    Ok(Some(put_on(ctx, thread.id, b"summary", summary)?))
}

fn lanes(ctx: &ExecCtx, thread: u64, wire: Wire, frames: &[Frame]) -> Result<Lanes, Error> {
    let mut lanes = Lanes::new();
    if wire != Wire::Harness {
        return Ok(lanes);
    }
    for frame in frames
        .iter()
        .filter(|frame| frame.body.role() == Some(Role::Assistant))
    {
        let Some(lane) = LANES.get(ctx, &(thread, frame.seq))? else {
            continue;
        };
        let answerer = ANSWERED.get(ctx, &(thread, frame.seq))?;
        let (service, account) = match answerer {
            Some(by) => (Some(by.service), by.account),
            None => (None, None),
        };
        let lane = model_wire::Lane {
            provider: lane.provider,
            model: lane.model,
            service,
            account,
        };
        lanes.insert(frame.seq, lane);
    }
    Ok(lanes)
}

fn opens(thread: &ThreadRow) -> bool {
    thread.sealed == 0 && thread.summary.is_none()
}

fn left_out(seq: u64, body: &Body) -> String {
    let text = body.text();
    format!(
        "left out of your context: frame {seq} is {} bytes in {} lines, too large for the model to take whole. Find lines in it with thread_search (frame {seq}) and read parts of it with thread_read (frame {seq}, an offset and a length).",
        text.len(),
        text.split('\n').count()
    )
}

fn withheld(seq: u64, body: Body) -> Body {
    let note = left_out(seq, &body);
    match body {
        Body::Result {
            call, tool, failed, ..
        } => Body::Result {
            call,
            tool,
            text: note,
            failed,
        },
        Body::Refused { call, tool, .. } => Body::Refused {
            call,
            tool,
            problem: note,
        },
        Body::Post { author, .. } => Body::Post { author, text: note },
        Body::Turn { .. } | Body::Notice { .. } | Body::Compaction { .. } => body,
    }
}

fn shown(ctx: &ExecCtx, thread: u64, frame: Frame) -> Frame {
    match WITHHELD.has(ctx, &(thread, frame.seq)) {
        true => Frame {
            body: withheld(frame.seq, frame.body),
            ..frame
        },
        false => frame,
    }
}

fn tail(ctx: &ExecCtx, thread: &ThreadRow) -> Result<Vec<Frame>, Error> {
    let whole = FRAMES.prefix_of(&thread.id);
    let range = Range::new(FRAMES.key(&(thread.id, thread.tail_from)), whole.end);
    let frames = FRAMES.scan(ctx, range)?.into_iter();
    Ok(frames
        .map(|(_, frame)| shown(ctx, thread.id, frame))
        .collect())
}

// whether a turn's answerer is the account a call goes to: an unpinned call may reach any
// account its service holds, so only a turn no account answered is its own
fn same(service: &str, account: &Option<String>, asked: &Asked) -> bool {
    service == asked.service && *account == asked.account
}

// the turns among `frames` whose server-tool blocks another account minted (or an answerer harness
// did not record): only the account that answered reads them as they were sent
fn foreign(ctx: &ExecCtx, thread: u64, frames: &[Frame], asked: &Asked) -> Result<Foreign, Error> {
    let mut found = Foreign::new();
    for frame in frames {
        let Body::Turn { said, .. } = &frame.body else {
            continue;
        };
        let minted = said.iter().any(|said| {
            matches!(
                said,
                Said::ServerToolUse { .. } | Said::ServerToolResult(_) | Said::Cited { .. }
            )
        });
        if !minted {
            continue;
        }
        let answerer = ANSWERED.get(ctx, &(thread, frame.seq))?;
        let own = answerer.is_some_and(|by| same(&by.service, &by.account, asked));
        if !own {
            found.insert(frame.seq);
        }
    }
    Ok(found)
}

fn seal(
    ctx: &ExecCtx,
    thread: &mut ThreadRow,
    asked: &Asked,
) -> Result<Option<(Role, Vec<Frame>)>, Error> {
    let wire = asked.wire;
    let mut groups = grouped(tail(ctx, thread)?);
    let Some(last) = groups.pop() else {
        return Ok(None);
    };
    for (role, frames) in groups {
        let end = frames[frames.len() - 1].seq;
        let looks = pictures::looks(ctx, thread.id, wire, &frames)?;
        let foreign = foreign(ctx, thread.id, &frames, asked)?;
        let lanes = lanes(ctx, thread.id, wire, &frames)?;
        let shown = (&looks, &foreign);
        let message = wire::message(wire, role, &frames, shown, &lanes, opens(thread), false);
        let line = [b"sealed/".as_slice(), &thread.sealed.to_be_bytes()].concat();
        let part = put_on(ctx, thread.id, &line, message)?;
        let sealed = Sealed {
            end,
            role,
            wire,
            part,
        };
        SEALED.put(ctx, &(thread.id, thread.sealed), &sealed);
        thread.sealed += 1;
    }
    thread.tail_from = last.1[0].seq;
    Ok(Some(last))
}

fn opening(ctx: &ExecCtx, thread: &ThreadRow, turn: &Turn) -> Result<Vec<BlobId>, Error> {
    let mut parts = vec![turn.header];
    parts.extend(summary_part(ctx, thread, turn.asked.wire)?);
    Ok(parts)
}

fn sealed_parts(ctx: &ExecCtx, thread: u64) -> Result<Vec<Sealed>, Error> {
    let sealed = SEALED.scan(ctx, SEALED.prefix_of(&thread))?;
    Ok(sealed.into_iter().map(|(_, sealed)| sealed).collect())
}

fn unseal(ctx: &ExecCtx, thread: &mut ThreadRow) -> Result<(), Error> {
    for (key, _) in SEALED.scan(ctx, SEALED.prefix_of(&thread.id))? {
        SEALED.remove(ctx, &key);
    }
    thread.sealed = 0;
    thread.tail_from = thread.base;
    ctx.blob_release_lines(held_on(thread.id, b"sealed/"));
    Ok(())
}

pub(crate) fn let_go(ctx: &ExecCtx, thread: &mut ThreadRow) -> Result<(), Error> {
    crate::failures::cleared(ctx, thread)?;
    crate::woken::let_go(ctx, thread.agent, thread.id)?;
    unseal(ctx, thread)?;
    ctx.blob_release_lines(held_on(thread.id, b""));
    Ok(())
}

// sealed parts written in another wire, or for another account than the call's, are written again:
// what a server tool's blocks say depends on the account that reads them. Parts sealed before
// harness kept their account were written for the thread's route
fn reseal(ctx: &ExecCtx, thread: &mut ThreadRow, asked: &Asked) -> Result<(), Error> {
    let other_wire = sealed_parts(ctx, thread.id)?
        .iter()
        .any(|sealed| sealed.wire != asked.wire);
    let sealed_for = match SEALED_FOR.get(ctx, &thread.id)? {
        Some(sealed_for) => Some(sealed_for),
        None => routes::route(ctx, thread.id)?.map(|route| (route.service, route.account)),
    };
    let moved = sealed_for.is_some_and(|(service, account)| !same(&service, &account, asked));
    if other_wire || moved {
        unseal(ctx, thread)?;
    }
    let target = (asked.service.clone(), asked.account.clone());
    SEALED_FOR.put(ctx, &thread.id, &target);
    Ok(())
}

fn turn_parts(ctx: &ExecCtx, thread: &mut ThreadRow, turn: &Turn) -> Result<Vec<BlobId>, Error> {
    let wire = turn.asked.wire;
    let Some((role, frames)) = seal(ctx, thread, &turn.asked)? else {
        return Err(wrong_state(
            "a thread with nothing said asks no model: send a message before asking the model",
        ));
    };
    let mut parts = opening(ctx, thread, turn)?;
    parts.extend(
        sealed_parts(ctx, thread.id)?
            .iter()
            .map(|sealed| sealed.part),
    );
    let looks = pictures::looks(ctx, thread.id, wire, &frames)?;
    let foreign = foreign(ctx, thread.id, &frames, &turn.asked)?;
    let lanes = lanes(ctx, thread.id, wire, &frames)?;
    let shown = (&looks, &foreign);
    let last = wire::message(wire, role, &frames, shown, &lanes, opens(thread), true);
    parts.push(put_on(ctx, thread.id, b"last", last)?);
    let target = turn.asked.target();
    let close = wire::close(wire, &target);
    parts.push(put_constant(ctx, b"close", wire, &target, close)?);
    Ok(parts)
}

fn summarize_parts(
    ctx: &ExecCtx,
    thread: &mut ThreadRow,
    turn: &Turn,
) -> Result<Option<(u64, Vec<BlobId>)>, Error> {
    let wire = turn.asked.wire;
    seal(ctx, thread, &turn.asked)?;
    let answered = |sealed: &Sealed| {
        let seen = sealed.end <= thread.seen;
        let allowed = thread
            .compact_before
            .is_none_or(|before| sealed.end < before);
        seen && allowed
    };
    let mut covered: Vec<Sealed> = sealed_parts(ctx, thread.id)?
        .into_iter()
        .take_while(answered)
        .collect();
    let settled = match covered.last() {
        Some(last) => settled(ctx, thread, last.end)?,
        None => std::collections::BTreeSet::new(),
    };
    let cut = covered
        .iter()
        .rposition(|sealed| sealed.role == Role::User && settled.contains(&sealed.end));
    covered.truncate(cut.map_or(0, |index| index + 1));
    let Some(through) = covered.last().map(|sealed| sealed.end) else {
        return Ok(None);
    };
    let mut parts = opening(ctx, thread, turn)?;
    parts.extend(covered.iter().map(|sealed| sealed.part));
    let target = turn.asked.target();
    let summarize = wire::summarize(wire, &target);
    parts.push(put_constant(ctx, b"summarize", wire, &target, summarize)?);
    Ok(Some((through, parts)))
}

fn settled(
    ctx: &ExecCtx,
    thread: &ThreadRow,
    through: u64,
) -> Result<std::collections::BTreeSet<u64>, Error> {
    let from = FRAMES.key(&(thread.id, thread.base));
    let range = Range::new(from, Some(FRAMES.key(&(thread.id, through + 1))));
    let mut open = std::collections::BTreeSet::new();
    let mut settled = std::collections::BTreeSet::new();
    for (_, frame) in FRAMES.scan(ctx, range)? {
        if let Body::Turn { said, .. } = &frame.body {
            for said in said {
                match said {
                    Said::ServerToolUse { id, .. } => {
                        open.insert(id.clone());
                    }
                    Said::ServerToolResult(block) => {
                        if let Some(id) = wire::answers(block) {
                            open.remove(&id);
                        }
                    }
                    _ => {}
                }
            }
        }
        if open.is_empty() {
            settled.insert(frame.seq);
        }
    }
    Ok(settled)
}

enum Step {
    Learn {
        asked: Asked,
        model: Reach,
    },
    Compact {
        asked: Asked,
        through: u64,
        parts: Vec<BlobId>,
    },
    Ask {
        asked: Asked,
        upto: u64,
        parts: Vec<BlobId>,
    },
    Again(Failure),
}

impl Step {
    fn asked(&self) -> Option<&Asked> {
        match self {
            Step::Learn { asked, .. } | Step::Compact { asked, .. } | Step::Ask { asked, .. } => {
                Some(asked)
            }
            Step::Again(_) => None,
        }
    }
}

pub(crate) async fn converse(ctx: &Ctx, id: u64) {
    let driven = match resume::recover(ctx, id).await {
        Ok(()) => drive(ctx, id).await,
        Err(error) => Err(error),
    };
    if let Err(error) = driven {
        stall(ctx, id, &error);
    }
    if let Err(error) = retire(ctx, id) {
        stall(ctx, id, &error);
    }
    crate::drafts::settled(ctx, id).await;
}

async fn drive(ctx: &Ctx, id: u64) -> Result<(), Error> {
    while let Some(step) = claim(ctx, id)? {
        if let Some(asked) = step.asked() {
            resume::watch(ctx, id, asked.wait, Some(asked.config)).await;
            crate::drafts::asking(ctx, id).await;
        }
        match step {
            Step::Learn { asked, model } => learn(ctx, id, asked, model).await?,
            Step::Compact {
                asked,
                through,
                parts,
            } => compact(ctx, id, asked, through, parts).await?,
            Step::Ask { asked, upto, parts } => ask(ctx, id, asked, upto, parts).await?,
            Step::Again(failure) => resume::again(ctx, id, failure).await?,
        }
    }
    Ok(())
}

fn may_ask(thread: &ThreadRow) -> bool {
    let idle = thread.asking == Asking::Idle;
    !thread.closed && idle && thread.due()
}

fn claim(ctx: &ExecCtx, id: u64) -> Result<Option<Step>, Error> {
    let mut thread = thread(ctx, id)?;
    // a paused turn that ended without its continuation (given up, refused) lets its held input go
    let left_held = thread.asking == Asking::Idle && !thread.paused() && thread.held > 0;
    if left_held {
        unhold(ctx, &mut thread)?;
        THREADS.put(ctx, &id, &thread);
    }
    if !may_ask(&thread) || resume::waits(ctx, id)? || settling(ctx, id) {
        return Ok(None);
    }
    let Some(agent) = running(ctx, thread.agent)? else {
        return Ok(None);
    };
    let step = plan(ctx, &mut thread, &agent)?;
    THREADS.put(ctx, &id, &thread);
    Ok(step)
}

fn plan(ctx: &ExecCtx, thread: &mut ThreadRow, agent: &Agent) -> Result<Option<Step>, Error> {
    let (configured, pin) = configured_pinned(ctx, &agent.path)?;
    let (snapshot, config, model) = match configured {
        Configured::Routed {
            snapshot,
            config,
            provider,
            route,
            window,
        } => {
            let reach = Reach {
                provider,
                name: config.model.clone(),
                services: route,
                window,
            };
            (snapshot, config, reach)
        }
        unready @ (Configured::Unreadable { .. }
        | Configured::Malformed { .. }
        | Configured::NoRoute { .. }
        | Configured::Unprovided { .. }) => {
            let text = unready.problem(&agent.path).unwrap_or_default();
            give_up(ctx, thread, text)?;
            return Ok(None);
        }
    };
    seed(ctx, thread, agent, &config.listen);
    let (pick, spoken) = match routes::pick(ctx, thread, &model, &pin)? {
        Picked::Chosen(pick, spoken) => (pick, spoken),
        Picked::Unlisted(text) => {
            give_up(ctx, thread, text)?;
            return Ok(None);
        }
        Picked::Unserved(services) => {
            let text = format!(
                "no member node serves {}: the model was not asked",
                services.join(" or ")
            );
            return Ok(Some(Step::Again(Failure::lost(text))));
        }
    };
    let asked = Asked {
        wire: spoken,
        provider: model.provider.clone(),
        service: pick.service,
        account: pick.account,
        thread: thread.id,
        wait: config.wait,
        config: snapshot,
    };
    let learnt = (model.window != Window::Unasked).then(|| model.provider.clone());
    let Some(provider) = learnt else {
        thread.asking = Asking::Learning;
        return Ok(Some(Step::Learn { asked, model }));
    };
    let window = model.window.tokens();
    reseal(ctx, thread, &asked)?;
    let mut system = config.paragraphs();
    let bound = binding(ctx, thread)?;
    system.push(manual(agent, &thread.title, bound.as_ref()));
    let files = crate::tools::file::load(ctx, &agent.name);
    system.extend(crate::tools::file::left_out(&files.problems));
    let header = Header {
        name: &model.name,
        max_tokens: config.max_tokens,
        thinking: config.thinking,
        server_tools: &config.server_tools,
        system: &system,
        file_tools: &files.tools,
    };
    let header = put_on(ctx, thread.id, b"header", wire::header(spoken, &header))?;
    let threshold = config.compact_at;
    let turn = Turn {
        asked,
        lane: Lane {
            provider,
            model: model.name.clone(),
        },
        model: model.name.clone(),
        max_tokens: config.max_tokens,
        header,
    };
    match crowded(thread.context, window, turn.max_tokens, threshold) {
        true => plan_summary(ctx, thread, turn),
        false => plan_turn(ctx, thread, turn),
    }
}

fn plan_turn(ctx: &ExecCtx, thread: &mut ThreadRow, turn: Turn) -> Result<Option<Step>, Error> {
    let parts = turn_parts(ctx, thread, &turn)?;
    let upto = thread.frames;
    thread.asking = Asking::Asked { upto };
    crate::replies::clear(ctx, thread.id)?;
    PASSES.remove(ctx, &thread.id);
    PARTIALS.put(ctx, &thread.id, &Partial::new(turn.asked.wire));
    ASKED.put(ctx, &thread.id, &turn.lane);
    Ok(Some(Step::Ask {
        asked: turn.asked,
        upto,
        parts,
    }))
}

fn plan_summary(ctx: &ExecCtx, thread: &mut ThreadRow, turn: Turn) -> Result<Option<Step>, Error> {
    let Some((through, parts)) = summarize_parts(ctx, thread, &turn)? else {
        return match thread.context {
            Context::Measured(_) => plan_turn(ctx, thread, turn),
            Context::Overflowed => plan_without_largest(ctx, thread, turn),
        };
    };
    thread.asking = Asking::Compacting { through };
    Ok(Some(Step::Compact {
        asked: turn.asked,
        through,
        parts,
    }))
}

fn plan_without_largest(
    ctx: &ExecCtx,
    thread: &mut ThreadRow,
    turn: Turn,
) -> Result<Option<Step>, Error> {
    let Some(frame) = largest_unseen(ctx, thread, turn.asked.wire)? else {
        let text = format!(
            "the conversation does not fit {}'s context window, even summarized and with everything new left out: start a new conversation or choose a model with a larger context window",
            turn.model
        );
        give_up(ctx, thread, text)?;
        thread.context = Context::Measured(0);
        thread.compact_before = None;
        return Ok(None);
    };
    WITHHELD.insert(ctx, &(thread.id, frame.seq));
    let what = named(&frame);
    let text = format!(
        "{what} alone does not fit {}'s context window, so the model reads it in parts: use thread_search and thread_read to inspect the omitted content",
        turn.model
    );
    append(ctx, thread, Body::Notice { text });
    plan_turn(ctx, thread, turn)
}

fn named(frame: &Frame) -> String {
    match &frame.body {
        Body::Result { call, tool, .. } | Body::Refused { call, tool, .. } => {
            format!("the result of {tool} call {call}")
        }
        Body::Post { .. } | Body::Turn { .. } | Body::Notice { .. } | Body::Compaction { .. } => {
            format!("message {}", frame.seq)
        }
    }
}

fn largest_unseen(ctx: &ExecCtx, thread: &ThreadRow, wire: Wire) -> Result<Option<Frame>, Error> {
    let unseen = tail(ctx, thread)?.into_iter().filter(|frame| {
        let new = frame.seq > thread.seen;
        let input = frame.body.role() == Some(Role::User);
        let kept = !WITHHELD.has(ctx, &(thread.id, frame.seq));
        new && input && kept
    });
    let sized = unseen.map(|frame| {
        let one = std::slice::from_ref(&frame);
        let none = (&Looks::new(), &Foreign::new());
        let message = wire::message(wire, Role::User, one, none, &Lanes::new(), true, false);
        (message.len(), frame)
    });
    Ok(sized.max_by_key(|(size, _)| *size).map(|(_, frame)| frame))
}

fn crowded(context: Context, window: Option<u64>, max_tokens: u32, threshold: Option<u64>) -> bool {
    match (context, window) {
        (Context::Overflowed, _) => true,
        (Context::Measured(tokens), _) if threshold.is_some_and(|at| tokens >= at) => true,
        (Context::Measured(_), None) => false,
        (Context::Measured(tokens), Some(window)) => {
            tokens.saturating_add(u64::from(max_tokens)) > window
        }
    }
}

async fn learn(ctx: &Ctx, id: u64, asked: Asked, model: Reach) -> Result<(), Error> {
    let head = wire::model_head(asked.wire, &model.name);
    let answer = call_model(ctx, &asked, head, Vec::new(), egress::Delivery::Whole).await;
    let mut thread = thread(ctx, id)?;
    let read = answer.and_then(|response| {
        let read = wire::learned(asked.wire, &response);
        read.map_err(|text| Failure::answered(text, &response))
    });
    let again = match read {
        Ok(read) => {
            let window = read.window.map_or(Window::Unreported, Window::Tokens);
            match providers::learned(ctx, &model, &asked.service, window, read.provider)? {
                None => None,
                Some(text) => failed(ctx, &mut thread, Failure::last(text))?,
            }
        }
        Err(failure) => failed(ctx, &mut thread, failure)?,
    };
    settle(ctx, &mut thread)?;
    THREADS.put(ctx, &id, &thread);
    retried(ctx, id, again).await
}

fn failed(
    ctx: &ExecCtx,
    thread: &mut ThreadRow,
    failure: Failure,
) -> Result<Option<Failure>, Error> {
    let pinned = PICKS
        .get(ctx, &thread.id)?
        .is_some_and(|pick| pick.account.is_some());
    let failure = failure.unpinned(pinned);
    let again = failure.again.is_some() && !thread.closed;
    if again {
        return Ok(Some(failure));
    }
    give_up(ctx, thread, failure.text)?;
    Ok(None)
}

async fn retried(ctx: &Ctx, id: u64, again: Option<Failure>) -> Result<(), Error> {
    match again {
        Some(failure) => resume::again(ctx, id, failure).await,
        None => {
            resume::answered(ctx, id);
            Ok(())
        }
    }
}

async fn compact(
    ctx: &Ctx,
    id: u64,
    asked: Asked,
    through: u64,
    parts: Vec<BlobId>,
) -> Result<(), Error> {
    let head = wire::summary_head(asked.wire);
    let answer = call_model(ctx, &asked, head, parts, egress::Delivery::Whole).await;
    let mut thread = thread(ctx, id)?;
    let read = answer.map(|response| (wire::summarized(asked.wire, &response), response));
    let again = match read {
        Ok((Summarized::Summary(summary), _)) => {
            summarize(ctx, &mut thread, asked.wire, through, summary)?;
            None
        }
        Ok((Summarized::Overflow, _)) => {
            thread.compact_before = Some(through);
            None
        }
        Ok((Summarized::Pictures(text), response)) => match unpictured(ctx, &mut thread, &text)? {
            true => None,
            false => failed(ctx, &mut thread, Failure::answered(text, &response))?,
        },
        Ok((Summarized::Failed(text), response)) => {
            failed(ctx, &mut thread, Failure::answered(text, &response))?
        }
        Ok((Summarized::TooLarge(text), response)) => match lightened(ctx, &mut thread, &text)? {
            true => None,
            false => failed(ctx, &mut thread, Failure::answered(text, &response))?,
        },
        Err(failure) => failed(ctx, &mut thread, failure)?,
    };
    settle(ctx, &mut thread)?;
    THREADS.put(ctx, &id, &thread);
    retried(ctx, id, again).await
}

fn summarize(
    ctx: &ExecCtx,
    thread: &mut ThreadRow,
    wire: Wire,
    through: u64,
    summary: Summary,
) -> Result<(), Error> {
    let block = summary.block;
    append(ctx, thread, Body::Compaction { wire, block });
    routes::answered(ctx, thread.id, thread.frames)?;
    tallies::summarized(ctx, thread.id)?;
    crate::room_rules::forget(ctx, thread.id)?;
    thread.summary = Some(thread.frames);
    thread.context = Context::Measured(summary.tokens);
    thread.base = through + 1;
    thread.compact_before = None;
    unseal(ctx, thread)
}

async fn ask(ctx: &Ctx, id: u64, asked: Asked, upto: u64, parts: Vec<BlobId>) -> Result<(), Error> {
    let head = wire::turn_head(asked.wire);
    let delivery = egress::Delivery::Events {
        deliver: streamed_as(id),
    };
    let answer = call_model(ctx, &asked, head, parts, delivery).await;
    let wire = asked.wire;
    let read = |partial: Option<Partial>| {
        answer.and_then(|response| read_answer(wire, &response, partial))
    };
    conclude(ctx, id, upto, asked.config, read).await
}

pub(crate) async fn conclude(
    ctx: &Ctx,
    id: u64,
    upto: u64,
    config: BlobId,
    read: impl FnOnce(Option<Partial>) -> Result<Answered, Failure>,
) -> Result<(), Error> {
    let partial = crate::replies::read(ctx, id)?;
    crate::replies::clear(ctx, id)?;
    take_back_showing(ctx, id).await?;
    let answered = read(partial.clone());
    let Turned { calls, again } = turned(ctx, id, upto, config, answered, partial)?;
    if thread(ctx, id)?.open_calls > 0 {
        resume::watch(ctx, id, None, Some(config)).await;
    }
    let runs = calls
        .into_iter()
        .map(|(index, call)| run_call(ctx, id, index, call));
    for ran in all(runs).await {
        ran?;
    }
    let mut thread = thread(ctx, id)?;
    let finished = thread.asking == Asking::Running && thread.open_calls == 0;
    if finished {
        settle(ctx, &mut thread)?;
        THREADS.put(ctx, &id, &thread);
    }
    retried(ctx, id, again).await
}

fn read_answer(
    wire: Wire,
    response: &http::Response,
    partial: Option<Partial>,
) -> Result<Answered, Failure> {
    let stream_ended = response.status == 200 && response.body.is_empty();
    let streamed = partial.filter(|partial| stream_ended && partial.next_seq > 0);
    match streamed {
        Some(partial) => match wire::folded(partial) {
            Answered::Failed(text) => Err(Failure::broke(text)),
            answered => Ok(answered),
        },
        None => match wire::answered(wire, response) {
            Answered::Failed(text) => Err(Failure::answered(text, response)),
            answered => Ok(answered),
        },
    }
}

enum Ending {
    Closed,
    Reply(wire::Reply),
    Overflow,
    Pictures(String),
    Failed(Failure),
    TooLarge(String),
}

fn ending(closed: bool, answered: Result<Answered, Failure>) -> Ending {
    match (closed, answered) {
        (true, _) => Ending::Closed,
        (false, Ok(Answered::Reply(reply))) => Ending::Reply(reply),
        (false, Ok(Answered::Overflow)) => Ending::Overflow,
        (false, Ok(Answered::Pictures(text))) => Ending::Pictures(text),
        (false, Ok(Answered::Failed(text))) => Ending::Failed(Failure::last(text)),
        (false, Ok(Answered::TooLarge(text))) => Ending::TooLarge(text),
        (false, Err(failure)) => Ending::Failed(failure),
    }
}

fn whole_blocks(partial: Partial) -> wire::Reply {
    let mut said = partial.said;
    if let Forming::Kept { .. } = partial.forming {
        said.pop();
    }
    wire::Reply {
        model: partial.model,
        said,
        stop: partial.stop.unwrap_or(Stop::ToolUse),
        usage: partial.usage,
    }
}

fn dispatched(
    ctx: &ExecCtx,
    id: u64,
    partial: Option<Partial>,
) -> Result<Option<wire::Reply>, Error> {
    let any = !LIVE.scan(ctx, LIVE.prefix_of(&id))?.is_empty();
    Ok(partial.filter(|_| any).map(whole_blocks))
}

struct Turned {
    calls: Vec<(u64, String)>,
    again: Option<Failure>,
}

fn turned(
    ctx: &ExecCtx,
    id: u64,
    upto: u64,
    config: BlobId,
    answered: Result<Answered, Failure>,
    partial: Option<Partial>,
) -> Result<Turned, Error> {
    let mut thread = thread(ctx, id)?;
    thread.asking = Asking::Running;
    let mut again = None;
    let calls = match ending(thread.closed, answered) {
        Ending::Closed => match dispatched(ctx, id, partial)? {
            Some(reply) => record(ctx, &mut thread, upto, config, reply)?,
            None => Vec::new(),
        },
        Ending::Reply(reply) => record(ctx, &mut thread, upto, config, reply)?,
        Ending::Overflow => {
            let calls = match dispatched(ctx, id, partial)? {
                Some(reply) => record(ctx, &mut thread, upto, config, reply)?,
                None => Vec::new(),
            };
            thread.context = Context::Overflowed;
            calls
        }
        Ending::Pictures(text) => {
            if !unpictured(ctx, &mut thread, &text)? {
                again = failed(ctx, &mut thread, Failure::last(text))?;
            }
            Vec::new()
        }
        Ending::TooLarge(text) => {
            if !lightened(ctx, &mut thread, &text)? {
                again = failed(ctx, &mut thread, Failure::last(text))?;
            }
            Vec::new()
        }
        Ending::Failed(failure) => match dispatched(ctx, id, partial)? {
            Some(reply) => {
                let calls = record(ctx, &mut thread, upto, config, reply)?;
                let text = failure.text;
                append(ctx, &mut thread, Body::Notice { text });
                calls
            }
            None => {
                again = failed(ctx, &mut thread, failure)?;
                Vec::new()
            }
        },
    };
    for (key, live) in LIVE.scan(ctx, LIVE.prefix_of(&id))? {
        if live.outcome.is_some() {
            SHOWN.remove(ctx, &(id, live.call));
        }
        LIVE.remove(ctx, &key);
    }
    THREADS.put(ctx, &id, &thread);
    Ok(Turned { calls, again })
}

fn stall(ctx: &ExecCtx, id: u64, error: &Error) {
    let cleared = crate::replies::clear(ctx, id);
    let Ok(Some(mut thread)) = cleared.and_then(|()| THREADS.get(ctx, &id)) else {
        return;
    };
    let text = format!("the agent stopped: {}", refusal(error));
    let ended = give_up(ctx, &mut thread, text).and_then(|()| settle(ctx, &mut thread));
    if ended.is_ok() {
        THREADS.put(ctx, &id, &thread);
    }
}

fn unpictured(ctx: &ExecCtx, thread: &mut ThreadRow, text: &str) -> Result<bool, Error> {
    if !pictures::hide(ctx, thread.id, text)? {
        return Ok(false);
    }
    unseal(ctx, thread)?;
    let text = format!(
        "{text}: every image so far reaches the model as words from now on, and it is asked again"
    );
    append(ctx, thread, Body::Notice { text });
    Ok(true)
}

fn lightened(ctx: &ExecCtx, thread: &mut ThreadRow, text: &str) -> Result<bool, Error> {
    if unpictured(ctx, thread, text)? {
        return Ok(true);
    }
    let Some(frame) = largest_input(ctx, thread)? else {
        return Ok(false);
    };
    WITHHELD.insert(ctx, &(thread.id, frame.seq));
    if frame.seq < thread.tail_from {
        unseal(ctx, thread)?;
    }
    let text = format!(
        "{} makes the request larger than the model's service takes ({text}), so the model reads it in parts: use thread_search and thread_read to inspect the omitted content",
        named(&frame)
    );
    append(ctx, thread, Body::Notice { text });
    Ok(true)
}

fn largest_input(ctx: &ExecCtx, thread: &ThreadRow) -> Result<Option<Frame>, Error> {
    let whole = FRAMES.prefix_of(&thread.id);
    let range = Range::new(FRAMES.key(&(thread.id, thread.base)), whole.end);
    let frames = FRAMES.scan(ctx, range)?.into_iter().map(|(_, frame)| frame);
    let kept = frames.filter(|frame| {
        let input = frame.body.role() == Some(Role::User);
        input && !WITHHELD.has(ctx, &(thread.id, frame.seq))
    });
    Ok(kept.max_by_key(|frame| frame.body.text().len()))
}

fn record(
    ctx: &ExecCtx,
    thread: &mut ThreadRow,
    upto: u64,
    config: BlobId,
    reply: wire::Reply,
) -> Result<Vec<(u64, String)>, Error> {
    thread.seen = upto;
    thread.last_stop = Some(reply.stop);
    crate::failures::cleared(ctx, thread)?;
    let usage = reply.usage;
    let summed = usage.input + usage.cache_read + usage.cache_write + usage.output;
    let tokens = PASSES.get(ctx, &thread.id)?.unwrap_or(summed);
    PASSES.remove(ctx, &thread.id);
    thread.context = Context::Measured(tokens);
    let asked: Vec<(u64, String, String, String)> = (0u64..)
        .zip(&reply.said)
        .filter_map(|(index, said)| match said {
            Said::ToolUse { id, name, input } => {
                Some((index, id.clone(), name.clone(), input.clone()))
            }
            Said::Text(_)
            | Said::Thinking { .. }
            | Said::RedactedThinking(_)
            | Said::ServerToolUse { .. }
            | Said::ServerToolResult(_)
            | Said::Cited { .. } => None,
        })
        .collect();
    let turn = Body::Turn {
        model: reply.model,
        config,
        said: reply.said,
        stop: reply.stop,
        usage,
    };
    append(ctx, thread, turn);
    if let Some(lane) = ASKED.get(ctx, &thread.id)? {
        LANES.put(ctx, &(thread.id, thread.frames), &lane);
    }
    routes::answered(ctx, thread.id, thread.frames)?;
    tallies::turned(ctx, thread.id, &usage)?;
    let mut calls = Vec::new();
    for (index, call, tool, input) in asked {
        match LIVE.get(ctx, &(thread.id, index))? {
            Some(Live {
                outcome: Some(body),
                ..
            }) => {
                append(ctx, thread, body);
                pictures::called(ctx, thread.id, &call, thread.frames)?;
            }
            Some(Live { outcome: None, .. }) => {}
            None => match place(ctx, thread, &call, &tool, &input)? {
                Placed::Refused(body) => append(ctx, thread, body),
                Placed::Relaying => calls.push((index, call)),
            },
        }
    }
    Ok(calls)
}
