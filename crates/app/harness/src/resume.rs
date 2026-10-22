//! Turns and tool calls a node restart or a harness update cut off. A model call, a tool call's
//! write and a watched stream keep egress's deadline, and new code's first run wakes each turn
//! under way before it: harness records what streamed whole, relays, fails or asks again.
use ::program::{BlobId, Cause, Ctx, EntryId, Error, ExecCtx, Principal, QueryCtx, all, code};
use ::program::{Range, unexpected_reply, wrong_state};
use abi::role::egress::{self, DEFAULT_WAIT, http};
use abi::role::identity::AccountNumber;

use crate::agents::{configured, harnessed_agent, running, thread};
use crate::bookings::note;
use crate::calls::{lost, refusal, run_call};
use crate::conversations::{binding, conversation};
use crate::routes;
use crate::schema::code_of;
use crate::state::{
    ASKED, AWAITED, Awaited, CALLS, FLIGHTS, FRAMES, Flight, PARTIALS, PICKS, REPORTING, REPORTS,
    RETRIES, RUNNING, Retry, STREAMING, THREADS, WATCHES,
};
use crate::tools::Write;
use crate::turns::{append, bound, conclude, converse, give_up, say, settle};
use crate::wire;
use crate::{Asking, Body, CallState, Flow, Op, Partial, Place, ThreadRow, heard_as, streamed_as};

const RETRY_AFTER: &str = "retry-after";
const RESET: &str = "anthropic-ratelimit-unified-5h-reset";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Again {
    Lost,
    Busy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Wait {
    after: Option<u64>,
    reset: Option<u64>,
}

impl Wait {
    fn until(self, now: u64) -> Option<u64> {
        let after = self
            .after
            .map(|seconds| now.saturating_add(seconds.saturating_mul(1000)));
        let reset = self.reset.map(|seconds| seconds.saturating_mul(1000));
        after.max(reset).filter(|until| *until > now)
    }
}

pub(crate) struct Failure {
    pub text: String,
    pub again: Option<Again>,
    pub wait: Option<Wait>,
    pub moves: bool,
}

impl Failure {
    fn new(text: impl Into<String>, again: Option<Again>) -> Failure {
        Failure {
            text: text.into(),
            again,
            wait: None,
            moves: false,
        }
    }

    pub fn last(text: impl Into<String>) -> Failure {
        Failure::new(text, None)
    }

    pub fn lost(text: impl Into<String>) -> Failure {
        Failure::new(text, Some(Again::Lost))
    }

    pub fn busy(text: impl Into<String>) -> Failure {
        Failure::new(text, Some(Again::Busy))
    }

    // a stream the model's service broke or failed
    pub fn broke(text: impl Into<String>) -> Failure {
        Failure {
            moves: true,
            ..Failure::busy(text)
        }
    }

    pub fn unpinned(self, pinned: bool) -> Failure {
        match self.again {
            None if pinned => Failure {
                moves: true,
                ..Failure::busy(self.text)
            },
            _ => self,
        }
    }

    // the model's own answer: a 429 or a 5xx says it was busy, anything else is final
    pub fn answered(text: String, response: &http::Response) -> Failure {
        let busy = response.status == 429 || response.status >= 500;
        if !busy {
            return Failure::last(text);
        }
        let seconds = |name| header(response, name).and_then(|value| value.parse().ok());
        let wait = Wait {
            after: seconds(RETRY_AFTER),
            reset: seconds(RESET),
        };
        let asked = wait.after.is_some() || wait.reset.is_some();
        Failure {
            wait: asked.then_some(wait),
            moves: true,
            ..Failure::busy(text)
        }
    }

    // egress's refusal: only what never succeeds unchanged is final (docs/programs.md, section 4)
    pub fn unreached(error: &Error) -> Failure {
        let text = format!("the model was not reached: {}", refusal(error));
        let unchanged = error.code == code::INVALID_INPUT || error.code == code::UNAUTHORIZED;
        let framed = error.code == code::CAPACITY;
        match (unchanged, framed) {
            (true, _) => Failure::last(text),
            (false, true) => Failure::lost(text),
            (false, false) => Failure {
                moves: true,
                ..Failure::lost(text)
            },
        }
    }
}

pub(crate) fn header<'a>(response: &'a http::Response, name: &str) -> Option<&'a str> {
    let found = response
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name));
    found.map(|(_, value)| value.trim())
}

