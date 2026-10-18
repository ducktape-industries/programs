//! Structured hunks are built from git::diff's edit ranges, never parsed
//! from patch text.
use crate::contract::*;
use crate::ops::cap;
use crate::reads::{Reading, entry_kind};
use ::program::Error;
use ::program::invalid;
use gitcore::{Mode, Oid, diff};
use store::Listing;

pub fn query(
    r: &Reading<'_>,
    height: u64,
    base: &Option<String>,
    head: &str,
    path: Option<&[u8]>,
    paging: &Listing,
) -> Result<Reply, Error> {
    if let Some(path) = path {
        crate::changes::check_path(path, false)?;
    }
    let old = base.as_ref().map(|s| r.tree_id(s)).transpose()?;
    let new = r.tree_id(head)?;
    let changes = r.result(diff::tree_diff(&r.store, old.as_ref(), Some(&new)))?;
    let changes: Vec<_> = changes
        .into_iter()
        .filter(|c| path.is_none_or(|p| c.path == p))
        .collect();
    let page = paging.slice(&changes)?.try_map(|c| file(r, &c))?;
    Ok(Reply::Diff {
        height,
        base: base
            .as_ref()
            .map(|s| r.oid(s).map(|o| o.to_hex()))
            .transpose()?,
        head: r.oid(head)?.to_hex(),
        total_files: changes.len() as u64,
        page,
    })
}

// A `Diff` reply as `DiffStat` answers it: each file's path as text and its counts, no hunks.
pub fn stat(reply: Reply) -> Result<Reply, Error> {
    let Reply::Diff {
        height,
        base,
        head,
        total_files,
        page,
    } = reply
    else {
        return Err(invalid("a diff answers Diff"));
    };
    let page = page.map(|file| {
        let path = file.new_path.as_ref().or(file.old_path.as_ref());
        FileStat {
            path: path.map_or_else(String::new, |path| {
                String::from_utf8_lossy(path).into_owned()
            }),
            status: file.status,
            old_kind: file.old_kind,
            new_kind: file.new_kind,
            old_size: file.old_size,
            new_size: file.new_size,
            content: file.content,
            additions: file.additions,
            deletions: file.deletions,
        }
    });
    Ok(Reply::DiffStat {
        height,
        base,
        head,
        total_files,
        page,
    })
}

/// One side of a file change: its mode and blob.
type Side = Option<(Mode, Oid)>;

fn file(r: &Reading<'_>, c: &diff::Change) -> Result<FileDiff, Error> {
    let (old, new, status) = sides(&c.kind);
    let old_size = size(r, old)?;
    let new_size = size(r, new)?;
    let mut result = FileDiff {
        old_path: old.map(|_| c.path.clone()),
        new_path: new.map(|_| c.path.clone()),
        status,
        old_oid: old.map(|(_, id)| id.to_hex()),
        new_oid: new.map(|(_, id)| id.to_hex()),
        old_kind: old.map(|(mode, _)| entry_kind(mode)),
        new_kind: new.map(|(mode, _)| entry_kind(mode)),
        old_size,
        new_size,
        content: Content::Text,
        additions: 0,
        deletions: 0,
        hunks: Vec::new(),
    };
    let gitlink = [old, new]
        .into_iter()
        .flatten()
        .any(|(m, _)| m == Mode::Gitlink);
    if gitlink {
        result.content = Content::Gitlink;
        return Ok(result);
    }
    if old_size > r.bounds.blob_bytes || new_size > r.bounds.blob_bytes {
        result.content = Content::Oversize;
        return Ok(result);
    }
    let read = |side: Side| side.map(|(_, id)| r.blob(&id, None)).transpose();
    let (old_blob, new_blob) = (read(old)?, read(new)?);
    let binary = [&old_blob, &new_blob]
        .into_iter()
        .flatten()
        .any(|b| b.content == Content::Binary);
    if binary {
        result.content = Content::Binary;
        return Ok(result);
    }
    let old = old_blob.as_ref().map_or(&[][..], |b| &b.bytes);
    let new = new_blob.as_ref().map_or(&[][..], |b| &b.bytes);
    // a diff that takes more than `merge_cost` edits to search is not
    // shown inline, like a blob past `blob_bytes`; the page goes on
    result.hunks = match hunks(old, new, cap(r.bounds.merge_cost)) {
        Err(gitcore::Error::CapReached) => {
            result.content = Content::Oversize;
            return Ok(result);
        }
        hunks => r.result(hunks)?,
    };
    for line in result.hunks.iter().flat_map(|h| &h.lines) {
        result.additions += u64::from(line.kind == LineKind::Added);
        result.deletions += u64::from(line.kind == LineKind::Deleted);
    }
    Ok(result)
}

