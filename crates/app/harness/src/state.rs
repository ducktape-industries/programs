//! Everything harness keeps: provider routes, threads by agent, current and bound threads, each place's
//! last thread, frames, held input, sealed parts, tool and live calls, withheld frames, streaming
//! replies, each thread's topic and tally, booked runs, watched streams, what a written key names.
use ::program::{BlobId, EntryId};
use abi::role::identity::AccountNumber;
use attribution::{Change, Source};
use borsh::{BorshDeserialize, BorshSerialize};
use store::{Item, KeyCodec, Kind, Map, Owned, Set};

use crate::pictures::{Hidden, Picture};
use crate::{
    Body, Booking, CallRow, FinalError, Frame, Partial, Place, ProviderRouteRow, Sealed,
    SettleWindow, Tally, ThreadRow, WaitRow, Window,
};

pub const CONVERSATIONS: Map<AccountNumber, Owned<Conversations>> = Map::new("conversations/");
// Each provider's route is its owner's, of the kind `harness.route`: only its owner's ops write it.
pub const PROVIDER_ROUTES: Map<String, Owned<ProviderRouteRow>> = Map::new("provider-route/");
// the context window a turn learned for a provider's model: any agent's turn writes it, so it lives
// beside the owned route
pub const PROVIDER_WINDOWS: Map<(String, String), Window> = Map::new("provider-window/");
pub const LAST: Map<AccountNumber, (Source, u64)> = Map::new("last/");
pub const THREADS: Map<u64, ThreadRow> = Map::new("thread/");
pub const BY_AGENT: Set<(AccountNumber, u64)> = Set::new("by-agent/");
pub const PLACES: Map<(AccountNumber, String), u64> = Map::new("place/");
pub const BOUND: Map<(AccountNumber, String), Binding> = Map::new("bound/");
pub const CURRENT: Map<AccountNumber, u64> = Map::new("current/");
pub const FRAMES: Map<(u64, u64), Frame> = Map::new("frame/");
pub const HELD: Map<(u64, u64), Body> = Map::new("held/");
pub const HELD_AT: Map<(u64, u64), String> = Map::new("held-at/");
pub const SEALED: Map<(u64, u64), Sealed> = Map::new("sealed/");
pub const RELINED: Item<u64> = Item::new("relined/next");
pub const LANES: Map<(u64, u64), Lane> = Map::new("lane/");
pub const ASKED: Map<u64, Lane> = Map::new("asked-lane/");
pub const CALLS: Map<(u64, String), CallRow> = Map::new("call/");
pub const WITHHELD: Set<(u64, u64)> = Set::new("withheld/");
pub const PARTIALS: Map<u64, Partial> = Map::new("partial/");
pub const PASSES: Map<u64, u64> = Map::new("last-pass/");
// a streaming reply's text as each event added it (`replies.rs`): by thread, block and event
pub const PIECES: Map<(u64, u64, u64), String> = Map::new("piece/");
pub const LIVE: Map<(u64, u64), Live> = Map::new("live/");
pub const SHOWN: Map<(u64, String), String> = Map::new("shown/");
// the pictures each frame came with (`pictures.rs`), where files holds each; a held input's
// until it is appended, a tool call's until its result is; and how far the model refused them
pub const PICTURES: Map<(u64, u64), Vec<Picture>> = Map::new("picture/");
pub const HELD_PICTURES: Map<(u64, u64), Vec<Picture>> = Map::new("held-picture/");
pub const CALL_PICTURES: Map<(u64, String), Vec<Picture>> = Map::new("call-picture/");
pub const HIDDEN: Map<u64, Hidden> = Map::new("hidden/");
/// a thread's topic: the one line its agent last set with `thread_topic`
pub const TOPICS: Map<u64, String> = Map::new("topic/");
pub const TALLIES: Map<u64, Tally> = Map::new("tally/");
pub const BOOKINGS: Map<u64, Booking> = Map::new("booking/");
pub const BOOKED: Set<(AccountNumber, u64)> = Set::new("booked/");
pub const WAKE_THREADS: Map<u64, u64> = Map::new("wake-thread/");
pub const ARMING: Set<(AccountNumber, u64, String)> = Set::new("arming/");
pub const UNCLAIMED: Map<(AccountNumber, u64), Change> = Map::new("unclaimed/");
pub const WAIT_WAKES: Map<u64, u64> = Map::new("wait-wake/");
pub const WAKE_WAITS: Map<u64, u64> = Map::new("wake-wait/");
pub const FLIGHTS: Map<u64, Flight> = Map::new("flight/");
pub const RETRIES: Map<u64, Retry> = Map::new("retry/");
pub const RETRY_SINCE: Map<u64, u64> = Map::new("retry-since/");
pub const REPORTS: Map<(AccountNumber, u64), String> = Map::new("report/");
pub const REPORTING: Set<AccountNumber> = Set::new("reporting/");
// a thread whose general input waits out its agent's `settle`, and the wake-up that ends it
pub const SETTLING: Map<u64, SettleWindow> = Map::new("settling/");
pub const BUDGETS: Map<(String, String), Budget> = Map::new("budget/");
pub const BUDGET_FACTS: Map<(String, String), BudgetFacts> = Map::new("budget-facts/");
pub const PACES: Map<String, Pace> = Map::new("pace/");
pub const COUNT: Item<u64> = Item::new("count");
pub const WATCHES: Map<u64, Watching> = Map::new("watch/");
pub const WATCH_COUNT: Item<u64> = Item::new("watch-count");
pub const HEARTBEATS: Map<u64, Heartbeat> = Map::new("watch-heartbeat/");
pub const CLOCK: Item<Clock> = Item::new("clock");
pub const WAITS: Map<u64, WaitRow> = Map::new("wait/");
pub const WAITING: Set<(AccountNumber, u64)> = Set::new("waiting/");
// each conversation's waits not over yet
pub const ARMED: Set<(u64, u64)> = Set::new("armed/");
// threads whose turn armed a wait: it ends once its calls are back
pub const ENDING: Set<u64> = Set::new("ending/");
pub const WAIT_COUNT: Item<u64> = Item::new("wait-count");
pub const PROMISES: Map<u64, crate::PromiseRow> = Map::new("promise/");
pub const PROMISING: Set<(AccountNumber, u64)> = Set::new("promising/");
pub const ASKING: Set<(AccountNumber, u64)> = Set::new("asking/");
pub const PROMISE_ENTRIES: Map<u64, u64> = Map::new("promise-entry/");
pub const PROMISE_COUNT: Item<u64> = Item::new("promise-count");
// each agent's open message waits, by channel
pub const MESSAGE_WAITS: Set<(AccountNumber, (String, u64))> = Set::new("wait-message/");
// each tool call whose write awaits its answer, and each stream a watch listens to, by thread:
// what tells a later run that the frame awaiting it is gone
pub const AWAITED: Map<(u64, String), Awaited> = Map::new("awaited/");
pub const RECEIPTED: Set<(u64, String)> = Set::new("receipted/");
pub const STREAMING: Map<(u64, u64), Flight> = Map::new("streaming/");
// the harness code that runs, and the block harness first ran it at (`resume::switched`)
pub const RUNNING: Item<(BlobId, u64)> = Item::new("running");
pub const ROUTES: Map<u64, crate::Route> = Map::new("route/");
// the account each thread's latest model call went to, and who answered it
pub const PICKS: Map<u64, Pick> = Map::new("pick/");
// the (service, account) pairs a thread's failed call passed over until it is answered
pub const TRIED: Set<(u64, (String, String))> = Set::new("tried/");
// who answered each of a thread's turns and summaries, by frame
pub const ANSWERED: Map<(u64, u64), crate::Answerer> = Map::new("answered/");
// the rooms harness seeded from an agent's `listen` (`listening::seed`): chat's choice stands after
pub const SEEDED: Set<(AccountNumber, String)> = Set::new("seeded/");
// message waits that watch their room (`Op::Watch`); one armed before that listened instead
pub const WATCHED: Set<u64> = Set::new("wait-watched/");
// what each conversation was last told of each chat room's rules (`room_rules.rs`)
pub const RULES_TOLD: Map<(u64, String), crate::room_rules::Told> = Map::new("rules-told/");
pub const SEALED_FOR: Map<u64, (String, Option<String>)> = Map::new("sealed-for/");
// the chat place each conversation's turn keeps its live draft in, where its forming post goes, and
// what the draft says the turn is doing (`drafts.rs`)
pub const DRAFTING: Map<u64, Drafted> = Map::new("drafting/");
pub const FINAL_ERRORS: Map<(AccountNumber, u64), FinalError> = Map::new("final-error/");
pub const RUNS: Map<(AccountNumber, String), Run> = Map::new("mention-run/");
pub const FLEET_RUNS: Set<(u64, u64)> = Set::new("fleet-run/");
pub const UNANSWERED: Map<(AccountNumber, u64), Unanswered> = Map::new("unanswered/");
pub const UNANSWERED_HELD: Set<(u64, AccountNumber, u64)> = Set::new("unanswered-held/");
pub const UNANSWERED_UNHEARD: Set<(AccountNumber, u64)> = Set::new("unanswered-unheard/");
pub const MODEL_KEPT: Map<u64, crate::ModelKept> = Map::new("model-kept/");

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Unanswered {
    pub channel: String,
    pub thread: Option<u64>,
    pub text: String,
    pub held_by: Option<u64>,
}
pub const SECTIONS: Map<(String, String), SectionRow> = Map::new("code-section/");

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SectionRow {
    pub code: BlobId,
    pub section: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Drafted {
    pub place: Place,
    pub doing: String,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Lane {
    pub provider: String,
    pub model: String,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Live {
    pub call: String,
    pub outcome: Option<Body>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Run {
    pub author: AccountNumber,
    pub mentioned: u64,
    pub last: u64,
    pub thread: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Binding {
    pub thread: u64,
    pub place: Place,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Conversations {
    pub manager: AccountNumber,
    // how many conversations were started or forked since it was claimed
    pub rev: u64,
}

impl Kind for Conversations {
    const KIND: &'static str = "harness.agent-conversation";
}

impl Kind for ProviderRouteRow {
    const KIND: &'static str = "harness.route";
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

// a tool call's write awaiting its answer since block `height`: harness's code then, as a code
// change cancels the frame that awaits it, and the block egress answers by when each write asks
// egress for an answer whole
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Awaited {
    pub height: u64,
    pub code: Option<BlobId>,
    pub deadline: Option<u64>,
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

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Heartbeat {
    pub height: u64,
    pub time: u64,
    pub body: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Clock {
    pub older: Tick,
    pub newer: Tick,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Tick {
    pub height: u64,
    pub time: u64,
}

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

#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct BudgetFacts {
    pub provider: Option<String>,
    pub identity: Option<String>,
    pub previous: Option<String>,
    pub changed: Option<u64>,
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

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Pick {
    pub service: String,
    pub account: Option<String>,
    pub summary: Option<u64>,
    pub candidates: Vec<(String, Option<String>)>,
    pub label: Option<String>,
    pub organization: Option<String>,
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
    Route,
    Budget,
    Armed(u64),
    FinalError { agent: AccountNumber, thread: u64 },
    Runs(AccountNumber),
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
            | Written::Armed(thread)
            | Written::Frame { thread, .. }
            | Written::Call { thread, .. }
            | Written::FinalError { thread, .. } => Some(*thread),
            Written::Agent(_) | Written::Route | Written::Budget | Written::Runs(_) => None,
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
            Written::Route => PROVIDER_ROUTES.key(&()),
            Written::Budget => BUDGETS.key(&()),
            Written::Armed(thread) => ARMED.key(thread),
            Written::FinalError { agent, thread } => FINAL_ERRORS.key(&(*agent, *thread)),
            Written::Runs(agent) => RUNS.key(&(*agent, String::new())),
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
    if let Some((agent, thread)) = under::<(AccountNumber, u64)>(&FINAL_ERRORS.key(&()), key) {
        return Some(Written::FinalError { agent, thread });
    }
    let threads: [(Vec<u8>, OfThread); 9] = [
        (THREADS.key(&()), Written::Thread),
        (HELD.key(&()), Written::Held),
        (WITHHELD.key(&()), Written::Withheld),
        (PARTIALS.key(&()), Written::Partial),
        (PIECES.key(&()), Written::Partial),
        (TOPICS.key(&()), Written::Topic),
        (TALLIES.key(&()), Written::Tally),
        (SETTLING.key(&()), Written::Settling),
        (ARMED.key(&()), Written::Armed),
    ];
    for (prefix, wrote) in threads {
        if let Some(thread) = under::<u64>(&prefix, key) {
            return Some(wrote(thread));
        }
    }
    if let Some(agent) = under::<AccountNumber>(&RUNS.key(&()), key) {
        return Some(Written::Runs(agent));
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
        BUDGET_FACTS.key(&()),
        PACES.key(&()),
        FLIGHTS.key(&()),
    ];
    if budget.iter().any(|prefix| key.starts_with(prefix)) {
        return Some(Written::Budget);
    }
    let routes = [PROVIDER_ROUTES.key(&()), PROVIDER_WINDOWS.key(&())];
    let route = routes.iter().any(|prefix| key.starts_with(prefix));
    route.then_some(Written::Route)
}
