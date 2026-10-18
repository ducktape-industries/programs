//! Turns a node restart or a harness update cut off. Each model call keeps egress's own deadline,
//! and a run harness schedules past it records a reply that streamed whole, relays lost calls
//! again or asks the model again; a failed call is asked again later, and its manager hears.
use ::program::{BlobId, Cause, Ctx, EntryId, Error, ExecCtx, Principal, QueryCtx, all, code};
use ::program::{unexpected_reply, wrong_state};
use abi::role::egress::{DEFAULT_WAIT, http};
use abi::role::identity::AccountNumber;

use crate::agents::{configured, harnessed_agent, hear_again, running, thread};
use crate::calls::{Settled, refusal, run_call};
use crate::conversations::{binding, conversation};
use crate::state::{
    CALLS, FLIGHTS, Flight, PARTIALS, REPORTING, REPORTS, RETRIES, Retry, THREADS, USAGE,
};
use crate::turns::{append, conclude, converse, give_up, say, settle};
use crate::wire;
use crate::{Asking, Body, CallState, Flow, Op, PROGRAM, Partial, Place, ThreadRow, UsageWindow};

const RETRY_AFTER: &str = "retry-after";
const UTILIZATION: &str = "anthropic-ratelimit-unified-5h-utilization";
const RESET: &str = "anthropic-ratelimit-unified-5h-reset";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Again {
    Lost,
    Busy,
}

// what a busy model asked for: seconds from now (`retry-after`), or a time (its usage window's
// reset, in seconds since the epoch)
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
}

impl Failure {
    fn new(text: impl Into<String>, again: Option<Again>) -> Failure {
        Failure {
            text: text.into(),
            again,
            wait: None,
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
            ..Failure::busy(text)
        }
    }

    // egress's refusal: only what never succeeds unchanged is final (docs/programs.md, section 4)
    pub fn unreached(error: &Error) -> Failure {
        let text = format!("the model was not reached: {}", refusal(error));
        let unchanged = error.code == code::INVALID_INPUT || error.code == code::UNAUTHORIZED;
        match unchanged {
            true => Failure::last(text),
            false => Failure::lost(text),
        }
    }
}

fn header<'a>(response: &'a http::Response, name: &str) -> Option<&'a str> {
    let found = response
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name));
    found.map(|(_, value)| value.trim())
}

// every answer names how much of the model's usage window is spent and when it resets: the last
// one stands, for whatever paces turns by it
pub(crate) fn window_seen(ctx: &ExecCtx, service: &str, response: &http::Response) {
    let utilization = header(response, UTILIZATION);
    let reset = header(response, RESET).and_then(|value| value.parse().ok());
    let (Some(utilization), Some(reset)) = (utilization, reset) else {
        return;
    };
    let window = UsageWindow {
        utilization: utilization.to_owned(),
        reset,
        height: ctx.env().height,
    };
    USAGE.put(ctx, &service.to_owned(), &window);
}

fn deadline(flight: &Flight) -> u64 {
    flight.made.max(flight.moved).saturating_add(flight.wait)
}

fn wake(id: u64) -> Op {
    Op::Resume { thread: id }
}

// egress answers a call by the end of its deadline's block, so a run after it that still finds
// the call open knows its frame is gone; with no scheduler, the thread's next input finds it
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
    flight.watch = ctx.schedule(after, PROGRAM, &wake(id)).await.ok();
    FLIGHTS.put(ctx, &id, &flight);
}

// egress moves a streaming call's deadline past each block that carries its events
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

// new input asks at once, however long the wait between attempts has grown; a time the model
// asked for still holds, as asking before it only spends a call
pub(crate) fn heard(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    let Some(retry) = RETRIES.get(ctx, &id)? else {
        return Ok(());
    };
    let at = ctx.env().height;
    RETRIES.put(ctx, &id, &Retry { at, ..retry });
    Ok(())
}

pub(crate) fn answered(ctx: &ExecCtx, id: u64) {
    RETRIES.remove(ctx, &id);
}

pub(crate) async fn again(ctx: &Ctx, id: u64, failure: Failure) -> Result<(), Error> {
    let mut row = thread(ctx, id)?;
    let Some(why) = failure.again.filter(|_| !row.closed) else {
        give_up(ctx, &mut row, failure.text);
        THREADS.put(ctx, &id, &row);
        return Ok(());
    };
    let env = ctx.env();
    let (height, now) = (env.height, env.time);
    let attempt = RETRIES
        .get(ctx, &id)?
        .map_or(1, |retry| retry.attempt.saturating_add(1));
    let until = failure.wait.and_then(|wait| wait.until(now));
    // the wait doubles from one block, with no last attempt; a time the model named is waited
    // out from the next block on, by the pace the chain keeps
    let gap = match until {
        Some(_) => 1,
        None => 1u64.checked_shl(attempt - 1).unwrap_or(u64::MAX),
    };
    let at = height.saturating_add(gap);
    let entry = match ctx.schedule(at, PROGRAM, &wake(id)).await {
        Ok(entry) => entry,
        Err(error) => {
            let text = format!(
                "{}; asking the model again needs the scheduler, which refused ({}), so the next input asks",
                failure.text,
                refusal(&error)
            );
            give_up(ctx, &mut row, text);
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
        None => format!("at block {at}"),
    };
    let text = format!(
        "{}; harness asks the model again {when} (attempt {attempt})",
        failure.text
    );
    append(ctx, &mut row, Body::Notice { text });
    THREADS.put(ctx, &id, &row);
    let first_lost = why == Again::Lost && attempt == 1;
    if first_lost {
        let line = format!("{}; it asks the model again {when}", failure.text);
        tell(ctx, &row, line, "egress failed").await?;
    }
    Ok(())
}

// the scheduler wakes by height and the model's wait ends in time: the pace the chain kept since
// the wait began says how many blocks the rest of it takes, never fewer than one
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
    let paced = match ctx.schedule(at, PROGRAM, &wake(id)).await {
        Ok(entry) => Retry {
            at,
            entry: Some(entry),
            ..retry
        },
        // with no wake left the wait can't be kept: the next input asks
        Err(_) => Retry {
            until: None,
            entry: None,
            ..retry
        },
    };
    RETRIES.put(ctx, &id, &paced);
}

