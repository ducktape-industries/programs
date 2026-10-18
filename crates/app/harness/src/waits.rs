//! Waits: one event an agent names (a block's height, a pod's end) wakes the conversation that
//! waits for it once, and the turn that armed it ends meanwhile. harness hears the event from the
//! scheduler or from the pods service's wait stream, never by asking again on a clock of its own.
use ::program::{
    Cause, Ctx, Error, ExecCtx, Principal, QueryCtx, corrupt, decoded, invalid, not_found,
    unauthorized, unexpected_reply, wrong_state,
};
use abi::role::egress::{DEFAULT_WAIT, Delivery, Event, Request, http};
use abi::role::identity::AccountNumber;
use abi::role::scheduler;
use serde_json::Value;
use store::{PageRequest, PageResponse};

use crate::agents::thread;
use crate::calls::refusal;
use crate::conversations::current;
use crate::state::{
    ARMED, ENDING, POD_WAITS, PodWatch, SCHEDULED, THREADS, WAIT_COUNT, WAIT_ENTRIES, WAITING,
    WAITS,
};
use crate::turns::{bound, converse, request, say};
use crate::{Asking, Body, Ended, Op, PROGRAM, ThreadRow, WaitFor, WaitRow, pod_event_as};

// the egress service the pods contract names
pub(crate) const PODS: &str = "pods";

pub(crate) fn what(event: &WaitFor) -> String {
    match event {
        WaitFor::Height { height } => format!("block {height}"),
        WaitFor::Pod { name } => format!("pod {name} to end"),
    }
}

fn kept(ctx: &QueryCtx, wait: u64) -> Result<WaitRow, Error> {
    WAITS
        .get(ctx, &wait)?
        .ok_or_else(|| not_found(format!("no wait {wait}")))
}

fn open(ctx: &QueryCtx, wait: u64) -> Result<Option<WaitRow>, Error> {
    Ok(WAITS.get(ctx, &wait)?.filter(|row| row.ended.is_none()))
}

fn by_harness(ctx: &QueryCtx) -> bool {
    ctx.env()
        .sending_program()
        .is_ok_and(|program| program == PROGRAM)
}

fn scheduler_of(ctx: &QueryCtx) -> Result<String, Error> {
    Ok(ctx.env().role(abi::role::SCHEDULER)?.clone())
}

async fn schedule(ctx: &Ctx, height: u64, op: &Op) -> Result<u64, Error> {
    let schedule = scheduler::Op::Schedule {
        height,
        target: PROGRAM.into(),
        payload: abi::encode(op),
    };
    let bound = scheduler_of(ctx)?;
    let answer = ctx.call_raw(bound.clone(), abi::encode(&schedule)).await?;
    abi::decode(&answer).map_err(|fault| unexpected_reply(&bound, "Schedule", &fault.sentence))
}

async fn unschedule(ctx: &Ctx, entry: u64) -> Result<(), Error> {
    let cancel = scheduler::Op::Cancel { entry };
    ctx.call_raw(scheduler_of(ctx)?, abi::encode(&cancel))
        .await?;
    Ok(())
}

pub(crate) async fn wait(ctx: &Ctx, thread_id: u64, event: WaitFor) -> Result<(), Error> {
    let agent = ctx.sender_account()?;
    let row = thread(ctx, thread_id)?;
    if row.agent != agent {
        return Err(invalid(format!(
            "conversation {thread_id} is not account {agent}'s"
        )));
    }
    if row.closed {
        return Err(wrong_state(format!("conversation {thread_id} is closed")));
    }
    let id = WAIT_COUNT.get(ctx)?.unwrap_or(0) + 1;
    WAIT_COUNT.put(ctx, &id);
    match &event {
        WaitFor::Height { height } => {
            let entry = schedule(ctx, *height, &Op::WaitDue).await?;
            WAIT_ENTRIES.put(ctx, &entry, &id);
            SCHEDULED.put(ctx, &id, &entry);
        }
        WaitFor::Pod { name } => {
            named_pod(name)?;
            watch_from_next_block(ctx, id).await?;
        }
    }
    let waiting = WaitRow {
        id,
        agent,
        thread: thread_id,
        made: ctx.env().height,
        event,
        ended: None,
    };
    WAITS.put(ctx, &id, &waiting);
    WAITING.insert(ctx, &(agent, id));
    ARMED.insert(ctx, &(thread_id, id));
    // the turn that armed it ends once its calls are back (`turns::settle`)
    let in_turn = thread(ctx, thread_id)?.asking != Asking::Idle;
    if in_turn {
        ENDING.insert(ctx, &thread_id);
    }
    ctx.set_return_data(abi::encode(&id));
    Ok(())
}

