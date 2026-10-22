//! Watchers: programs told of every commit files makes, each as a message of its own
//! (`deliver ++ borsh(SnapshotRow)`) whose refusal never fails the commit.
use ::program::{Error, ExecCtx, ProgramId, Reply};

use crate::SnapshotRow;
use crate::state::WATCHERS;

pub(crate) fn watch(ctx: &ExecCtx, deliver: Vec<u8>) -> Result<(), Error> {
    let program = ctx.env().sending_program()?;
    match deliver.is_empty() {
        true => WATCHERS.remove(ctx, &program),
        false => WATCHERS.put(ctx, &program, &deliver),
    }
    Ok(())
}

pub(crate) fn tell(ctx: &ExecCtx, row: &SnapshotRow) -> Result<(), Error> {
    let encoded = abi::encode(row);
    for (program, deliver) in WATCHERS.all(ctx)? {
        let mut payload = deliver;
        payload.extend_from_slice(&encoded);
        ctx.emit_raw(program, payload, Reply::Wanted);
    }
    Ok(())
}

pub(crate) fn migrate(
    ctx: &ExecCtx,
    height: u64,
    first: Option<ProgramId>,
    last: Option<ProgramId>,
) -> Result<(), Error> {
    let renamed = ctx.renames(height)?;
    let mut range = WATCHERS.prefix_of(&());
    if let Some(first) = first {
        range.start = WATCHERS.key(&first);
    }
    if let Some(last) = last {
        range.end = Some([WATCHERS.key(&last), vec![0]].concat());
    }
    for (program, deliver) in WATCHERS.scan(ctx, range)? {
        let Some(to) = renamed.moved(&program) else {
            continue;
        };
        WATCHERS.remove(ctx, &program);
        if !WATCHERS.has(ctx, to) {
            WATCHERS.put(ctx, to, &deliver);
        }
    }
    Ok(())
}
