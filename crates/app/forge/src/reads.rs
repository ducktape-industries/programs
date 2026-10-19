//! Object reads over the existing loose-object store; no pack parsing and
//! no persistent writes.
use crate::contract::*;
use crate::objects::{ObjectStore, object_not_held};
use crate::ops::{cap, refusal_of};
use crate::state::{load_repo, parse_oid, repo_hash, resolve};
use ::program::Error;
use ::program::{QueryCtx, invalid, not_found};
use gitcore::{Commit, Hash, Kind, Mode, Objects, Oid, Signature, Tag, Tree};
use std::collections::BTreeSet;
use store::Listing;

pub struct Reading<'a> {
    pub store: ObjectStore<'a>,
    pub hash: Hash,
    pub bounds: &'a Bounds,
}
impl Reading<'_> {
    pub fn result<T>(&self, r: gitcore::Result<T>) -> Result<T, Error> {
        r.map_err(refusal_of)
    }
    pub fn oid(&self, s: &str) -> Result<Oid, Error> {
        parse_oid(self.hash, s)
    }
    pub fn commit_id(&self, mut id: Oid) -> Result<Oid, Error> {
        loop {
            let object = self
                .result(self.store.get(&id))?
                .ok_or_else(|| object_not_held(id))?;
            match object.kind {
                Kind::Tag => id = self.result(Tag::parse(&object.body, self.hash))?.object,
                Kind::Commit => return Ok(id),
                _ => return Err(invalid("expected a commit or a tag pointing to a commit")),
            }
        }
    }
    pub fn commit(&self, id: &Oid) -> Result<Commit, Error> {
        let object = self
            .result(self.store.get(id))?
            .ok_or_else(|| object_not_held(id))?;
        if object.kind != Kind::Commit {
            return Err(invalid("expected a commit object"));
        }
        self.result(Commit::parse(&object.body, self.hash))
    }
    pub fn tree(&self, id: &Oid) -> Result<Tree, Error> {
        let object = self
            .result(self.store.get(id))?
            .ok_or_else(|| object_not_held(id))?;
        if object.kind != Kind::Tree {
            return Err(invalid("expected a tree object"));
        }
        self.result(Tree::parse(&object.body, self.hash))
    }
    pub fn tree_id(&self, at: &str) -> Result<Oid, Error> {
        let id = self.oid(at)?;
        let object = self
            .result(self.store.get(&id))?
            .ok_or_else(|| object_not_held(id))?;
        match object.kind {
            Kind::Tree => Ok(id),
            Kind::Commit => Ok(self.result(Commit::parse(&object.body, self.hash))?.tree),
            _ => Err(invalid("tree endpoint must be a commit or tree")),
        }
    }
    pub fn blob(&self, id: &Oid, range: Option<ByteRange>) -> Result<BlobView, Error> {
        let header = self.result(self.store.header(id))?;
        if header.kind != "blob" {
            return Err(invalid("expected a blob object"));
        }
        let requested = range.unwrap_or(ByteRange {
            offset: 0,
            len: header.len.min(self.bounds.blob_bytes),
        });
        if requested.offset > header.len
            || requested.len > self.bounds.blob_bytes
            || requested.offset.checked_add(requested.len).is_none()
        {
            return Err(invalid("invalid blob byte range"));
        }
        let mut blob = BlobView {
            oid: id.to_hex(),
            size: header.len,
            content: Content::Oversize,
            range: ByteRange {
                offset: requested.offset,
                len: 0,
            },
            bytes: Vec::new(),
        };
        if header.len > self.bounds.blob_bytes {
            if range.is_some() {
                let len = requested.len.min(header.len - requested.offset);
                self.judged_range(id, &mut blob, len)?;
            }
            return Ok(blob);
        }
        let object = self
            .result(self.store.get(id))?
            .ok_or_else(|| object_not_held(id))?;
        if object.body.contains(&0) || std::str::from_utf8(&object.body).is_err() {
            blob.content = Content::Binary;
            return Ok(blob);
        }
        blob.content = Content::Text;
        let end = requested
            .offset
            .saturating_add(requested.len)
            .min(header.len);
        blob.bytes = object.body[requested.offset as usize..end as usize].to_vec();
        blob.range.len = blob.bytes.len() as u64;
        Ok(blob)
    }

    // A range of a blob too big to read whole, read alone and judged on its own bytes: text when
    // it holds no NUL and is UTF-8 but for a code point cut at an edge, else binary in that range.
    fn judged_range(&self, id: &Oid, blob: &mut BlobView, len: u64) -> Result<(), Error> {
        let bytes = self
            .store
            .read(id, blob.range.offset, len)
            .ok_or_else(|| object_not_held(id))?;
        blob.range.len = bytes.len() as u64;
        if bytes.contains(&0) || !utf8_within(&bytes) {
            blob.content = Content::Binary;
            return Ok(());
        }
        blob.content = Content::Text;
        blob.bytes = bytes;
        Ok(())
    }
}

