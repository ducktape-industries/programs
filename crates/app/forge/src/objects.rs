//! `gitcore::Objects` over the store's blobs: a git object's blob id is
//! its oid, so no map sits between them.

use std::cell::Cell;
use std::collections::BTreeMap;

use ::program::{BlobHeader, BlobId, Error, HashKind};
use ::program::{ExecCtx, QueryCtx, code, not_found};
use gitcore::{Error as GitError, Hash, Kind, Object, Objects, Oid, WriteObjects};

pub(crate) fn object_not_held(oid: impl std::fmt::Display) -> Error {
    not_found(format!("object {oid} is not held by this node"))
}

pub struct ObjectStore<'a> {
    sandbox: &'a QueryCtx,
    budget: Option<Cell<(u64, u64)>>,
    max_object_size: u64,
}

impl<'a> ObjectStore<'a> {
    pub fn new(sandbox: &'a QueryCtx) -> ObjectStore<'a> {
        ObjectStore {
            sandbox,
            budget: None,
            max_object_size: u64::MAX,
        }
    }

    pub fn querying(sandbox: &'a QueryCtx, bounds: &crate::Bounds, reads: u64) -> Self {
        Self {
            budget: Some(Cell::new((reads, bounds.diff_bytes))),
            max_object_size: bounds.max_object_size,
            ..Self::new(sandbox)
        }
    }

    pub fn header(&self, id: &Oid) -> gitcore::Result<BlobHeader> {
        self.sandbox
            .blob_stat(blob_id_of(id))
            .ok_or(GitError::MissingObject(*id))
    }

    pub fn read(&self, id: &Oid, offset: u64, len: u64) -> Option<Vec<u8>> {
        self.sandbox.blob_read(blob_id_of(id), offset, len)
    }
}

impl Objects for ObjectStore<'_> {
    fn get(&self, id: &Oid) -> gitcore::Result<Option<Object>> {
        if let Some(budget) = &self.budget {
            let header = match self.sandbox.blob_stat(blob_id_of(id)) {
                Some(header) => header,
                None => return Ok(None),
            };
            let (reads, bytes) = budget.get();
            if reads == 0 || header.len > bytes || header.len > self.max_object_size {
                return Err(GitError::CapReached);
            }
            budget.set((reads - 1, bytes - header.len));
        }
        get(self.sandbox, id)
    }

    fn has(&self, id: &Oid) -> gitcore::Result<bool> {
        Ok(self.sandbox.blob_stat(blob_id_of(id)).is_some())
    }
}

fn get(sandbox: &QueryCtx, id: &Oid) -> gitcore::Result<Option<Object>> {
    let Some(blob) = sandbox.blob_get(blob_id_of(id)) else {
        return Ok(None);
    };
    let kind = Kind::parse(blob.kind.as_bytes())?;
    Ok(Some(Object::new(kind, blob.body)))
}

/// The object store a push or a stage writes into. Objects wait here until
/// the whole pack is admitted ([`ObjectWriter::flush`]), so a pack that
/// does not unpack, and an op that is refused, store nothing.
// ponytail: holds a pack's objects in memory until admitted; the pack is
// in memory already and `Bounds.max_objects`/`max_object_size` cap both.
pub struct ObjectWriter<'a> {
    sandbox: &'a ExecCtx,
    hash: Hash,
    pending: BTreeMap<Oid, Object>,
}

impl<'a> ObjectWriter<'a> {
    pub fn new(sandbox: &'a ExecCtx, hash: Hash) -> Self {
        ObjectWriter {
            sandbox,
            hash,
            pending: BTreeMap::new(),
        }
    }

    /// Stores every object the admitted pack brought.
    pub fn flush(self) -> Result<(), Error> {
        for (id, object) in self.pending {
            let stored = self.sandbox.blob_put(
                hash_kind_of(self.hash),
                object.kind.as_str(),
                object.body,
            )?;
            let named = blob_id_of(&id);
            if stored != named {
                return Err(misnamed(&id, &stored));
            }
        }
        Ok(())
    }
}

impl Objects for ObjectWriter<'_> {
    fn get(&self, id: &Oid) -> gitcore::Result<Option<Object>> {
        if let Some(object) = self.pending.get(id) {
            return Ok(Some(object.clone()));
        }
        get(self.sandbox, id)
    }

    fn has(&self, id: &Oid) -> gitcore::Result<bool> {
        Ok(self.pending.contains_key(id) || self.sandbox.blob_stat(blob_id_of(id)).is_some())
    }
}

impl WriteObjects for ObjectWriter<'_> {
    fn keep(&mut self, id: Oid, object: Object) -> gitcore::Result<()> {
        self.pending.insert(id, object);
        Ok(())
    }
}

fn misnamed(named: &Oid, stored: &BlobId) -> Error {
    Error::new(
        code::PROTOCOL,
        format!("the pack names an object {named}, but the node stored its bytes as {stored:?}"),
    )
}

pub fn hash_of(kind: HashKind) -> gitcore::Hash {
    match kind {
        HashKind::Sha1 => gitcore::Hash::Sha1,
        HashKind::Sha256 => gitcore::Hash::Sha256,
    }
}

pub fn hash_kind_of(hash: gitcore::Hash) -> HashKind {
    match hash {
        gitcore::Hash::Sha1 => HashKind::Sha1,
        gitcore::Hash::Sha256 => HashKind::Sha256,
    }
}

pub fn blob_id_of(oid: &Oid) -> BlobId {
    match oid {
        Oid::Sha1(digest) => BlobId::Sha1(*digest),
        Oid::Sha256(digest) => BlobId::Sha256(*digest),
    }
}

#[cfg(test)]
mod tests;