fn deadline(flight: &Flight) -> u64 {
    flight.made.max(flight.moved).saturating_add(flight.wait)
}

fn wake(id: u64) -> Op {
    Op::Resume { thread: id }
}

pub(crate) async fn watch(ctx: &Ctx, id: u64, wait: Option<u64>, config: Option<BlobId>) {
    let made = ctx.env().height;
    let mut flight = Flight {
        made,
        moved: made,
        wait: wait.unwrap_or(DEFAULT_WAIT),
        config,
        watch: None,
    };
    let after = deadline(&flight).saturating_add(1);
    flight.watch = ctx
        .schedule(after, &ctx.env().program, &wake(id))
        .await
        .ok();
    FLIGHTS.put(ctx, &id, &flight);
}

pub(crate) async fn awaiting(ctx: &Ctx, id: u64, call: &str, writes: &[Write]) {
    let key = (id, call.to_owned());
    let height = ctx.env().height;
    let code = match AWAITED.get(ctx, &key) {
        Ok(Some(awaited)) => awaited.code,
        _ => code_of(ctx, &ctx.env().program).ok(),
    };
    let deadline = answered_by(ctx, height, writes);
    if let Some(deadline) = deadline {
        let after = deadline.saturating_add(1);
        let _ = ctx.schedule(after, &ctx.env().program, &wake(id)).await;
    }
    let awaited = Awaited {
        height,
        code,
        deadline,
    };
    AWAITED.put(ctx, &key, &awaited);
}

fn answered_by(ctx: &QueryCtx, height: u64, writes: &[Write]) -> Option<u64> {
    let egress = ctx.env().roles.get(abi::role::EGRESS).cloned()?;
    let mut blocks = 0u64;
    for write in writes {
        if write.target != egress {
            return None;
        }
        let op = abi::decode::<egress::Op>(&write.payload).ok()?;
        let egress::Op::Request {
            delivery: egress::Delivery::Whole,
            wait,
            ..
        } = op
        else {
            return None;
        };
        blocks = blocks.saturating_add(wait.unwrap_or(DEFAULT_WAIT));
    }
    (blocks > 0).then_some(height.saturating_add(blocks))
}

fn lost_why(awaited: &Awaited, code: Option<BlobId>, height: u64) -> Option<String> {
    let sent = awaited.height;
    let updated = matches!((awaited.code, code), (Some(then), Some(now)) if then != now);
    if updated {
        return Some(format!(
            "harness was updated while this call awaited the answer to what it sent at block {sent}"
        ));
    }
    let deadline = awaited.deadline.filter(|deadline| height > *deadline)?;
    Some(format!(
        "harness heard no answer by block {deadline}, egress's deadline for what this call sent at block {sent}: a harness update cut off the run awaiting it"
    ))
}

async fn lost_writes(ctx: &Ctx, id: u64) -> Result<(), Error> {
    let open = AWAITED.scan(ctx, AWAITED.prefix_of(&id))?;
    if open.is_empty() {
        return Ok(());
    }
    let height = ctx.env().height;
    let code = code_of(ctx, &ctx.env().program).ok();
    let mut failed = 0usize;
    for ((_, call), awaited) in open {
        let Some(why) = lost_why(&awaited, code, height) else {
            continue;
        };
        AWAITED.remove(ctx, &(id, call.clone()));
        let arm = crate::woken::Call {
            agent: thread(ctx, id)?.agent,
            thread: id,
            call: &call,
        };
        crate::woken::ended(ctx, &arm)?;
        let text = format!(
            "{why}, so its answer was lost. It may have run: check, and run it again if it is still needed"
        );
        if lost(ctx, id, &call, text)? {
            failed += 1;
        }
    }
    if failed == 0 {
        return Ok(());
    }
    let row = thread(ctx, id)?;
    let line = match failed {
        1 => "a tool call's answer was lost to a harness update; harness failed the call, and the model hears why: read the failed call before retrying it".to_owned(),
        count => format!(
            "{count} tool calls' answers were lost to a harness update; harness failed them, and the model hears why: read the failed calls before retrying them"
        ),
    };
    tell(ctx, &row, line, "a tool call's answer was lost").await
}

