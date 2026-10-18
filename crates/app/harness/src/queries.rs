//! Every read: what harness resolves for an agent, a thread, an agent's threads newest first,
//! a transcript in order, the input held while a thread's loop runs, a thread's tool calls, the
//! tool catalog, the thread a place last reached, the model records, the frames left out of the
//! model's view with their size, the reply a thread's model call is streaming back, and the line
//! a thread's agent says it is about.
use ::program::{Error, QueryCtx, corrupt};
use abi::role::identity::AccountNumber;
use store::{PageRequest, PageResponse};

use crate::state::{
    BY_AGENT, CALLS, FRAMES, HELD, MODELS, PARTIALS, PLACES, THREADS, TOPICS, WITHHELD,
};
use crate::{Body, CallRow, Frame, ModelRow, Partial, ThreadRow, WithheldFrame};

pub(crate) fn partial(ctx: &QueryCtx, thread: u64) -> Result<Option<Partial>, Error> {
    PARTIALS.get(ctx, &thread)
}

pub(crate) fn topic(ctx: &QueryCtx, thread: u64) -> Result<Option<String>, Error> {
    TOPICS.get(ctx, &thread)
}

pub(crate) fn model(ctx: &QueryCtx, id: String) -> Result<Option<ModelRow>, Error> {
    MODELS.get(ctx, &id)
}

pub(crate) fn models(ctx: &QueryCtx, page: &PageRequest) -> Result<PageResponse<ModelRow>, Error> {
    Ok(MODELS.range(ctx, page)?.map(|(_, row)| row))
}

pub(crate) fn thread(ctx: &QueryCtx, id: u64) -> Result<Option<ThreadRow>, Error> {
    THREADS.get(ctx, &id)
}

pub(crate) fn threads(
    ctx: &QueryCtx,
    agent: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<ThreadRow>, Error> {
    BY_AGENT.range_of(ctx, &agent, page)?.try_map(|(_, key)| {
        let id = u64::MAX - key;
        THREADS.get(ctx, &id)?.ok_or_else(|| {
            corrupt(
                "thread",
                &id.to_be_bytes(),
                "an agent names a thread that is gone",
            )
        })
    })
}

pub(crate) fn frames(
    ctx: &QueryCtx,
    thread: u64,
    page: &PageRequest,
) -> Result<PageResponse<Frame>, Error> {
    Ok(FRAMES.range_of(ctx, &thread, page)?.map(|(_, frame)| frame))
}

pub(crate) fn calls(
    ctx: &QueryCtx,
    thread: u64,
    page: &PageRequest,
) -> Result<PageResponse<CallRow>, Error> {
    Ok(CALLS.range_of(ctx, &thread, page)?.map(|(_, call)| call))
}

pub(crate) fn thread_at(
    ctx: &QueryCtx,
    agent: AccountNumber,
    place: String,
) -> Result<Option<ThreadRow>, Error> {
    match PLACES.get(ctx, &(agent, place))? {
        Some(id) => THREADS.get(ctx, &id),
        None => Ok(None),
    }
}

pub(crate) fn withheld(ctx: &QueryCtx, thread: u64) -> Result<Vec<WithheldFrame>, Error> {
    let left_out = WITHHELD.scan(ctx, WITHHELD.prefix_of(&thread))?;
    left_out
        .into_iter()
        .map(|key| {
            let frame = FRAMES.get(ctx, &key)?.ok_or_else(|| {
                corrupt(
                    "withheld",
                    &WITHHELD.key(&key),
                    "names a frame that is gone",
                )
            })?;
            Ok(WithheldFrame {
                seq: key.1,
                size: frame.body.text().len() as u64,
            })
        })
        .collect()
}

pub(crate) fn held(ctx: &QueryCtx, thread: u64) -> Result<Vec<Body>, Error> {
    Ok(HELD
        .scan(ctx, HELD.prefix_of(&thread))?
        .into_iter()
        .map(|(_, body)| body)
        .collect())
}
