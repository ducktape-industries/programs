//! Everything harness keeps: models, threads by agent, current and bound threads, each place's
//! last thread, frames, held input, sealed parts, tool calls and live calls, withheld frames,
//! streaming replies, each thread's topic and tally, booked runs, what a written key belongs to.
use ::program::{BlobId, EntryId};
use abi::role::identity::AccountNumber;
use attribution::{Change, Source};
use borsh::{BorshDeserialize, BorshSerialize};
use store::{Item, KeyCodec, Map, Set};

use crate::{
    Body, Booking, CallRow, Frame, ModelRow, Partial, Place, Sealed, Tally, ThreadRow, UsageWindow,
};

pub const MODELS: Map<String, ModelRow> = Map::new("model/");
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
pub const COUNT: Item<u64> = Item::new("count");

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
    Agent(AccountNumber),
    Model,
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
            | Written::Frame { thread, .. }
            | Written::Call { thread, .. } => Some(*thread),
            Written::Agent(_) | Written::Model => None,
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
            Written::Agent(agent) => CURRENT.key(agent),
            Written::Model => MODELS.key(&()),
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
    let threads: [(Vec<u8>, OfThread); 6] = [
        (THREADS.key(&()), Written::Thread),
        (HELD.key(&()), Written::Held),
        (WITHHELD.key(&()), Written::Withheld),
        (PARTIALS.key(&()), Written::Partial),
        (TOPICS.key(&()), Written::Topic),
        (TALLIES.key(&()), Written::Tally),
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
    key.starts_with(&MODELS.key(&())).then_some(Written::Model)
}
