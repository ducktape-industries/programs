//! Everything files keeps in its own state: the head, every snapshot it minted in
//! commit order, and named pins. Trees and bytes live in blobs, never here.
use ::program::{BlobId, Error, QueryCtx, not_found};
use store::{Item, Map};

use crate::objects::digest;
use crate::{PinRow, SnapshotRow};

pub const HEAD: Item<SnapshotRow> = Item::new("head").shared();
pub const HISTORY: Map<u64, SnapshotRow> = Map::new("history/").shared();
pub const SNAPSHOTS: Map<[u8; 32], u64> = Map::new("snapshot/").shared();
pub const PINS: Map<String, PinRow> = Map::new("pin/").shared();
pub const COUNT: Item<u64> = Item::new("count").shared();

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

// Every table here is general data: only this program's code governs it.
pub(crate) const TABLES: &[::program::Table] = &[
    HEAD.table(),
    HISTORY.table(),
    SNAPSHOTS.table(),
    PINS.table(),
    COUNT.table(),
];
