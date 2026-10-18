//! What a snapshot is made of, each a kernel blob: chunks of bytes, files that list
//! their chunks, trees of named nodes with each node's creator and write list, and
//! snapshots naming a root tree.
use std::collections::BTreeMap;

use ::program::{
    AccountNumber, BlobId, Error, ExecCtx, HashKind, Ownership, Principal, QueryCtx, corrupt,
    invalid,
};
use borsh::{BorshDeserialize, BorshSerialize};

use crate::{CHUNK_SIZE, Node};

pub const CHUNK: &str = "chunk";
pub const FILE: &str = "file";
pub const TREE: &str = "tree";
pub const SNAPSHOT: &str = "snapshot";

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

pub(crate) fn digest(id: &BlobId) -> Result<[u8; 32], Error> {
    match id {
        BlobId::Sha256(digest) => Ok(*digest),
        BlobId::Sha1(_) => Err(invalid("files names its objects by sha256")),
    }
}

fn put<T: BorshSerialize>(ctx: &ExecCtx, kind: &str, object: &T) -> Result<BlobId, Error> {
    ctx.blob_put(HashKind::Sha256, kind, abi::encode(object))
}

fn get<T: BorshDeserialize>(ctx: &QueryCtx, kind: &str, id: &BlobId) -> Result<T, Error> {
    let blob = ctx
        .blob_get(*id)
        .ok_or_else(|| corrupt(kind, id.digest(), "the network holds no such blob"))?;
    if blob.kind != kind {
        return Err(corrupt(
            kind,
            id.digest(),
            format!("the blob is a {}", blob.kind),
        ));
    }
    abi::decode(&blob.body).map_err(|refusal| corrupt(kind, id.digest(), refusal.sentence))
}

pub(crate) fn put_file(ctx: &ExecCtx, file: &FileObj) -> Result<BlobId, Error> {
    put(ctx, FILE, file)
}

pub(crate) fn put_tree(ctx: &ExecCtx, tree: &Tree) -> Result<BlobId, Error> {
    put(ctx, TREE, tree)
}

pub(crate) fn put_snapshot(ctx: &ExecCtx, snapshot: &SnapshotObj) -> Result<BlobId, Error> {
    put(ctx, SNAPSHOT, snapshot)
}

pub(crate) fn file(ctx: &QueryCtx, id: &BlobId) -> Result<FileObj, Error> {
    get(ctx, FILE, id)
}

pub(crate) fn tree(ctx: &QueryCtx, id: &BlobId) -> Result<Tree, Error> {
    get(ctx, TREE, id)
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
                "{} is a {} blob, not a chunk",
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
