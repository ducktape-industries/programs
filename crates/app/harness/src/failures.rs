//! A conversation's final errors in a row (`FINAL_ERRORS`, by agent and thread): each input that
//! ends with no answer (`turns::give_up`) counts one, the next answered turn or the conversation's
//! close clears them, and `Query::FinalErrors` answers them.
use ::program::{Error, ExecCtx, QueryCtx};
use abi::role::identity::AccountNumber;
use store::{PageRequest, PageResponse};

use crate::state::FINAL_ERRORS;
use crate::{FinalError, ThreadRow};

pub(crate) fn failed(ctx: &ExecCtx, thread: &ThreadRow, text: String) -> Result<(), Error> {
    if thread.closed {
        return Ok(());
    }
    let key = (thread.agent, thread.id);
    let env = ctx.env();
    let run = FINAL_ERRORS.get(ctx, &key)?;
    let error = FinalError {
        thread: thread.id,
        text,
        frame: thread.frames,
        height: env.height,
        time: env.time,
        since: run.as_ref().map_or(env.height, |run| run.since),
        count: run.map_or(1, |run| run.count.saturating_add(1)),
    };
    FINAL_ERRORS.put(ctx, &key, &error);
    Ok(())
}

pub(crate) fn cleared(ctx: &ExecCtx, thread: &ThreadRow) -> Result<(), Error> {
    let key = (thread.agent, thread.id);
    if FINAL_ERRORS.get(ctx, &key)?.is_some() {
        FINAL_ERRORS.remove(ctx, &key);
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
