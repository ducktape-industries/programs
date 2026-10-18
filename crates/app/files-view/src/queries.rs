//! Typed reads of `files`, access and identity. A folder or a file is read at one
//! snapshot: the first answer names it, and every later page and read of
//! the same screen asks that snapshot, so a commit landing in between never
//! mixes two trees on one page.
use std::sync::Arc;

use abi::role::access::Writers;
use files::{BlobId, PageRequest};
use view::prelude::*;

use crate::state::{
    Folder, HISTORY_PAGE, History, Names, Opened, PREVIEW, ROOT, SHOW_MOST, Snapshot,
    picture_format,
};

/// A folder's entries, every page of them, and who writes it.
pub(crate) async fn folder(host: Host, path: String) -> Result<Folder, Error> {
    let mut snapshot = None;
    let mut entries = Vec::new();
    let mut after = None;
    loop {
        let (at, page) = host
            .query(files::ask::List {
                path: path.clone(),
                at: snapshot,
                page: PageRequest { after, limit: None },
            })
            .await?;
        snapshot = at;
        entries.extend(page.items);
        match page.next {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    // folders first, each kind by name as `files` keeps a tree
    entries.sort_by_key(|entry| !matches!(entry.node, files::Node::Dir { .. }));
    let writers = writers(&host, &path).await?;
    Ok(Folder {
        path,
        snapshot,
        entries,
        writers,
    })
}

/// Who writes `path`; `/` names nothing a change can, so it has no answer.
async fn writers(host: &Host, path: &str) -> Result<Option<Writers>, Error> {
    if path == ROOT {
        return Ok(None);
    }
    host.query(access::ask::Writers {
        program: files::PROGRAM.into(),
        key: path.to_owned(),
    })
    .await
    .map(Some)
}

/// What is at `path` at the newest snapshot, if anything.
pub(crate) async fn entry(host: Host, path: String) -> Result<Option<files::EntryInfo>, Error> {
    let (_, entry) = host.query(files::ask::Stat { path, at: None }).await?;
    Ok(entry)
}

/// What is at `path`, its first [`PREVIEW`] bytes (the whole of a
/// picture the app draws), and who writes it. `kept` is the picture on
/// screen and the content it was read from: while the file's content is
/// the same, it is kept and not read again.
pub(crate) async fn file(
    host: Host,
    path: String,
    kept: Option<(BlobId, Arc<Image>)>,
) -> Result<Opened, Error> {
    let (snapshot, entry) = host
        .query(files::ask::Stat {
            path: path.clone(),
            at: None,
        })
        .await?;
    let (bytes, whole, picture) = match entry.as_ref().map(|entry| &entry.node) {
        None | Some(files::Node::Dir { .. }) => (Vec::new(), true, None),
        Some(node) => {
            let contents = host
                .query(files::ask::Read {
                    path: path.clone(),
                    at: snapshot,
                    offset: 0,
                    len: PREVIEW,
                })
                .await?;
            let picture = match (node, kept) {
                (files::Node::File { file, .. }, Some((was, kept))) if *file == was => Some(kept),
                // a link's bytes are its target, never a picture's
                (files::Node::File { .. }, _) => picture(&host, &path, snapshot, &contents).await?,
                (_, _) => None,
            };
            (contents.bytes, contents.eof, picture)
        }
    };
    let writers = match entry {
        Some(_) => writers(&host, &path).await?,
        None => None,
    };
    Ok(Opened {
        path,
        snapshot,
        entry,
        bytes,
        whole,
        picture,
        writers,
    })
}

/// The whole picture a file's first bytes begin, read on from them a
/// chunk at a time at the same snapshot; `None` when they begin none, or
/// the file holds more than [`SHOW_MOST`].
async fn picture(
    host: &Host,
    path: &str,
    at: Option<BlobId>,
    first: &files::Contents,
) -> Result<Option<Arc<Image>>, Error> {
    let Some(format) = picture_format(&first.bytes) else {
        return Ok(None);
    };
    if first.size > SHOW_MOST {
        return Ok(None);
    }
    let mut bytes = first.bytes.clone();
    while (bytes.len() as u64) < first.size {
        let next = host
            .query(files::ask::Read {
                path: path.to_owned(),
                at,
                offset: bytes.len() as u64,
                len: files::CHUNK_SIZE,
            })
            .await?;
        // a read at one snapshot short of the size it states would draw
        // half a picture, or never end
        if next.bytes.is_empty() {
            return Err(view::host::malformed(format!(
                "{path} ends at {} of its {} bytes",
                bytes.len(),
                first.size
            )));
        }
        bytes.extend(next.bytes);
    }
    Ok(Some(Arc::new(Image::from_bytes(format, bytes))))
}

/// The newest [`HISTORY_PAGE`] snapshots, and every pin.
pub(crate) async fn history(host: Host) -> Result<History, Error> {
    let newest = host
        .query(files::ask::History {
            page: PageRequest::first(HISTORY_PAGE),
        })
        .await?;
    let pins = host
        .query_all(|after| files::ask::Pins {
            page: PageRequest { after, limit: None },
        })
        .await?;
    Ok(History {
        snapshots: newest.items,
        older: newest.next.is_some(),
        pins,
    })
}

/// The snapshot `id`, and what it changed against its parent; the first
/// snapshot's whole tree is new, which this does not list.
pub(crate) async fn snapshot(host: Host, id: BlobId) -> Result<Snapshot, Error> {
    let row = host.query(files::ask::Snapshot { id }).await?;
    let changes = match row.as_ref().and_then(|row| row.parent) {
        Some(from) => {
            host.query(files::ask::Diff {
                from,
                to: id,
                prefix: String::new(),
            })
            .await?
        }
        None => Vec::new(),
    };
    Ok(Snapshot { id, row, changes })
}

/// The name identity gives `account`, if any.
pub(crate) async fn name(host: Host, account: u64) -> Result<Option<String>, Error> {
    let profile = host
        .query(identity::ask::Profile { number: account })
        .await?;
    Ok(profile.map(|profile| profile.name))
}

/// Every account's name.
// ponytail: every account, on every identity change; ask only the
// authors on screen (`Profile` each) once the network holds thousands.
pub(crate) async fn names(host: Host) -> Result<Names, Error> {
    let accounts = host
        .query_all(|after| identity::ask::List {
            page: identity::PageRequest { after, limit: None },
        })
        .await?;
    let names = accounts
        .into_iter()
        .map(|account| (account.number, account.card.name))
        .collect();
    Ok(Names(names))
}
