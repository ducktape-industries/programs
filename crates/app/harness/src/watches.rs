//! Tool calls that wake their conversation later: a tool whose run streams (`Run::Stream`, or
//! `Run::Measured` at a measured heartbeat) is answered at once, harness's own scheduled run
//! (`Op::Watch`) sends the stream's request, each event is `Op::Heard`, the conversation hears once.
use ::program::{Cause, Ctx, Error, ExecCtx, HashKind, unauthorized};
use abi::role::egress::{self, Event, http};
use serde_json::{Value, json};

use crate::bookings::tell;
use crate::calls::refusal;
use crate::state::{
    HEARTBEATS, Heartbeat, MEASURES, Measure, Tick, WATCH_COUNT, WATCHES, Watching,
};
use crate::turns::bound;
use crate::{Op, PART, ThreadRow, heard_as, tools};

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
    let me = ctx.env().program.clone();
    ctx.schedule(next, me, &Op::Watch { watch }).await?;
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

pub(crate) async fn open_paced(
    ctx: &Ctx,
    thread: &ThreadRow,
    call: &str,
    tool: &str,
    request: egress::Request,
    body: Option<String>,
    about: String,
) -> Result<Value, Error> {
    let opened = open(ctx, thread, call, tool, request, about).await?;
    let watch = WATCH_COUNT.get(ctx)?.unwrap_or(0);
    let beat = Heartbeat {
        height: ctx.env().height,
        time: ctx.env().time,
        body,
    };
    HEARTBEATS.put(ctx, &watch, &beat);
    Ok(opened)
}

fn paced(
    ctx: &ExecCtx,
    row: &Watching,
    beat: &Heartbeat,
) -> Result<Option<egress::Request>, String> {
    let opened = Tick {
        height: beat.height,
        time: beat.time,
    };
    let Some(every) = crate::waits::alive_every(ctx, opened)? else {
        return Ok(None);
    };
    let egress::Request::Http { service, head, .. } = &row.request else {
        return Err("a paced stream is an HTTP request".to_owned());
    };
    let mut head = head.clone();
    head.headers.push(("every".to_owned(), every.to_string()));
    let body = match &beat.body {
        Some(text) => {
            let held = tools::held_for(row.thread, &row.call);
            let part = ctx.blob_put_on(HashKind::Sha256, PART, text.clone().into_bytes(), held);
            vec![part.map_err(|error| refusal(&error))?]
        }
        None => Vec::new(),
    };
    Ok(Some(egress::Request::Http {
        service: service.clone(),
        head,
        body,
    }))
}

async fn measured(
    ctx: &Ctx,
    row: &Watching,
    measure: &Measure,
) -> Result<Option<egress::Request>, String> {
    let opened = Tick {
        height: measure.height,
        time: measure.time,
    };
    let Some(every) = crate::waits::alive_every(ctx, opened)? else {
        return Ok(None);
    };
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
    let answered = ctx
        .call_raw_as(row.agent, egress, abi::encode(&op))
        .await
        .map_err(|error| format!("the pod was not read: {}", refusal(&error)))?;
    let response: http::Response = abi::decode(&answered)
        .map_err(|fault| format!("the pod's read is no HTTP response: {}", fault.sentence))?;
    if response.status != 200 {
        let said = String::from_utf8_lossy(&response.body);
        return Err(format!(
            "the pod's read answered {}: {said}",
            response.status
        ));
    }
    let mut body: Value = serde_json::from_str(&measure.body).map_err(|error| error.to_string())?;
    body["every"] = json!(every);
    let held = tools::held_for(row.thread, &row.call);
    crate::tools::pods::posted(ctx, held, target, &body)
        .map(Some)
        .map_err(|error| refusal(&error))
}