/// A change's old and new sides, and what kind of change it is.
fn sides(kind: &diff::ChangeKind) -> (Side, Side, FileStatus) {
    use diff::ChangeKind as K;
    match *kind {
        K::Added { mode, id } => (None, Some((mode, id)), FileStatus::Added),
        K::Deleted { mode, id } => (Some((mode, id)), None, FileStatus::Deleted),
        K::Modified {
            old_mode,
            new_mode,
            old,
            new,
        } => {
            let retyped = old_mode.is_file() != new_mode.is_file()
                || (old_mode != new_mode && !old_mode.is_file());
            let status = if retyped {
                FileStatus::TypeChanged
            } else {
                FileStatus::Modified
            };
            (Some((old_mode, old)), Some((new_mode, new)), status)
        }
        K::ModeChanged {
            old_mode,
            new_mode,
            id,
        } => (
            Some((old_mode, id)),
            Some((new_mode, id)),
            FileStatus::ModeChanged,
        ),
    }
}

/// A side's blob size; a gitlink or an absent side has none.
fn size(r: &Reading<'_>, side: Side) -> Result<u64, Error> {
    match side {
        Some((Mode::Gitlink, _)) | None => Ok(0),
        Some((_, id)) => {
            let header = r.result(r.store.header(&id))?;
            if header.kind != "blob" {
                return Err(invalid("tree leaf must name a blob"));
            }
            Ok(header.len)
        }
    }
}

/// A diff line at its (old, new) row position.
type Row = (usize, usize, DiffLine);

/// Lines of context kept around each change.
const CONTEXT: usize = 3;

fn hunks(old: &[u8], new: &[u8], cost: usize) -> gitcore::Result<Vec<DiffHunk>> {
    let rows = rows(old, new, cost)?;
    Ok(groups(&rows)
        .into_iter()
        .map(|(start, end)| hunk(&rows, start, end))
        .collect())
}

/// Every line of both sides, in order, as context, deleted or added.
fn rows(old: &[u8], new: &[u8], cost: usize) -> gitcore::Result<Vec<Row>> {
    let a = diff::split_lines(old);
    let b = diff::split_lines(new);
    let line = |kind, old: Option<usize>, new: Option<usize>, bytes: &[u8]| DiffLine {
        kind,
        old_line: old.map(|i| i as u64 + 1),
        new_line: new.map(|j| j as u64 + 1),
        bytes: bytes.to_vec(),
    };
    let mut rows = Vec::new();
    for edit in diff::lines(old, new, cost)? {
        match edit.op {
            diff::Op::Equal => rows.extend(
                edit.old
                    .zip(edit.new)
                    .map(|(i, j)| (i, j, line(LineKind::Context, Some(i), Some(j), a[i]))),
            ),
            diff::Op::Delete => rows.extend(edit.old.map(|i| {
                let at = edit.new.start;
                (i, at, line(LineKind::Deleted, Some(i), None, a[i]))
            })),
            diff::Op::Insert => rows.extend(edit.new.map(|j| {
                let at = edit.old.start;
                (at, j, line(LineKind::Added, None, Some(j), b[j]))
            })),
        }
    }
    Ok(rows)
}

/// The row ranges each hunk covers: every change with its context, ranges
/// that touch merged.
fn groups(rows: &[Row]) -> Vec<(usize, usize)> {
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for (i, (_, _, line)) in rows.iter().enumerate() {
        if line.kind == LineKind::Context {
            continue;
        }
        let start = i.saturating_sub(CONTEXT);
        let end = (i + CONTEXT + 1).min(rows.len());
        if let Some(last) = groups.last_mut().filter(|last| start <= last.1) {
            last.1 = end;
        } else {
            groups.push((start, end));
        }
    }
    groups
}

fn hunk(rows: &[Row], start: usize, end: usize) -> DiffHunk {
    let lines: Vec<_> = rows[start..end]
        .iter()
        .map(|(_, _, line)| line.clone())
        .collect();
    let old_count = lines.iter().filter(|l| l.old_line.is_some()).count() as u64;
    let new_count = lines.iter().filter(|l| l.new_line.is_some()).count() as u64;
    DiffHunk {
        old: LineRange {
            start: rows[start].0 as u64 + u64::from(old_count != 0),
            count: old_count,
        },
        new: LineRange {
            start: rows[start].1 as u64 + u64::from(new_count != 0),
            count: new_count,
        },
        lines,
    }
}