pub(crate) async fn listening(ctx: &Ctx, id: u64, watch: u64) {
    let made = ctx.env().height;
    let mut flight = Flight {
        made,
        moved: made,
        wait: DEFAULT_WAIT,
        config: None,
        watch: None,
    };
    let after = deadline(&flight).saturating_add(1);
    flight.watch = ctx
        .schedule(after, &ctx.env().program, &wake(id))
        .await
        .ok();
    STREAMING.put(ctx, &(id, watch), &flight);
}

pub(crate) fn stream_moved(ctx: &ExecCtx, id: u64, watch: u64) -> Result<(), Error> {
    let key = (id, watch);
    let height = ctx.env().height;
    let Some(flight) = STREAMING
        .get(ctx, &key)?
        .filter(|flight| flight.moved < height)
    else {
        return Ok(());
    };
    let moved = Flight {
        moved: height,
        ..flight
    };
    STREAMING.put(ctx, &key, &moved);
    Ok(())
}

pub(crate) fn unlistened(ctx: &ExecCtx, id: u64, watch: u64) {
    STREAMING.remove(ctx, &(id, watch));
}

async fn lost_streams(ctx: &Ctx, id: u64, since: Option<u64>) -> Result<(), Error> {
    let env = ctx.env();
    let height = env.height;
    for ((_, watch), flight) in STREAMING.scan(ctx, STREAMING.prefix_of(&id))? {
        let key = (id, watch);
        let Some(row) = WATCHES.get(ctx, &watch)? else {
            STREAMING.remove(ctx, &key);
            continue;
        };
        let cut = since.is_some_and(|since| flight.made < since);
        let end = deadline(&flight);
        if !cut && height <= end {
            continue;
        }
        let own = matches!(env.cause, Cause::Scheduled { entry } if Some(entry) == flight.watch);
        if delivered(ctx, &heard_as(watch))? {
            if own || height > end {
                let moved = Flight {
                    moved: height.max(flight.moved),
                    ..flight
                };
                let after = deadline(&moved).saturating_add(1);
                let watch = ctx
                    .schedule(after, &ctx.env().program, &wake(id))
                    .await
                    .ok();
                STREAMING.put(ctx, &key, &Flight { watch, ..moved });
            }
            continue;
        }
        STREAMING.remove(ctx, &key);
        WATCHES.remove(ctx, &watch);
        ctx.blob_release_lines(crate::tools::held_for(row.thread, &row.call));
        let text = format!(
            "{} call {} ({}) ended unheard: egress no longer holds its stream, and a harness update cut off the run that listened for its end, so {}",
            row.tool,
            row.call,
            row.about,
            crate::watches::lost(&row)
        );
        if let Some(told) = note(ctx, row.agent, Some(row.thread), text)?
            && told != id
        {
            let next = height.saturating_add(1);
            let _ = ctx.schedule(next, &ctx.env().program, &wake(told)).await;
        }
    }
    Ok(())
}

pub(crate) fn moved(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    let height = ctx.env().height;
    let Some(flight) = FLIGHTS
        .get(ctx, &id)?
        .filter(|flight| flight.moved < height)
    else {
        return Ok(());
    };
    let moved = Flight {
        moved: height,
        ..flight
    };
    FLIGHTS.put(ctx, &id, &moved);
    Ok(())
}

pub(crate) fn waits(ctx: &QueryCtx, id: u64) -> Result<bool, Error> {
    let env = ctx.env();
    let Some(retry) = RETRIES.get(ctx, &id)? else {
        return Ok(false);
    };
    let early = env.height < retry.at;
    let held = retry.until.is_some_and(|until| env.time < until);
    Ok(early || held)
}

pub(crate) fn heard(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    let Some(retry) = RETRIES.get(ctx, &id)? else {
        return Ok(());
    };
    let at = ctx.env().height;
    let until = None;
    RETRIES.put(ctx, &id, &Retry { at, until, ..retry });
    Ok(())
}

