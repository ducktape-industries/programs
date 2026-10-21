//! What a snapshot is made of, each a kernel blob: chunks of bytes, files that list their chunks,
//! trees of named nodes with each node's creator and write list, and snapshots naming a root tree.
//! Each tree and file is held on its path's line, so a newer put there lets the old one go.
use std::collections::BTreeMap;

use ::program::{
    AccountNumber, BlobId, Error, ExecCtx, HashKind, Ownership, Principal, QueryCtx, corrupt,
    invalid, not_found,
};
use borsh::{BorshDeserialize, BorshSerialize};

use crate::{CHUNK_SIZE, Node};

pub const CHUNK: &str = "chunk";
pub const FILE: &str = "file";
pub const TREE: &str = "tree";
pub const SNAPSHOT: &str = "snapshot";

pub(crate) const SNAPSHOT_LINE: &[u8] = b"s";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct FileObj {
    pub size: u64,
    pub chunks: Vec<BlobId>,
    pub meta: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Tree {
    pub entries: BTreeMap<String, Node>,
    pub access: BTreeMap<String, Access>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Access {
    pub creator: Option<AccountNumber>,
    pub write: Option<Ownership>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SnapshotObj {
    pub root: BlobId,
    pub parent: Option<BlobId>,
    pub author: Principal,
    pub height: u64,
    pub time: u64,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum At {
    Head,
    Older,
}

pub(crate) fn digest(id: &BlobId) -> Result<[u8; 32], Error> {
    match id {
        BlobId::Sha256(digest) => Ok(*digest),
        BlobId::Sha1(_) => Err(invalid("files names its objects by sha256")),
    }
}

pub(crate) fn under(segments: &[String]) -> Vec<u8> {
    let mut prefix = vec![b'n'];
    for segment in segments {
        prefix.extend_from_slice(&(segment.len() as u32).to_be_bytes());
        prefix.extend_from_slice(segment.as_bytes());
    }
    prefix
}

pub(crate) fn line(segments: &[String]) -> Vec<u8> {
    let mut line = under(segments);
    line.push(0);
    line
}

pub(crate) fn put_on(
    ctx: &ExecCtx,
    kind: &str,
    body: Vec<u8>,
    line: Vec<u8>,
) -> Result<BlobId, Error> {
    ctx.blob_put_on(HashKind::Sha256, kind, body, line)
}

pub(crate) fn put_snapshot(ctx: &ExecCtx, snapshot: &SnapshotObj) -> Result<BlobId, Error> {
    put_on(ctx, SNAPSHOT, abi::encode(snapshot), SNAPSHOT_LINE.to_vec())
}

pub(crate) fn pruned(kind: &str, id: &BlobId) -> Error {
    not_found(format!(
        "{kind} {} is pruned on this node; an archival node keeps history",
        abi::hex(id.digest())
    ))
}

pub(crate) fn body(ctx: &QueryCtx, kind: &str, id: &BlobId, at: At) -> Result<Vec<u8>, Error> {
    let blob = ctx.blob_get(*id).ok_or_else(|| match at {
        At::Head => corrupt(kind, id.digest(), "the network holds no such blob"),
        At::Older => pruned(kind, id),
    })?;
    if blob.kind != kind {
        return Err(corrupt(
            kind,
            id.digest(),
            format!("the blob is a {}", blob.kind),
        ));
    }
    Ok(blob.body)
}

pub(crate) fn decoded<T: BorshDeserialize>(
    kind: &str,
    id: &BlobId,
    body: &[u8],
) -> Result<T, Error> {
    abi::decode(body).map_err(|refusal| corrupt(kind, id.digest(), refusal.sentence))
}

fn get<T: BorshDeserialize>(ctx: &QueryCtx, kind: &str, id: &BlobId, at: At) -> Result<T, Error> {
    decoded(kind, id, &body(ctx, kind, id, at)?)
}

pub(crate) fn file_at(ctx: &QueryCtx, id: &BlobId, at: At) -> Result<FileObj, Error> {
    get(ctx, FILE, id, at)
}

pub(crate) fn tree_at(ctx: &QueryCtx, id: &BlobId, at: At) -> Result<Tree, Error> {
    get(ctx, TREE, id, at)
}

pub(crate) fn tree(ctx: &QueryCtx, id: &BlobId) -> Result<Tree, Error> {
    tree_at(ctx, id, At::Head)
}

pub(crate) fn chunk_count(size: u64) -> u64 {
    size.div_ceil(CHUNK_SIZE)
}

pub(crate) fn chunk_len(size: u64, index: u64) -> u64 {
    let start = index * CHUNK_SIZE;
    size.saturating_sub(start).min(CHUNK_SIZE)
}

pub(crate) fn check_chunks(ctx: &QueryCtx, size: u64, chunks: &[BlobId]) -> Result<(), Error> {
    let expected = chunk_count(size);
    if chunks.len() as u64 != expected {
        return Err(invalid(format!(
            "a file of {size} bytes is {expected} chunks, not {}",
            chunks.len()
        )));
    }
    for (index, id) in chunks.iter().enumerate() {
        digest(id)?;
        let Some(header) = ctx.blob_stat(*id) else {
            return Err(::program::not_found(format!(
                "chunk {} is not on the network: put it through the blob upload first",
                abi::hex(id.digest())
            )));
        };
        if header.kind != CHUNK {
            return Err(invalid(format!(
                "{} is a {} blob, not a chunk: upload the data as a chunk blob",
                abi::hex(id.digest()),
                header.kind
            )));
        }
        let want = chunk_len(size, index as u64);
        if header.len != want {
            return Err(invalid(format!(
                "chunk {index} of a {size}-byte file is {want} bytes, not {}",
                header.len
            )));
        }
    }
    Ok(())
}

pub(crate) fn read(
    ctx: &QueryCtx,
    file: &FileObj,
    offset: u64,
    len: u64,
) -> Result<Vec<u8>, Error> {
    let end = offset.saturating_add(len).min(file.size);
    let mut bytes = Vec::new();
    let mut at = offset.min(end);
    while at < end {
        let index = at / CHUNK_SIZE;
        let within = at % CHUNK_SIZE;
        let take = (end - at).min(CHUNK_SIZE - within);
        let Some(chunk) = file.chunks.get(index as usize) else {
            return Err(corrupt(FILE, &[], format!("a file lacks chunk {index}")));
        };
        let piece = ctx
            .blob_read(*chunk, within, take)
            .ok_or_else(|| corrupt(CHUNK, chunk.digest(), "the network holds no such chunk"))?;
        if piece.len() as u64 != take {
            return Err(corrupt(
                CHUNK,
                chunk.digest(),
                "the chunk is shorter than its file says",
            ));
        }
        bytes.extend_from_slice(&piece);
        at += take;
    }
    Ok(bytes)
}
