//! A streaming reply as rows that grow with it: the thread's [`Partial`] keeps each said block
//! without its text, and the text each event adds is a piece of its own, written once.
use ::program::{Error, ExecCtx, QueryCtx};

use crate::state::{PARTIALS, PIECES};
use crate::{Partial, Said};

fn text(said: &Said) -> &str {
    match said {
        Said::Text(text)
        | Said::Thinking { text, .. }
        | Said::RedactedThinking(text)
        | Said::ServerToolResult(text)
        | Said::Cited { text, .. }
        | Said::ToolUse { input: text, .. }
        | Said::ServerToolUse { input: text, .. } => text,
    }
}

fn text_mut(said: &mut Said) -> &mut String {
    match said {
        Said::Text(text)
        | Said::Thinking { text, .. }
        | Said::RedactedThinking(text)
        | Said::ServerToolResult(text)
        | Said::Cited { text, .. }
        | Said::ToolUse { input: text, .. }
        | Said::ServerToolUse { input: text, .. } => text,
    }
}

pub(crate) fn read(ctx: &QueryCtx, thread: u64) -> Result<Option<Partial>, Error> {
    let Some(mut partial) = PARTIALS.get(ctx, &thread)? else {
        return Ok(None);
    };
    for ((_, block, _), piece) in PIECES.scan(ctx, PIECES.prefix_of(&thread))? {
        let at = usize::try_from(block).ok();
        if let Some(said) = at.and_then(|at| partial.said.get_mut(at)) {
            text_mut(said).push_str(&piece);
        }
    }
    Ok(Some(partial))
}

pub(crate) fn keep(
    ctx: &ExecCtx,
    thread: u64,
    seq: u64,
    before: &Partial,
    after: &Partial,
) -> Result<(), Error> {
    let whole = PARTIALS
        .get(ctx, &thread)?
        .is_some_and(|head| head.said.iter().any(|said| !text(said).is_empty()));
    let mut head = after.clone();
    for (at, said) in head.said.iter_mut().enumerate() {
        let now = std::mem::take(text_mut(said));
        let block = at as u64;
        let was = before.said.get(at).map(text).filter(|_| !whole);
        match was.and_then(|was| now.strip_prefix(was)) {
            Some("") => {}
            Some(grown) => PIECES.put(ctx, &(thread, block, seq), &grown.to_owned()),
            None => {
                forget(ctx, thread, block)?;
                if !now.is_empty() {
                    PIECES.put(ctx, &(thread, block, seq), &now);
                }
            }
        }
    }
    for at in head.said.len()..before.said.len() {
        forget(ctx, thread, at as u64)?;
    }
    PARTIALS.put(ctx, &thread, &head);
    Ok(())
}

fn forget(ctx: &ExecCtx, thread: u64, block: u64) -> Result<(), Error> {
    for (key, _) in PIECES.scan(ctx, PIECES.prefix_of(&(thread, block)))? {
        PIECES.remove(ctx, &key);
    }
    Ok(())
}

pub(crate) fn clear(ctx: &ExecCtx, thread: u64) -> Result<(), Error> {
    PARTIALS.remove(ctx, &thread);
    for (key, _) in PIECES.scan(ctx, PIECES.prefix_of(&thread))? {
        PIECES.remove(ctx, &key);
    }
    Ok(())
}
