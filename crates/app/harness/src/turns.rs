//! The loop consensus runs for a thread: read the agent's configuration and its model's record,
//! learn the model's context window, summarize the thread when its next answer would not fit,
//! ask the model in its wire for a streamed reply, build the turn from the streamed events or
//! the whole answer, record the results of calls that ran while it streamed, run the rest, and
//! ask again once every result is back.
use ::program::{
    BlobId, Ctx, Error, ExecCtx, HashKind, ProgramId, QueryCtx, Range, all, unexpected_reply,
    wrong_state,
};
use abi::role::egress::{self, http};

use crate::agents::{Agent, configured, hear_again, manual, running, thread};
use crate::calls::{Placed, place, refusal, run_call, take_back_showing};
use crate::conversations::{binding, retire};
use crate::listening::mirror;
use crate::models::learned;
use crate::resume::{self, Failure};
use crate::state::{
    FLIGHTS, FRAMES, HELD, LIVE, Live, PARTIALS, RETRIES, SEALED, SHOWN, THREADS, WITHHELD,
};
use crate::tallies;
use crate::waiting::settling;
use crate::wire::{self, Answered, Header, Summarized, Summary};
use crate::{
    Asking, Body, Configured, Context, Forming, Frame, ModelRow, PART, Partial, Role, Said, Sealed,
    Stop, ThreadRow, Threshold, Window, Wire, streamed_as,
};

struct Asked {
    wire: Wire,
    service: String,
    wait: Option<u64>,
    config: BlobId,
}

struct Turn {
    asked: Asked,
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
    let looping = thread.asking != Asking::Idle;
    if !looping {
        append(ctx, thread, body);
        return;
    }
    HELD.put(ctx, &(thread.id, thread.held), &body);
    thread.held += 1;
}

pub(crate) fn settle(ctx: &ExecCtx, thread: &mut ThreadRow) -> Result<(), Error> {
    for index in 0..thread.held {
        let key = (thread.id, index);
        if let Some(body) = HELD.get(ctx, &key)? {
            append(ctx, thread, body);
        }
        HELD.remove(ctx, &key);
    }
    thread.held = 0;
    thread.asking = Asking::Idle;
    FLIGHTS.remove(ctx, &thread.id);
    Ok(())
}

pub(crate) fn give_up(ctx: &ExecCtx, thread: &mut ThreadRow, text: String) {
    append(ctx, thread, Body::Notice { text });
    thread.seen = thread.seen.max(thread.last_input);
    thread.last_stop = None;
    RETRIES.remove(ctx, &thread.id);
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
    let request_ = egress::Request::Http {
        service: asked.service.clone(),
        head,
        body: parts,
    };
    let answer = request(ctx, request_, delivery, asked.wait).await;
    let bytes = answer.map_err(|error| Failure::unreached(&error))?;
    let response: http::Response = abi::decode(&bytes)
        .map_err(|_| Failure::last("the egress answer is not an HTTP response"))?;
    resume::window_seen(ctx, &asked.service, &response);
    Ok(response)
}

fn put_part(ctx: &ExecCtx, text: String) -> Result<BlobId, Error> {
    ctx.blob_put(HashKind::Sha256, PART, text.into_bytes())
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
    Ok(Some(put_part(ctx, summary)?))
}

fn opens(thread: &ThreadRow) -> bool {
    thread.sealed == 0 && thread.summary.is_none()
}

