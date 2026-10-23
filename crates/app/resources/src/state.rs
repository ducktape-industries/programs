//! Every table resources keeps (README.md "Rows"), each row and index under its prefix, and the
//! rows only the rules read: a policy with the one it replaced, a walk's place, a request's state.
use borsh::{BorshDeserialize, BorshSerialize};
use store::{Item, Kind, Map, Owned, Set};

use crate::{
    AccountNumber, AdmittedRow, Assignment, BeatRow, KindRow, OfferRow, Policy, Record, RequestRow,
    RequestState, Usage,
};

pub struct KindOwned;

impl Kind for KindOwned {
    const KIND: &'static str = "resources.kind";
}

pub struct RequestOwned;

impl Kind for RequestOwned {
    const KIND: &'static str = "resources.request";
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PolicyRow {
    pub policy: Policy,
    pub height: u64,
    pub before: Option<(Policy, u64)>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct StateRow {
    pub attempt: u32,
    pub state: RequestState,
    pub class: String,
    pub group: String,
}

pub use crate::ordering::WalkRow;

pub const KINDS: Map<String, Owned<KindRow, KindOwned>> = Map::new("kind/");
pub const POLICIES: Map<String, PolicyRow> = Map::new("policy/");
pub const ADMITTED: Map<(String, AccountNumber), AdmittedRow> = Map::new("admitted/");
pub const RANKS: Map<(String, AccountNumber), u32> = Map::new("rank/");
pub const OFFERS: Map<(AccountNumber, String), OfferRow> = Map::new("offer/");
pub const KIND_OFFERS: Set<(String, AccountNumber, String)> = Set::new("kind-offer/");
pub const IDLE: Set<(String, u64, AccountNumber, String)> = Set::new("idle/");
pub const NOTARY_OF: Set<(String, String)> = Set::new("notary-of/");
pub const WALK_AT: Set<(String, u64)> = Set::new("walk-at/");
pub const WALKS: Map<String, WalkRow> = Map::new("walk/");
pub const BEATS: Map<(AccountNumber, String), BeatRow> = Map::new("beat/");
pub const CHECKS: Map<(AccountNumber, String), u64> = Map::new("check/");
pub const REQUESTS: Map<u64, Owned<RequestRow, RequestOwned>> = Map::new("request/");
pub const STATES: Map<u64, StateRow> = Map::new("state/");
pub const QUEUE: Set<(String, String, String, u64)> = Set::new("queue/");
pub const ASSIGNED: Map<(u64, u32), Assignment> = Map::new("assigned/");
pub const HOLDS: Set<(AccountNumber, String, u64)> = Set::new("holds/");
pub const DECLINED: Set<(u64, AccountNumber, String)> = Set::new("declined/");
pub const USED: Map<(u64, u32), Usage> = Map::new("used/");
pub const LEDGER: Map<(String, String, String, u64), u64> = Map::new("ledger/");
pub const RECORDS: Map<(AccountNumber, String), Record> = Map::new("record/");
pub const DISPUTED: Set<(u64, u32)> = Set::new("disputed/");
pub const CHARGED: Map<(u64, u32, String), u64> = Map::new("charged/");
pub const LAST: Item<u64> = Item::new("last");
