//! Typed reads of `files`, access and identity. A folder or a file is read at one
//! snapshot: the first answer names it, and every later page and read of
//! the same screen asks that snapshot, so a commit landing in between never
//! mixes two trees on one page.
use std::sync::Arc;

use abi::role::access::Writers;
use files::{BlobId, PageRequest};
use view::prelude::*;

use crate::state::{
    Folder, HISTORY_PAGE, History, Names, Opened, PREVIEW, ROOT, Snapshot, picture_format,
};

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
    entries.sort_by_key(|entry| !matches!(entry.node, files::Node::Dir { .. }));
    let writers = writers(&host, &path).await?;
    Ok(Folder {
        path,
        snapshot,
        entries,
        writers,
    })
}

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

pub(crate) async fn entry(host: Host, path: String) -> Result<Option<files::EntryInfo>, Error> {
    let (_, entry) = host.query(files::ask::Stat { path, at: None }).await?;
    Ok(entry)
}

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
                (files::Node::File { .. }, _) => {
                    read_picture_at_snapshot(&host, &path, snapshot, &contents).await?
                }
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

async fn read_picture_at_snapshot(
    host: &Host,
    path: &str,
    at: Option<BlobId>,
    first: &files::Contents,
) -> Result<Option<Arc<Image>>, Error> {
    let Some(format) = picture_format(&first.bytes) else {
        return Ok(None);
    };
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

pub(crate) async fn name(host: Host, account: u64) -> Result<Option<String>, Error> {
    let profile = host
        .query(identity::ask::Profile { number: account })
        .await?;
    Ok(profile.map(|profile| profile.name))
}

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