pub(crate) async fn recover(ctx: &Ctx, id: u64) -> Result<(), Error> {
    let row = thread(ctx, id)?;
    if row.asking == Asking::Idle {
        return Ok(());
    }
    let flight = FLIGHTS.get(ctx, &id)?;
    let height = ctx.env().height;
    // a call made before harness kept flights was cut off by the update that brought this code
    let cut = flight
        .as_ref()
        .is_none_or(|flight| height > deadline(flight));
    if !cut {
        return Ok(());
    }
    let text = match &flight {
        Some(flight) => format!(
            "the model call made at block {} was cut off: harness heard no answer by block {}",
            flight.made,
            deadline(flight)
        ),
        None => "the model call was cut off by a harness update".to_owned(),
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
            let whole = PARTIALS
                .get(ctx, &id)?
                .is_some_and(|partial| streamed(&partial));
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

fn streamed(partial: &Partial) -> bool {
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

// a relay egress lost never let its call run, so relaying it again runs it once; a write still
// out is the awaited program's to answer
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
    // the turn is recorded, so no live place reads the index
    let runs = cut.into_iter().map(|call| run_call(ctx, id, 0, call));
    let mut settled = Settled::default();
    for ran in all(runs).await {
        settled = settled.and(ran?);
    }
    hear_again(ctx, settled.hearings);
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
        match ctx.schedule(next, PROGRAM, &report).await {
            Ok(_) => REPORTING.insert(ctx, &row.manager),
            Err(_) => {
                ctx.emit(PROGRAM, &report, ::program::Reply::None);
            }
        }
    }
    if let Some(place) = binding(ctx, row)? {
        post(ctx, row, &place, why).await;
    }
    Ok(())
}

// one line in the room a bound conversation answers in, for whoever watches it; a room that
// refuses it loses nothing, as the thread's notice and the manager's line say the same
async fn post(ctx: &Ctx, row: &ThreadRow, place: &Place, why: &str) {
    let height = ctx.env().height;
    let thread = match place {
        Place::Channel { .. } => None,
        Place::Thread { root, .. } => Some(*root),
    };
    let text = format!(
        "harness: conversation {}'s turn resumed after block {height} ({why})",
        row.id
    );
    let op = chat::Op::PostMessage {
        channel_id: place.channel().to_owned(),
        message_id: format!("harness-resume-{}-{height}", row.id),
        blocks: chat::parse_message(&text),
        thread,
    };
    let _ = ctx.call_raw(chat::PROGRAM, abi::encode(&op)).await;
}

pub(crate) async fn resume(ctx: Ctx, id: u64) -> Result<(), Error> {
    thread(&ctx, id)?;
    rewatch(&ctx, id).await;
    repace(&ctx, id).await;
    converse(&ctx, id).await;
    Ok(())
}

// a stream still coming moved egress's deadline past this run: watch again from there
async fn rewatch(ctx: &Ctx, id: u64) {
    let Ok(Some(flight)) = FLIGHTS.get(ctx, &id) else {
        return;
    };
    let own = matches!(ctx.env().cause, Cause::Scheduled { entry } if Some(entry) == flight.watch);
    let after = deadline(&flight).saturating_add(1);
    if !(own && after > ctx.env().height) {
        return;
    }
    let watch: Option<EntryId> = ctx.schedule(after, PROGRAM, &wake(id)).await.ok();
    FLIGHTS.put(ctx, &id, &Flight { watch, ..flight });
}

pub(crate) async fn report(ctx: Ctx, manager: AccountNumber) -> Result<(), Error> {
    REPORTING.remove(&ctx, &manager);
    let mut lines = Vec::new();
    for (key, line) in REPORTS.scan(&ctx, REPORTS.prefix_of(&manager))? {
        REPORTS.remove(&ctx, &key);
        lines.push(format!("- {line}"));
    }
    // a person holds no conversation here: each thread's notice is the record
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
    let asked = Query::OfProgram(PROGRAM.into());
    match ctx.query::<Query, Reply>(identity, &asked)? {
        Reply::Account(Some(number)) => Ok(Principal::Account(number)),
        Reply::Account(None) => Ok(Principal::Root),
        other => Err(unexpected_reply(identity, "OfProgram", &other)),
    }
}
