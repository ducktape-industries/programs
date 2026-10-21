//! What each thread's turns used and how often it was summarized, kept as each turn and summary
//! is recorded, as a fork copies its frames, and by `AdoptTallies` for a thread from before tallies.
use ::program::{Error, ExecCtx, QueryCtx};

use crate::state::{FRAMES, TALLIES, THREADS};
use crate::{Tally, Usage};

pub(crate) fn tally(ctx: &QueryCtx, thread: u64) -> Result<Tally, Error> {
    if let Some(tally) = TALLIES.get(ctx, &thread)? {
        return Ok(tally);
    }
    let mut tally = Tally::default();
    for (_, frame) in FRAMES.scan(ctx, FRAMES.prefix_of(&thread))? {
        tally.count(&frame.body);
    }
    Ok(tally)
}

pub(crate) fn kept(ctx: &ExecCtx, thread: u64) -> Result<(), Error> {
    if TALLIES.has(ctx, &thread) {
        return Ok(());
    }
    let tally = tally(ctx, thread)?;
    TALLIES.put(ctx, &thread, &tally);
    Ok(())
}

pub(crate) fn adopt(ctx: &ExecCtx, after: Option<Vec<u8>>, limit: u64) -> Result<(), Error> {
    let batch = THREADS.batch(ctx, after.as_deref(), limit)?;
    let mut threads = batch.rows.into_iter().map(|(thread, _)| thread);
    let untallied = threads.find(|thread| !TALLIES.has(ctx, thread));
    let next = match untallied {
        Some(thread) => {
            kept(ctx, thread)?;
            Some(THREADS.key(&thread))
        }
        None => batch.next,
    };
    ctx.set_return_data(abi::encode(&next));
    Ok(())
}

pub(crate) fn turned(ctx: &ExecCtx, thread: u64, usage: &Usage) -> Result<(), Error> {
    counted(ctx, thread, usage, 0)
}

pub(crate) fn summarized(ctx: &ExecCtx, thread: u64) -> Result<(), Error> {
    counted(ctx, thread, &Usage::default(), 1)
}

fn counted(ctx: &ExecCtx, thread: u64, usage: &Usage, summaries: u64) -> Result<(), Error> {
    let tally = match TALLIES.get(ctx, &thread)? {
        Some(mut tally) => {
            tally.add(usage, summaries);
            tally
        }
        None => tally(ctx, thread)?,
    };
    TALLIES.put(ctx, &thread, &tally);
    Ok(())
}
