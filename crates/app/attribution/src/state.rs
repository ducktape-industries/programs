//! Everything attribution keeps: each object's current relations, every change in order
//! with its indexes by recipient and by object, and the prefix each registered program puts
//! before a change handed to it.
use store::{Item, Map, Set};

use crate::{AccountNumber, Change, ProgramId, Relations};

pub type ObjectKey = (ProgramId, String, String);

pub const OBJECTS: Map<ObjectKey, Relations> = Map::new("object/");
pub const CHANGES: Map<u64, Change> = Map::new("change/");
pub const TO: Set<(AccountNumber, u64)> = Set::new("to/");
pub const OF: Set<(ProgramId, String, String, u64)> = Set::new("of/");
pub const REGISTERED: Map<ProgramId, Vec<u8>> = Map::new("registered/");
pub const SEQ: Item<u64> = Item::new("seq");
