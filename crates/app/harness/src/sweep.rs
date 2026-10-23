//! `Sweep`: a standing run every `DEFAULT_WAIT` blocks that books each model retry no booking holds
//! (the scheduler refused it in a frame at the kernel's nesting cap), so none waits on other
//! traffic. A stopgap until the host nests runs without a cap, when it goes.
use ::program::{Ctx, Error, QueryCtx};
use abi::role::egress::DEFAULT_WAIT;

use crate::Op;
use crate::resume::wake;
use crate::state::{RETRIES, Retry, SWEEP_AT};

pub(crate) async fn sweep(ctx: &Ctx) -> Result<(), Error> {
    let env = ctx.env();
    let height = env.height;
    for (id, retry) in RETRIES.all(ctx)? {
        if retry.entry.is_some() {
            continue;
        }
        let at = retry.at.max(height.saturating_add(1));
        if let Ok(entry) = ctx.schedule(at, &env.program, &wake(id)).await {
            let booked = Retry {
                at,
                entry: Some(entry),
                ..retry
            };
            RETRIES.put(ctx, &id, &booked);
        }
    }
    let armed = SWEEP_AT.get(ctx)?.is_some_and(|at| at > height);
    if !armed {
        let at = height.saturating_add(DEFAULT_WAIT);
        ctx.schedule(at, &env.program, &Op::Sweep).await?;
        SWEEP_AT.put(ctx, &at);
    }
    Ok(())
}

pub(crate) fn words(ctx: &QueryCtx, height: u64) -> Result<String, Error> {
    let before = "or an answer on its service before";
    Ok(match SWEEP_AT.get(ctx)?.filter(|at| *at > height) {
        Some(at) => format!("harness's sweep at block {at} books it, {before}"),
        None => format!("harness's sweep books it once it runs, {before}"),
    })
}
