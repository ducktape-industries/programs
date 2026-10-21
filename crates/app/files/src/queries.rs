//! Every read: the head or a minted snapshot resolved first, then its trees walked.
//! A list, find or grep answers a page whose cursor stays on the snapshot it walked; an
//! ownership read answers a path's creator, its write list and who writes it.
use std::cmp::Ordering;
use std::collections::BTreeMap;

use ::program::{BlobId, Error, QueryCtx, invalid, not_found};

use crate::cursor::Walk;
use crate::objects::{digest, file, read as read_file, tree};
use crate::paths::{Roots, join, segments};
use crate::state::{HEAD, HISTORY, PINS, SNAPSHOTS, newest_first, resolve};
use crate::tree::node_at;
use crate::{
    Contents, DiffEntry, DiffKind, EntryInfo, Hit, Node, PageRequest, PageResponse, PinRow,
    SnapshotRow,
};

pub(crate) fn head(ctx: &QueryCtx) -> Result<Option<SnapshotRow>, Error> {
    HEAD.get(ctx)
}

pub(crate) fn snapshot(ctx: &QueryCtx, id: &BlobId) -> Result<Option<SnapshotRow>, Error> {
    let Ok(digest) = digest(id) else {
        return Ok(None);
    };
    let Some(seq) = SNAPSHOTS.get(ctx, &digest)? else {
        return Ok(None);
    };
    HISTORY.get(ctx, &newest_first(seq))
}

pub(crate) fn history(
    ctx: &QueryCtx,
    page: &PageRequest,
) -> Result<PageResponse<SnapshotRow>, Error> {
    Ok(HISTORY.range(ctx, page)?.map(|(_, row)| row))
}

pub(crate) fn pins(ctx: &QueryCtx, page: &PageRequest) -> Result<PageResponse<PinRow>, Error> {
    Ok(PINS.range(ctx, page)?.map(|(_, row)| row))
}

pub(crate) fn pin(ctx: &QueryCtx, name: &str) -> Result<Option<PinRow>, Error> {
    PINS.get(ctx, &name.to_owned())
}

fn info(ctx: &QueryCtx, path: String, node: Node) -> Result<EntryInfo, Error> {
    let meta = match &node {
        Node::File { file: id, .. } => file(ctx, id)?.meta,
        Node::Dir { .. } | Node::Symlink { .. } => BTreeMap::new(),
    };
    Ok(EntryInfo { path, node, meta })
}

fn root_node(ctx: &QueryCtx, root: &BlobId) -> Result<Node, Error> {
    let entries = tree(ctx, root)?.entries.len() as u64;
    Ok(Node::Dir {
        tree: *root,
        entries,
    })
}

pub(crate) fn stat(
    ctx: &QueryCtx,
    path: &str,
    at: Option<&BlobId>,
) -> Result<(Option<BlobId>, Option<EntryInfo>), Error> {
    let segments = segments(path)?;
    let Some(row) = resolve(ctx, at)? else {
        return Ok((None, None));
    };
    let node = match segments.is_empty() {
        true => Some(root_node(ctx, &row.root)?),
        false => node_at(ctx, &row.root, &segments)?,
    };
    let entry = node
        .map(|node| info(ctx, join(&segments), node))
        .transpose()?;
    Ok((Some(row.id), entry))
}

pub(crate) fn list(
    ctx: &QueryCtx,
    path: &str,
    at: Option<&BlobId>,
    page: &PageRequest,
) -> Result<(Option<BlobId>, PageResponse<EntryInfo>), Error> {
    let segments = segments(path)?;
    let row = resolve(ctx, at)?;
    let snapshot = row.as_ref().map(|row| row.id);
    let walk = Walk::open(ctx, page, &("list", &segments, snapshot))?;
    let dir = match &row {
        Some(row) => directory(ctx, &row.root, &segments)?,
        None => None,
    };
    let Some(dir) = dir else {
        let empty_root_is_listable =
            segments.is_empty() || Roots::read(ctx)?.is_namespace_root(&segments);
        if !empty_root_is_listable {
            return Err(not_found(format!("no directory {}", join(&segments))));
        }
        return Ok((snapshot, walk.page(Vec::new(), None)));
    };
    let mut rest = tree(ctx, &dir)?
        .entries
        .into_iter()
        .filter(|(name, _)| walk.after.as_ref().is_none_or(|after| name > after));
    let mut items = Vec::new();
    let mut last = None;
    for (name, node) in rest.by_ref().take(walk.limit) {
        let mut child = segments.clone();
        child.push(name.clone());
        items.push(info(ctx, join(&child), node)?);
        last = Some(name);
    }
    let next = rest.next().and(last);
    Ok((snapshot, walk.page(items, next)))
}

fn directory(ctx: &QueryCtx, root: &BlobId, segments: &[String]) -> Result<Option<BlobId>, Error> {
    if segments.is_empty() {
        return Ok(Some(*root));
    }
    match node_at(ctx, root, segments)? {
        Some(Node::Dir { tree, .. }) => Ok(Some(tree)),
        Some(other) => Err(invalid(format!(
            "{} is a {}, not a directory",
            join(segments),
            other.kind()
        ))),
        None => Ok(None),
    }
}

