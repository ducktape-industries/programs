//! The writes besides a commit: naming a snapshot with a pin, or dropping one. Who drops a pin
//! is its key's list in the access program, its owner alone by default.
use ::program::{BlobId, Error, ExecCtx, Principal, already_exists, invalid, not_found};

use crate::PinRow;
use crate::state::{PINS, minted};

pub(crate) fn pin(
    ctx: &ExecCtx,
    owner: Principal,
    name: String,
    snapshot: BlobId,
) -> Result<(), Error> {
    let readable = !name.is_empty() && !name.chars().any(char::is_control);
    if !readable {
        return Err(invalid(
            "a pin's name is not empty and has no control characters",
        ));
    }
    minted(ctx, &snapshot)?;
    if PINS.has(ctx, &name) {
        return Err(already_exists(format!(
            "a pin named {name} exists: choose a different pin name or remove the existing pin"
        )));
    }
    let row = PinRow {
        name: name.clone(),
        snapshot,
        owner,
        height: ctx.env().height,
    };
    PINS.put(ctx, &name, &row);
    Ok(())
}

pub(crate) fn unpin(ctx: &ExecCtx, name: String) -> Result<(), Error> {
    if !PINS.has(ctx, &name) {
        return Err(not_found(format!("no pin named {name}")));
    }
    PINS.remove(ctx, &name);
    Ok(())
}