pub(crate) async fn room(ctx: &Ctx, service: &str, providers: &[String]) {
    let Ok(retries) = RETRIES.all(ctx) else {
        return;
    };
    let next = ctx.env().height.saturating_add(1);
    let mut wakes = Vec::new();
    for (id, retry) in retries {
        let on = PICKS
            .get(ctx, &id)
            .ok()
            .flatten()
            .is_some_and(|pick| pick.service == service);
        let lane = ASKED.get(ctx, &id).ok().flatten();
        let spoken = lane.is_some_and(|lane| providers.contains(&lane.provider));
        let later = retry.until.is_some() || retry.at > next;
        let pulled = later && on && spoken;
        if pulled {
            let until = None;
            RETRIES.put(
                ctx,
                &id,
                &Retry {
                    at: next,
                    until,
                    ..retry
                },
            );
            wakes.push(wake(id));
        }
    }
    all(wakes
        .iter()
        .map(|op| ctx.schedule(next, ctx.env().program.clone(), op)))
    .await;
}

pub(crate) async fn answered_on(ctx: &Ctx, service: &str, id: u64) {
    let Some(lane) = ASKED.get(ctx, &id).ok().flatten() else {
        return;
    };
    room(ctx, service, &[lane.provider]).await;
}

pub(crate) fn answered(ctx: &ExecCtx, id: u64) {
    RETRIES.remove(ctx, &id);
}

pub(crate) async fn again(ctx: &Ctx, id: u64, failure: Failure) -> Result<(), Error> {
    let mut row = thread(ctx, id)?;
    let Some(why) = failure.again.filter(|_| !row.closed) else {
        give_up(ctx, &mut row, failure.text)?;
        THREADS.put(ctx, &id, &row);
        return Ok(());
    };
    let env = ctx.env();
    let (height, now) = (env.height, env.time);
    let attempt = RETRIES
        .get(ctx, &id)?
        .map_or(1, |retry| retry.attempt.saturating_add(1));
    let until = failure.wait.and_then(|wait| wait.until(now));
    let moved = failure.moves && routes::failed(ctx, id)?;
    let until = until.filter(|_| !moved);
    let gap = match until {
        Some(_) => 1,
        None if moved => 1,
        None => 1u64
            .checked_shl(attempt - 1)
            .map_or(DEFAULT_WAIT, |gap| gap.min(DEFAULT_WAIT)),
    };
    let at = height.saturating_add(gap);
    let entry = match ctx.schedule(at, &ctx.env().program, &wake(id)).await {
        Ok(entry) => entry,
        Err(error) => {
            let text = format!(
                "{}; asking the model again needs the scheduler, which refused ({}), so the next input asks",
                failure.text,
                refusal(&error)
            );
            give_up(ctx, &mut row, text)?;
            THREADS.put(ctx, &id, &row);
            return Ok(());
        }
    };
    let retry = Retry {
        attempt,
        at,
        until,
        since: height,
        since_time: now,
        entry: Some(entry),
    };
    RETRIES.put(ctx, &id, &retry);
    let when = match until {
        Some(until) => format!(
            "once the model's wait ends at {} (seconds since the epoch)",
            until.div_ceil(1000)
        ),
        None if moved => format!("at block {at} with no account named, so its service picks one"),
        None => format!("at block {at}"),
    };
    let text = format!(
        "{}; harness asks the model again {when} (attempt {attempt})",
        failure.text
    );
    append(ctx, &mut row, Body::Notice { text: text.clone() });
    let ceiling = until.is_some() || gap == DEFAULT_WAIT;
    crate::failures::retrying(ctx, &row, text, attempt, ceiling)?;
    THREADS.put(ctx, &id, &row);
    let first_lost = why == Again::Lost && attempt == 1;
    if first_lost {
        let line = format!("{}; it asks the model again {when}", failure.text);
        tell(ctx, &row, line, "egress failed").await?;
    }
    Ok(())
}

async fn repace(ctx: &Ctx, id: u64) {
    let Ok(Some(retry)) = RETRIES.get(ctx, &id) else {
        return;
    };
    let env = ctx.env();
    let own = matches!(env.cause, Cause::Scheduled { entry } if Some(entry) == retry.entry);
    let Some(until) = retry.until.filter(|until| own && env.time < *until) else {
        return;
    };
    let blocks = env.height.saturating_sub(retry.since);
    let took = env.time.saturating_sub(retry.since_time);
    let left = until.saturating_sub(env.time);
    let more = match took {
        0 => 1,
        took => left.saturating_mul(blocks).div_ceil(took).max(1),
    };
    let at = env.height.saturating_add(more);
    let paced = match ctx.schedule(at, &ctx.env().program, &wake(id)).await {
        Ok(entry) => Retry {
            at,
            entry: Some(entry),
            ..retry
        },
        Err(_) => Retry {
            until: None,
            entry: None,
            ..retry
        },
    };
    RETRIES.put(ctx, &id, &paced);
}