pub(crate) fn read(
    ctx: &QueryCtx,
    path: &str,
    at: Option<&BlobId>,
    offset: u64,
    len: u64,
) -> Result<Contents, Error> {
    let segments = segments(path)?;
    let row = resolve(ctx, at)?
        .ok_or_else(|| not_found(format!("nothing is at {path}: no commit yet")))?;
    if segments.is_empty() {
        return Err(invalid("/ is a directory, not a file"));
    }
    let node = node_at(ctx, &row.root, &segments)?
        .ok_or_else(|| not_found(format!("nothing is at {path}")))?;
    let (size, bytes) = match node {
        Node::File { file: id, size, .. } => (size, read_file(ctx, &file(ctx, &id)?, offset, len)?),
        Node::Symlink { target } => {
            let bytes = target.as_bytes();
            let start = (offset as usize).min(bytes.len());
            let end = start.saturating_add(len as usize).min(bytes.len());
            (bytes.len() as u64, bytes[start..end].to_vec())
        }
        Node::Dir { .. } => return Err(invalid(format!("{path} is a directory, not a file"))),
    };
    let eof = offset.saturating_add(bytes.len() as u64) >= size;
    Ok(Contents {
        snapshot: row.id,
        size,
        offset,
        bytes,
        eof,
    })
}

fn may_hold(dir: &str, prefix: &str) -> bool {
    dir.starts_with(prefix) || prefix.starts_with(dir)
}

fn after(path: &str, cursor: Option<&str>) -> bool {
    cursor.is_none_or(|cursor| path.split('/').cmp(cursor.split('/')) == Ordering::Greater)
}

fn reaches_past(dir: &str, cursor: Option<&str>) -> bool {
    let Some(cursor) = cursor else {
        return true;
    };
    for (ours, theirs) in dir.split('/').zip(cursor.split('/')) {
        match ours.cmp(theirs) {
            Ordering::Less => return false,
            Ordering::Greater => return true,
            Ordering::Equal => {}
        }
    }
    true
}

struct Finding<'a> {
    prefix: &'a str,
    after: Option<&'a str>,
    limit: usize,
    found: Vec<EntryInfo>,
    more: bool,
}

pub(crate) fn find(
    ctx: &QueryCtx,
    prefix: &str,
    at: Option<&BlobId>,
    page: &PageRequest,
) -> Result<(Option<BlobId>, PageResponse<EntryInfo>), Error> {
    let row = resolve(ctx, at)?;
    let snapshot = row.as_ref().map(|row| row.id);
    let walk = Walk::open(ctx, page, &("find", prefix, snapshot))?;
    let Some(row) = row else {
        return Ok((snapshot, walk.page(Vec::new(), None)));
    };
    let mut finding = Finding {
        prefix,
        after: walk.after.as_deref(),
        limit: walk.limit,
        found: Vec::new(),
        more: false,
    };
    find_in(ctx, &row.root, "", &mut finding)?;
    let next = match finding.more {
        true => finding.found.last().map(|entry| entry.path.clone()),
        false => None,
    };
    Ok((snapshot, walk.page(finding.found, next)))
}

fn find_in(ctx: &QueryCtx, dir: &BlobId, base: &str, finding: &mut Finding) -> Result<(), Error> {
    for (name, node) in tree(ctx, dir)?.entries {
        if finding.more {
            return Ok(());
        }
        let path = format!("{base}/{name}");
        let hit = path.starts_with(finding.prefix) && after(&path, finding.after);
        if hit {
            if finding.found.len() == finding.limit {
                finding.more = true;
                return Ok(());
            }
            finding.found.push(info(ctx, path.clone(), node.clone())?);
        }
        let Node::Dir { tree: child, .. } = node else {
            continue;
        };
        let worth_descending =
            may_hold(&path, finding.prefix) && reaches_past(&path, finding.after);
        if worth_descending {
            find_in(ctx, &child, &path, finding)?;
        }
    }
    Ok(())
}

struct Grepping<'a> {
    pattern: &'a str,
    prefix: &'a str,
    after: Option<&'a str>,
    limit: usize,
    hits: Vec<Hit>,
    scanned: Option<String>,
    next: Option<String>,
    done: bool,
}

pub(crate) fn grep(
    ctx: &QueryCtx,
    pattern: &str,
    prefix: &str,
    at: Option<&BlobId>,
    page: &PageRequest,
) -> Result<(Option<BlobId>, PageResponse<Hit>), Error> {
    if pattern.is_empty() {
        return Err(invalid("a pattern is not empty"));
    }
    let row = resolve(ctx, at)?;
    let snapshot = row.as_ref().map(|row| row.id);
    let walk = Walk::open(ctx, page, &("grep", pattern, prefix, snapshot))?;
    let Some(row) = row else {
        return Ok((snapshot, walk.page(Vec::new(), None)));
    };
    let mut grepping = Grepping {
        pattern,
        prefix,
        after: walk.after.as_deref(),
        limit: walk.limit,
        hits: Vec::new(),
        scanned: None,
        next: None,
        done: false,
    };
    grep_in(ctx, &row.root, "", &mut grepping)?;
    let next = grepping.next.take();
    Ok((snapshot, walk.page(grepping.hits, next)))
}

