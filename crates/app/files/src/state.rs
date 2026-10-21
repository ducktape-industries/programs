//! Everything files keeps in its own state: the head, every snapshot it minted in commit order,
//! what each commit changed, named pins (files.pin) and guarded paths (files.path). Trees and
//! bytes live in blobs.
use ::program::{BlobId, Error, ProgramId, QueryCtx, not_found};
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use store::{Item, Kind, Map, Owned};

use crate::objects::digest;
use crate::{DiffEntry, PinRow, SnapshotRow};

pub const HEAD: Item<SnapshotRow> = Item::new("head");
pub const HISTORY: Map<u64, SnapshotRow> = Map::new("history/");
pub const SNAPSHOTS: Map<[u8; 32], u64> = Map::new("snapshot/");
pub const PINS: Map<String, Owned<PinRow>> = Map::new("pin/");
pub const GUARDED: Map<String, Owned<PathGuard>> = Map::new("guarded/");
pub const COUNT: Item<u64> = Item::new("count");
pub const CHANGED: Map<String, u64> = Map::new("changed/");
pub const TOUCHED: Map<String, u64> = Map::new("touched/");
pub const DIFFS: Map<u64, Vec<DiffEntry>> = Map::new("diff/");
pub const RECORDED_FROM: Item<u64> = Item::new("recorded_from");
// each program told of every commit, and the prefix it hears them under
pub const WATCHERS: Map<ProgramId, Vec<u8>> = Map::new("watchers/");

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct PathGuard {
    pub rev: u64,
}

impl Kind for PathGuard {
    const KIND: &'static str = "files.path";
}

impl Kind for PinRow {
    const KIND: &'static str = "files.pin";
}

pub fn guard_key(path: &str) -> Vec<u8> {
    GUARDED.key(&path.to_owned())
}

pub(crate) fn newest_first(seq: u64) -> u64 {
    u64::MAX - seq
}

pub(crate) fn minted(ctx: &QueryCtx, id: &BlobId) -> Result<SnapshotRow, Error> {
    let unknown = || {
        not_found(format!(
            "files minted no snapshot {}",
            abi::hex(id.digest())
        ))
    };
    let seq = SNAPSHOTS.get(ctx, &digest(id)?)?.ok_or_else(unknown)?;
    HISTORY.get(ctx, &newest_first(seq))?.ok_or_else(unknown)
}

pub(crate) fn resolve(ctx: &QueryCtx, at: Option<&BlobId>) -> Result<Option<SnapshotRow>, Error> {
    match at {
        Some(id) => minted(ctx, id).map(Some),
        None => HEAD.get(ctx),
    }
}
