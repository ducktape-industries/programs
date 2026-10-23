//! One commit: each change checked against the `changed/` and `touched/` rows written since its
//! base, the head's tree edited change by change with every object on its path's line, then a new
//! snapshot that records its diff. A change rewrites its nearest guard; a move carries its guards.
use std::collections::{BTreeMap, BTreeSet};

use ::program::{
    AccountNumber, BlobId, Error, ExecCtx, Need, Principal, QueryCtx, Reply, corrupt, invalid,
    stale,
};

use crate::objects::{
    At, FILE, FileObj, SnapshotObj, check_chunks, digest, line, put_on, put_snapshot, under,
};
use crate::paths::{Roots, change_path, join};
use crate::state::{
    CHANGED, COUNT, DIFFS, HEAD, HISTORY, RECORDED_FROM, SNAPSHOTS, TOUCHED, minted, newest_first,
};
use crate::tree::{TreeEdit, node_at};
use crate::{Change, Content, DiffEntry, DiffKind, Node, SnapshotRow};

enum Step {
    Put {
        segments: Vec<String>,
        exec: bool,
        meta: BTreeMap<String, String>,
        size: u64,
        chunks: Vec<BlobId>,
    },
    Mkdir(Vec<String>),
    Remove(Vec<String>),
    Rename {
        from: Vec<String>,
        to: Vec<String>,
    },
    Symlink {
        segments: Vec<String>,
        target: String,
    },
}

impl Step {
    fn paths(&self) -> Vec<&[String]> {
        match self {
            Step::Put { segments, .. }
            | Step::Mkdir(segments)
            | Step::Remove(segments)
            | Step::Symlink { segments, .. } => vec![segments],
            Step::Rename { from, to } => vec![from, to],
        }
    }
}

pub(crate) fn needs(changes: &[Change]) -> Vec<Need> {
    changes
        .iter()
        .flat_map(|change| match change {
            Change::Put { path, .. } | Change::Mkdir { path } | Change::Symlink { path, .. } => {
                vec![Need::write(path.clone())]
            }
            Change::Remove { path } => vec![Need::delete(path.clone())],
            Change::Move { from, to } => vec![Need::write(from.clone()), Need::write(to.clone())],
            Change::SetOwnership { .. } => Vec::new(),
        })
        .collect()
}

pub(crate) fn commit(
    ctx: &ExecCtx,
    author: Principal,
    base: Option<BlobId>,
    message: String,
    changes: Vec<Change>,
) -> Result<(), Error> {
    if changes.is_empty() {
        return Err(invalid("a commit carries at least one change"));
    }
    let base = base
        .as_ref()
        .map(|id| minted(ctx, id))
        .transpose()?
        .map(|row| row.seq);
    let head = HEAD.get(ctx)?;
    let steps = plan(ctx, changes)?;
    let alone = steps.len() == 1;
    let by = author.account();
    let roots = Roots::read(ctx)?;
    let mut edit = TreeEdit::over(head.as_ref().map(|row| row.root));
    let mut named = BTreeSet::new();
    let mut entries = Vec::new();
    for step in steps {
        if by.is_some() {
            for segments in step.paths() {
                roots.beneath(segments)?;
            }
        }
        unchanged_since_base(ctx, base, head.as_ref(), &step.paths())?;
        named.extend(step.paths().into_iter().map(<[String]>::to_vec));
        apply(ctx, by, alone, &roots, &mut edit, &mut entries, step)?;
    }
    let (root, _) = edit.build(ctx)?;
    let snapshot = SnapshotObj {
        root,
        parent: head.map(|row| row.id),
        author,
        height: ctx.env().height,
        time: ctx.env().time,
        message,
    };
    let id = put_snapshot(ctx, &snapshot)?;
    let row = record(ctx, id, snapshot, &named, entries)?;
    crate::watchers::tell(ctx, &row)?;
    ctx.set_return_data(abi::encode(&id));
    Ok(())
}

