//! Watchers: programs told of every ref forge moves and each change's events (`Watched`), each
//! event a message of its own (`deliver ++ borsh(Watched)`) whose refusal never fails forge's
//! write.
use ::program::{Error, ExecCtx, Reply};

use crate::contract::Watched;
use crate::state::WATCHERS;

pub(crate) fn watch(ctx: &ExecCtx, deliver: Vec<u8>) -> Result<(), Error> {
    let program = ctx.env().sending_program()?;
    match deliver.is_empty() {
        true => WATCHERS.remove(ctx, &program),
        false => WATCHERS.put(ctx, &program, &deliver),
    }
    Ok(())
}

pub(crate) fn tell(ctx: &ExecCtx, event: &Watched) -> Result<(), Error> {
    let encoded = abi::encode(event);
    for (program, deliver) in WATCHERS.all(ctx)? {
        let mut payload = deliver;
        payload.extend_from_slice(&encoded);
        ctx.emit_raw(program, payload, Reply::Wanted);
    }
    Ok(())
}
