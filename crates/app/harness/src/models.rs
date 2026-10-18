//! The model records: an id the first account to set it owns, the wire the model speaks, the
//! egress service that reaches it, the vendor's name for it, the context window harness learns
//! from the vendor, and its `compact_at` default. Only its owner changes or removes a record.
use ::program::{Error, ExecCtx, QueryCtx, invalid, not_found};
use store::{PageRequest, PageResponse};

use crate::state::{COMPACT_AT, MODELS, StoredModel};
use crate::{ModelRow, Window, Wire};

fn lowercase_name(text: &str) -> bool {
    !text.is_empty()
        && text.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_.".contains(&byte)
        })
}

fn checked(id: &str, service: &str, name: &str) -> Result<(), Error> {
    if !lowercase_name(id) {
        return Err(invalid(
            "a model id is lowercase letters, digits, -, _ and .",
        ));
    }
    let reserved = abi::role::egress::RESERVED.contains(&service);
    if !lowercase_name(service) || reserved {
        return Err(invalid(
            "the model is reached through a registered egress service, not http or relay",
        ));
    }
    let vendor_named = !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._@:".contains(&byte));
    if !vendor_named {
        return Err(invalid(
            "a vendor's model name is letters, digits, -, ., _, @ and :",
        ));
    }
    Ok(())
}

pub(crate) fn model(ctx: &QueryCtx, id: &str) -> Result<Option<ModelRow>, Error> {
    let id = id.to_owned();
    let Some(stored) = MODELS.get(ctx, &id)? else {
        return Ok(None);
    };
    Ok(Some(stored.row(COMPACT_AT.get(ctx, &id)?)))
}

pub(crate) fn models(ctx: &QueryCtx, page: &PageRequest) -> Result<PageResponse<ModelRow>, Error> {
    let page = MODELS.range(ctx, page)?;
    page.try_map(|(id, stored)| Ok(stored.row(COMPACT_AT.get(ctx, &id)?)))
}

pub(crate) fn set_model(
    ctx: &ExecCtx,
    id: String,
    wire: Wire,
    service: String,
    name: String,
) -> Result<(), Error> {
    checked(&id, &service, &name)?;
    let existing = MODELS.get(ctx, &id)?;
    let owner = match &existing {
        Some(row) => row.owner,
        None => ctx.sender_account()?,
    };
    let same_model = |row: &StoredModel| row.wire == wire && row.name == name;
    let window = existing
        .filter(same_model)
        .map_or(Window::Unasked, |row| row.window);
    let row = StoredModel {
        id: id.clone(),
        owner,
        wire,
        service,
        name,
        window,
    };
    MODELS.put(ctx, &id, &row);
    Ok(())
}

pub(crate) fn remove_model(ctx: &ExecCtx, id: String) -> Result<(), Error> {
    if !MODELS.has(ctx, &id) {
        return Err(not_found(format!("no model {id}")));
    }
    MODELS.remove(ctx, &id);
    COMPACT_AT.remove(ctx, &id);
    Ok(())
}

pub(crate) fn set_compact_at(
    ctx: &ExecCtx,
    id: String,
    compact_at: Option<u64>,
) -> Result<(), Error> {
    if !MODELS.has(ctx, &id) {
        return Err(not_found(format!("no model {id}")));
    }
    match compact_at {
        Some(0) => return Err(invalid("compact_at, when given, is at least 1 token")),
        Some(tokens) => COMPACT_AT.put(ctx, &id, &tokens),
        None => COMPACT_AT.remove(ctx, &id),
    }
    Ok(())
}

pub(crate) fn learned(ctx: &ExecCtx, asked: &ModelRow, window: Window) -> Result<(), Error> {
    let Some(mut row) = MODELS.get(ctx, &asked.id)? else {
        return Ok(());
    };
    let still_asked = row.wire == asked.wire && row.name == asked.name;
    if still_asked {
        row.window = window;
        MODELS.put(ctx, &asked.id, &row);
    }
    Ok(())
}
