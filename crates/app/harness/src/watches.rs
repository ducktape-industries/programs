//! Tool calls that wake their conversation later: a tool whose run streams (`Run::Stream`, or
//! `Run::Measured` at a measured heartbeat) is answered at once, harness's own scheduled run
//! (`Op::Watch`) sends the stream's request, each event is `Op::Heard`, the conversation hears once.
use ::program::{Cause, Ctx, Error, unauthorized};
use abi::role::egress::{self, Event, http};
use serde_json::{Value, json};

use crate::bookings::tell;
use crate::calls::refusal;
use crate::state::{MEASURES, Measure, WATCH_COUNT, WATCHES, Watching};
use crate::turns::bound;
use crate::{Op, PROGRAM, ThreadRow, heard_as, tools};

pub(crate) async fn open(
    ctx: &Ctx,
    thread: &ThreadRow,
    call: &str,
    tool: &str,
    request: egress::Request,
    about: String,
) -> Result<Value, Error> {
    let watch = WATCH_COUNT.get(ctx)?.unwrap_or(0) + 1;
    let next = ctx.env().height.saturating_add(1);
    ctx.schedule(next, PROGRAM, &Op::Watch { watch }).await?;
    WATCH_COUNT.put(ctx, &watch);
    let row = Watching {
        thread: thread.id,
        agent: thread.agent,
        call: call.to_owned(),
        tool: tool.to_owned(),
        about: about.clone(),
        request,
    };
    WATCHES.put(ctx, &watch, &row);
    Ok(json!({
        "streaming": true,
        "watch": watch,
        "about": about,
        "wakes": "this conversation gets one message when it ends",
    }))
}

// a stream whose heartbeat harness measures: its watch opens the block after, as any does, and
// first reads the pod, which measures the trip and refuses a pod the service doesn't know
pub(crate) async fn open_measured(
    ctx: &Ctx,
    thread: &ThreadRow,
    call: &str,
    tool: &str,
    (pod, target, body): (String, String, Value),
    about: String,
) -> Result<Value, Error> {
    let head = http::Head {
        method: http::Method::Post,
        target,
        headers: Vec::new(),
    };
    let unsent = egress::Request::Http {
        service: crate::waits::PODS.to_owned(),
        head,
        body: Vec::new(),
    };
    let opened = open(ctx, thread, call, tool, unsent, about).await?;
    let watch = WATCH_COUNT.get(ctx)?.unwrap_or(0);
    let measure = Measure {
        height: ctx.env().height,
        time: ctx.env().time,
        pod,
        body: body.to_string(),
    };
    MEASURES.put(ctx, &watch, &measure);
    Ok(opened)
}

// the request a measured stream sends: its body with the heartbeat a request's lifetime leaves
// (`waits::alive_every`), from the block time since it was made and the trip of the pod's read
async fn measured(ctx: &Ctx, row: &Watching, measure: &Measure) -> Result<egress::Request, String> {
    let env = ctx.env();
    let blocks = env.height.saturating_sub(measure.height).max(1);
    let block_ms = env.time.saturating_sub(measure.time) / blocks;
    let egress::Request::Http { head, .. } = &row.request else {
        return Err("a measured stream is an HTTP request".to_owned());
    };
    let target = head.target.clone();
    let read = http::Head {
        method: http::Method::Get,
        target: format!("/v1/pods/{}", measure.pod),
        headers: Vec::new(),
    };
    let op = egress::Op::Request {
        request: egress::Request::Http {
            service: crate::waits::PODS.to_owned(),
            head: read,
            body: Vec::new(),
        },
        delivery: egress::Delivery::Whole,
        wait: None,
    };
    let egress = bound(ctx).map_err(|error| refusal(&error))?;
    let asked_at = env.time;
    let answered = ctx
        .call_raw_as(row.agent, egress, abi::encode(&op))
        .await
        .map_err(|error| format!("the pod was not read: {}", refusal(&error)))?;
    let trip = ctx.env().time.saturating_sub(asked_at);
    let response: http::Response = abi::decode(&answered)
        .map_err(|fault| format!("the pod's read is no HTTP response: {}", fault.sentence))?;
    if response.status != 200 {
        let said = String::from_utf8_lossy(&response.body);
        return Err(format!(
            "the pod's read answered {}: {said}",
            response.status
        ));
    }
    let every = crate::waits::alive_every(block_ms, trip)?;
    let mut body: Value = serde_json::from_str(&measure.body).map_err(|error| error.to_string())?;
    body["every"] = json!(every);
    crate::tools::pods::posted(ctx, target, &body).map_err(|error| refusal(&error))
}

