//! Everything attribution keeps: each object's current relations, every change in order
//! with its indexes by recipient and by object, and the prefix each registered program puts
//! before a change handed to it.
use store::{Item, Map, Set};

use crate::{AccountNumber, Change, ProgramId, Relations};

pub type ObjectKey = (ProgramId, String, String);

pub const OBJECTS: Map<ObjectKey, Relations> = Map::new("object/").shared();
pub const CHANGES: Map<u64, Change> = Map::new("change/").shared();
pub const TO: Set<(AccountNumber, u64)> = Set::new("to/").shared();
pub const OF: Set<(ProgramId, String, String, u64)> = Set::new("of/").shared();
pub const REGISTERED: Map<ProgramId, Vec<u8>> = Map::new("registered/").shared();
pub const SEQ: Item<u64> = Item::new("seq").shared();

// Every table here is general data: only this program's code governs it.
pub(crate) const TABLES: &[::program::Table] = &[
    OBJECTS.table(),
    CHANGES.table(),
    TO.table(),
    OF.table(),
    REGISTERED.table(),
    SEQ.table(),
];
