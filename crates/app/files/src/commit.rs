//! One commit: every change checked, every touched path compared between the base
//! and the head, then the head's tree edited change by change, written back and named by a
//! new snapshot. What each change needs of the access program is [`needs`]; a move carries
//! the lists at and below its path to where it lands.
use std::collections::{BTreeMap, BTreeSet};

use ::program::{
    AccountNumber, BlobId, Error, ExecCtx, Need, Principal, QueryCtx, Reply, invalid, stale,
};

use crate::objects::{FileObj, SnapshotObj, check_chunks, digest, put_file, put_snapshot};
use crate::paths::{beneath_the_roots, change_path, join};
use crate::state::{COUNT, HEAD, HISTORY, SNAPSHOTS, minted, newest_first};
use crate::tree::{TreeEdit, node_at};
use crate::{Change, Content, Node, SnapshotRow, carried};

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
    let base_root = base
        .as_ref()
        .map(|id| minted(ctx, id))
        .transpose()?
        .map(|row| row.root);
    let head = HEAD.get(ctx)?;
    let head_root = head.as_ref().map(|row| row.root);
    let steps = plan(ctx, changes)?;
    let by = author.account();
    let mut edit = TreeEdit::over(head_root);
    for step in steps {
        if by.is_some() {
            step.paths().into_iter().try_for_each(beneath_the_roots)?;
        }
        unchanged_since_base(ctx, base_root, head_root, &step.paths())?;
        apply(ctx, by, &mut edit, step)?;
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
    record(ctx, id, snapshot)?;
    ctx.set_return_data(abi::encode(&id));
    Ok(())
}

fn plan(ctx: &QueryCtx, changes: Vec<Change>) -> Result<Vec<Step>, Error> {
    let mut seen = BTreeSet::new();
    let mut touch = |segments: &Vec<String>| {
        let fresh = seen.insert(segments.clone());
        if !fresh {
            return Err(invalid(format!(
                "{} is touched twice in one commit",
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
                let Content::Chunks { size, chunks } = content else {
                    return Err(carried());
                };
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

fn unchanged_since_base(
    ctx: &QueryCtx,
    base: Option<BlobId>,
    head: Option<BlobId>,
    touched: &[&[String]],
) -> Result<(), Error> {
    if base == head {
        return Ok(());
    }
    let node = |root: Option<BlobId>, segments: &[String]| match root {
        Some(root) => node_at(ctx, &root, segments),
        None => Ok(None),
    };
    for segments in touched {
        if node(base, segments)? != node(head, segments)? {
            return Err(stale(format!(
                "{} changed since the base snapshot: read it again and retry",
                join(segments)
            )));
        }
    }
    Ok(())
}

fn apply(
    ctx: &ExecCtx,
    by: Option<AccountNumber>,
    edit: &mut TreeEdit,
    step: Step,
) -> Result<(), Error> {
    match step {
        Step::Put {
            segments,
            exec,
            meta,
            size,
            chunks,
        } => {
            let file = put_file(ctx, &FileObj { size, chunks, meta })?;
            edit.put(ctx, &segments, Node::File { file, size, exec }, by)
        }
        Step::Mkdir(segments) => edit.mkdir(ctx, &segments, by),
        Step::Remove(segments) => edit.remove(ctx, &segments),
        Step::Rename { from, to } => {
            edit.rename(ctx, &from, &to)?;
            let carried = access::Op::Move {
                program: crate::PROGRAM.into(),
                from: join(&from),
                to: join(&to),
            };
            let checker = ctx.env().role(abi::role::ACCESS)?.clone();
            ctx.emit(checker, &carried, Reply::None);
            Ok(())
        }
        Step::Symlink { segments, target } => {
            edit.put(ctx, &segments, Node::Symlink { target }, by)
        }
    }
}

fn record(ctx: &ExecCtx, id: BlobId, snapshot: SnapshotObj) -> Result<(), Error> {
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
    Ok(())
}
