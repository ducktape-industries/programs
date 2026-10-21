//! Every read: the head or a minted snapshot resolved first, then its trees walked; an object a
//! newer put let go answers pruned. A list, find or grep answers a page whose cursor stays on the
//! snapshot it walked. A diff folds the changes each commit recorded.
use std::cmp::Ordering;
use std::collections::{BTreeMap, VecDeque};

use ::program::{BlobId, Error, QueryCtx, corrupt, invalid, not_found};

use crate::cursor::Walk;
use crate::objects::{At, digest, file_at, read as read_file, tree_at};
use crate::paths::{Roots, join, segments};
use crate::state::{
    DIFFS, HEAD, HISTORY, PINS, RECORDED_FROM, SNAPSHOTS, minted, newest_first, resolve,
};
use crate::tree::node_at;
use crate::{
    CHUNK_SIZE, Contents, DiffEntry, DiffKind, EntryInfo, Hit, Node, PageRequest, PageResponse,
    PinRow, SnapshotRow,
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

fn age(ctx: &QueryCtx, row: &SnapshotRow) -> Result<At, Error> {
    let head = HEAD.get(ctx)?.map(|head| head.id);
    Ok(match head == Some(row.id) {
        true => At::Head,
        false => At::Older,
    })
}

fn info(ctx: &QueryCtx, path: String, node: Node, at: At) -> Result<EntryInfo, Error> {
    let meta = match &node {
        Node::File { file: id, .. } => file_at(ctx, id, at)?.meta,
        Node::Dir { .. } | Node::Symlink { .. } => BTreeMap::new(),
    };
    Ok(EntryInfo { path, node, meta })
}

fn root_node(ctx: &QueryCtx, root: &BlobId, at: At) -> Result<Node, Error> {
    let entries = tree_at(ctx, root, at)?.entries.len() as u64;
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
    let at = age(ctx, &row)?;
    let node = match segments.is_empty() {
        true => Some(root_node(ctx, &row.root, at)?),
        false => node_at(ctx, &row.root, &segments, at)?,
    };
    let entry = node
        .map(|node| info(ctx, join(&segments), node, at))
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
    let found = match &row {
        Some(row) => {
            let at = age(ctx, row)?;
            directory(ctx, &row.root, &segments, at)?.map(|dir| (dir, at))
        }
        None => None,
    };
    let Some((dir, at)) = found else {
        let empty_root_is_listable =
            segments.is_empty() || Roots::read(ctx)?.is_namespace_root(&segments);
        if !empty_root_is_listable {
            return Err(not_found(format!("no directory {}", join(&segments))));
        }
        return Ok((snapshot, walk.page(Vec::new(), None)));
    };
    let mut rest = tree_at(ctx, &dir, at)?
        .entries
        .into_iter()
        .filter(|(name, _)| walk.after.as_ref().is_none_or(|after| name > after));
    let mut items = Vec::new();
    let mut last = None;
    for (name, node) in rest.by_ref().take(walk.limit) {
        let mut child = segments.clone();
        child.push(name.clone());
        items.push(info(ctx, join(&child), node, at)?);
        last = Some(name);
    }
    let next = rest.next().and(last);
    Ok((snapshot, walk.page(items, next)))
}

fn directory(
    ctx: &QueryCtx,
    root: &BlobId,
    segments: &[String],
    at: At,
) -> Result<Option<BlobId>, Error> {
    if segments.is_empty() {
        return Ok(Some(*root));
    }
    match node_at(ctx, root, segments, at)? {
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
    let at = age(ctx, &row)?;
    let node = node_at(ctx, &row.root, &segments, at)?
        .ok_or_else(|| not_found(format!("nothing is at {path}")))?;
    let (size, bytes) = match node {
        Node::File { file: id, size, .. } => {
            (size, read_file(ctx, &file_at(ctx, &id, at)?, offset, len)?)
        }
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
    at: At,
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
        at: age(ctx, &row)?,
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
    for (name, node) in tree_at(ctx, dir, finding.at)?.entries {
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
            finding
                .found
                .push(info(ctx, path.clone(), node.clone(), finding.at)?);
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

pub(crate) const GREP_BYTES: u64 = CHUNK_SIZE;

#[derive(Clone, Copy)]
struct Place {
    offset: u64,
    line: u64,
}

const START: Place = Place { offset: 0, line: 1 };

struct Grepping<'a> {
    pattern: &'a str,
    prefix: &'a str,
    after: Option<&'a str>,
    inside: Option<Place>,
    limit: usize,
    at: At,
    hits: Vec<Hit>,
    read: u64,
    scanned: Option<String>,
    next: Option<String>,
    done: bool,
}

impl Grepping<'_> {
    fn spent(&self) -> bool {
        self.read >= GREP_BYTES || self.hits.len() >= self.limit
    }

    fn start_of(&self, path: &str) -> Option<Place> {
        if !path.starts_with(self.prefix) {
            return None;
        }
        if after(path, self.after) {
            return Some(START);
        }
        self.inside.filter(|_| self.after == Some(path))
    }

    fn stop(&mut self, next: Option<String>) {
        self.next = next;
        self.done = true;
    }
}

fn resumed(cursor: Option<&str>) -> Result<(Option<&str>, Option<Place>), Error> {
    let Some(cursor) = cursor else {
        return Ok((None, None));
    };
    let unread = || invalid("a grep cursor does not decode");
    let mut parts = cursor.split('\0');
    let path = parts.next().unwrap_or(cursor);
    let place = match (parts.next(), parts.next(), parts.next()) {
        (None, None, None) => None,
        (Some(offset), Some(line), None) => Some(Place {
            offset: offset.parse().map_err(|_| unread())?,
            line: line.parse().map_err(|_| unread())?,
        }),
        _ => return Err(unread()),
    };
    Ok((Some(path), place))
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
    let (after, inside) = resumed(walk.after.as_deref())?;
    let mut grepping = Grepping {
        pattern,
        prefix,
        after,
        inside,
        limit: walk.limit,
        at: age(ctx, &row)?,
        hits: Vec::new(),
        read: 0,
        scanned: None,
        next: None,
        done: false,
    };
    grep_in(ctx, &row.root, "", &mut grepping)?;
    let next = grepping.next.take();
    Ok((snapshot, walk.page(grepping.hits, next)))
}

fn grep_in(ctx: &QueryCtx, dir: &BlobId, base: &str, grepping: &mut Grepping) -> Result<(), Error> {
    for (name, node) in tree_at(ctx, dir, grepping.at)?.entries {
        if grepping.done {
            return Ok(());
        }
        let path = format!("{base}/{name}");
        match node {
            Node::File { file: id, size, .. } => {
                if let Some(from) = grepping.start_of(&path) {
                    grep_file(ctx, path, &id, size, from, grepping)?;
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
    from: Place,
    grepping: &mut Grepping,
) -> Result<(), Error> {
    if grepping.spent() {
        let next = grepping.scanned.clone();
        grepping.stop(next);
        return Ok(());
    }
    let file = file_at(ctx, id, grepping.at)?;
    let mut reading = Reading {
        spans: crate::grep::Spans::new(grepping.pattern),
        kept: grepping.pattern.split('\n').count() - 1,
        recent: VecDeque::new(),
        carried: Vec::new(),
        line: from.line,
    };
    let mut at = from.offset;
    while at < size {
        let resume = reading.resume(at);
        let stops = grepping.spent() && resume.offset > from.offset;
        if stops {
            let next = format!("{path}\0{}\0{}", resume.offset, resume.line);
            grepping.stop(Some(next));
            return Ok(());
        }
        let take = (size - at).min(CHUNK_SIZE - at % CHUNK_SIZE);
        let piece = read_file(ctx, &file, at, take)?;
        grepping.read += take;
        at += take;
        reading.carried.extend_from_slice(&piece);
        reading.lines(&path, at, grepping);
    }
    reading.last(&path, at, grepping);
    grepping.scanned = Some(path);
    Ok(())
}

struct Reading<'a> {
    spans: crate::grep::Spans<'a, u64>,
    kept: usize,
    recent: VecDeque<Place>,
    carried: Vec<u8>,
    line: u64,
}

impl Reading<'_> {
    fn resume(&self, at: u64) -> Place {
        let partial = Place {
            offset: at - self.carried.len() as u64,
            line: self.line,
        };
        self.recent.front().copied().unwrap_or(partial)
    }

    fn lines(&mut self, path: &str, at: u64, grepping: &mut Grepping) {
        let carried = std::mem::take(&mut self.carried);
        let mut offset = at - carried.len() as u64;
        let mut rest = carried.as_slice();
        while let Some(end) = rest.iter().position(|&byte| byte == b'\n') {
            self.see(path, offset, &rest[..end], grepping);
            offset += end as u64 + 1;
            rest = &rest[end + 1..];
        }
        self.carried = rest.to_vec();
    }

    fn last(&mut self, path: &str, at: u64, grepping: &mut Grepping) {
        let carried = std::mem::take(&mut self.carried);
        self.see(path, at - carried.len() as u64, &carried, grepping);
    }

    fn see(&mut self, path: &str, offset: u64, bytes: &[u8], grepping: &mut Grepping) {
        let place = Place {
            offset,
            line: self.line,
        };
        self.line += 1;
        if self.kept > 0 {
            self.recent.push_back(place);
            if self.recent.len() > self.kept {
                self.recent.pop_front();
            }
        }
        let text = String::from_utf8_lossy(bytes);
        if let Some((line, text)) = self.spans.line(place.line, &text) {
            grepping.hits.push(Hit {
                path: path.to_owned(),
                line,
                text,
            });
        }
    }
}

pub(crate) fn diff(
    ctx: &QueryCtx,
    from: &BlobId,
    to: &BlobId,
    prefix: &str,
) -> Result<Vec<DiffEntry>, Error> {
    let from = minted(ctx, from)?;
    let to = minted(ctx, to)?;
    let (older, newer, flip) = match from.seq <= to.seq {
        true => (&from, &to, false),
        false => (&to, &from, true),
    };
    let recorded = RECORDED_FROM
        .get(ctx)?
        .is_some_and(|first| older.seq + 1 >= first);
    if !recorded {
        let ages = (age(ctx, &from)?, age(ctx, &to)?);
        let mut out = Vec::new();
        diff_trees(ctx, &from.root, &to.root, "", prefix, ages, &mut out)?;
        return Ok(out);
    }
    let mut folded = BTreeMap::new();
    for seq in older.seq + 1..=newer.seq {
        let entries = DIFFS.get(ctx, &seq)?.ok_or_else(|| {
            corrupt(
                "diff/",
                &seq.to_be_bytes(),
                "files recorded no changes for this commit",
            )
        })?;
        for entry in entries {
            fold(&mut folded, segments(&entry.path)?, entry.kind);
        }
    }
    let mut out = Vec::new();
    for (segments, (first, last)) in folded {
        let Some(kind) = net(first, last) else {
            continue;
        };
        let kind = match (flip, kind) {
            (true, DiffKind::Added) => DiffKind::Removed,
            (true, DiffKind::Removed) => DiffKind::Added,
            (_, kind) => kind,
        };
        push(&mut out, &join(&segments), kind, prefix);
    }
    Ok(out)
}

fn fold(
    folded: &mut BTreeMap<Vec<String>, (DiffKind, DiffKind)>,
    segments: Vec<String>,
    kind: DiffKind,
) {
    if kind == DiffKind::Removed {
        folded.retain(|path, _| !(path.len() > segments.len() && path.starts_with(&segments)));
    }
    folded
        .entry(segments)
        .and_modify(|(_, last)| *last = kind)
        .or_insert((kind, kind));
}

fn net(first: DiffKind, last: DiffKind) -> Option<DiffKind> {
    match (first, last) {
        (DiffKind::Added, DiffKind::Removed) => None,
        (DiffKind::Added, _) => Some(DiffKind::Added),
        (_, DiffKind::Removed) => Some(DiffKind::Removed),
        _ => Some(DiffKind::Modified),
    }
}

fn diff_trees(
    ctx: &QueryCtx,
    from: &BlobId,
    to: &BlobId,
    base: &str,
    prefix: &str,
    ages: (At, At),
    out: &mut Vec<DiffEntry>,
) -> Result<(), Error> {
    if from == to {
        return Ok(());
    }
    let mut from = tree_at(ctx, from, ages.0)?.entries;
    let mut to = tree_at(ctx, to, ages.1)?.entries;
    let mut names: Vec<String> = from.keys().chain(to.keys()).cloned().collect();
    names.sort();
    names.dedup();
    for name in names {
        let path = format!("{base}/{name}");
        if !may_hold(&path, prefix) {
            continue;
        }
        match (from.remove(&name), to.remove(&name)) {
            (Some(old), None) => side(ctx, &path, &old, DiffKind::Removed, prefix, ages.0, out)?,
            (None, Some(new)) => side(ctx, &path, &new, DiffKind::Added, prefix, ages.1, out)?,
            (Some(old), Some(new)) if old == new => {}
            (Some(Node::Dir { tree: old, .. }), Some(Node::Dir { tree: new, .. })) => {
                diff_trees(ctx, &old, &new, &path, prefix, ages, out)?
            }
            (Some(old), Some(new)) => {
                push(out, &path, DiffKind::Modified, prefix);
                children(ctx, &path, &old, DiffKind::Removed, prefix, ages.0, out)?;
                children(ctx, &path, &new, DiffKind::Added, prefix, ages.1, out)?;
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
    at: At,
    out: &mut Vec<DiffEntry>,
) -> Result<(), Error> {
    push(out, path, kind, prefix);
    children(ctx, path, node, kind, prefix, at, out)
}

fn children(
    ctx: &QueryCtx,
    base: &str,
    node: &Node,
    kind: DiffKind,
    prefix: &str,
    at: At,
    out: &mut Vec<DiffEntry>,
) -> Result<(), Error> {
    let Node::Dir { tree: dir, .. } = node else {
        return Ok(());
    };
    for (name, child) in tree_at(ctx, dir, at)?.entries {
        let path = format!("{base}/{name}");
        if may_hold(&path, prefix) {
            side(ctx, &path, &child, kind, prefix, at, out)?;
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