fn plan(ctx: &QueryCtx, changes: Vec<Change>) -> Result<Vec<Step>, Error> {
    let mut seen = BTreeSet::new();
    let mut touch = |segments: &Vec<String>| {
        let fresh = seen.insert(segments.clone());
        if !fresh {
            return Err(invalid(format!(
                "{} is touched twice in one commit: combine its changes into a single entry",
                join(segments)
            )));
        }
        Ok(())
    };
    let mut steps = Vec::with_capacity(changes.len());
    for change in changes {
        let step = match change {
            Change::Put {
                path,
                exec,
                meta,
                content,
            } => {
                let segments = change_path(&path)?;
                touch(&segments)?;
                check_meta(&meta)?;
                let Content::Chunks { size, chunks } = content;
                check_chunks(ctx, size, &chunks)?;
                Step::Put {
                    segments,
                    exec,
                    meta,
                    size,
                    chunks,
                }
            }
            Change::Mkdir { path } => {
                let segments = change_path(&path)?;
                touch(&segments)?;
                Step::Mkdir(segments)
            }
            Change::Remove { path } => {
                let segments = change_path(&path)?;
                touch(&segments)?;
                Step::Remove(segments)
            }
            Change::Move { from, to } => {
                let from = change_path(&from)?;
                let to = change_path(&to)?;
                touch(&from)?;
                touch(&to)?;
                Step::Rename { from, to }
            }
            Change::Symlink { path, target } => {
                let segments = change_path(&path)?;
                touch(&segments)?;
                if target.is_empty() {
                    return Err(invalid("a symlink's target is not empty"));
                }
                Step::Symlink { segments, target }
            }
            Change::SetOwnership { .. } => return Err(retired()),
        };
        steps.push(step);
    }
    Ok(steps)
}

pub(crate) fn retired() -> Error {
    ::program::unauthorized("a path's write list is set in the access program, by the path's owner")
}

fn check_meta(meta: &BTreeMap<String, String>) -> Result<(), Error> {
    if meta.keys().any(String::is_empty) {
        return Err(invalid("a metadata key is not empty"));
    }
    Ok(())
}

fn changed_since(segments: &[String]) -> Error {
    stale(format!(
        "{} changed since the base snapshot: read it again and retry",
        join(segments)
    ))
}

fn moved_since(
    ctx: &QueryCtx,
    segments: &[String],
    moved: &[String],
    seq: u64,
) -> Result<Error, Error> {
    let row = HISTORY.get(ctx, &newest_first(seq))?.ok_or_else(|| {
        corrupt(
            "history/",
            &newest_first(seq).to_be_bytes(),
            "files recorded a change by a commit it keeps no row of",
        )
    })?;
    let what = match moved.len() == segments.len() {
        true => "it".to_owned(),
        false => format!("{} above it", join(moved)),
    };
    let by = match &row.author {
        Principal::Account(number) => format!("account {number}"),
        Principal::Root => "the network".to_owned(),
        Principal::Foreign(foreign) => foreign.to_string(),
    };
    Ok(stale(format!(
        "{} changed since the base snapshot: snapshot {} (seq {}, by {by} at block {}, {:?}) changed {what}; read it at the head and retry",
        join(segments),
        abi::hex(row.id.digest()),
        row.seq,
        row.height,
        row.message
    )))
}

fn unchanged_since_base(
    ctx: &QueryCtx,
    base: Option<u64>,
    head: Option<&SnapshotRow>,
    touched: &[&[String]],
) -> Result<(), Error> {
    let Some(head) = head else {
        return Ok(());
    };
    let Some(base) = base else {
        for segments in touched {
            if node_at(ctx, &head.root, segments, At::Head)?.is_some() {
                return Err(changed_since(segments));
            }
        }
        return Ok(());
    };
    if base == head.seq {
        return Ok(());
    }
    let recorded = RECORDED_FROM
        .get(ctx)?
        .is_some_and(|first| base + 1 >= first);
    if !recorded {
        return Err(stale(
            "the base snapshot is older than the changes files records: read again and retry",
        ));
    }
    let after = |seq: Option<u64>| seq.filter(|seq| *seq > base);
    for segments in touched {
        let mut newest = after(CHANGED.get(ctx, &join(segments))?).map(|seq| (seq, segments.len()));
        for len in 1..segments.len() {
            let folder = after(TOUCHED.get(ctx, &join(&segments[..len]))?).map(|seq| (seq, len));
            newest = newest.max(folder);
        }
        if let Some((seq, len)) = newest {
            return Err(moved_since(ctx, segments, &segments[..len], seq)?);
        }
    }
    Ok(())
}

fn entry(segments: &[String], kind: DiffKind) -> DiffEntry {
    DiffEntry {
        path: join(segments),
        kind,
    }
}

