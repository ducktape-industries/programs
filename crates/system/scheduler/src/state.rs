//! Everything the scheduler keeps: each entry by its id, kept after it runs with its receipt,
//! the entries still due by height and id, and the id the next entry takes.
use store::{Item, Map, Set};

use crate::{Entry, EntryId};

pub const ENTRIES: Map<EntryId, Entry> = Map::new("entry/").shared();
pub const DUE: Set<(u64, EntryId)> = Set::new("due/").shared();
pub const NEXT: Item<EntryId> = Item::new("next").shared();

// Every table here is general data: only this program's code governs it.
pub(crate) const TABLES: &[::program::Table] = &[ENTRIES.table(), DUE.table(), NEXT.table()];
