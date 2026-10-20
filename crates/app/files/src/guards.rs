//! Guarded paths, kind `files.path`: a row only where an owner narrowed who writes a path and
//! everything beneath it. Each write rewrites the nearest row at or above the path it touches, so
//! the kernel's guard decides it by that row's policy; with no row at or above, anyone writes.
use ::program::{
    Error, ExecCtx, Party, QueryCtx, Range, Reply, already_exists, not_found, unauthorized,
    wrong_state,
};

use crate::gate::resource;
use crate::paths::{CHAT, change_path, join};
use crate::state::{GUARDED, PathGuard};
use crate::{Op, PROGRAM};

pub(crate) fn nearest(
    ctx: &QueryCtx,
    segments: &[String],
) -> Result<Option<(String, PathGuard)>, Error> {
    for len in (1..=segments.len()).rev() {
        let path = join(&segments[..len]);
        if let Some(row) = GUARDED.get(ctx, &path)? {
            return Ok(Some((path, row)));
        }
    }
    Ok(None)
}

pub(crate) fn touch(ctx: &ExecCtx, segments: &[String]) -> Result<(), Error> {
    if let Some((path, row)) = nearest(ctx, segments)? {
        GUARDED.put(ctx, &path, &PathGuard { rev: row.rev + 1 });
    }
    Ok(())
}

pub(crate) fn at_or_beneath(
    ctx: &QueryCtx,
    segments: &[String],
) -> Result<Vec<(String, PathGuard)>, Error> {
    let path = join(segments);
    let mut rows: Vec<_> = GUARDED
        .get(ctx, &path)?
        .map(|row| (path.clone(), row))
        .into_iter()
        .collect();
    let mut beneath = GUARDED.key(&());
    beneath.extend_from_slice(path.as_bytes());
    beneath.push(b'/');
    rows.extend(GUARDED.scan(ctx, Range::prefix(beneath))?);
    Ok(rows)
}

pub(crate) fn carried(segments: &[String]) -> bool {
    segments.len() > 3 && segments[0] == CHAT
}

pub(crate) fn claim_carried(ctx: &ExecCtx, segments: &[String]) -> Result<(), Error> {
    let path = join(segments);
    if carried(segments) && !GUARDED.has(ctx, &path) {
        GUARDED.put(ctx, &path, &PathGuard { rev: 0 });
    }
    Ok(())
}

pub(crate) fn replace(ctx: &ExecCtx, segments: &[String]) -> Result<(), Error> {
    touch(ctx, segments)?;
    let path = join(segments);
    for (beneath, _) in at_or_beneath(ctx, segments)? {
        if beneath != path {
            GUARDED.remove(ctx, &beneath);
        }
    }
    Ok(())
}

pub(crate) fn remove(ctx: &ExecCtx, segments: &[String]) -> Result<(), Error> {
    touch(ctx, segments)?;
    for (path, _) in at_or_beneath(ctx, segments)? {
        GUARDED.remove(ctx, &path);
    }
    Ok(())
}

pub(crate) fn rename(
    ctx: &ExecCtx,
    from: &[String],
    to: &[String],
    alone: bool,
) -> Result<(), Error> {
    if let Some((path, _)) = at_or_beneath(ctx, to)?.into_iter().next() {
        return Err(wrong_state(format!(
            "{path} keeps a guard of its own: Unguard it before moving a path there"
        )));
    }
    touch(ctx, from)?;
    touch(ctx, to)?;
    let moved = at_or_beneath(ctx, from)?;
    if moved.is_empty() {
        return Ok(());
    }
    let (old, new) = (join(from), join(to));
    if !alone {
        return Err(wrong_state(format!(
            "{old} keeps guards beneath it, whose owners and policies move only once this commit \
             ends: move it in a commit of its own, then write there"
        )));
    }
    let ownership = ctx.env().role(abi::role::OWNERSHIP).ok().cloned();
    let mut vacated = Vec::with_capacity(moved.len());
    for (path, row) in moved {
        let landed = format!("{new}{}", &path[old.len()..]);
        GUARDED.put(ctx, &path, &PathGuard { rev: row.rev + 1 });
        ctx.set(GUARDED.key(&landed), abi::encode(&row));
        if let Some(role) = &ownership {
            let carried = ownership::Op::Moved {
                from_key: GUARDED.key(&path),
                to_key: GUARDED.key(&landed),
            };
            ctx.emit(role.clone(), &carried, Reply::None);
        }
        vacated.push(path);
    }
    ctx.emit(PROGRAM, &Op::Vacate { paths: vacated }, Reply::None);
    Ok(())
}

pub(crate) fn vacate(ctx: &ExecCtx, paths: Vec<String>) -> Result<(), Error> {
    if !ownership::by_itself(ctx)? {
        return Err(unauthorized(
            "files alone vacates the guards a move carried away, in its own message",
        ));
    }
    for path in paths {
        GUARDED.remove(ctx, &path);
    }
    Ok(())
}

pub(crate) fn guard(ctx: &ExecCtx, path: String) -> Result<(), Error> {
    let segments = change_path(&path)?;
    let path = join(&segments);
    if GUARDED.has(ctx, &path) {
        return Err(already_exists(format!(
            "{path} is guarded already: change its policy with ownership Relink"
        )));
    }
    let owner = resource(ctx, &path)?
        .owner
        .ok_or_else(|| not_found(format!("nothing owns {path}: nothing is there yet")))?;
    let env = ctx.env();
    let theirs = match &owner {
        Party::Account(number) => env.sender.as_ref().and_then(|by| by.account()) == Some(*number),
        Party::Program(program) => env.sent_by(program).is_ok(),
    };
    if !theirs {
        return Err(unauthorized(format!(
            "{path} is {}'s: its owner alone guards it",
            party(&owner)
        )));
    }
    GUARDED.put(ctx, &path, &PathGuard { rev: 0 });
    Ok(())
}

pub(crate) fn unguard(ctx: &ExecCtx, path: String) -> Result<(), Error> {
    let path = join(&change_path(&path)?);
    if !GUARDED.has(ctx, &path) {
        return Err(not_found(format!("{path} keeps no guard of its own")));
    }
    GUARDED.remove(ctx, &path);
    Ok(())
}

pub(crate) fn guarded(ctx: &QueryCtx, path: &str) -> Result<Option<String>, Error> {
    let segments = change_path(path)?;
    Ok(nearest(ctx, &segments)?.map(|(path, _)| path))
}

fn party(party: &Party) -> String {
    match party {
        Party::Account(number) => format!("account {number}"),
        Party::Program(program) => format!("program {program}"),
    }
}
