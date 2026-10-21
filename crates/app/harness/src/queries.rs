//! Every read: an agent, a thread, an agent's threads (alone or with topic and tally) and bound
//! ones, a transcript or a span of it from a frame on, held input, tool calls or one call, the tool
//! catalog, a place's last thread, withheld frames with their size, a streaming reply and a topic.
use ::program::{Error, QueryCtx, corrupt};
use abi::role::identity::AccountNumber;
use store::{PageRequest, PageResponse};

use crate::state::{
    BOUND, BY_AGENT, CALLS, FRAMES, HELD, PLACES, SETTLING, TALLIES, THREADS, TOPICS, WITHHELD,
};
use crate::{Body, CallRow, Conversation, Frame, Partial, SettleWindow, ThreadRow, WithheldFrame};

pub(crate) fn partial(ctx: &QueryCtx, thread: u64) -> Result<Option<Partial>, Error> {
    crate::replies::read(ctx, thread)
}

pub(crate) fn topic(ctx: &QueryCtx, thread: u64) -> Result<Option<String>, Error> {
    TOPICS.get(ctx, &thread)
}

pub(crate) fn settling(ctx: &QueryCtx, thread: u64) -> Result<Option<SettleWindow>, Error> {
    SETTLING.get(ctx, &thread)
}

pub(crate) fn thread(ctx: &QueryCtx, id: u64) -> Result<Option<ThreadRow>, Error> {
    THREADS.get(ctx, &id)
}

fn named(ctx: &QueryCtx, id: u64, what: &str) -> Result<ThreadRow, Error> {
    THREADS
        .get(ctx, &id)?
        .ok_or_else(|| corrupt("thread", &id.to_be_bytes(), what))
}

pub(crate) fn threads(
    ctx: &QueryCtx,
    agent: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<ThreadRow>, Error> {
    BY_AGENT
        .range_of(ctx, &agent, page)?
        .try_map(|(_, key)| named(ctx, u64::MAX - key, "an agent names a thread that is gone"))
}

pub(crate) fn conversations(
    ctx: &QueryCtx,
    agent: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<Conversation>, Error> {
    BY_AGENT.range_of(ctx, &agent, page)?.try_map(|(_, key)| {
        let id = u64::MAX - key;
        let row = named(ctx, id, "an agent names a thread that is gone")?;
        Ok(Conversation {
            row,
            topic: TOPICS.get(ctx, &id)?,
            tally: TALLIES.get(ctx, &id)?,
        })
    })
}

pub(crate) fn bound(
    ctx: &QueryCtx,
    agent: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<ThreadRow>, Error> {
    BOUND.range_of(ctx, &agent, page)?.try_map(|(_, binding)| {
        named(
            ctx,
            binding.thread,
            "a bound place names a thread that is gone",
        )
    })
}

pub(crate) fn frames(
    ctx: &QueryCtx,
    thread: u64,
    page: &PageRequest,
) -> Result<PageResponse<Frame>, Error> {
    Ok(FRAMES.range_of(ctx, &thread, page)?.map(|(_, frame)| frame))
}

pub(crate) fn frames_from(
    ctx: &QueryCtx,
    thread: u64,
    from: u64,
    page: &PageRequest,
) -> Result<PageResponse<Frame>, Error> {
    let listing = page.listing(ctx, FRAMES.key(&thread))?;
    let mut scan = listing.scan_ahead(&FRAMES.key(&thread));
    scan.start = scan.start.max(FRAMES.key(&(thread, from)));
    let rows = FRAMES
        .scan(ctx, scan)?
        .into_iter()
        .map(|(key, frame)| (FRAMES.key(&key), frame));
    Ok(listing.reply(rows))
}

pub(crate) fn calls(
    ctx: &QueryCtx,
    thread: u64,
    page: &PageRequest,
) -> Result<PageResponse<CallRow>, Error> {
    Ok(CALLS.range_of(ctx, &thread, page)?.map(|(_, call)| call))
}

pub(crate) fn call(ctx: &QueryCtx, thread: u64, call: String) -> Result<Option<CallRow>, Error> {
    CALLS.get(ctx, &(thread, call))
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