// UTF-8 but for the code points a range's edges cut: continuation bytes at its start, an
// unfinished sequence at its end.
fn utf8_within(bytes: &[u8]) -> bool {
    let cut = bytes
        .iter()
        .take(3)
        .take_while(|byte| **byte & 0xC0 == 0x80)
        .count();
    match std::str::from_utf8(&bytes[cut..]) {
        Ok(_) => true,
        Err(error) => error.error_len().is_none(),
    }
}

fn signature(sig: Signature) -> GitSignature {
    GitSignature {
        name: sig.name,
        email: sig.email,
        time: sig.time,
        offset_minutes: sig.offset_minutes,
    }
}
pub fn entry_kind(mode: Mode) -> EntryKind {
    match mode {
        Mode::Regular => EntryKind::File,
        Mode::Executable => EntryKind::Executable,
        Mode::Symlink => EntryKind::Symlink,
        Mode::Directory => EntryKind::Directory,
        Mode::Gitlink => EntryKind::Gitlink,
    }
}
/// A repository's objects, read within `reads` object reads of `bounds`.
fn reading<'a>(
    ctx: &'a QueryCtx,
    name: &str,
    bounds: &'a Bounds,
    reads: u64,
) -> Result<Reading<'a>, Error> {
    let hash = repo_hash(&load_repo(ctx, name)?);
    Ok(Reading {
        store: ObjectStore::querying(ctx, bounds, reads),
        hash,
        bounds,
    })
}

/// A page of the history below `from`, less what `exclude` reaches.
pub fn log(
    ctx: &QueryCtx,
    height: u64,
    bounds: &Bounds,
    name: &str,
    from: &Revision,
    exclude: Option<&Revision>,
    listing: &Listing,
) -> Result<Reply, Error> {
    // a listed commit is one read; an excluded one two (its presence, then
    // its body), under its own log_walk budget; plus the two revisions
    let reads = bounds.log_walk.saturating_mul(3).saturating_add(2);
    let r = reading(ctx, name, bounds, reads)?;
    let tip = r.commit_id(resolve(ctx, name, from, r.hash)?)?;
    let hidden = match exclude {
        Some(exclude) => vec![r.commit_id(resolve(ctx, name, exclude, r.hash)?)?],
        None => Vec::new(),
    };
    // ponytail: repeat the complete walk up to log_walk; index history if larger repos need it.
    let ids = r.result(gitcore::walk::commits(
        &r.store,
        &[tip],
        &hidden,
        cap(bounds.log_walk),
    ))?;
    let page = listing.slice(&ids)?.try_map(|id| {
        let c = r.commit(&id)?;
        Ok(CommitInfo {
            oid: id.to_hex(),
            tree: c.tree.to_hex(),
            parents: c.parents.iter().map(Oid::to_hex).collect(),
            author: signature(c.author),
            committer: signature(c.committer),
            message: c.message,
        })
    })?;
    Ok(Reply::Log {
        height,
        tip: tip.to_hex(),
        page,
    })
}

