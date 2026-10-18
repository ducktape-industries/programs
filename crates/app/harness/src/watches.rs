//! Tool calls that wake their conversation later: a tool whose run streams (`Run::Stream`) has
//! its call answered at once, harness's own scheduled run (`Op::Watch`) sends the stream's egress
//! request, each event comes back as `Op::Heard`, and the conversation hears once.
use ::program::{Cause, Ctx, Error, unauthorized};
use abi::role::egress::{self, Event, http};
use serde_json::{Value, json};

use crate::bookings::tell;
use crate::calls::refusal;
use crate::state::{WATCH_COUNT, WATCHES, Watching};
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
    let op = egress::Op::Request {
        request: row.request.clone(),
        delivery: egress::Delivery::Events {
            deliver: heard_as(watch),
        },
        wait: None,
    };
    let answered = ctx
        .call_raw_as(row.agent, bound(&ctx)?, abi::encode(&op))
        .await;
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
    let said = match tools::find(&row.tool) {
        Some(entry) => (entry.heard)(&event.data)?,
        None => None,
    };
    let Some(said) = said else {
        return Ok(());
    };
    WATCHES.remove(ctx, &watch);
    let text = format!(
        "{} call {} ({}) ended: {said}",
        row.tool, row.call, row.about
    );
    woken(ctx, &row, text).await
}

async fn woken(ctx: &Ctx, row: &Watching, text: String) -> Result<(), Error> {
    tell(ctx, row.agent, Some(row.thread), text).await
}