pub(crate) async fn unwait(ctx: &Ctx, wait: u64) -> Result<(), Error> {
    let row = kept(ctx, wait)?;
    let theirs = ctx.sender_account()? == row.agent;
    if !theirs {
        return Err(unauthorized(format!(
            "wait {wait} is account {}'s to cancel",
            row.agent
        )));
    }
    if let Some(ended) = &row.ended {
        return Err(wrong_state(format!(
            "wait {wait} ended at block {}",
            ended.height
        )));
    }
    if let Some(entry) = SCHEDULED.get(ctx, &wait)? {
        unschedule(ctx, entry).await?;
    }
    if let Some(PodWatch {
        entry: Some(entry), ..
    }) = POD_WAITS.get(ctx, &wait)?
    {
        unschedule(ctx, entry).await?;
    }
    disarm(ctx, &row)?;
    WAITS.remove(ctx, &wait);
    WAITING.remove(ctx, &(row.agent, wait));
    Ok(())
}

// what a wait holds besides its row: its place among its conversation's armed waits, its
// scheduler entry, its pod's watch
fn disarm(ctx: &ExecCtx, row: &WaitRow) -> Result<(), Error> {
    ARMED.remove(ctx, &(row.thread, row.id));
    if let Some(entry) = SCHEDULED.get(ctx, &row.id)? {
        WAIT_ENTRIES.remove(ctx, &entry);
        SCHEDULED.remove(ctx, &row.id);
    }
    POD_WAITS.remove(ctx, &row.id);
    Ok(())
}

// the wait ends, and its conversation (else the agent's canonical one) hears why, once
async fn fire(ctx: &Ctx, wait: u64, what: String) -> Result<(), Error> {
    let Some(mut row) = open(ctx, wait)? else {
        return Ok(());
    };
    row.ended = Some(Ended {
        height: ctx.env().height,
        text: what.clone(),
    });
    WAITS.put(ctx, &wait, &row);
    disarm(ctx, &row)?;
    let Some(mut told) = told(ctx, &row)? else {
        return Ok(());
    };
    let still = still_armed(ctx, told.id)?;
    let text = format!("Wait {wait} is over: {what}.{still}");
    let author = Principal::Root;
    say(ctx, &mut told, Body::Post { author, text });
    THREADS.put(ctx, &told.id, &told);
    converse(ctx, told.id).await;
    Ok(())
}

fn told(ctx: &QueryCtx, row: &WaitRow) -> Result<Option<ThreadRow>, Error> {
    match THREADS.get(ctx, &row.thread)?.filter(|told| !told.closed) {
        Some(told) => Ok(Some(told)),
        None => current(ctx, row.agent),
    }
}

pub(crate) async fn wait_due(ctx: &Ctx) -> Result<(), Error> {
    let (Cause::Scheduled { entry }, true) = (ctx.env().cause.clone(), by_harness(ctx)) else {
        return Err(unauthorized(
            "a wait for a height is due only as harness's own scheduled entry, at its block",
        ));
    };
    let Some(wait) = WAIT_ENTRIES.get(ctx, &entry)? else {
        return Ok(());
    };
    let height = ctx.env().height;
    fire(ctx, wait, format!("block {height} has come")).await
}

// a pod's name as Kubernetes takes one, so it stands in a path whole
fn named_pod(name: &str) -> Result<(), Error> {
    let fits = !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-.".contains(&byte)
        });
    match fits {
        true => Ok(()),
        false => Err(invalid(format!(
            "{name:?} is no pod's name: lowercase letters, digits, - and ."
        ))),
    }
}

