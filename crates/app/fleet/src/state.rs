//! Every table fleet keeps: workloads and their runs, each run's claim row (its asker's), the
//! queue, the machines with what each holds and its last beat, and the counter.
use borsh::{BorshDeserialize, BorshSerialize};
use resources::ordering::{Ledger, Queue, WalkRow};
use store::{Item, Kind, Map, Owned, Set};

use crate::{AccountNumber, Filed, MachineId, MachineRow, Piece, RunRow, Workload};

// Who may move a run along (claim, cancel, requeue it): each run's claim row is created by its
// Ask, so its asker owns it, and their policy for `fleet.claim` decides who else writes it.
#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Claim {
    pub machine: Option<MachineId>,
    pub attempt: u32,
    pub cancelled: bool,
}

pub struct ClaimKind;

impl Kind for ClaimKind {
    const KIND: &'static str = "fleet.claim";
}

pub const WORKLOADS: Map<u64, Workload> = Map::new("w/");
pub const RUNS: Map<u64, RunRow> = Map::new("r/");
pub const CLAIMS: Map<u64, Owned<Claim, ClaimKind>> = Map::new("c/");
pub const FILED: Map<u64, Filed> = Map::new("f/");
pub const QUEUE: Queue = Set::new("qq/");
pub const WALKS: Map<String, WalkRow> = Map::new("wk/");
pub const WALK_AT: Set<(String, u64)> = Set::new("wa/");
pub const LEDGER: Ledger = Map::new("lg/");
pub const MACHINE_KINDS: Map<(AccountNumber, String), String> = Map::new("mk/");
pub const IDLE: Set<(String, u64, AccountNumber, String)> = Set::new("i/");
pub const MACHINES: Map<(AccountNumber, String), MachineRow> = Map::new("m/");
// Index: the runs each machine holds (assigned, running or stopping).
pub const HOLDS: Set<(AccountNumber, String, u64)> = Set::new("h/");
// Each machine's last beat, apart from its row, so a beat writes one height.
pub const BEATS: Map<(AccountNumber, String), u64> = Map::new("b/");
// The machines that declined a run, which it is never assigned to again.
pub const DECLINED: Set<(u64, AccountNumber, String)> = Set::new("d/");
pub const LAST: Item<u64> = Item::new("last");
// Each end told and not yet answered, by the seq of its message: the run and attempt it told of.
pub const TELLING: Map<u64, (u64, u32)> = Map::new("t/");
// How many ends were told in chat, which gives each line a message id of its own.
pub const TOLD: Item<u64> = Item::new("told");
pub const CALLS: Map<(u64, u32, u64), u64> = Map::new("n/");
pub const PIECES: Map<(u64, u32, u64, u64), Piece> = Map::new("p/");
