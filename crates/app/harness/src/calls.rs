//! A tool call's life: placed and relayed once its block of the model's reply is whole, run as the
//! agent when its relay comes back, its result held until its turn is recorded. A streamable call
//! shows its input as it arrives; what it showed is taken back if its reply starts over or breaks.
use ::program::{Ctx, Error, ExecCtx};
use abi::role::egress::{self, RELAY};
use attribution::Change;
use serde_json::Value;

use crate::agents::{running, thread};
use crate::state::{CALLS, DEFERRED, LIVE, Live, PARTIALS, SHOWN, THREADS};
use crate::tools::prefix::prefix;
use crate::tools::{self, Entry, Run, Seat, Show, Write};
use crate::turns::{append, request, serves, settle};
use crate::{Asking, Body, CallRow, CallState, Said, ThreadRow};

pub(crate) fn refusal(error: &Error) -> String {
    format!("{}: {}", error.code, error.message)
}

#[derive(Default)]
pub(crate) struct Settled {
    pub carry_on: bool,
    pub hearings: Vec<Change>,
}

impl Settled {
    pub fn and(mut self, other: Settled) -> Settled {
        self.carry_on |= other.carry_on;
        self.hearings.extend(other.hearings);
        self
    }
}

fn call_id_fits(call: &str) -> bool {
    !call.is_empty()
        && call
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn planned(
    ctx: &ExecCtx,
    thread: &ThreadRow,
    call: &str,
    tool: &str,
    input: &str,
) -> Result<(), String> {
    if !call_id_fits(call) {
        return Err("a call id is letters, digits, _ and -".to_owned());
    }
    let made_before = CALLS
        .get(ctx, &(thread.id, call.to_owned()))
        .map_err(|error| error.message)?
        .is_some_and(|row| row.state != CallState::Showing);
    if made_before {
        return Err(format!("call {call} was made before"));
    }
    let entry = tools::find(tool).ok_or_else(|| format!("there is no tool named {tool}"))?;
    let input: Value =
        serde_json::from_str(input).map_err(|error| format!("the input is not JSON: {error}"))?;
    (entry.check)(&input)?;
    let relaying = serves(ctx, RELAY).map_err(|error| error.message)?;
    if !relaying {
        return Err(format!(
            "no member node serves {RELAY}, so no tool call can run"
        ));
    }
    Ok(())
}

pub(crate) enum Placed {
    Refused(Body),
    Relaying,
}

pub(crate) fn place(
    ctx: &ExecCtx,
    thread: &mut ThreadRow,
    call: &str,
    tool: &str,
    input: &str,
) -> Result<Placed, Error> {
    let key = (thread.id, call.to_owned());
    let showing = CALLS
        .get(ctx, &key)?
        .filter(|row| row.state == CallState::Showing);
    if let Err(problem) = planned(ctx, thread, call, tool, input) {
        let refused = Body::Refused {
            call: call.to_owned(),
            tool: tool.to_owned(),
            problem,
        };
        return Ok(Placed::Refused(refused));
    }
    let row = match showing {
        Some(row) => CallRow {
            input: input.to_owned(),
            state: CallState::Relaying,
            ..row
        },
        None => {
            thread.open_calls += 1;
            CallRow {
                thread: thread.id,
                id: call.to_owned(),
                tool: tool.to_owned(),
                input: input.to_owned(),
                made: ctx.env().height,
                state: CallState::Relaying,
            }
        }
    };
    CALLS.put(ctx, &key, &row);
    Ok(Placed::Relaying)
}

fn mark(ctx: &ExecCtx, id: u64, call: &str, state: CallState) -> Result<(), Error> {
    let key = (id, call.to_owned());
    if let Some(mut row) = CALLS.get(ctx, &key)? {
        row.state = state;
        CALLS.put(ctx, &key, &row);
    }
    Ok(())
}

fn shown(ctx: &ExecCtx, id: u64, call: &str) -> Result<Option<Value>, Error> {
    let text = SHOWN.get(ctx, &(id, call.to_owned()))?;
    Ok(text.and_then(|text| serde_json::from_str(&text).ok()))
}

async fn written(ctx: &Ctx, agent: u64, writes: Vec<Write>) -> Result<Vec<u8>, Error> {
    let mut output = Vec::new();
    for write in writes {
        output = ctx.call_raw_as(agent, write.target, write.payload).await?;
    }
    Ok(output)
}

enum TakenBack {
    NothingShown,
    Undone,
    Refused(Error),
}

async fn take_back(ctx: &Ctx, thread: &ThreadRow, row: &CallRow) -> Result<TakenBack, Error> {
    let Some(entry) = tools::find(&row.tool).filter(|entry| entry.streams) else {
        return Ok(TakenBack::NothingShown);
    };
    let Some(shown) = shown(ctx, thread.id, &row.id)? else {
        return Ok(TakenBack::NothingShown);
    };
    let seat = Seat {
        thread: thread.id,
        agent: thread.agent,
        call: &row.id,
        shown: Some(&shown),
    };
    let writes = (entry.void)(ctx, &seat)?;
    SHOWN.remove(ctx, &(thread.id, row.id.clone()));
    Ok(match written(ctx, thread.agent, writes).await {
        Ok(_) => TakenBack::Undone,
        Err(error) => TakenBack::Refused(error),
    })
}

fn notice(ctx: &ExecCtx, id: u64, text: String) -> Result<(), Error> {
    let mut thread = thread(ctx, id)?;
    append(ctx, &mut thread, Body::Notice { text });
    THREADS.put(ctx, &id, &thread);
    Ok(())
}

fn drop_hearings(ctx: &ExecCtx, id: u64, call: &str) -> Result<(), Error> {
    for (key, _) in DEFERRED.scan(ctx, DEFERRED.prefix_of(&(id, call.to_owned())))? {
        DEFERRED.remove(ctx, &key);
    }
    Ok(())
}

fn hearings(ctx: &ExecCtx, id: u64, call: &str) -> Result<Vec<Change>, Error> {
    let deferred = DEFERRED.scan(ctx, DEFERRED.prefix_of(&(id, call.to_owned())))?;
    Ok(deferred
        .into_iter()
        .map(|(key, change)| {
            DEFERRED.remove(ctx, &key);
            change
        })
        .collect())
}

async fn taken_back(ctx: &Ctx, id: u64, call: &str, why: &str) -> Result<(), Error> {
    let owner = thread(ctx, id)?;
    let Some(row) = CALLS.get(ctx, &(id, call.to_owned()))? else {
        return Ok(());
    };
    drop_hearings(ctx, id, call)?;
    let words = match take_back(ctx, &owner, &row).await? {
        TakenBack::NothingShown => "it did not run".to_owned(),
        TakenBack::Undone => "what it showed was taken back".to_owned(),
        TakenBack::Refused(error) => {
            format!("taking back what it showed failed: {}", refusal(&error))
        }
    };
    let tool = &row.tool;
    notice(
        ctx,
        id,
        format!("{why}, so {tool} call {call} did not finish: {words}"),
    )
}

fn voided(ctx: &ExecCtx, id: u64, call: &str) -> Result<(), Error> {
    let height = ctx.env().height;
    mark(ctx, id, call, CallState::Voided { height })
}

fn release(ctx: &ExecCtx, id: u64) -> Result<Settled, Error> {
    let mut thread = thread(ctx, id)?;
    thread.open_calls = thread.open_calls.saturating_sub(1);
    let recorded = thread.asking == Asking::Running;
    let carry_on = recorded && thread.open_calls == 0;
    if carry_on {
        settle(ctx, &mut thread)?;
    }
    THREADS.put(ctx, &id, &thread);
    Ok(Settled {
        carry_on,
        hearings: Vec::new(),
    })
}

pub(crate) async fn void_reply(ctx: &Ctx, id: u64) -> Result<Settled, Error> {
    let why = "the model's reply started over";
    let mut settled = Settled::default();
    for ((_, index), live) in LIVE.scan(ctx, LIVE.prefix_of(&id))? {
        LIVE.remove(ctx, &(id, index));
        let Some(row) = CALLS.get(ctx, &(id, live.call.clone()))? else {
            continue;
        };
        match row.state {
            CallState::Showing => {
                taken_back(ctx, id, &row.id, why).await?;
                voided(ctx, id, &row.id)?;
                settled = settled.and(release(ctx, id)?);
            }
            CallState::Relaying | CallState::Writing => {
                taken_back(ctx, id, &row.id, why).await?;
                voided(ctx, id, &row.id)?;
            }
            CallState::Done { .. } => {
                let result = live.outcome.map(|body| body.text()).unwrap_or_default();
                let streams = tools::find(&row.tool).is_some_and(|entry| entry.streams);
                match streams {
                    true => taken_back(ctx, id, &row.id, why).await?,
                    false => notice(
                        ctx,
                        id,
                        format!(
                            "{why} after {} call {} ran, and what it did stays: {result}",
                            row.tool, row.id
                        ),
                    )?,
                }
                voided(ctx, id, &row.id)?;
            }
            CallState::Voided { .. } => {}
        }
    }
    Ok(settled)
}

pub(crate) async fn take_back_showing(ctx: &Ctx, id: u64) -> Result<Settled, Error> {
    let why = "the model's reply ended before its input was whole";
    let mut settled = Settled::default();
    for ((_, index), live) in LIVE.scan(ctx, LIVE.prefix_of(&id))? {
        let showing = CALLS
            .get(ctx, &(id, live.call.clone()))?
            .is_some_and(|row| row.state == CallState::Showing);
        if !showing {
            continue;
        }
        LIVE.remove(ctx, &(id, index));
        taken_back(ctx, id, &live.call, why).await?;
        voided(ctx, id, &live.call)?;
        settled = settled.and(release(ctx, id)?);
    }
    Ok(settled)
}

fn said_at(ctx: &ExecCtx, id: u64, index: u64) -> Result<Option<(String, String, String)>, Error> {
    let Some(partial) = PARTIALS.get(ctx, &id)? else {
        return Ok(None);
    };
    let said = usize::try_from(index)
        .ok()
        .and_then(|index| partial.said.get(index).cloned());
    Ok(match said {
        Some(Said::ToolUse { id, name, input }) => Some((id, name, input)),
        Some(Said::Text(_) | Said::Thinking { .. } | Said::RedactedThinking(_)) | None => None,
    })
}

enum Showable {
    Fresh,
    Showing,
    Not,
}

fn showable(ctx: &ExecCtx, thread: &ThreadRow, call: &str) -> Result<Showable, Error> {
    if !call_id_fits(call) {
        return Ok(Showable::Not);
    }
    Ok(match CALLS.get(ctx, &(thread.id, call.to_owned()))? {
        None => Showable::Fresh,
        Some(row) if row.state == CallState::Showing => Showable::Showing,
        Some(_) => Showable::Not,
    })
}

pub(crate) async fn show(ctx: &Ctx, id: u64, index: u64) -> Result<Settled, Error> {
    let Some((call, tool, input)) = said_at(ctx, id, index)? else {
        return Ok(Settled::default());
    };
    let Some(entry) = tools::find(&tool).filter(|entry| entry.streams) else {
        return Ok(Settled::default());
    };
    let owner = thread(ctx, id)?;
    let runs = running(ctx, owner.agent)?.is_some();
    if owner.closed || !runs {
        return Ok(Settled::default());
    }
    let showable = showable(ctx, &owner, &call)?;
    if let Showable::Not = showable {
        return Ok(Settled::default());
    }
    let before = shown(ctx, id, &call)?;
    let seat = Seat {
        thread: id,
        agent: owner.agent,
        call: &call,
        shown: before.as_ref(),
    };
    let Ok(Show::Write { writes, shown }) = (entry.show)(ctx, &seat, &prefix(&input)) else {
        return Ok(Settled::default());
    };
    if let Showable::Fresh = showable {
        opened_showing(ctx, id, index, &call, &tool, &input)?;
    }
    SHOWN.put(ctx, &(id, call.clone()), &shown.to_string());
    match written(ctx, owner.agent, writes).await {
        Ok(_) => Ok(Settled::default()),
        Err(error) => finish(ctx, id, index, &call, refusal(&error), true),
    }
}

fn opened_showing(
    ctx: &ExecCtx,
    id: u64,
    index: u64,
    call: &str,
    tool: &str,
    input: &str,
) -> Result<(), Error> {
    let mut owner = thread(ctx, id)?;
    owner.open_calls += 1;
    THREADS.put(ctx, &id, &owner);
    let row = CallRow {
        thread: id,
        id: call.to_owned(),
        tool: tool.to_owned(),
        input: input.to_owned(),
        made: ctx.env().height,
        state: CallState::Showing,
    };
    CALLS.put(ctx, &(id, call.to_owned()), &row);
    let live = Live {
        call: call.to_owned(),
        outcome: None,
    };
    LIVE.put(ctx, &(id, index), &live);
    Ok(())
}

pub(crate) async fn dispatch(ctx: &Ctx, id: u64, index: u64) -> Result<Settled, Error> {
    let Some((call, tool, input)) = said_at(ctx, id, index)? else {
        return Ok(Settled::default());
    };
    let finished = LIVE
        .get(ctx, &(id, index))?
        .is_some_and(|live| live.outcome.is_some());
    if finished {
        return Ok(Settled::default());
    }
    let mut owner = thread(ctx, id)?;
    let placed = place(ctx, &mut owner, &call, &tool, &input)?;
    THREADS.put(ctx, &id, &owner);
    match placed {
        Placed::Refused(body) => {
            let showing = CALLS
                .get(ctx, &(id, call.clone()))?
                .is_some_and(|row| row.state == CallState::Showing);
            if showing {
                taken_back(ctx, id, &call, "its input was refused").await?;
                let outcome = Some(body.clone());
                LIVE.put(
                    ctx,
                    &(id, index),
                    &Live {
                        call: call.clone(),
                        outcome,
                    },
                );
                return finish_refused(ctx, id, &call);
            }
            let live = Live {
                call,
                outcome: Some(body),
            };
            LIVE.put(ctx, &(id, index), &live);
            Ok(Settled::default())
        }
        Placed::Relaying => {
            let live = Live {
                call: call.clone(),
                outcome: None,
            };
            LIVE.put(ctx, &(id, index), &live);
            run_call(ctx, id, index, call).await
        }
    }
}

fn finish_refused(ctx: &ExecCtx, id: u64, call: &str) -> Result<Settled, Error> {
    let height = ctx.env().height;
    mark(
        ctx,
        id,
        call,
        CallState::Done {
            height,
            failed: true,
        },
    )?;
    release(ctx, id)
}

fn is_voided(ctx: &ExecCtx, id: u64, call: &str) -> Result<bool, Error> {
    Ok(CALLS
        .get(ctx, &(id, call.to_owned()))?
        .is_some_and(|row| matches!(row.state, CallState::Voided { .. })))
}

pub(crate) async fn run_call(
    ctx: &Ctx,
    id: u64,
    index: u64,
    call: String,
) -> Result<Settled, Error> {
    let note = call.as_bytes().to_vec();
    let relay = egress::Request::Relay { note };
    let relayed = request(ctx, relay, egress::Delivery::Whole, None).await;
    if is_voided(ctx, id, &call)? {
        return release(ctx, id);
    }
    let (text, failed) = match relayed {
        Ok(_) => perform(ctx, id, &call).await?,
        Err(error) => (format!("the relay failed: {}", refusal(&error)), true),
    };
    finish(ctx, id, index, &call, text, failed)
}

enum Ready {
    Closed,
    Stopped,
    Unknown,
    Runs(&'static Entry),
}

fn ready(thread: &ThreadRow, runs: bool, tool: &str) -> Ready {
    match (thread.closed, runs, tools::find(tool)) {
        (true, _, _) => Ready::Closed,
        (false, false, _) => Ready::Stopped,
        (false, true, None) => Ready::Unknown,
        (false, true, Some(entry)) => Ready::Runs(entry),
    }
}

async fn perform(ctx: &Ctx, id: u64, call: &str) -> Result<(String, bool), Error> {
    let owner = thread(ctx, id)?;
    let Some(row) = CALLS.get(ctx, &(id, call.to_owned()))? else {
        return Ok((format!("call {call} is not open"), true));
    };
    let runs = running(ctx, owner.agent)?.is_some();
    let entry = match ready(&owner, runs, &row.tool) {
        Ready::Closed => {
            let words = match take_back(ctx, &owner, &row).await? {
                TakenBack::NothingShown => "",
                TakenBack::Undone => ", and what it showed was taken back",
                TakenBack::Refused(_) => ", and taking back what it showed failed",
            };
            return Ok((
                format!("the thread closed before the call ran{words}"),
                true,
            ));
        }
        Ready::Stopped => {
            return Ok((
                "the agent stopped running here before the call ran: its script no longer calls harness, or it is not active".to_owned(),
                true,
            ));
        }
        Ready::Unknown => return Ok((format!("there is no tool named {}", row.tool), true)),
        Ready::Runs(entry) => entry,
    };
    let input: Value = serde_json::from_str(&row.input).unwrap_or(Value::Null);
    let before = shown(ctx, id, call)?;
    let seat = Seat {
        thread: id,
        agent: owner.agent,
        call,
        shown: before.as_ref(),
    };
    match (entry.run)(ctx, &seat, input) {
        Ok(Run::Done(value)) => Ok((value.to_string(), false)),
        Ok(Run::Write(writes)) => {
            mark(ctx, id, call, CallState::Writing)?;
            Ok(match written(ctx, owner.agent, writes).await {
                Ok(output) => {
                    keep_shown(ctx, id, call, entry, &row.input);
                    match (entry.render)(&output) {
                        Ok(value) => (value.to_string(), false),
                        Err(refused) => (refusal(&refused), true),
                    }
                }
                Err(error) => (refusal(&error), true),
            })
        }
        Err(error) => Ok((refusal(&error), true)),
    }
}

fn keep_shown(ctx: &ExecCtx, id: u64, call: &str, entry: &Entry, input: &str) {
    if entry.streams {
        SHOWN.put(ctx, &(id, call.to_owned()), &input.to_owned());
    }
}

fn finish(
    ctx: &ExecCtx,
    id: u64,
    index: u64,
    call: &str,
    text: String,
    failed: bool,
) -> Result<Settled, Error> {
    let key = (id, call.to_owned());
    let Some(mut row) = CALLS.get(ctx, &key)? else {
        return Ok(Settled::default());
    };
    let result = Body::Result {
        call: row.id.clone(),
        tool: row.tool.clone(),
        text,
        failed,
    };
    row.state = CallState::Done {
        height: ctx.env().height,
        failed,
    };
    CALLS.put(ctx, &key, &row);
    let mut owner = thread(ctx, id)?;
    match owner.asking {
        Asking::Asked { .. } => {
            let live = Live {
                call: call.to_owned(),
                outcome: Some(result),
            };
            LIVE.put(ctx, &(id, index), &live);
        }
        Asking::Idle | Asking::Learning | Asking::Compacting { .. } | Asking::Running => {
            SHOWN.remove(ctx, &key);
            append(ctx, &mut owner, result);
            THREADS.put(ctx, &id, &owner);
        }
    }
    let heard = match failed {
        true => {
            drop_hearings(ctx, id, call)?;
            Vec::new()
        }
        false => hearings(ctx, id, call)?,
    };
    let settled = release(ctx, id)?;
    Ok(Settled {
        hearings: heard,
        ..settled
    })
}

pub(crate) fn deferred_while_shown(ctx: &ExecCtx, change: &Change) -> Result<bool, Error> {
    let Some((id, call)) = tools::shown_by(&change.source) else {
        return Ok(false);
    };
    let showing = CALLS
        .get(ctx, &(id, call.clone()))?
        .is_some_and(|row| row.state == CallState::Showing);
    if showing {
        DEFERRED.put(ctx, &(id, call, change.seq), change);
    }
    Ok(showing)
}
