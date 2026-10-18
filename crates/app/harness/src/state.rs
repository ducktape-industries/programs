//! Everything harness keeps: models, threads by agent, current and bound threads, each place's
//! last thread, frames, held input, sealed parts, tool and live calls, withheld frames, streaming
//! replies, each thread's topic and tally, booked runs, watched streams, what a written key names.
use ::program::{BlobId, EntryId};
use abi::role::identity::AccountNumber;
use attribution::{Change, Source};
use borsh::{BorshDeserialize, BorshSerialize};
use store::{Item, KeyCodec, Map, Set};

use crate::{
    Body, Booking, CallRow, Frame, ModelRow, Partial, Place, Sealed, SettleWindow, Tally,
    ThreadRow, UsageWindow, Window, Wire,
};

pub const MODELS: Map<String, StoredModel> = Map::new("model/");
// a model's `compact_at` default, beside its record
pub const COMPACT_AT: Map<String, u64> = Map::new("compact/");
pub const LAST: Map<AccountNumber, (Source, u64)> = Map::new("last/");
pub const THREADS: Map<u64, ThreadRow> = Map::new("thread/");
pub const BY_AGENT: Set<(AccountNumber, u64)> = Set::new("by-agent/");
pub const PLACES: Map<(AccountNumber, String), u64> = Map::new("place/");
pub const BOUND: Map<(AccountNumber, String), Binding> = Map::new("bound/");
pub const CURRENT: Map<AccountNumber, u64> = Map::new("current/");
pub const FRAMES: Map<(u64, u64), Frame> = Map::new("frame/");
pub const HELD: Map<(u64, u64), Body> = Map::new("held/");
pub const SEALED: Map<(u64, u64), Sealed> = Map::new("sealed/");
pub const CALLS: Map<(u64, String), CallRow> = Map::new("call/");
pub const WITHHELD: Set<(u64, u64)> = Set::new("withheld/");
pub const PARTIALS: Map<u64, Partial> = Map::new("partial/");
pub const LIVE: Map<(u64, u64), Live> = Map::new("live/");
pub const SHOWN: Map<(u64, String), String> = Map::new("shown/");
pub const DEFERRED: Map<(u64, String, u64), Change> = Map::new("deferred/");
/// a thread's topic: the one line its agent last set with `thread_topic`
pub const TOPICS: Map<u64, String> = Map::new("topic/");
pub const TALLIES: Map<u64, Tally> = Map::new("tally/");
pub const BOOKINGS: Map<u64, Booking> = Map::new("booking/");
pub const BOOKED: Set<(AccountNumber, u64)> = Set::new("booked/");
pub const FLIGHTS: Map<u64, Flight> = Map::new("flight/");
pub const RETRIES: Map<u64, Retry> = Map::new("retry/");
pub const REPORTS: Map<(AccountNumber, u64), String> = Map::new("report/");
pub const REPORTING: Set<AccountNumber> = Set::new("reporting/");
pub const USAGE: Map<String, UsageWindow> = Map::new("usage/");
// a thread whose general input waits out its agent's `settle`, and the wake-up that ends it
pub const SETTLING: Map<u64, SettleWindow> = Map::new("settling/");
pub const BUDGETS: Map<(String, String), Budget> = Map::new("budget/");
pub const PACES: Map<String, Pace> = Map::new("pace/");
pub const READS: Map<String, crate::StatusRead> = Map::new("status/");
pub const COUNT: Item<u64> = Item::new("count");
pub const WATCHES: Map<u64, Watching> = Map::new("watch/");
pub const WATCH_COUNT: Item<u64> = Item::new("watch-count");

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Live {
    pub call: String,
    pub outcome: Option<Body>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Binding {
    pub thread: u64,
    pub place: Place,
}

// a model's record as harness stored it before `compact_at`, which `COMPACT_AT` holds
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct StoredModel {
    pub id: String,
    pub owner: AccountNumber,
    pub wire: Wire,
    pub service: String,
    pub name: String,
    pub window: Window,
}

impl StoredModel {
    pub fn row(self, compact_at: Option<u64>) -> ModelRow {
        ModelRow {
            id: self.id,
            owner: self.owner,
            wire: self.wire,
            service: self.service,
            name: self.name,
            window: self.window,
            compact_at,
        }
    }
}

// a model call in flight, or a turn's relays: egress answers it by its deadline, and `watch` is
// the run harness scheduled past that
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Flight {
    pub made: u64,
    pub moved: u64,
    pub wait: u64,
    pub config: Option<BlobId>,
    pub watch: Option<EntryId>,
}

// a failed model call asked again: not before block `at`, nor before time `until` when the model
// named one, paced from block `since` at `since_time`
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Retry {
    pub attempt: u32,
    pub at: u64,
    pub until: Option<u64>,
    pub since: u64,
    pub since_time: u64,
    pub entry: Option<EntryId>,
}

// a tool call whose stream harness opens and hears for its agent: the egress request, and
// the call and conversation it wakes once the stream says its end
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Watching {
    pub thread: u64,
    pub agent: AccountNumber,
    pub call: String,
    pub tool: String,
    pub about: String,
    pub request: abi::role::egress::Request,
}

// one model account's usage windows, as its last answer or status reading named them, at
// (service, ccs's name for the account, else the model's id for it)
#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Budget {
    pub label: Option<String>,
    pub organization: Option<String>,
    pub status: Option<String>,
    pub fallback: Option<String>,
    pub error: Option<String>,
    pub windows: Vec<BudgetWindow>,
    pub height: u64,
    pub time: u64,
}