fn left_out(seq: u64, body: &Body) -> String {
    let text = body.text();
    format!(
        "left out of your context: frame {seq} is {} bytes in {} lines, too large for your context window alone. Find lines in it with thread_search (frame {seq}) and read parts of it with thread_read (frame {seq}, an offset and a length).",
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

fn seal(
    ctx: &ExecCtx,
    thread: &mut ThreadRow,
    wire: Wire,
) -> Result<Option<(Role, Vec<Frame>)>, Error> {
    let mut groups = grouped(tail(ctx, thread)?);
    let Some(last) = groups.pop() else {
        return Ok(None);
    };
    for (role, frames) in groups {
        let end = frames[frames.len() - 1].seq;
        let message = wire::message(wire, role, &frames, opens(thread), false);
        let part = put_part(ctx, message)?;
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
    Ok(())
}

fn reseal(ctx: &ExecCtx, thread: &mut ThreadRow, wire: Wire) -> Result<(), Error> {
    let other_wire = sealed_parts(ctx, thread.id)?
        .iter()
        .any(|sealed| sealed.wire != wire);
    if other_wire {
        unseal(ctx, thread)?;
    }
    Ok(())
}

fn turn_parts(ctx: &ExecCtx, thread: &mut ThreadRow, turn: &Turn) -> Result<Vec<BlobId>, Error> {
    let wire = turn.asked.wire;
    let Some((role, frames)) = seal(ctx, thread, wire)? else {
        return Err(wrong_state("a thread with nothing said asks no model"));
    };
    let mut parts = opening(ctx, thread, turn)?;
    parts.extend(
        sealed_parts(ctx, thread.id)?
            .iter()
            .map(|sealed| sealed.part),
    );
    let last = wire::message(wire, role, &frames, opens(thread), true);
    parts.push(put_part(ctx, last)?);
    parts.push(put_part(ctx, wire::close(wire).into())?);
    Ok(parts)
}

fn summarize_parts(
    ctx: &ExecCtx,
    thread: &mut ThreadRow,
    turn: &Turn,
) -> Result<Option<(u64, Vec<BlobId>)>, Error> {
    let wire = turn.asked.wire;
    seal(ctx, thread, wire)?;
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
    let cut = covered.iter().rposition(|sealed| sealed.role == Role::User);
    covered.truncate(cut.map_or(0, |index| index + 1));
    let Some(through) = covered.last().map(|sealed| sealed.end) else {
        return Ok(None);
    };
    let mut parts = opening(ctx, thread, turn)?;
    parts.extend(covered.iter().map(|sealed| sealed.part));
    parts.push(put_part(ctx, wire::summarize(wire).into())?);
    Ok(Some((through, parts)))
}

enum Step {
    Learn {
        asked: Asked,
        model: ModelRow,
    },
    Compact {
        asked: Asked,
        through: u64,
        parts: Vec<BlobId>,
    },
    Ask {
        asked: Asked,
        upto: u64,
        summarized: bool,
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
}

async fn drive(ctx: &Ctx, id: u64) -> Result<(), Error> {
    while let Some(step) = claim(ctx, id)? {
        if let Some(asked) = step.asked() {
            resume::watch(ctx, id, asked.wait, Some(asked.config)).await;
        }
        match step {
            Step::Learn { asked, model } => learn(ctx, id, asked, model).await?,
            Step::Compact {
                asked,
                through,
                parts,
            } => compact(ctx, id, asked, through, parts).await?,
            Step::Ask {
                asked,
                upto,
                summarized,
                parts,
            } => ask(ctx, id, asked, upto, summarized, parts).await?,
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
    let (snapshot, config, model) = match configured(ctx, &agent.path)? {
        Configured::Ready {
            snapshot,
            config,
            model,
        } => (snapshot, config, model),
        unready @ (Configured::Unreadable { .. }
        | Configured::Malformed { .. }
        | Configured::UnknownModel { .. }) => {
            let text = unready.problem(&agent.path).unwrap_or_default();
            give_up(ctx, thread, text);
            return Ok(None);
        }
    };
    mirror(ctx, thread, agent, &config.listen);
    if !serves(ctx, &model.service)? {
        let text = format!(
            "no member node serves {}: the model was not asked",
            model.service
        );
        return Ok(Some(Step::Again(Failure::lost(text))));
    }
    let asked = Asked {
        wire: model.wire,
        service: model.service.clone(),
        wait: config.wait,
        config: snapshot,
    };
    let window = match model.window {
        Window::Unasked => {
            thread.asking = Asking::Learning;
            return Ok(Some(Step::Learn { asked, model }));
        }
        Window::Unreported => None,
        Window::Tokens(tokens) => Some(tokens),
    };
    reseal(ctx, thread, model.wire)?;
    let mut system = config.paragraphs();
    let bound = binding(ctx, thread)?;
    system.push(manual(agent, &thread.title, bound.as_ref()));
    let header = Header {
        name: &model.name,
        max_tokens: config.max_tokens,
        thinking: config.thinking,
        server_tools: &config.server_tools,
        system: &system,
    };
    let header = put_part(ctx, wire::header(model.wire, &header))?;
    let threshold = Threshold::of(&config, Some(&model)).map(Threshold::tokens);
    let turn = Turn {
        asked,
        model: model.id,
        max_tokens: config.max_tokens,
        header,
    };
    match crowded(thread, window, turn.max_tokens, threshold) {
        true => plan_summary(ctx, thread, turn),
        false => plan_turn(ctx, thread, turn),
    }
}

fn plan_turn(ctx: &ExecCtx, thread: &mut ThreadRow, turn: Turn) -> Result<Option<Step>, Error> {
    let parts = turn_parts(ctx, thread, &turn)?;
    let upto = thread.frames;
    thread.asking = Asking::Asked { upto };
    PARTIALS.put(ctx, &thread.id, &Partial::new(turn.asked.wire));
    let summarized = thread.summary.is_some();
    Ok(Some(Step::Ask {
        asked: turn.asked,
        upto,
        summarized,
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
            "the conversation does not fit {}'s context window, even summarized and with everything new left out",
            turn.model
        );
        give_up(ctx, thread, text);
        thread.context = Context::Measured(0);
        thread.compact_before = None;
        return Ok(None);
    };
    WITHHELD.insert(ctx, &(thread.id, frame.seq));
    let what = match &frame.body {
        Body::Result { call, tool, .. } | Body::Refused { call, tool, .. } => {
            format!("the result of {tool} call {call}")
        }
        Body::Post { .. } | Body::Turn { .. } | Body::Notice { .. } | Body::Compaction { .. } => {
            format!("message {}", frame.seq)
        }
    };
    let text = format!(
        "{what} alone does not fit {}'s context window, so the model reads it in parts",
        turn.model
    );
    append(ctx, thread, Body::Notice { text });
    plan_turn(ctx, thread, turn)
}

fn largest_unseen(ctx: &ExecCtx, thread: &ThreadRow, wire: Wire) -> Result<Option<Frame>, Error> {
    let unseen = tail(ctx, thread)?.into_iter().filter(|frame| {
        let new = frame.seq > thread.seen;
        let input = frame.body.role() == Some(Role::User);
        let kept = !WITHHELD.has(ctx, &(thread.id, frame.seq));
        new && input && kept
    });
    let sized = unseen.map(|frame| {
        let message = wire::message(wire, Role::User, std::slice::from_ref(&frame), true, false);
        (message.len(), frame)
    });
    Ok(sized.max_by_key(|(size, _)| *size).map(|(_, frame)| frame))
}

// compact once the context reaches the threshold in effect, or when the next answer would not
// fit the window, which holds whatever the threshold
fn crowded(
    thread: &ThreadRow,
    window: Option<u64>,
    max_tokens: u32,
    threshold: Option<u64>,
) -> bool {
    match (thread.context, window) {
        (Context::Overflowed, _) => true,
        (Context::Measured(tokens), _) if threshold.is_some_and(|at| tokens >= at) => true,
        (Context::Measured(_), None) => false,
        (Context::Measured(tokens), Some(window)) => {
            tokens.saturating_add(u64::from(max_tokens)) > window
        }
    }
}

async fn learn(ctx: &Ctx, id: u64, asked: Asked, model: ModelRow) -> Result<(), Error> {
    let head = wire::model_head(asked.wire, &model.name);
    let answer = call_model(ctx, &asked, head, Vec::new(), egress::Delivery::Whole).await;
    let mut thread = thread(ctx, id)?;
    let read = answer.and_then(|response| {
        let window = wire::window(asked.wire, &response);
        window.map_err(|text| Failure::answered(text, &response))
    });
    let again = match read {
        Ok(Some(tokens)) => {
            learned(ctx, &model, Window::Tokens(tokens))?;
            None
        }
        Ok(None) => {
            learned(ctx, &model, Window::Unreported)?;
            None
        }
        Err(failure) => failed(ctx, &mut thread, failure),
    };
    settle(ctx, &mut thread)?;
    THREADS.put(ctx, &id, &thread);
    retried(ctx, id, again).await
}

// a failure worth asking again waits until the thread is settled; any other ends the turn here
fn failed(ctx: &ExecCtx, thread: &mut ThreadRow, failure: Failure) -> Option<Failure> {
    let again = failure.again.is_some() && !thread.closed;
    if again {
        return Some(failure);
    }
    give_up(ctx, thread, failure.text);
    None
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
        Ok((Summarized::Failed(text), response)) => {
            failed(ctx, &mut thread, Failure::answered(text, &response))
        }
        Err(failure) => failed(ctx, &mut thread, failure),
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
    tallies::summarized(ctx, thread.id)?;
    thread.summary = Some(thread.frames);
    thread.context = Context::Measured(summary.tokens);
    thread.base = through + 1;
    thread.compact_before = None;
    unseal(ctx, thread)
}

async fn ask(
    ctx: &Ctx,
    id: u64,
    asked: Asked,
    upto: u64,
    summarized: bool,
    parts: Vec<BlobId>,
) -> Result<(), Error> {
    let head = wire::turn_head(asked.wire, summarized);
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

// what follows a model call's answer, or the answer harness found lost (`resume::recover`)
pub(crate) async fn conclude(
    ctx: &Ctx,
    id: u64,
    upto: u64,
    config: BlobId,
    read: impl FnOnce(Option<Partial>) -> Result<Answered, Failure>,
) -> Result<(), Error> {
    let partial = PARTIALS.get(ctx, &id)?;
    PARTIALS.remove(ctx, &id);
    let mut settled = take_back_showing(ctx, id).await?;
    let answered = read(partial.clone());
    let Turned { calls, again } = turned(ctx, id, upto, config, answered, partial)?;
    if thread(ctx, id)?.open_calls > 0 {
        resume::watch(ctx, id, None, Some(config)).await;
    }
    let runs = calls
        .into_iter()
        .map(|(index, call)| run_call(ctx, id, index, call));
    for ran in all(runs).await {
        settled = settled.and(ran?);
    }
    let mut thread = thread(ctx, id)?;
    let finished = thread.asking == Asking::Running && thread.open_calls == 0;
    if finished {
        settle(ctx, &mut thread)?;
        THREADS.put(ctx, &id, &thread);
    }
    hear_again(ctx, settled.hearings);
    retried(ctx, id, again).await
}

fn read_answer(
    wire: Wire,
    response: &http::Response,
    partial: Option<Partial>,
) -> Result<Answered, Failure> {
    let stream_ended = response.status == 200 && response.body.is_empty();
    let streamed = partial.filter(|partial| stream_ended && partial.next_seq > 0);
    // a stream that broke, stopped short or failed is the model's to answer again
    match streamed {
        Some(partial) => match wire::folded(partial) {
            Answered::Failed(text) => Err(Failure::busy(text)),
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
    Failed(Failure),
}

fn ending(closed: bool, answered: Result<Answered, Failure>) -> Ending {
    match (closed, answered) {
        (true, _) => Ending::Closed,
        (false, Ok(Answered::Reply(reply))) => Ending::Reply(reply),
        (false, Ok(Answered::Overflow)) => Ending::Overflow,
        (false, Ok(Answered::Failed(text))) => Ending::Failed(Failure::last(text)),
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
        Ending::Failed(failure) => match dispatched(ctx, id, partial)? {
            Some(reply) => {
                let calls = record(ctx, &mut thread, upto, config, reply)?;
                let text = failure.text;
                append(ctx, &mut thread, Body::Notice { text });
                calls
            }
            None => {
                again = failed(ctx, &mut thread, failure);
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
    PARTIALS.remove(ctx, &id);
    let Ok(Some(mut thread)) = THREADS.get(ctx, &id) else {
        return;
    };
    give_up(
        ctx,
        &mut thread,
        format!("the agent stopped: {}", refusal(error)),
    );
    if settle(ctx, &mut thread).is_ok() {
        THREADS.put(ctx, &id, &thread);
    }
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
    let usage = reply.usage;
    let tokens = usage.input + usage.cache_read + usage.cache_write + usage.output;
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
    tallies::turned(ctx, thread.id, &usage)?;
    let mut calls = Vec::new();
    for (index, call, tool, input) in asked {
        match LIVE.get(ctx, &(thread.id, index))? {
            Some(Live {
                outcome: Some(body),
                ..
            }) => append(ctx, thread, body),
            Some(Live { outcome: None, .. }) => {}
            None => match place(ctx, thread, &call, &tool, &input)? {
                Placed::Refused(body) => append(ctx, thread, body),
                Placed::Relaying => calls.push((index, call)),
            },
        }
    }
    Ok(calls)
}