/// A page of the directory at `path` under the commit or tree `at`.
pub fn tree(
    ctx: &QueryCtx,
    height: u64,
    bounds: &Bounds,
    name: &str,
    at: &str,
    path: &[u8],
    listing: &Listing,
) -> Result<Reply, Error> {
    crate::changes::check_path(path, true)?;
    let r = reading(ctx, name, bounds, bounds.tree_walk)?;
    let root = r.tree_id(at)?;
    let tree = if path.is_empty() {
        root
    } else {
        let entry = r
            .result(gitcore::walk::tree_at_path(&r.store, &root, path))?
            .ok_or_else(|| not_found("no entry at this path"))?;
        if entry.mode != Mode::Directory {
            return Err(invalid("tree path names a directory"));
        }
        entry.id
    };
    let entries = r.tree(&tree)?.entries;
    Ok(Reply::Tree {
        height,
        tree: tree.to_hex(),
        page: listing.slice(&entries)?.map(|e| TreeInfo {
            name: e.name,
            oid: e.id.to_hex(),
            kind: entry_kind(e.mode),
        }),
    })
}

/// One blob, or the asked range of it.
pub fn blob(
    ctx: &QueryCtx,
    height: u64,
    bounds: &Bounds,
    name: &str,
    oid: &str,
    range: Option<ByteRange>,
) -> Result<Reply, Error> {
    let r = reading(ctx, name, bounds, bounds.tree_walk)?;
    Ok(Reply::Blob {
        height,
        blob: r.blob(&r.oid(oid)?, range)?,
    })
}

/// A page of the files that differ from `base` (the empty tree if none)
/// to `head`, under `path` if given.
#[allow(clippy::too_many_arguments)]
pub fn diff(
    ctx: &QueryCtx,
    height: u64,
    bounds: &Bounds,
    name: &str,
    base: &Option<String>,
    head: &str,
    path: Option<&[u8]>,
    listing: &Listing,
) -> Result<Reply, Error> {
    let r = reading(ctx, name, bounds, bounds.tree_walk)?;
    crate::diffs::query(&r, height, base, head, path, listing)
}

/// How `from` stands against `into`: ahead, behind, and their base.
pub fn comparison(
    ctx: &QueryCtx,
    height: u64,
    bounds: &Bounds,
    name: &str,
    from: &Revision,
    into: &Revision,
) -> Result<Reply, Error> {
    let reads = bounds
        .log_walk
        .saturating_mul(8)
        .saturating_add(bounds.tree_walk);
    let r = reading(ctx, name, bounds, reads)?;
    let from = r.commit_id(resolve(ctx, name, from, r.hash)?)?;
    let into = r.commit_id(resolve(ctx, name, into, r.hash)?)?;
    compare(&r, height, from, into)
}

fn compare(r: &Reading<'_>, height: u64, from: Oid, into: Oid) -> Result<Reply, Error> {
    let source: BTreeSet<_> = r
        .result(gitcore::walk::commits(
            &r.store,
            &[from],
            &[],
            cap(r.bounds.log_walk),
        ))?
        .into_iter()
        .collect();
    let target: BTreeSet<_> = r
        .result(gitcore::walk::commits(
            &r.store,
            &[into],
            &[],
            cap(r.bounds.log_walk),
        ))?
        .into_iter()
        .collect();
    let ahead = source.difference(&target).count() as u64;
    let behind = target.difference(&source).count() as u64;
    let base = r.result(gitcore::walk::merge_base(
        &r.store,
        &from,
        &into,
        cap(r.bounds.log_walk),
    ))?;
    // Ancestry facts only: whether the endpoints merge cleanly is the git
    // client's to find out, so a change is mergeable here when it fast-forwards.
    let mergeability = match base {
        None => Mergeability::Unrelated,
        Some(b) if b == from => Mergeability::UpToDate,
        Some(b) if b == into => Mergeability::FastForward,
        Some(_) => Mergeability::Diverged,
    };
    Ok(Reply::Compare {
        height,
        comparison: Comparison {
            from: from.to_hex(),
            into: into.to_hex(),
            base: base.map(|o| o.to_hex()),
            ahead,
            behind,
            mergeability,
        },
    })
}