fn grep_in(ctx: &QueryCtx, dir: &BlobId, base: &str, grepping: &mut Grepping) -> Result<(), Error> {
    for (name, node) in tree(ctx, dir)?.entries {
        if grepping.done {
            return Ok(());
        }
        let path = format!("{base}/{name}");
        match node {
            Node::File { file: id, size, .. } => {
                let candidate = path.starts_with(grepping.prefix) && after(&path, grepping.after);
                if candidate {
                    grep_file(ctx, path, &id, size, grepping)?;
                }
            }
            Node::Symlink { .. } => {}
            Node::Dir { tree: child, .. } => {
                let worth_descending =
                    may_hold(&path, grepping.prefix) && reaches_past(&path, grepping.after);
                if worth_descending {
                    grep_in(ctx, &child, &path, grepping)?;
                }
            }
        }
    }
    Ok(())
}

fn grep_file(
    ctx: &QueryCtx,
    path: String,
    id: &BlobId,
    size: u64,
    grepping: &mut Grepping,
) -> Result<(), Error> {
    let page_full = grepping.hits.len() >= grepping.limit;
    if page_full {
        grepping.next = grepping.scanned.clone();
        grepping.done = true;
        return Ok(());
    }
    let bytes = read_file(ctx, &file(ctx, id)?, 0, size)?;
    let mut spans = crate::grep::Spans::new(grepping.pattern);
    for (index, line) in bytes.split(|&byte| byte == b'\n').enumerate() {
        let text = String::from_utf8_lossy(line);
        if let Some((line, text)) = spans.line(index as u64 + 1, &text) {
            grepping.hits.push(Hit {
                path: path.clone(),
                line,
                text,
            });
        }
    }
    grepping.scanned = Some(path);
    Ok(())
}

pub(crate) fn diff(
    ctx: &QueryCtx,
    from: &BlobId,
    to: &BlobId,
    prefix: &str,
) -> Result<Vec<DiffEntry>, Error> {
    let from = crate::state::minted(ctx, from)?.root;
    let to = crate::state::minted(ctx, to)?.root;
    let mut out = Vec::new();
    diff_trees(ctx, &from, &to, "", prefix, &mut out)?;
    Ok(out)
}

fn diff_trees(
    ctx: &QueryCtx,
    from: &BlobId,
    to: &BlobId,
    base: &str,
    prefix: &str,
    out: &mut Vec<DiffEntry>,
) -> Result<(), Error> {
    if from == to {
        return Ok(());
    }
    let mut from = tree(ctx, from)?.entries;
    let mut to = tree(ctx, to)?.entries;
    let mut names: Vec<String> = from.keys().chain(to.keys()).cloned().collect();
    names.sort();
    names.dedup();
    for name in names {
        let path = format!("{base}/{name}");
        if !may_hold(&path, prefix) {
            continue;
        }
        match (from.remove(&name), to.remove(&name)) {
            (Some(old), None) => side(ctx, &path, &old, DiffKind::Removed, prefix, out)?,
            (None, Some(new)) => side(ctx, &path, &new, DiffKind::Added, prefix, out)?,
            (Some(old), Some(new)) if old == new => {}
            (Some(Node::Dir { tree: old, .. }), Some(Node::Dir { tree: new, .. })) => {
                diff_trees(ctx, &old, &new, &path, prefix, out)?
            }
            (Some(old), Some(new)) => {
                push(out, &path, DiffKind::Modified, prefix);
                children(ctx, &path, &old, DiffKind::Removed, prefix, out)?;
                children(ctx, &path, &new, DiffKind::Added, prefix, out)?;
            }
            (None, None) => unreachable!("every name came from one side"),
        }
    }
    Ok(())
}

fn side(
    ctx: &QueryCtx,
    path: &str,
    node: &Node,
    kind: DiffKind,
    prefix: &str,
    out: &mut Vec<DiffEntry>,
) -> Result<(), Error> {
    push(out, path, kind, prefix);
    children(ctx, path, node, kind, prefix, out)
}

fn children(
    ctx: &QueryCtx,
    base: &str,
    node: &Node,
    kind: DiffKind,
    prefix: &str,
    out: &mut Vec<DiffEntry>,
) -> Result<(), Error> {
    let Node::Dir { tree: dir, .. } = node else {
        return Ok(());
    };
    for (name, child) in tree(ctx, dir)?.entries {
        let path = format!("{base}/{name}");
        if may_hold(&path, prefix) {
            side(ctx, &path, &child, kind, prefix, out)?;
        }
    }
    Ok(())
}

fn push(out: &mut Vec<DiffEntry>, path: &str, kind: DiffKind, prefix: &str) {
    if !path.starts_with(prefix) {
        return;
    }
    out.push(DiffEntry {
        path: path.to_owned(),
        kind,
    });
}
