//! Watchers: programs told of every commit files makes, each as a message of its own
//! (`deliver ++ borsh(SnapshotRow)`) whose refusal never fails the commit.
use ::program::{Error, ExecCtx, Reply};

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
