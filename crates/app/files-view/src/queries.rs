//! Typed reads of `files`, its roots (its entry's params in the programs role), ownership and
//! identity. A folder or file is read at the snapshot its first answer names, so a commit in
//! between never mixes two trees on a page; a later read answering it pruned restarts at the head.
use std::sync::Arc;

use files::{BlobId, PageRequest};
use policy_view::item;
use view::prelude::*;

use crate::policies::guard_item;
use crate::state::{
    Folder, Guard, HISTORY_PAGE, History, Names, Opened, PREVIEW, ROOT, Snapshot, picture_format,
};

pub(crate) async fn folder(host: Host, path: String, reader: Option<u64>) -> Result<Folder, Error> {
    let (snapshot, mut entries) = loop {
        if let Ok(read) = listing(&host, &path).await? {
            break read;
        }
    };
    entries.sort_by_key(|entry| !matches!(entry.node, files::Node::Dir { .. }));
    let guard = guard(&host, &path, reader).await?;
    Ok(Folder {
        path,
        snapshot,
        entries,
        guard,
    })
}

struct Pruned;

type Listed = (Option<BlobId>, Vec<files::EntryInfo>);

async fn listing(host: &Host, path: &str) -> Result<Result<Listed, Pruned>, Error> {
    let mut snapshot = None;
    let mut entries = Vec::new();
    let mut after = None;
    loop {
        let asked = host
            .query(files::ask::List {
                path: path.to_owned(),
                at: snapshot,
                page: PageRequest { after, limit: None },
            })
            .await;
        let read_at_a_pruned_snapshot = snapshot.is_some() && asked.as_ref().is_err_and(pruned);
        if read_at_a_pruned_snapshot {
            return Ok(Err(Pruned));
        }
        let (at, page) = asked?;
        snapshot = at;
        entries.extend(page.items);
        match page.next {
            Some(next) => after = Some(next),
            None => return Ok(Ok((snapshot, entries))),
        }
    }
}

fn pruned(refusal: &Error) -> bool {
    refusal.code == files::code::NOT_FOUND
}

async fn read_at(
    host: &Host,
    path: &str,
    at: Option<BlobId>,
    offset: u64,
    len: u64,
) -> Result<Result<files::Contents, Pruned>, Error> {
    let asked = host
        .query(files::ask::Read {
            path: path.to_owned(),
            at,
            offset,
            len,
        })
        .await;
    let read_at_a_pruned_snapshot = at.is_some() && asked.as_ref().is_err_and(pruned);
    if read_at_a_pruned_snapshot {
        return Ok(Err(Pruned));
    }
    asked.map(Ok)
}

async fn guard(host: &Host, path: &str, reader: Option<u64>) -> Result<Option<Guard>, Error> {
    if path == ROOT {
        return Ok(None);
    }
    let at = host
        .query(files::ask::Guarded {
            path: path.to_owned(),
        })
        .await?;
    let owner = host
        .query(files::ask::Owner {
            path: path.to_owned(),
        })
        .await?;
    let admits = match &at {
        Some(at) => {
            let reader = reader.map(ownership::Party::Account);
            let standing = item::standing(host.clone(), guard_item(at), reader).await?;
            standing.admits
        }
        None => None,
    };
    Ok(Some(Guard { at, admits, owner }))
}

pub(crate) async fn entry(host: Host, path: String) -> Result<Option<files::EntryInfo>, Error> {
    let (_, entry) = host.query(files::ask::Stat { path, at: None }).await?;
    Ok(entry)
}

pub(crate) async fn file(
    host: Host,
    path: String,
    reader: Option<u64>,
    kept: Option<(BlobId, Arc<Image>)>,
) -> Result<Opened, Error> {
    loop {
        if let Ok(opened) = open_file(&host, &path, reader, kept.clone()).await? {
            return Ok(opened);
        }
    }
}

async fn open_file(
    host: &Host,
    path: &str,
    reader: Option<u64>,
    kept: Option<(BlobId, Arc<Image>)>,
) -> Result<Result<Opened, Pruned>, Error> {
    let (snapshot, entry) = host
        .query(files::ask::Stat {
            path: path.to_owned(),
            at: None,
        })
        .await?;
    let (bytes, whole, picture) = match entry.as_ref().map(|entry| &entry.node) {
        None | Some(files::Node::Dir { .. }) => (Vec::new(), true, None),
        Some(node) => {
            let Ok(contents) = read_at(host, path, snapshot, 0, PREVIEW).await? else {
                return Ok(Err(Pruned));
            };
            let picture = match (node, kept) {
                (files::Node::File { file, .. }, Some((was, kept))) if *file == was => Some(kept),
                (files::Node::File { .. }, _) => {
                    let read = read_picture_at_snapshot(host, path, snapshot, &contents).await?;
                    let Ok(picture) = read else {
                        return Ok(Err(Pruned));
                    };
                    picture
                }
                (_, _) => None,
            };
            (contents.bytes, contents.eof, picture)
        }
    };
    let guard = match entry {
        Some(_) => guard(host, path, reader).await?,
        None => None,
    };
    let mut opened = Opened {
        path: path.to_owned(),
        snapshot,
        entry,
        bytes,
        whole,
        picture,
        guard,
        doc: None,
    };
    opened.doc = opened.parsed();
    Ok(Ok(opened))
}

async fn read_picture_at_snapshot(
    host: &Host,
    path: &str,
    at: Option<BlobId>,
    first: &files::Contents,
) -> Result<Result<Option<Arc<Image>>, Pruned>, Error> {
    let Some(format) = picture_format(&first.bytes) else {
        return Ok(Ok(None));
    };
    let mut bytes = first.bytes.clone();
    while (bytes.len() as u64) < first.size {
        let offset = bytes.len() as u64;
        let Ok(next) = read_at(host, path, at, offset, files::CHUNK_SIZE).await? else {
            return Ok(Err(Pruned));
        };
        if next.bytes.is_empty() {
            return Err(view::host::malformed(format!(
                "{path} ends at {} of its {} bytes",
                bytes.len(),
                first.size
            )));
        }
        bytes.extend(next.bytes);
    }
    Ok(Ok(Some(Arc::new(Image::from_bytes(format, bytes)))))
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

pub(crate) async fn roots(host: Host) -> Result<files::paths::Roots, Error> {
    let (_, entry) = host
        .query(programs::ask::Program(files::PROGRAM.into()))
        .await?;
    let params = entry.map(|entry| entry.params).unwrap_or_default();
    files::paths::Roots::of(&params).map_err(|refusal| view::host::malformed(refusal.message))
}