async fn ask_pods(ctx: &Ctx, target: String, delivery: Delivery) -> Result<http::Response, Error> {
    let head = http::Head {
        method: http::Method::Get,
        target,
        headers: Vec::new(),
    };
    let asked = Request::Http {
        service: PODS.to_owned(),
        head,
        body: Vec::new(),
    };
    let answer = request(ctx, asked, delivery, None).await?;
    decoded(PROGRAM, "http::Response", &answer)
}

fn refused_by_pods(what: &str, answer: &http::Response) -> String {
    let error = serde_json::from_slice::<Value>(&answer.body)
        .ok()
        .and_then(|body| body["error"].as_str().map(str::to_owned))
        .unwrap_or_else(|| String::from_utf8_lossy(&answer.body).into_owned());
    format!("{what} answered {}: {error}", answer.status)
}

// the pod's stream opens in harness's own run the block after, so the block time is measured
// between the two
async fn watch_from_next_block(ctx: &Ctx, wait: u64) -> Result<(), Error> {
    let height = ctx.env().height;
    let entry = schedule(ctx, height + 1, &Op::PodWait { wait }).await?;
    let watch = PodWatch {
        height,
        time: ctx.env().time,
        entry: Some(entry),
    };
    POD_WAITS.put(ctx, &wait, &watch);
    Ok(())
}

// A pod's wait, in harness's own scheduled run: the pod read once, which measures the trip a
// request to the pods service takes to land, then its wait stream
// (`GET /v1/pods/NAME/wait?every=S`), whose `alive` events come as often as a request lasts
// (DEFAULT_WAIT blocks of the measured block time) less that trip, so each lands before the
// request it keeps alive expires. A stream that ends with the wait still open (a node restart,
// an expiry) opens again from the next block. Sent by anyone else, it opens the stream again
// from the next block: how a stream a harness update lost is opened again.
pub(crate) async fn pod_wait(ctx: &Ctx, wait: u64) -> Result<(), Error> {
    let Some(row) = open(ctx, wait)? else {
        return Err(wrong_state(format!("wait {wait} is not open")));
    };
    let WaitFor::Pod { name } = &row.event else {
        return Err(invalid(format!("wait {wait} waits for no pod")));
    };
    let Some(watch) = POD_WAITS.get(ctx, &wait)? else {
        let key = POD_WAITS.key(&wait);
        return Err(corrupt("pod wait", &key, "an open pod wait has no watch"));
    };
    let scheduled = match ctx.env().cause {
        Cause::Scheduled { entry } => Some(entry),
        _ => None,
    };
    match (by_harness(ctx), scheduled) {
        (true, Some(entry)) if watch.entry == Some(entry) => {}
        // a run a later one took the place of
        (true, Some(_)) => return Ok(()),
        _ => {
            if let Some(entry) = watch.entry {
                unschedule(ctx, entry).await?;
            }
            return watch_from_next_block(ctx, wait).await;
        }
    }
    let env = ctx.env();
    let blocks = env.height.saturating_sub(watch.height).max(1);
    let block_ms = env.time.saturating_sub(watch.time) / blocks;
    let streaming = PodWatch {
        entry: None,
        ..watch
    };
    POD_WAITS.put(ctx, &wait, &streaming);
    let asked_at = env.time;
    let read = ask_pods(ctx, format!("/v1/pods/{name}"), Delivery::Whole).await;
    let trip = ctx.env().time.saturating_sub(asked_at);
    if let Some(over) = unwatchable(name, read) {
        return fire(ctx, wait, over).await;
    }
    let lasts = DEFAULT_WAIT.saturating_mul(block_ms);
    let every = lasts.saturating_sub(trip) / 1000;
    if every == 0 {
        let over = format!(
            "harness can't wait on pod {name}: a request lasts {lasts} ms ({DEFAULT_WAIT} blocks of {block_ms} ms) and the pod's read took {trip} ms to land, which leaves no whole second between the stream's alive events"
        );
        return fire(ctx, wait, over).await;
    }
    let target = format!("/v1/pods/{name}/wait?every={every}");
    let events = Delivery::Events {
        deliver: pod_event_as(wait),
    };
    let streamed = ask_pods(ctx, target, events).await;
    let opened_again = POD_WAITS
        .get(ctx, &wait)?
        .is_some_and(|watch| watch.entry.is_some());
    if open(ctx, wait)?.is_none() || opened_again {
        return Ok(());
    }
    match streamed {
        Ok(answer) if answer.status != 200 => {
            let over = unwatchable(name, Ok(answer)).unwrap_or_default();
            fire(ctx, wait, over).await
        }
        Ok(_) | Err(_) => watch_from_next_block(ctx, wait).await,
    }
}

