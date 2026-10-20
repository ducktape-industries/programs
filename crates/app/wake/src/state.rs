//! Every table wake keeps: each wake and its account's index, what each kind of wake listens on
//! while it is open (a scheduler entry, a pod's stream, a ref, change or path, a program's code), and
//! for each ref, change or path the last wake an event of it reached.
use ::program::{AccountNumber, EntryId};
use abi::role::programs::Entry;
use borsh::{BorshDeserialize, BorshSerialize};
use store::{Item, Map, Set};

use crate::WakeRow;

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PodWatch {
    pub height: u64,
    pub time: u64,
    pub entry: Option<EntryId>,
    pub token: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct FileWatch {
    pub path: String,
    pub node: Option<files::Node>,
}

pub const WAKES: Map<u64, WakeRow> = Map::new("wake/");
pub const BY_ACCOUNT: Set<(AccountNumber, u64)> = Set::new("account/");
pub const LAST: Item<u64> = Item::new("last");
pub const ENTRIES: Map<EntryId, u64> = Map::new("entry/");
pub const SCHEDULED: Map<u64, EntryId> = Map::new("scheduled/");
pub const PODS: Map<u64, PodWatch> = Map::new("pod/");
pub const REF_WAKES: Set<((String, String), u64)> = Set::new("ref/");
pub const WAKE_REFS: Map<u64, (String, String)> = Map::new("wake-ref/");
pub const CHANGE_WAKES: Set<((String, u64), u64)> = Set::new("change/");
pub const FORGE_COUNT: Item<u64> = Item::new("forge-count");
pub const FILE_WAKES: Map<u64, FileWatch> = Map::new("file/");
pub const PATH_WAKES: Set<(String, u64)> = Set::new("path/");
pub const FIRED: Map<Vec<u8>, u64> = Map::new("fired/");
pub const FILE_COUNT: Item<u64> = Item::new("file-count");
pub const PROGRAM_WAKES: Map<u64, Option<Entry>> = Map::new("program/");
pub const TELLING: Map<u64, u64> = Map::new("telling/");