fn apply(
    ctx: &ExecCtx,
    by: Option<AccountNumber>,
    alone: bool,
    roots: &Roots,
    edit: &mut TreeEdit,
    entries: &mut Vec<DiffEntry>,
    step: Step,
) -> Result<(), Error> {
    match &step {
        Step::Put { segments, .. } | Step::Symlink { segments, .. } => {
            crate::guards::replace(ctx, segments)?;
            if by.is_some() {
                crate::guards::claim_carried(ctx, roots, segments)?;
            }
        }
        Step::Mkdir(segments) => {
            crate::guards::touch(ctx, segments)?;
            if by.is_some() {
                crate::guards::claim_carried(ctx, roots, segments)?;
            }
        }
        Step::Remove(segments) => crate::guards::remove(ctx, segments)?,
        Step::Rename { from, to } => crate::guards::rename(ctx, from, to, alone)?,
    }
    match step {
        Step::Put {
            segments,
            exec,
            meta,
            size,
            chunks,
        } => {
            let kind = written(ctx, edit, &segments)?;
            ctx.blob_release_lines(under(&segments));
            let body = abi::encode(&FileObj { size, chunks, meta });
            let file = put_on(ctx, FILE, body.clone(), line(&segments))?;
            edit.keep(file, body);
            edit.put(ctx, &segments, Node::File { file, size, exec }, by, roots)?;
            made(edit, entries);
            entries.push(entry(&segments, kind));
        }
        Step::Mkdir(segments) => {
            edit.mkdir(ctx, &segments, by, roots)?;
            made(edit, entries);
            entries.push(entry(&segments, DiffKind::Added));
        }
        Step::Remove(segments) => {
            edit.remove(ctx, &segments)?;
            ctx.blob_release_lines(under(&segments));
            entries.push(entry(&segments, DiffKind::Removed));
        }
        Step::Rename { from, to } => {
            edit.rename(ctx, &from, &to)?;
            ctx.blob_release_lines(under(&from));
            entries.push(entry(&from, DiffKind::Removed));
            entries.push(entry(&to, DiffKind::Added));
            if let Some(checker) = ::program::access_checker(&ctx.env().roles).cloned() {
                let carried = access::Op::Move {
                    program: crate::PROGRAM.into(),
                    from: join(&from),
                    to: join(&to),
                };
                ctx.emit(checker, &carried, Reply::None);
            }
        }
        Step::Symlink { segments, target } => {
            let kind = written(ctx, edit, &segments)?;
            ctx.blob_release_lines(under(&segments));
            edit.put(ctx, &segments, Node::Symlink { target }, by, roots)?;
            made(edit, entries);
            entries.push(entry(&segments, kind));
        }
    }
    Ok(())
}

fn written(ctx: &QueryCtx, edit: &TreeEdit, segments: &[String]) -> Result<DiffKind, Error> {
    Ok(match edit.holds(ctx, segments)? {
        true => DiffKind::Modified,
        false => DiffKind::Added,
    })
}

fn made(edit: &mut TreeEdit, entries: &mut Vec<DiffEntry>) {
    for dir in edit.made() {
        entries.push(entry(&dir, DiffKind::Added));
    }
}

fn record(
    ctx: &ExecCtx,
    id: BlobId,
    snapshot: SnapshotObj,
    named: &BTreeSet<Vec<String>>,
    entries: Vec<DiffEntry>,
) -> Result<SnapshotRow, Error> {
    let seq = COUNT.get(ctx)?.unwrap_or(0) + 1;
    let row = SnapshotRow {
        id,
        seq,
        parent: snapshot.parent,
        root: snapshot.root,
        author: snapshot.author,
        height: snapshot.height,
        time: snapshot.time,
        message: snapshot.message,
    };
    COUNT.put(ctx, &seq);
    HISTORY.put(ctx, &newest_first(seq), &row);
    SNAPSHOTS.put(ctx, &digest(&id)?, &seq);
    HEAD.put(ctx, &row);
    let mut changed = BTreeSet::new();
    for segments in named {
        TOUCHED.put(ctx, &join(segments), &seq);
        changed.extend((1..=segments.len()).map(|len| join(&segments[..len])));
    }
    for path in changed {
        CHANGED.put(ctx, &path, &seq);
    }
    DIFFS.put(ctx, &seq, &entries);
    if RECORDED_FROM.get(ctx)?.is_none() {
        RECORDED_FROM.put(ctx, &seq);
    }
    Ok(row)
}