pub(crate) async fn watch(ctx: Ctx, watch: u64) -> Result<(), Error> {
    let env = ctx.env();
    let scheduled = matches!(env.cause, Cause::Scheduled { .. });
    let by_harness = env
        .sending_program()
        .is_ok_and(|program| program == PROGRAM);
    if !(scheduled && by_harness) {
        return Err(unauthorized(
            "a watch opens only as harness's own scheduled run",
        ));
    }
    let Some(row) = WATCHES.get(&ctx, &watch)? else {
        return Ok(());
    };
    crate::resume::listening(&ctx, row.thread, watch).await;
    let row = match MEASURES.get(&ctx, &watch)? {
        None => row,
        Some(measure) => {
            MEASURES.remove(&ctx, &watch);
            match measured(&ctx, &row, &measure).await {
                Ok(request) => {
                    let row = Watching { request, ..row };
                    WATCHES.put(&ctx, &watch, &row);
                    row
                }
                Err(why) => {
                    WATCHES.remove(&ctx, &watch);
                    let text = format!(
                        "{} call {} ({}) did not start: {why}",
                        row.tool, row.call, row.about
                    );
                    return woken(&ctx, &row, text).await;
                }
            }
        }
    };
    let op = egress::Op::Request {
        request: row.request.clone(),
        delivery: egress::Delivery::Events {
            deliver: heard_as(watch),
        },
        wait: None,
    };
    crate::resume::listening(&ctx, row.thread, watch).await;
    let answered = ctx
        .call_raw_as(row.agent, bound(&ctx)?, abi::encode(&op))
        .await;
    crate::resume::unlistened(&ctx, row.thread, watch);
    // an event that said the end woke the conversation already
    if !WATCHES.has(&ctx, &watch) {
        return Ok(());
    }
    WATCHES.remove(&ctx, &watch);
    let why = unheard(answered);
    let text = format!(
        "{} call {} ({}) ended without saying how: {why}",
        row.tool, row.call, row.about
    );
    woken(&ctx, &row, text).await
}

// why a stream ended before any event said its end
fn unheard(answered: Result<Vec<u8>, Error>) -> String {
    let output = match answered {
        Ok(output) => output,
        Err(error) => {
            return format!(
                "the stream broke off ({}); what it ran may still be running",
                refusal(&error)
            );
        }
    };
    match abi::decode::<http::Response>(&output) {
        Ok(response) if response.body.is_empty() => format!(
            "the stream ended with status {} and said nothing more",
            response.status
        ),
        Ok(response) => format!(
            "the service answered {}: {}",
            response.status,
            String::from_utf8_lossy(&response.body)
        ),
        Err(fault) => format!("the answer is no HTTP response: {}", fault.sentence),
    }
}

pub(crate) async fn heard(ctx: &Ctx, watch: u64, event: Event) -> Result<(), Error> {
    let egress = bound(ctx)?;
    let from_egress = ctx
        .env()
        .sending_program()
        .is_ok_and(|program| program == egress);
    if !from_egress {
        return Err(unauthorized(
            "only the program bound to the egress role hands a watch its events",
        ));
    }
    let Some(row) = WATCHES.get(ctx, &watch)? else {
        return Ok(());
    };
    crate::resume::stream_moved(ctx, row.thread, watch)?;
    let said = match tools::find(&row.tool) {
        Some(entry) => (entry.heard)(&event.data)?,
        None => None,
    };
    let Some(said) = said else {
        return Ok(());
    };
    WATCHES.remove(ctx, &watch);
    crate::resume::unlistened(ctx, row.thread, watch);
    let text = format!(
        "{} call {} ({}) ended: {said}",
        row.tool, row.call, row.about
    );
    woken(ctx, &row, text).await
}

async fn woken(ctx: &Ctx, row: &Watching, text: String) -> Result<(), Error> {
    tell(ctx, row.agent, Some(row.thread), text).await
}