async fn switched(ctx: &Ctx) -> Result<Option<u64>, Error> {
    let Ok(code) = code_of(ctx, &ctx.env().program) else {
        return Ok(None);
    };
    if let Some((running, since)) = RUNNING.get(ctx)?
        && running == code
    {
        return Ok(Some(since));
    }
    let since = ctx.env().height;
    RUNNING.put(ctx, &(code, since));
    let mut wakes = Vec::new();
    for (id, row) in THREADS.all(ctx)? {
        let before = FLIGHTS
            .get(ctx, &id)?
            .is_none_or(|flight| flight.made < since);
        let listened = !STREAMING.scan(ctx, STREAMING.prefix_of(&id))?.is_empty();
        if (row.asking != Asking::Idle && before) || listened {
            wakes.push(wake(id));
        }
    }
    let next = since.saturating_add(1);
    all(wakes
        .iter()
        .map(|op| ctx.schedule(next, ctx.env().program.clone(), op)))
    .await;
    Ok(Some(since))
}

fn still_streaming(ctx: &QueryCtx, row: &ThreadRow) -> Result<bool, Error> {
    let asked = matches!(row.asking, Asking::Asked { .. });
    let whole = PARTIALS
        .get(ctx, &row.id)?
        .is_some_and(|partial| streamed(&partial));
    if row.closed || !asked || whole {
        return Ok(false);
    }
    held_by_egress(ctx, row.id)
}

fn held_by_egress(ctx: &QueryCtx, id: u64) -> Result<bool, Error> {
    delivered(ctx, &streamed_as(id))
}

