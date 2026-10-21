//! `HoldSealed`: each part an open thread sealed before harness held sealed parts on lines is put
//! again on its thread's `sealed/<n>` line, and a closed idle thread lets go of its sealed rows; one
//! thread a block from harness's own scheduled run, resuming at `relined/next` until none is left.
use ::program::{Cause, Ctx, Error, HashKind, Range, wrong_state};

use crate::state::{RELINED, SEALED, THREADS};
use crate::turns::{held_on, let_go};
use crate::{Asking, Op};

pub(crate) async fn hold_sealed(ctx: &Ctx) -> Result<(), Error> {
    let env = ctx.env();
    let height = env.height;
    let own = matches!(env.cause, Cause::Scheduled { .. })
        && env
            .sending_program()
            .is_ok_and(|program| program == env.program);
    let from = match (own, RELINED.get(ctx)?) {
        (false, Some(_)) => {
            return Err(wrong_state(
                "a pass holding sealed parts on lines is running: it ends by itself",
            ));
        }
        (true, None) => return Ok(()),
        (_, from) => from.unwrap_or(0),
    };
    let mut first = Range::prefix(SEALED.prefix());
    first.start = SEALED.key(&(from, 0u64));
    let Some(((thread, _), _)) = SEALED.scan(ctx, first.limit(1))?.into_iter().next() else {
        ctx.delete(RELINED.key());
        return Ok(());
    };
    match THREADS.get(ctx, &thread)? {
        Some(mut row) if row.closed => {
            if row.asking == Asking::Idle {
                let_go(ctx, &mut row)?;
                THREADS.put(ctx, &thread, &row);
            }
        }
        _ => hold(ctx, thread)?,
    }
    let Some(next) = thread.checked_add(1) else {
        ctx.delete(RELINED.key());
        return Ok(());
    };
    RELINED.put(ctx, &next);
    ctx.schedule(height.saturating_add(1), &env.program, &Op::HoldSealed)
        .await?;
    Ok(())
}

fn hold(ctx: &Ctx, thread: u64) -> Result<(), Error> {
    for ((_, n), sealed) in SEALED.scan(ctx, SEALED.prefix_of(&thread))? {
        let Some(blob) = ctx.blob_get(sealed.part) else {
            continue;
        };
        let line = [b"sealed/".as_slice(), &n.to_be_bytes()].concat();
        ctx.blob_put_on(
            HashKind::Sha256,
            blob.kind,
            blob.body,
            held_on(thread, &line),
        )?;
    }
    Ok(())
}
