//! The settle window: general input to an idle conversation whose agent names `settle` waits that
//! long, and a call's result to one idle since an earlier block waits out the next block, so one
//! turn hears what came with it; direct input (a mention, a direct message) ends either at once.
use ::program::{Cause, Ctx, Error, QueryCtx, code};

use crate::agents::{configured, running, thread};
use crate::state::SETTLING;
use crate::turns::converse;
use crate::{Asking, Op, SettleWindow};

pub(crate) fn settling(ctx: &QueryCtx, id: u64) -> bool {
    SETTLING.has(ctx, &id)
}

pub(crate) async fn gather(ctx: &Ctx, id: u64) {
    let already_open = SETTLING.has(ctx, &id);
    let scheduled = ctx.env().roles.get(abi::role::SCHEDULER).is_some();
    let opens = scheduled && !already_open;
    if !opens {
        return;
    }
    let env = ctx.env();
    let wake = Op::Settle { thread: id };
    let Ok(entry) = ctx.schedule(env.height + 1, &env.program, &wake).await else {
        return;
    };
    let window = SettleWindow {
        opened: env.height,
        opened_time: env.time,
        until: env.time,
        entry,
    };
    SETTLING.put(ctx, &id, &window);
}

pub(crate) async fn wait(ctx: &Ctx, id: u64) -> Result<bool, Error> {
    if let Some(window) = SETTLING.get(ctx, &id)? {
        let over = ctx.env().time >= window.until;
        if !over {
            return Ok(true);
        }
        forget(ctx, id, window.entry).await?;
        return Ok(false);
    }
    let row = thread(ctx, id)?;
    let idle = !row.closed && row.asking == Asking::Idle;
    let scheduled = ctx.env().roles.get(abi::role::SCHEDULER).is_some();
    if !(idle && scheduled) {
        return Ok(false);
    }
    let Some(agent) = running(ctx, row.agent)? else {
        return Ok(false);
    };
    let setup = configured(ctx, &agent.path)?;
    let Some(settle) = setup.config().and_then(|(_, config)| config.settle) else {
        return Ok(false);
    };
    let env = ctx.env();
    let wake = Op::Settle { thread: id };
    let Ok(entry) = ctx
        .schedule(env.height + 1, &ctx.env().program, &wake)
        .await
    else {
        return Ok(false);
    };
    let window = SettleWindow {
        opened: env.height,
        opened_time: env.time,
        until: env.time.saturating_add(settle),
        entry,
    };
    SETTLING.put(ctx, &id, &window);
    Ok(true)
}

pub(crate) async fn end(ctx: &Ctx, id: u64) -> Result<(), Error> {
    let Some(window) = SETTLING.get(ctx, &id)? else {
        return Ok(());
    };
    forget(ctx, id, window.entry).await
}

async fn forget(ctx: &Ctx, id: u64, entry: u64) -> Result<(), Error> {
    SETTLING.remove(ctx, &id);
    match ctx.cancel(entry).await {
        Err(error) if error.code == code::WRONG_STATE => Ok(()),
        cancelled => cancelled,
    }
}

pub(crate) async fn due(ctx: Ctx, id: u64) -> Result<(), Error> {
    let Some(window) = SETTLING.get(&ctx, &id)? else {
        return Ok(());
    };
    let env = ctx.env();
    let its_own = matches!(env.cause, Cause::Scheduled { entry } if entry == window.entry);
    let over = env.time >= window.until;
    if !(over || its_own) {
        return Err(Error::new(
            code::NOT_YET,
            format!(
                "thread {id}'s settle window ends at time {}: wait until the settle window ends",
                window.until
            ),
        ));
    }
    if !over {
        let next = env.height + blocks_left(&window, env.height, env.time);
        let wake = Op::Settle { thread: id };
        if let Ok(entry) = ctx.schedule(next, &ctx.env().program, &wake).await {
            SETTLING.put(&ctx, &id, &SettleWindow { entry, ..window });
            return Ok(());
        }
    }
    match its_own {
        true => SETTLING.remove(&ctx, &id),
        false => forget(&ctx, id, window.entry).await?,
    }
    converse(&ctx, id).await;
    Ok(())
}

fn blocks_left(window: &SettleWindow, height: u64, time: u64) -> u64 {
    let blocks = height.saturating_sub(window.opened);
    let took = time.saturating_sub(window.opened_time);
    let left = window.until.saturating_sub(time);
    match took {
        0 => 1,
        took => left.saturating_mul(blocks).div_ceil(took).max(1),
    }
}
