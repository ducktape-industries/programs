//! The rules: a program schedules only as itself and only for a later height, only the program
//! that scheduled an entry cancels it and only before it runs, and only the kernel says it ran.
use abi::Receipt;
use program::{Error, ExecCtx, ProgramId, QueryCtx, invalid, not_found, unauthorized, wrong_state};

use crate::state::{DUE, ENTRIES, NEXT};
use crate::{Entry, EntryId, Reply};

fn kept(ctx: &QueryCtx, id: EntryId) -> Result<Entry, Error> {
    ENTRIES
        .get(ctx, &id)?
        .ok_or_else(|| not_found(format!("no entry {id}")))
}

fn unran(entry: &Entry) -> Result<(), Error> {
    let ran = entry.receipt.is_some();
    if ran {
        return Err(wrong_state(format!("entry {} ran", entry.id)));
    }
    Ok(())
}

pub(crate) fn schedule(
    ctx: &ExecCtx,
    height: u64,
    target: ProgramId,
    payload: Vec<u8>,
) -> Result<(), Error> {
    let scheduler = ctx.env().sending_program()?;
    let now = ctx.env().height;
    let later = height > now;
    if !later {
        return Err(invalid(format!(
            "a run is scheduled for a height after {now}, not {height}"
        )));
    }
    let id = NEXT.get(ctx)?.unwrap_or(0);
    let entry = Entry {
        id,
        scheduler,
        height,
        target,
        payload,
        receipt: None,
    };
    NEXT.put(ctx, &(id + 1));
    ENTRIES.put(ctx, &id, &entry);
    DUE.insert(ctx, &(height, id));
    ctx.set_return_data(abi::encode(&id));
    Ok(())
}

pub(crate) fn cancel(ctx: &ExecCtx, id: EntryId) -> Result<(), Error> {
    let canceller = ctx.env().sending_program()?;
    let entry = kept(ctx, id)?;
    let theirs = entry.scheduler == canceller;
    if !theirs {
        return Err(unauthorized(format!(
            "entry {id} is {}'s to cancel",
            entry.scheduler
        )));
    }
    unran(&entry)?;
    ENTRIES.remove(ctx, &id);
    DUE.remove(ctx, &(entry.height, id));
    Ok(())
}

pub(crate) fn ran(ctx: &ExecCtx, id: EntryId, receipt: Receipt) -> Result<(), Error> {
    ctx.env().kernel()?;
    let mut entry = kept(ctx, id)?;
    unran(&entry)?;
    DUE.remove(ctx, &(entry.height, id));
    entry.receipt = Some(receipt);
    ENTRIES.put(ctx, &id, &entry);
    Ok(())
}

pub(crate) fn due(
    ctx: &QueryCtx,
    height: u64,
    after: Option<EntryId>,
    limit: Option<u32>,
) -> Result<Reply, Error> {
    let mut range = DUE.prefix_of(&height);
    if let Some(after) = after {
        range = range.after(DUE.key(&(height, after)));
    }
    range.limit = limit.map(u64::from);
    let mut entries = Vec::new();
    for (_, id) in DUE.scan(ctx, range)? {
        entries.push(kept(ctx, id)?);
    }
    let full = limit.is_some_and(|limit| entries.len() == limit as usize);
    let next = match full {
        true => entries.last().map(|entry| entry.id),
        false => None,
    };
    Ok(Reply::Due { entries, next })
}

pub(crate) fn migrate(
    ctx: &ExecCtx,
    height: u64,
    first: Option<(u64, EntryId)>,
    last: Option<(u64, EntryId)>,
) -> Result<(), Error> {
    let renamed = ctx.renames(height)?;
    let mut range = DUE.prefix_of(&());
    if let Some(first) = first {
        range.start = DUE.key(&first);
    }
    if let Some(last) = last {
        range.end = Some([DUE.key(&last), vec![0]].concat());
    }
    for (_, id) in DUE.scan(ctx, range)? {
        let entry = kept(ctx, id)?;
        let moved = Entry {
            scheduler: renamed.id(&entry.scheduler),
            target: renamed.id(&entry.target),
            ..entry.clone()
        };
        if moved != entry {
            ENTRIES.put(ctx, &id, &moved);
        }
    }
    Ok(())
}

pub(crate) fn entry(ctx: &QueryCtx, id: EntryId) -> Result<Option<Entry>, Error> {
    ENTRIES.get(ctx, &id)
}
