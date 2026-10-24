//! A conversation's model asks with no answer in a row (`FINAL_ERRORS`, by agent and thread):
//! each input that ends unanswered (`turns::give_up`) and each busy retry from its backoff's
//! ceiling on counts one, the next answered turn or a close clears them, `FinalErrors` reads them.
use ::program::{Error, ExecCtx, QueryCtx};
use abi::role::identity::AccountNumber;
use store::{PageRequest, PageResponse};

use crate::state::{FINAL_ERRORS, RETRY_SINCE};
use crate::{FinalError, ThreadRow};

pub(crate) fn failed(ctx: &ExecCtx, thread: &ThreadRow, text: String) -> Result<(), Error> {
    if thread.closed {
        return Ok(());
    }
    let first = RETRY_SINCE.get(ctx, &thread.id)?;
    if first.is_some() {
        RETRY_SINCE.remove(ctx, &thread.id);
    }
    put(ctx, thread, text, first, 1)
}

pub(crate) fn retrying(
    ctx: &ExecCtx,
    thread: &ThreadRow,
    text: String,
    attempt: u32,
    ceiling: bool,
) -> Result<(), Error> {
    if thread.closed {
        return Ok(());
    }
    let first = match RETRY_SINCE.get(ctx, &thread.id)? {
        Some(first) => first,
        None => {
            let height = ctx.env().height;
            RETRY_SINCE.put(ctx, &thread.id, &height);
            height
        }
    };
    let listed = FINAL_ERRORS.get(ctx, &(thread.agent, thread.id))?.is_some();
    let shown = ceiling || listed;
    if !shown {
        return Ok(());
    }
    put(ctx, thread, text, Some(first), u64::from(attempt))
}

fn put(
    ctx: &ExecCtx,
    thread: &ThreadRow,
    text: String,
    first: Option<u64>,
    count: u64,
) -> Result<(), Error> {
    let key = (thread.agent, thread.id);
    let env = ctx.env();
    let run = FINAL_ERRORS.get(ctx, &key)?;
    let began = first.unwrap_or(env.height);
    let error = FinalError {
        thread: thread.id,
        text,
        frame: thread.frames,
        height: env.height,
        time: env.time,
        since: run.as_ref().map_or(began, |run| run.since),
        count: run.map_or(count, |run| run.count.saturating_add(1)),
    };
    FINAL_ERRORS.put(ctx, &key, &error);
    Ok(())
}

pub(crate) fn cleared(ctx: &ExecCtx, thread: &ThreadRow) -> Result<(), Error> {
    let key = (thread.agent, thread.id);
    crate::state::MODEL_FAILURES.remove(ctx, &thread.id);
    if FINAL_ERRORS.get(ctx, &key)?.is_some() {
        FINAL_ERRORS.remove(ctx, &key);
    }
    if RETRY_SINCE.get(ctx, &thread.id)?.is_some() {
        RETRY_SINCE.remove(ctx, &thread.id);
    }
    Ok(())
}

pub(crate) fn of_agent(
    ctx: &QueryCtx,
    agent: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<FinalError>, Error> {
    Ok(FINAL_ERRORS
        .range_of(ctx, &agent, page)?
        .map(|(_, error)| error))
}
