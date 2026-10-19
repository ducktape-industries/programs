//! Everything the scheduler keeps: each entry by its id, kept after it runs with its receipt,
//! the entries still due by height and id, and the id the next entry takes.
use store::{Item, Map, Set};

use crate::{Entry, EntryId};

pub const ENTRIES: Map<EntryId, Entry> = Map::new("entry/");
pub const DUE: Set<(u64, EntryId)> = Set::new("due/");
pub const NEXT: Item<EntryId> = Item::new("next");