fn delivered(ctx: &QueryCtx, deliver: &[u8]) -> Result<bool, Error> {
    use abi::role::validators::{Query, Reply};
    let validators = ctx.env().role(abi::role::VALIDATORS)?.clone();
    let members = match ctx.query::<Query, Reply>(&validators, &Query::Members)? {
        Reply::Members(members) => members,
        other => return Err(unexpected_reply(&validators, "Members", &other)),
    };
    let program = bound(ctx)?;
    let ours = egress::Delivery::Events {
        deliver: deliver.to_vec(),
    };
    for member in members {
        let asked = egress::Query::Assigned { key: member.key };
        let rows = match ctx.query(&program, &asked)? {
            egress::Reply::Requests(rows) => rows,
            other => return Err(unexpected_reply(&program, "Assigned", &other)),
        };
        let held = rows.iter().any(|row| row.delivery == ours);
        if held {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) async fn recover(ctx: &Ctx, id: u64) -> Result<(), Error> {
    let since = switched(ctx).await?;
    lost_writes(ctx, id).await?;
    lost_streams(ctx, id, since).await?;
    let row = thread(ctx, id)?;
    if row.asking == Asking::Idle {
        return Ok(());
    }
    let flight = FLIGHTS.get(ctx, &id)?;
    let height = ctx.env().height;
    let lapsed = flight
        .as_ref()
        .is_none_or(|flight| height > deadline(flight));
    let switch = since.filter(|since| flight.as_ref().is_some_and(|flight| flight.made < *since));
    let cut = match (lapsed, switch) {
        (true, _) => true,
        (false, Some(_)) => !still_streaming(ctx, &row)?,
        (false, None) => false,
    };
    if !cut {
        return Ok(());
    }
    let text = match (&flight, switch) {
        (Some(flight), Some(since)) if !lapsed => format!(
            "the model call made at block {} was cut off: harness's code changed by block {since}",
            flight.made
        ),
        (Some(flight), _) => format!(
            "the model call made at block {} was cut off: harness heard no answer by block {}",
            flight.made,
            deadline(flight)
        ),
        (None, _) => "the model call was cut off by a harness update".to_owned(),
    };
    match row.asking {
        Asking::Idle => Ok(()),
        Asking::Learning | Asking::Compacting { .. } => {
            let mut row = row;
            settle(ctx, &mut row)?;
            THREADS.put(ctx, &id, &row);
            again(ctx, id, Failure::lost(text)).await
        }
        Asking::Asked { upto } => {
            let config = match flight.and_then(|flight| flight.config) {
                Some(config) => config,
                None => configured_now(ctx, &row)?,
            };
            let whole = crate::replies::read(ctx, id)?.is_some_and(|partial| streamed(&partial));
            let read = move |partial: Option<Partial>| match partial.filter(streamed) {
                Some(partial) => Ok(wire::folded(partial)),
                None => Err(Failure::lost(text)),
            };
            conclude(ctx, id, upto, config, read).await?;
            if whole {
                let line = "its model's reply had streamed whole when the call was cut off; harness recorded it and placed its calls";
                tell(ctx, &row, line.to_owned(), "its model call was lost").await?;
            }
            Ok(())
        }
        Asking::Running => rerun(ctx, id, flight.and_then(|flight| flight.config)).await,
    }
}

pub(crate) fn streamed(partial: &Partial) -> bool {
    matches!(partial.flow, Flow::Ended | Flow::Overflowed)
}

fn configured_now(ctx: &QueryCtx, row: &ThreadRow) -> Result<BlobId, Error> {
    let agent = harnessed_agent(ctx, row.agent)?;
    let path = agent.map(|agent| agent.path).unwrap_or_default();
    let snapshot = configured(ctx, &path)?
        .config()
        .map(|(snapshot, _)| snapshot);
    snapshot.ok_or_else(|| {
        wrong_state(format!(
            "thread {}'s model call was cut off, and its agent's configuration {path} can't be read",
            row.id
        ))
    })
}

async fn rerun(ctx: &Ctx, id: u64, config: Option<BlobId>) -> Result<(), Error> {
    let cut: Vec<String> = CALLS
        .scan(ctx, CALLS.prefix_of(&id))?
        .into_iter()
        .filter(|(_, call)| call.state == CallState::Relaying)
        .map(|(_, call)| call.id)
        .collect();
    let mut row = thread(ctx, id)?;
    if cut.is_empty() {
        if row.open_calls == 0 {
            settle(ctx, &mut row)?;
            THREADS.put(ctx, &id, &row);
        }
        return Ok(());
    }
    watch(ctx, id, None, config).await;
    let line = format!(
        "the relays of {} placed tool calls were cut off; harness relays them again",
        cut.len()
    );
    tell(ctx, &row, line, "its tool calls' relays were lost").await?;
    let runs = cut.into_iter().map(|call| run_call(ctx, id, 0, call));
    for ran in all(runs).await {
        ran?;
    }
    Ok(())
}

async fn tell(ctx: &Ctx, row: &ThreadRow, line: String, why: &str) -> Result<(), Error> {
    let line = format!(
        "thread {} ({}, agent {}): {line}",
        row.id, row.title, row.agent
    );
    REPORTS.put(ctx, &(row.manager, row.id), &line);
    if !REPORTING.has(ctx, &row.manager) {
        let report = Op::Report {
            manager: row.manager,
        };
        let next = ctx.env().height.saturating_add(1);
        match ctx.schedule(next, &ctx.env().program, &report).await {
            Ok(_) => REPORTING.insert(ctx, &row.manager),
            Err(_) => {
                ctx.emit(&ctx.env().program, &report, ::program::Reply::None);
            }
        }
    }
    if let Some(place) = binding(ctx, row)? {
        post(ctx, row, &place, why).await;
    }
    Ok(())
}

async fn post(ctx: &Ctx, row: &ThreadRow, place: &Place, why: &str) {
    let height = ctx.env().height;
    let thread = match place {
        Place::Channel { .. } => None,
        Place::Thread { root, .. } => Some(*root),
    };
    let agent = row.agent.to_string();
    let mut tail = vec![agent.as_str()];
    tail.extend(row.place.split('/').filter(|part| !part.is_empty()));
    let concerns = match running(ctx, row.manager) {
        Ok(Some(_)) => Vec::new(),
        _ => vec![row.manager],
    };
    let op = chat::Op::Notify {
        channel_id: place.channel().to_owned(),
        message_id: format!("harness-resume-{}-{height}", row.id),
        thread,
        notice: chat::Notice {
            kind: "turn".into(),
            group: format!("resumed {}", row.id),
            subject: format!("conversation {}", row.id),
            link: ducklink::path(&ctx.env().program, &tail),
            status: chat::message::Status::Note,
            detail: format!("turn resumed after block {height} ({why})"),
            concerns,
        },
    };
    let _ = ctx.call_raw(chat::PROGRAM, abi::encode(&op)).await;
}

pub(crate) async fn resume(ctx: Ctx, id: u64) -> Result<(), Error> {
    thread(&ctx, id)?;
    rewatch(&ctx, id).await;
    repace(&ctx, id).await;
    let_go(&ctx, id);
    reask(&ctx, id)?;
    converse(&ctx, id).await;
    Ok(())
}

fn let_go(ctx: &ExecCtx, id: u64) {
    if !matches!(ctx.env().cause, Cause::Scheduled { .. }) {
        RETRIES.remove(ctx, &id);
    }
}

fn reask(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    let env = ctx.env();
    if matches!(env.cause, Cause::Scheduled { .. }) {
        return Ok(());
    }
    let mut row = thread(ctx, id)?;
    let idle = !row.closed && row.asking == Asking::Idle && row.last_input > 0;
    if !idle || row.due() || RETRIES.get(ctx, &id)?.is_some() {
        return Ok(());
    }
    let since = Range::new(FRAMES.key(&(id, row.last_input)), FRAMES.prefix_of(&id).end);
    let answered = FRAMES
        .scan(ctx, since)?
        .into_iter()
        .any(|(_, frame)| matches!(frame.body, Body::Turn { .. }));
    if answered {
        return Ok(());
    }
    let by = match &env.sender {
        Some(Principal::Account(number)) => format!("account {number}"),
        Some(Principal::Foreign(foreign)) => crate::agents::foreign_handle(ctx, *foreign),
        Some(Principal::Root) | None => "the network".to_owned(),
    };
    row.seen = row.last_input - 1;
    let text = format!("re-asked by {by} after a turn that gave up");
    append(ctx, &mut row, Body::Notice { text });
    THREADS.put(ctx, &id, &row);
    Ok(())
}

async fn rewatch(ctx: &Ctx, id: u64) {
    let Ok(Some(flight)) = FLIGHTS.get(ctx, &id) else {
        return;
    };
    let own = matches!(ctx.env().cause, Cause::Scheduled { entry } if Some(entry) == flight.watch);
    let after = deadline(&flight).saturating_add(1);
    if !(own && after > ctx.env().height) {
        return;
    }
    let watch: Option<EntryId> = ctx
        .schedule(after, &ctx.env().program, &wake(id))
        .await
        .ok();
    FLIGHTS.put(ctx, &id, &Flight { watch, ..flight });
}

pub(crate) async fn report(ctx: Ctx, manager: AccountNumber) -> Result<(), Error> {
    REPORTING.remove(&ctx, &manager);
    let mut lines = Vec::new();
    for (key, line) in REPORTS.scan(&ctx, REPORTS.prefix_of(&manager))? {
        REPORTS.remove(&ctx, &key);
        lines.push(format!("- {line}"));
    }
    let Some(agent) = running(&ctx, manager)?.filter(|_| !lines.is_empty()) else {
        return Ok(());
    };
    let mut row = conversation(&ctx, &agent, "")?;
    let turns = match lines.len() {
        1 => "a turn".to_owned(),
        count => format!("{count} turns"),
    };
    let text = format!(
        "harness picked up {turns} a node restart or a harness update cut off:\n{}",
        lines.join("\n")
    );
    let author = own_account(&ctx)?;
    say(&ctx, &mut row, Body::Post { author, text });
    THREADS.put(&ctx, &row.id, &row);
    converse(&ctx, row.id).await;
    Ok(())
}

fn own_account(ctx: &QueryCtx) -> Result<Principal, Error> {
    use abi::role::identity::{Query, Reply};
    let identity = &ctx.env().role(abi::role::IDENTITY)?.clone();
    let asked = Query::OfProgram(ctx.env().program.clone());
    match ctx.query::<Query, Reply>(identity, &asked)? {
        Reply::Account(Some(number)) => Ok(Principal::Account(number)),
        Reply::Account(None) => Ok(Principal::Root),
        other => Err(unexpected_reply(identity, "OfProgram", &other)),
    }
}