pub(crate) async fn watch(ctx: Ctx, watch: u64) -> Result<(), Error> {
    let env = ctx.env();
    let scheduled = matches!(env.cause, Cause::Scheduled { .. });
    let by_harness = env
        .sending_program()
        .is_ok_and(|program| program == env.program);
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
                Ok(None) => {
                    let env = ctx.env();
                    let later = Measure {
                        height: env.height,
                        time: env.time,
                        ..measure
                    };
                    MEASURES.put(&ctx, &watch, &later);
                    return next_block(&ctx, row.thread, watch).await;
                }
                Ok(Some(request)) => {
                    let row = Watching { request, ..row };
                    WATCHES.put(&ctx, &watch, &row);
                    row
                }
                Err(why) => {
                    WATCHES.remove(&ctx, &watch);
                    ctx.blob_release_lines(tools::held_for(row.thread, &row.call));
                    let text = format!(
                        "{} call {} ({}) did not start: {why}",
                        row.tool, row.call, row.about
                    );
                    return woken(&ctx, &row, text).await;
                }
            }
        }
    };
    let row = match HEARTBEATS.get(&ctx, &watch)? {
        None => row,
        Some(beat) => {
            HEARTBEATS.remove(&ctx, &watch);
            match paced(&ctx, &row, &beat) {
                Ok(None) => {
                    let env = ctx.env();
                    let later = Heartbeat {
                        height: env.height,
                        time: env.time,
                        ..beat
                    };
                    HEARTBEATS.put(&ctx, &watch, &later);
                    return next_block(&ctx, row.thread, watch).await;
                }
                Ok(Some(request)) => {
                    let row = Watching { request, ..row };
                    WATCHES.put(&ctx, &watch, &row);
                    row
                }
                Err(why) => {
                    WATCHES.remove(&ctx, &watch);
                    ctx.blob_release_lines(tools::held_for(row.thread, &row.call));
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
    if !WATCHES.has(&ctx, &watch) {
        return Ok(());
    }
    WATCHES.remove(&ctx, &watch);
    ctx.blob_release_lines(tools::held_for(row.thread, &row.call));
    let why = unheard(answered, &lost(&row));
    let text = format!("{} call {} ({}) {why}", row.tool, row.call, row.about);
    woken(&ctx, &row, text).await
}

async fn next_block(ctx: &Ctx, thread: u64, watch: u64) -> Result<(), Error> {
    crate::resume::unlistened(ctx, thread, watch);
    let next = ctx.env().height.saturating_add(1);
    let me = ctx.env().program.clone();
    ctx.schedule(next, me, &Op::Watch { watch }).await?;
    Ok(())
}

const LOST: &str = "this is not its result: what it ran may still be running, so re-attach with \
                    build.sh wait <run> (a build.sh run keeps its status on the pod's disk), or look \
                    at the pod, before running it again";

pub(crate) fn lost(row: &Watching) -> String {
    match (tools::find(&row.tool), &row.request) {
        (None, egress::Request::Http { service, .. }) => format!(
            "this is not its result: what it ran may still be running on {service}, so check there before running it again"
        ),
        _ => LOST.to_owned(),
    }
}

fn unheard(answered: Result<Vec<u8>, Error>, lost: &str) -> String {
    let output = match answered {
        Ok(output) => output,
        Err(error) => {
            let why = refusal(&error);
            return format!("lost its stream, which broke off ({why}): {lost}");
        }
    };
    match abi::decode::<http::Response>(&output) {
        Ok(response) if response.body.is_empty() => format!(
            "lost its stream, which closed with status {} before its end event (the service \
             that streamed it stopped before the end, as a restart there does): {lost}",
            response.status
        ),
        Ok(response) => format!(
            "ended without saying how: the service answered {}: {}",
            response.status,
            String::from_utf8_lossy(&response.body)
        ),
        Err(fault) => format!(
            "ended without saying how: the answer is no HTTP response: {}",
            fault.sentence
        ),
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
        None => tools::file::heard(&event.data),
    };
    let Some(said) = said else {
        return Ok(());
    };
    WATCHES.remove(ctx, &watch);
    ctx.blob_release_lines(tools::held_for(row.thread, &row.call));
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
