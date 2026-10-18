//! What each thread's turns used and how often it was summarized, kept as each turn and summary
//! is recorded, so no reader adds up a whole transcript to know it.
use ::program::{Error, ExecCtx, QueryCtx};

use crate::state::{FRAMES, TALLIES};
use crate::{Tally, Usage};

pub(crate) fn tally(ctx: &QueryCtx, thread: u64) -> Result<Tally, Error> {
    if let Some(tally) = TALLIES.get(ctx, &thread)? {
        return Ok(tally);
    }
    // a thread older than its tally: its frames, added up
    let mut tally = Tally::default();
    for (_, frame) in FRAMES.scan(ctx, FRAMES.prefix_of(&thread))? {
        tally.count(&frame.body);
    }
    Ok(tally)
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
        // the frame just appended is among those a missing tally adds up
        None => tally(ctx, thread)?,
    };
    TALLIES.put(ctx, &thread, &tally);
    Ok(())
}