// why a pod can't be waited on, from the pods service's answer about it
fn unwatchable(name: &str, answer: Result<http::Response, Error>) -> Option<String> {
    match answer {
        Ok(answer) if answer.status == 200 => None,
        Ok(answer) if answer.status == 404 => Some(format!("the pods service knows no pod {name}")),
        Ok(answer) => Some(format!(
            "harness can't wait on pod {name}: {}",
            refused_by_pods("the pods service", &answer)
        )),
        Err(error) => Some(format!(
            "harness couldn't reach the pods service to wait on pod {name}: {}",
            refusal(&error)
        )),
    }
}

// an `ended` event ends the wait with the pods service's line; an `alive` one has done its work
// by landing, which moved the stream's expiry on
pub(crate) async fn pod_event(ctx: &Ctx, wait: u64, event: Event) -> Result<(), Error> {
    let egress = bound(ctx)?;
    let from_egress = ctx
        .env()
        .sending_program()
        .is_ok_and(|program| program == egress);
    if !from_egress {
        return Err(unauthorized(
            "only the program bound to the egress role hands harness a pod's wait events",
        ));
    }
    let Some(row) = open(ctx, wait)? else {
        return Ok(());
    };
    let WaitFor::Pod { name } = &row.event else {
        return Ok(());
    };
    let Some(line) = ended_line(&event.data, name) else {
        return Ok(());
    };
    fire(ctx, wait, line).await
}

// an event is its raw lines up to the blank line: `event: ended` and `data: {"line", ...}`
fn ended_line(raw: &[u8], name: &str) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    let is_ended = text.lines().any(|line| line.trim_end() == "event: ended");
    if !is_ended {
        return None;
    }
    let data = text
        .lines()
        .find_map(|line| line.strip_prefix("data:"))
        .and_then(|data| serde_json::from_str::<Value>(data.trim()).ok());
    let line = data.as_ref().and_then(|data| data["line"].as_str());
    Some(match line {
        Some(line) => line.to_owned(),
        None => format!("pod {name} ended"),
    })
}

// a line on each wait the conversation still holds, for an input that wakes it before them
pub(crate) fn still_armed(ctx: &QueryCtx, thread: u64) -> Result<String, Error> {
    let mut armed = Vec::new();
    for (_, id) in ARMED.scan(ctx, ARMED.prefix_of(&thread))? {
        if let Some(row) = open(ctx, id)? {
            armed.push(format!("wait {id} for {}", what(&row.event)));
        }
    }
    Ok(match armed.is_empty() {
        true => String::new(),
        false => format!("\n(This conversation still waits: {}.)", armed.join("; ")),
    })
}

pub(crate) fn waits(
    ctx: &QueryCtx,
    agent: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<WaitRow>, Error> {
    WAITING.range_of(ctx, &agent, page)?.try_map(|(_, id)| {
        WAITS
            .get(ctx, &id)?
            .ok_or_else(|| corrupt("waiting", &id.to_be_bytes(), "names a wait that is gone"))
    })
}

pub(crate) fn armed(
    ctx: &QueryCtx,
    thread: u64,
    page: &PageRequest,
) -> Result<PageResponse<WaitRow>, Error> {
    ARMED.range_of(ctx, &thread, page)?.try_map(|(_, id)| {
        WAITS
            .get(ctx, &id)?
            .ok_or_else(|| corrupt("armed", &id.to_be_bytes(), "names a wait that is gone"))
    })
}

pub(crate) fn wait_row(ctx: &QueryCtx, wait: u64) -> Result<Option<WaitRow>, Error> {
    WAITS.get(ctx, &wait)
}