// a usage window: used as sent and in millionths, its reset in seconds since the epoch, and
// where harness first saw it before that reset, which its pace is measured from
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct BudgetWindow {
    pub name: String,
    pub utilization: String,
    pub used: u64,
    pub reset: u64,
    pub status: Option<String>,
    pub since: Seen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Seen {
    pub used: u64,
    pub time: u64,
}

// a service's answers in clock minute `minute` (block time / 60 s), and in the minute before it
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Pace {
    pub minute: u64,
    pub calls: u64,
    pub before: u64,
}

pub(crate) fn newest_first(id: u64) -> u64 {
    u64::MAX - id
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Written {
    Thread(u64),
    Frame { thread: u64, seq: u64 },
    Call { thread: u64, call: String },
    Held(u64),
    Withheld(u64),
    Partial(u64),
    Topic(u64),
    Tally(u64),
    Settling(u64),
    Agent(AccountNumber),
    Model,
    Budget,
}

impl Written {
    pub fn thread(&self) -> Option<u64> {
        match self {
            Written::Thread(thread)
            | Written::Held(thread)
            | Written::Withheld(thread)
            | Written::Partial(thread)
            | Written::Topic(thread)
            | Written::Tally(thread)
            | Written::Settling(thread)
            | Written::Frame { thread, .. }
            | Written::Call { thread, .. } => Some(*thread),
            Written::Agent(_) | Written::Model | Written::Budget => None,
        }
    }

    // a key that `written` names so: what a block writing it lists
    pub fn key(&self) -> Vec<u8> {
        match self {
            Written::Thread(thread) => THREADS.key(thread),
            Written::Frame { thread, seq } => FRAMES.key(&(*thread, *seq)),
            Written::Call { thread, call } => CALLS.key(&(*thread, call.clone())),
            Written::Held(thread) => HELD.key(thread),
            Written::Withheld(thread) => WITHHELD.key(thread),
            Written::Partial(thread) => PARTIALS.key(thread),
            Written::Topic(thread) => TOPICS.key(thread),
            Written::Tally(thread) => TALLIES.key(thread),
            Written::Settling(thread) => SETTLING.key(thread),
            Written::Agent(agent) => CURRENT.key(agent),
            Written::Model => MODELS.key(&()),
            Written::Budget => BUDGETS.key(&()),
        }
    }
}

fn under<K: KeyCodec>(prefix: &[u8], key: &[u8]) -> Option<K> {
    let mut rest = key.strip_prefix(prefix)?;
    K::decode_key(&mut rest)
}

// what a key of a table keyed by thread names
type OfThread = fn(u64) -> Written;

pub fn written(key: &[u8]) -> Option<Written> {
    if let Some((thread, seq)) = under::<(u64, u64)>(&FRAMES.key(&()), key) {
        return Some(Written::Frame { thread, seq });
    }
    if let Some((thread, call)) = under::<(u64, String)>(&CALLS.key(&()), key) {
        return Some(Written::Call { thread, call });
    }
    let threads: [(Vec<u8>, OfThread); 7] = [
        (THREADS.key(&()), Written::Thread),
        (HELD.key(&()), Written::Held),
        (WITHHELD.key(&()), Written::Withheld),
        (PARTIALS.key(&()), Written::Partial),
        (TOPICS.key(&()), Written::Topic),
        (TALLIES.key(&()), Written::Tally),
        (SETTLING.key(&()), Written::Settling),
    ];
    for (prefix, wrote) in threads {
        if let Some(thread) = under::<u64>(&prefix, key) {
            return Some(wrote(thread));
        }
    }
    let agents = [
        CURRENT.key(&()),
        LAST.key(&()),
        BY_AGENT.key(&()),
        PLACES.key(&()),
        BOUND.key(&()),
    ];
    for prefix in agents {
        if let Some(agent) = under::<AccountNumber>(&prefix, key) {
            return Some(Written::Agent(agent));
        }
    }
    let budget = [
        BUDGETS.key(&()),
        PACES.key(&()),
        FLIGHTS.key(&()),
        READS.key(&()),
    ];
    if budget.iter().any(|prefix| key.starts_with(prefix)) {
        return Some(Written::Budget);
    }
    let models = [MODELS.key(&()), COMPACT_AT.key(&())];
    let model = models.iter().any(|prefix| key.starts_with(prefix));
    model.then_some(Written::Model)
}
