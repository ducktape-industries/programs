//! The `harness` program: agents that consensus runs. An agent is a program account whose script
//! calls harness and names its configuration in files (`config.rs`); what it hears reaches harness
//! through `attribution`, its model through the egress role (`wire/`, the account by `routes.rs`).
mod adopt;
mod agents;
mod bookings;
mod budget;
mod calls;
mod codec;
pub mod config;
mod conversations;
mod gate;
mod listening;
mod models;
mod pictures;
mod program;
mod queries;
mod resume;
mod routes;
mod schema;
mod state;
mod streams;
mod tallies;
#[cfg(test)]
mod tests;
mod tools;
mod turns;
mod waiting;
mod waits;
mod watches;
mod wire;

pub use ::program::{BlobId, Principal};
pub use abi::role::egress::Event;
pub use abi::role::identity::{AccountNumber, Standing};
pub use attribution::Change;
pub use config::{Config, Malformed, ServerTool, Thinking};
pub use program::Harness;
pub use store::{PageRequest, PageResponse};

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

pub use gate::{agent_key, conversation_key, model_key, thread_key};
pub use state::{Written, written};

// The owned tables: the keys ownership records a model and an agent's conversations under.
pub mod tables {
    pub use crate::state::{CONVERSATIONS, MODELS};
}

pub const PROGRAM: &str = "harness";
pub const PART: &str = "part";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Op {
    Close {
        thread: u64,
    },
    Attributed(Box<Change>),
    SetModel {
        id: String,
        wire: Wire,
        service: String,
        name: String,
    },
    RemoveModel {
        id: String,
    },
    Streamed {
        thread: u64,
        event: Event,
    },
    NewThread {
        agent: AccountNumber,
    },
    ForkThread {
        agent: AccountNumber,
    },
    OpenThread {
        agent: AccountNumber,
        place: Place,
    },
    Book {
        height: u64,
        target: String,
        payload: Vec<u8>,
        thread: Option<u64>,
    },
    Unbook {
        entry: u64,
    },
    RunBooked,
    Resume {
        thread: u64,
    },
    Report {
        manager: AccountNumber,
    },
    Settle {
        thread: u64,
    },
    Watch {
        watch: u64,
    },
    Heard {
        watch: u64,
        event: Event,
    },
    SetCompactAt {
        id: String,
        compact_at: Option<u64>,
    },
    ReadBudget {
        service: String,
    },
    Wait {
        thread: u64,
        event: WaitFor,
    },
    Unwait {
        wait: u64,
    },
    WaitDue,
    PodWait {
        wait: u64,
    },
    PodEvent {
        wait: u64,
        event: Event,
    },
    Forged(forge::Watched),
    ClaimConversations {
        agent: AccountNumber,
    },
    // one-time (J-011 ⑥): records owners for harness's owned rows from before the guard
    AdoptOwners {
        kind: String,
        after: Option<Vec<u8>>,
        limit: u64,
    },
    // the services a model is reached through, its record's first: harness picks among them
    SetServices {
        id: String,
        services: Vec<String>,
    },
    ReadBudgets,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Place {
    Channel { channel: String },
    Thread { channel: String, root: u64 },
}

impl Place {
    pub fn key(&self) -> String {
        match self {
            Place::Channel { channel } => format!("{}/{channel}", chat::PROGRAM),
            Place::Thread { channel, root } => format!("{}/{channel}/{root}", chat::PROGRAM),
        }
    }

    pub fn channel(&self) -> &str {
        match self {
            Place::Channel { channel } | Place::Thread { channel, .. } => channel,
        }
    }

    pub fn words(&self) -> String {
        match self {
            Place::Channel { channel } => format!("chat channel {channel}"),
            Place::Thread { channel, root } => {
                format!("the thread of message {root} in chat channel {channel}")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Wire {
    AnthropicMessages,
}

impl Wire {
    pub const ALL: &'static [Wire] = &[Wire::AnthropicMessages];

    pub fn label(self) -> &'static str {
        match self {
            Wire::AnthropicMessages => "Anthropic Messages",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Window {
    Unasked,
    Unreported,
    Tokens(u64),
}

impl Window {
    pub fn tokens(self) -> Option<u64> {
        match self {
            Window::Tokens(tokens) => Some(tokens),
            Window::Unasked | Window::Unreported => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct ModelRow {
    pub id: String,
    pub owner: AccountNumber,
    pub wire: Wire,
    pub service: String,
    pub name: String,
    pub window: Window,
    pub compact_at: Option<u64>,
}

// when a thread starts compacting: the agent's `compact_at`, else its model's
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Threshold {
    Agent(u64),
    Model(u64),
}

impl Threshold {
    pub fn of(config: &Config, model: Option<&ModelRow>) -> Option<Threshold> {
        let model = model
            .and_then(|model| model.compact_at)
            .map(Threshold::Model);
        config.compact_at.map(Threshold::Agent).or(model)
    }

    pub fn tokens(self) -> u64 {
        match self {
            Threshold::Agent(tokens) | Threshold::Model(tokens) => tokens,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct AgentSetup {
    pub agent: AccountNumber,
    pub manager: AccountNumber,
    pub standing: Standing,
    pub path: String,
    pub configured: Configured,
    pub current: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Configured {
    Unreadable {
        reason: String,
    },
    Malformed {
        snapshot: BlobId,
        malformed: Malformed,
    },
    UnknownModel {
        snapshot: BlobId,
        config: Config,
    },
    Ready {
        snapshot: BlobId,
        config: Config,
        model: ModelRow,
    },
}

impl Configured {
    pub fn problem(&self, path: &str) -> Option<String> {
        match self {
            Configured::Unreadable { reason } => Some(format!(
                "the agent's configuration {path} could not be read: {reason}"
            )),
            Configured::Malformed { malformed, .. } => Some(format!(
                "the agent's configuration {path} is malformed: {malformed}"
            )),
            Configured::UnknownModel { config, .. } => Some(format!(
                "the agent's configuration {path} names model {}, which harness has no record of",
                config.model
            )),
            Configured::Ready { .. } => None,
        }
    }

    pub fn config(&self) -> Option<(BlobId, &Config)> {
        match self {
            Configured::UnknownModel { snapshot, config }
            | Configured::Ready {
                snapshot, config, ..
            } => Some((*snapshot, config)),
            Configured::Unreadable { .. } | Configured::Malformed { .. } => None,
        }
    }

    pub fn threshold(&self) -> Option<Threshold> {
        let (_, config) = self.config()?;
        Threshold::of(config, self.model())
    }

    pub fn model(&self) -> Option<&ModelRow> {
        match self {
            Configured::Ready { model, .. } => Some(model),
            Configured::Unreadable { .. }
            | Configured::Malformed { .. }
            | Configured::UnknownModel { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Frame {
    pub seq: u64,
    pub height: u64,
    pub time: u64,
    pub body: Body,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Body {
    Post {
        author: Principal,
        text: String,
    },
    Turn {
        model: String,
        config: BlobId,
        said: Vec<Said>,
        stop: Stop,
        usage: Usage,
    },
    Refused {
        call: String,
        tool: String,
        problem: String,
    },
    Result {
        call: String,
        tool: String,
        text: String,
        failed: bool,
    },
    Notice {
        text: String,
    },
    Compaction {
        wire: Wire,
        block: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Role {
    User,
    Assistant,
}

impl Body {
    pub fn role(&self) -> Option<Role> {
        match self {
            Body::Post { .. } | Body::Refused { .. } | Body::Result { .. } => Some(Role::User),
            Body::Turn { .. } => Some(Role::Assistant),
            Body::Notice { .. } | Body::Compaction { .. } => None,
        }
    }

    pub fn text(&self) -> String {
        match self {
            Body::Post { text, .. } | Body::Result { text, .. } | Body::Notice { text } => {
                text.clone()
            }
            Body::Refused { problem, .. } => problem.clone(),
            Body::Turn { said, .. } => said
                .iter()
                .filter_map(Said::text)
                .collect::<Vec<_>>()
                .join("\n"),
            Body::Compaction { wire, block } => wire::summary_words(*wire, block),
        }
    }
}

impl Said {
    fn text(&self) -> Option<String> {
        match self {
            Said::Text(text) | Said::Thinking { text, .. } | Said::Cited { text, .. } => {
                Some(text.clone())
            }
            Said::ToolUse { name, input, .. } | Said::ServerToolUse { name, input, .. } => {
                Some(format!("{name} {input}"))
            }
            Said::ServerToolResult(block) => Some(block.clone()),
            Said::RedactedThinking(_) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Said {
    Text(String),
    Thinking {
        text: String,
        signature: String,
    },
    RedactedThinking(String),
    ToolUse {
        id: String,
        name: String,
        input: String,
    },
    ServerToolUse {
        id: String,
        name: String,
        input: String,
    },
    ServerToolResult(String),
    Cited {
        text: String,
        citations: Vec<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Stop {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
    Pause,
    Refusal,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema,
)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema,
)]
pub struct Tally {
    pub usage: Usage,
    pub summaries: u64,
}

impl Tally {
    pub fn add(&mut self, usage: &Usage, summaries: u64) {
        self.usage.input += usage.input;
        self.usage.output += usage.output;
        self.usage.cache_read += usage.cache_read;
        self.usage.cache_write += usage.cache_write;
        self.summaries += summaries;
    }

    pub fn count(&mut self, body: &Body) {
        match body {
            Body::Turn { usage, .. } => self.add(usage, 0),
            Body::Compaction { .. } => self.add(&Usage::default(), 1),
            Body::Post { .. }
            | Body::Refused { .. }
            | Body::Result { .. }
            | Body::Notice { .. } => {}
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Partial {
    pub wire: Wire,
    pub model: String,
    pub said: Vec<Said>,
    pub forming: Forming,
    pub stop: Option<Stop>,
    pub usage: Usage,
    pub next_seq: u64,
    pub flow: Flow,
}

impl Partial {
    pub fn new(wire: Wire) -> Partial {
        Partial {
            wire,
            model: String::new(),
            said: Vec::new(),
            forming: Forming::Nothing,
            stop: None,
            usage: Usage::default(),
            next_seq: 0,
            flow: Flow::Open,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Forming {
    Nothing,
    Kept { index: u64 },
    Skipped { index: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Flow {
    Open,
    Ended,
    Overflowed,
    Broken { reason: String },
    Failed { text: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Asking {
    Idle,
    Learning,
    Compacting { through: u64 },
    Asked { upto: u64 },
    Running,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct ThreadRow {
    pub id: u64,
    pub agent: AccountNumber,
    pub manager: AccountNumber,
    pub place: String,
    pub title: String,
    pub opened: u64,
    pub closed: bool,
    pub frames: u64,
    pub last_input: u64,
    pub seen: u64,
    pub last_stop: Option<Stop>,
    pub asking: Asking,
    pub open_calls: u32,
    pub held: u64,
    pub sealed: u64,
    pub base: u64,
    pub tail_from: u64,
    pub summary: Option<u64>,
    pub context: Context,
    pub compact_before: Option<u64>,
    pub last_height: u64,
    pub last_time: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Sealed {
    pub end: u64,
    pub role: Role,
    pub wire: Wire,
    pub part: BlobId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Context {
    Measured(u64),
    Overflowed,
}

impl ThreadRow {
    pub fn due(&self) -> bool {
        let unseen_input = self.last_input > self.seen;
        let paused = self.last_stop == Some(Stop::Pause);
        unseen_input || paused
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct WithheldFrame {
    pub seq: u64,
    pub size: u64,
}

// a thread's settle window: opened at a block and its time, it ends once a block's time reaches
// `until`; `entry` is the scheduled wake-up that comes to see
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct SettleWindow {
    pub opened: u64,
    pub opened_time: u64,
    pub until: u64,
    pub entry: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum CallState {
    Relaying,
    Writing,
    Done { height: u64, failed: bool },
    Showing,
    Voided { height: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct CallRow {
    pub thread: u64,
    pub id: String,
    pub tool: String,
    pub input: String,
    pub made: u64,
    pub state: CallState,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub schema: String,
    pub writes: bool,
}

// how much of a model service's usage window the last answer said is spent, and when the window
// resets (seconds since the epoch), as its headers put them
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct UsageWindow {
    pub utilization: String,
    pub reset: u64,
    pub height: u64,
}

// a whole usage window, in the millionths `WindowOutlook::used` counts
pub const WHOLE_WINDOW: u64 = 1_000_000;

// the model budget: each model account's windows, each service's calls in the last whole minute
// and in this one so far, the turns with a model call or relays out, each service's last status
// reading, and `time`, the newest block's (ms)
#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct BudgetReport {
    pub time: u64,
    pub turns: u64,
    pub services: Vec<ServicePace>,
    pub accounts: Vec<AccountBudget>,
    pub reads: Vec<StatusRead>,
}

// the last time `ReadBudget` read a service's status of every account it holds (ccs's
// `/ccs/status`), and what came of it
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct StatusRead {
    pub service: String,
    pub height: u64,
    pub time: u64,
    pub outcome: StatusOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum StatusOutcome {
    // it named this many accounts, each now on its row
    Read { accounts: u64 },
    // the service keeps no status to read (it answered 404)
    Unavailable,
    // why it could not be read
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct ServicePace {
    pub service: String,
    pub last_minute: u64,
    pub this_minute: u64,
}

// one model account as its last answer or status reading put it: `account` is ccs's name for it
// (`label`) once ccs names it, else the model's id for it (`organization`), empty when neither
// is known; `status` and `fallback` are the model's words for it, `error` ccs's when it could
// not read the account
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct AccountBudget {
    pub service: String,
    pub account: String,
    pub label: Option<String>,
    pub organization: Option<String>,
    pub status: Option<String>,
    pub fallback: Option<String>,
    pub error: Option<String>,
    pub height: u64,
    pub time: u64,
    pub windows: Vec<WindowOutlook>,
}

// one usage window: used as sent and in millionths, its reset (seconds since the epoch), when
// harness first saw this window (ms), and when it reaches all of its budget at the pace it
// climbed at since (ms); none while it has not climbed or once its reset has passed
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct WindowOutlook {
    pub name: String,
    pub utilization: String,
    pub used: u64,
    pub reset: u64,
    pub status: Option<String>,
    pub since: u64,
    pub runs_out: Option<u64>,
}

// the service and ccs account a thread's prompt cache was last written on (none: the service
// chose), the summary the prompt it wrote began with, and the block it answered at
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Route {
    pub service: String,
    pub account: Option<String>,
    pub summary: Option<u64>,
    pub height: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Routed {
    pub thread: u64,
    pub route: Route,
}

// who answered a thread's turn or summary at frame `seq`: the service, ccs's name for the account
// and the model's id for it, and whether the thread moved there from the account before, which
// re-wrote its prompt cache (the turn's `usage.cache_write` says how much)
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Answerer {
    pub seq: u64,
    pub service: String,
    pub account: Option<String>,
    pub organization: Option<String>,
    pub moved: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Booking {
    pub entry: u64,
    pub agent: AccountNumber,
    pub thread: Option<u64>,
    pub booked: u64,
    pub height: u64,
    pub target: String,
    pub payload: Vec<u8>,
    pub ran: Option<Ran>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Ran {
    pub height: u64,
    pub failed: bool,
    pub text: String,
}

// what a wait (`wait_for`) waits for: a block's height, a pod of the pods service to end, a chat
// message, a forge change to move, close or merge, or a forge ref to move
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum WaitFor {
    Height {
        height: u64,
    },
    Pod {
        name: String,
    },
    Message {
        channel: String,
        from: Option<AccountNumber>,
        mentions_me: bool,
    },
    Change {
        repo: String,
        n: u64,
    },
    Ref {
        repo: String,
        reference: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct WaitRow {
    pub id: u64,
    pub agent: AccountNumber,
    pub thread: u64,
    pub made: u64,
    pub event: WaitFor,
    pub ended: Option<Ended>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Ended {
    pub height: u64,
    pub text: String,
}

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Ask,
)]
#[ask(Harness)]
pub enum Query {
    #[ask(Reply::Agent(Option<AgentSetup>))]
    Agent { agent: AccountNumber },
    #[ask(Reply::Thread(Option<ThreadRow>))]
    #[reads(state::THREADS)]
    Thread { thread: u64 },
    #[ask(Reply::Threads(PageResponse<ThreadRow>))]
    #[reads(state::BY_AGENT, state::THREADS)]
    Threads {
        agent: AccountNumber,
        page: PageRequest,
    },
    #[ask(Reply::Frames(PageResponse<Frame>))]
    #[reads(state::FRAMES)]
    Frames { thread: u64, page: PageRequest },
    #[ask(Reply::Held(Vec<Body>))]
    #[reads(state::HELD)]
    Held { thread: u64 },
    #[ask(Reply::Calls(PageResponse<CallRow>))]
    #[reads(state::CALLS)]
    Calls { thread: u64, page: PageRequest },
    #[ask(Reply::Tools(Vec<ToolInfo>))]
    Tools,
    #[ask(Reply::Thread(Option<ThreadRow>))]
    #[reads(state::PLACES, state::THREADS)]
    ThreadAt { agent: AccountNumber, place: String },
    #[ask(Reply::Model(Option<ModelRow>))]
    #[reads(state::MODELS, state::COMPACT_AT, state::WINDOWS)]
    Model { id: String },
    #[ask(Reply::Models(PageResponse<ModelRow>))]
    #[reads(state::MODELS, state::COMPACT_AT, state::WINDOWS)]
    Models { page: PageRequest },
    #[ask(Reply::Withheld(Vec<WithheldFrame>))]
    #[reads(state::WITHHELD, state::FRAMES)]
    Withheld { thread: u64 },
    #[ask(Reply::Partial(Option<Partial>))]
    #[reads(state::PARTIALS)]
    Partial { thread: u64 },
    /// the line the thread's agent last set with `thread_topic`, if it set one
    #[ask(Reply::Topic(Option<String>))]
    #[reads(state::TOPICS)]
    Topic { thread: u64 },
    #[ask(Reply::Threads(PageResponse<ThreadRow>))]
    #[reads(state::BOUND, state::THREADS)]
    Bound {
        agent: AccountNumber,
        page: PageRequest,
    },
    #[ask(Reply::Tally(Tally))]
    #[reads(state::TALLIES, state::FRAMES)]
    Tally { thread: u64 },
    #[ask(Reply::Frames(PageResponse<Frame>))]
    #[reads(state::FRAMES)]
    FramesFrom {
        thread: u64,
        from: u64,
        page: PageRequest,
    },
    #[ask(Reply::Call(Option<CallRow>))]
    #[reads(state::CALLS)]
    Call { thread: u64, call: String },
    #[ask(Reply::Bookings(PageResponse<Booking>))]
    #[reads(state::BOOKED, state::BOOKINGS)]
    Bookings {
        agent: AccountNumber,
        page: PageRequest,
    },
    #[ask(Reply::Booking(Option<Booking>))]
    #[reads(state::BOOKINGS)]
    Booking { entry: u64 },
    #[ask(Reply::UsageWindow(Option<UsageWindow>))]
    #[reads(state::USAGE)]
    UsageWindow { service: String },
    #[ask(Reply::Settling(Option<SettleWindow>))]
    #[reads(state::SETTLING)]
    Settling { thread: u64 },
    #[ask(Reply::Budget(BudgetReport))]
    #[reads(state::BUDGETS, state::PACES, state::FLIGHTS, state::READS)]
    Budget,
    #[ask(Reply::Waits(PageResponse<WaitRow>))]
    #[reads(state::WAITING, state::WAITS)]
    Waits {
        agent: AccountNumber,
        page: PageRequest,
    },
    #[ask(Reply::Wait(Option<WaitRow>))]
    #[reads(state::WAITS)]
    Wait { wait: u64 },
    #[ask(Reply::Waits(PageResponse<WaitRow>))]
    #[reads(state::ARMED, state::WAITS)]
    Armed { thread: u64, page: PageRequest },
    // `Agent` with the system prompt left out (an empty `system`): what a
    // list of agents, read again on every block, needs of each
    #[ask(Reply::Agent(Option<AgentSetup>))]
    Card { agent: AccountNumber },
    // every service a model is reached through, its record's first
    #[ask(Reply::Services(Option<Vec<String>>))]
    #[reads(state::MODELS, state::MODEL_SERVICES)]
    Services { model: String },
    #[ask(Reply::Route(Option<Route>))]
    #[reads(state::ROUTES)]
    Route { thread: u64 },
    #[ask(Reply::Routes(PageResponse<Routed>))]
    #[reads(state::ROUTES)]
    Routes { page: PageRequest },
    #[ask(Reply::Answerers(PageResponse<Answerer>))]
    #[reads(state::ANSWERED)]
    Answerers { thread: u64, page: PageRequest },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Agent(Option<AgentSetup>),
    Thread(Option<ThreadRow>),
    Threads(PageResponse<ThreadRow>),
    Frames(PageResponse<Frame>),
    Held(Vec<Body>),
    Calls(PageResponse<CallRow>),
    Tools(Vec<ToolInfo>),
    Model(Option<ModelRow>),
    Models(PageResponse<ModelRow>),
    Withheld(Vec<WithheldFrame>),
    Partial(Option<Partial>),
    Topic(Option<String>),
    Tally(Tally),
    Call(Option<CallRow>),
    Bookings(PageResponse<Booking>),
    Booking(Option<Booking>),
    UsageWindow(Option<UsageWindow>),
    Settling(Option<SettleWindow>),
    Budget(BudgetReport),
    Waits(PageResponse<WaitRow>),
    Wait(Option<WaitRow>),
    Services(Option<Vec<String>>),
    Route(Option<Route>),
    Routes(PageResponse<Routed>),
    Answerers(PageResponse<Answerer>),
}

pub fn attributed_as() -> Vec<u8> {
    vec![1]
}

pub fn streamed_as(thread: u64) -> Vec<u8> {
    let mut deliver = vec![4];
    deliver.extend(thread.to_le_bytes());
    deliver
}

// the prefix each event of a watched tool call's stream comes back under: `Op::Heard`
pub fn heard_as(watch: u64) -> Vec<u8> {
    let mut deliver = vec![15];
    deliver.extend(watch.to_le_bytes());
    deliver
}

// the prefix forge's watchers tell harness each event under, read as `Op::Forged`
pub fn forged_as() -> Vec<u8> {
    vec![23]
}

// the prefix egress hands each event of a pod's wait stream under, read as `Op::PodEvent`
pub fn pod_event_as(wait: u64) -> Vec<u8> {
    let mut deliver = vec![22];
    deliver.extend(wait.to_le_bytes());
    deliver
}

// what one of harness's own egress requests waits for, read back from the request alone: a
// turn's streamed reply, a model's window, a thread's summary, a tool call's relay
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Awaited {
    Reply { thread: u64 },
    Window { name: String },
    Summary,
    Relay { call: String },
}

pub fn awaited(
    request: &abi::role::egress::Request,
    delivery: &abi::role::egress::Delivery,
) -> Option<Awaited> {
    use abi::role::egress::{Delivery, Request};
    match (request, delivery) {
        (Request::Relay { note }, _) => {
            let call = String::from_utf8(note.clone()).ok()?;
            Some(Awaited::Relay { call })
        }
        (Request::Http { .. }, Delivery::Events { deliver }) => {
            let bytes: [u8; 8] = deliver.get(1..)?.try_into().ok()?;
            let thread = u64::from_le_bytes(bytes);
            (streamed_as(thread) == *deliver).then_some(Awaited::Reply { thread })
        }
        (Request::Http { head, .. }, Delivery::Whole) => wire::awaited(&unpinned(head)),
    }
}

// a whole request's head without the account harness pinned it to, which says nothing of what it asks
fn unpinned(head: &abi::role::egress::http::Head) -> abi::role::egress::http::Head {
    let mut head = head.clone();
    head.headers
        .retain(|(key, _)| !key.eq_ignore_ascii_case(budget::CCS_ACCOUNT));
    head
}

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let (title, fields) = match op {
        Op::Close { thread } => (format!("Close thread {thread}"), Vec::new()),
        Op::Attributed(change) => (
            format!("Account {} is related to an object", change.recipient),
            vec![field("change", Value::count(change.seq))],
        ),
        Op::SetModel {
            id,
            wire,
            service,
            name,
        } => (
            format!("Set model · {id}"),
            vec![
                field("model", Value::text(id)),
                field("wire", Value::text(wire.label())),
                field("service", Value::text(service)),
                field("vendor's name", Value::text(name)),
            ],
        ),
        Op::RemoveModel { id } => (
            format!("Remove model · {id}"),
            vec![field("model", Value::text(id))],
        ),
        Op::NewThread { agent } => (
            format!("Start agent {agent}'s canonical conversation afresh"),
            vec![field("agent", Value::count(*agent))],
        ),
        Op::ForkThread { agent } => (
            format!("Fork agent {agent}'s conversation"),
            vec![field("agent", Value::count(*agent))],
        ),
        Op::Streamed { thread, event } => (
            format!("Event {} of thread {thread}'s model reply", event.seq),
            vec![field("data", Value::count(event.data.len() as u64))],
        ),
        Op::OpenThread { agent, place } => (
            format!(
                "Open a conversation with agent {agent} in {}",
                place.words()
            ),
            vec![
                field("agent", Value::count(*agent)),
                field("place", Value::text(place.key())),
            ],
        ),
        Op::Book { height, target, .. } => (
            format!("Book a run of {target} at height {height}"),
            vec![
                field("height", Value::count(*height)),
                field("program", Value::text(target)),
            ],
        ),
        Op::Unbook { entry } => (
            format!("Cancel booked run {entry}"),
            vec![field("entry", Value::count(*entry))],
        ),
        Op::RunBooked => ("Run a booked run".to_owned(), Vec::new()),
        Op::Resume { thread } => (
            format!(
                "Resume thread {thread}'s turn and tool calls if a restart or an update cut them off"
            ),
            vec![field("thread", Value::count(*thread))],
        ),
        Op::Report { manager } => (
            format!("Tell account {manager} which turns harness picked up"),
            vec![field("manager", Value::count(*manager))],
        ),
        Op::Settle { thread } => (
            format!("End thread {thread}'s settle window once it is due"),
            vec![field("thread", Value::count(*thread))],
        ),
        Op::Watch { watch } => (
            format!("Open the stream of watched call {watch}"),
            vec![field("watch", Value::count(*watch))],
        ),
        Op::Heard { watch, event } => (
            format!("Event {} of watched call {watch}'s stream", event.seq),
            vec![field("data", Value::count(event.data.len() as u64))],
        ),
        Op::SetCompactAt { id, compact_at } => {
            let at = match compact_at {
                Some(tokens) => format!("{tokens} tokens"),
                None => "when the window fills".to_owned(),
            };
            (
                format!("Set when model {id} starts compacting"),
                vec![
                    field("model", Value::text(id)),
                    field("compacts at", Value::text(at)),
                ],
            )
        }
        Op::ReadBudget { service } => (
            format!("Read {service}'s status of every model account it holds"),
            vec![field("service", Value::text(service))],
        ),
        Op::Wait { thread, event } => (
            format!(
                "Wake conversation {thread} when it is time for {}",
                waits::what(event)
            ),
            vec![field("thread", Value::count(*thread))],
        ),
        Op::Unwait { wait } => (
            format!("Cancel wait {wait}"),
            vec![field("wait", Value::count(*wait))],
        ),
        Op::WaitDue => ("A wait's block has come".to_owned(), Vec::new()),
        Op::PodWait { wait } => (
            format!("Hear wait {wait}'s pod end"),
            vec![field("wait", Value::count(*wait))],
        ),
        Op::PodEvent { wait, event } => (
            format!("Event {} of wait {wait}'s pod stream", event.seq),
            vec![field("data", Value::count(event.data.len() as u64))],
        ),
        Op::Forged(_) => ("Hear a ref or a change forge moved".to_owned(), Vec::new()),
        Op::ClaimConversations { agent } => (
            format!("Claim who starts agent {agent}'s conversations"),
            vec![field("agent", Value::count(*agent))],
        ),
        Op::AdoptOwners { kind, .. } => (
            format!("Record the owners of {kind} items"),
            vec![field("kind", Value::text(kind))],
        ),
        Op::SetServices { id, services } => (
            format!("Set the services that reach model {id}"),
            vec![
                field("model", Value::text(id)),
                field("services", Value::text(services.join(", "))),
            ],
        ),
        Op::ReadBudgets => (
            "Read every model service's status of the model accounts it holds".to_owned(),
            Vec::new(),
        ),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        [
            "Close",
            "Attributed",
            "SetModel",
            "RemoveModel",
            "Streamed",
            "NewThread",
            "ForkThread",
            "OpenThread",
            "Book",
            "Unbook",
            "RunBooked",
            "Resume",
            "Report",
            "Settle",
            "Watch",
            "Heard",
            "SetCompactAt",
            "ReadBudget",
            "Wait",
            "Unwait",
            "WaitDue",
            "PodWait",
            "PodEvent",
            "Forged",
            "ClaimConversations",
            "AdoptOwners",
            "SetServices",
            "ReadBudgets",
        ]
    );
}

#[test]
fn query_variants_only_append() {
    assert_eq!(
        describe::variants::<Query>(),
        [
            "Agent",
            "Thread",
            "Threads",
            "Frames",
            "Held",
            "Calls",
            "Tools",
            "ThreadAt",
            "Model",
            "Models",
            "Withheld",
            "Partial",
            "Topic",
            "Bound",
            "Tally",
            "FramesFrom",
            "Call",
            "Bookings",
            "Booking",
            "UsageWindow",
            "Settling",
            "Budget",
            "Waits",
            "Wait",
            "Armed",
            "Card",
            "Services",
            "Route",
            "Routes",
            "Answerers"
        ]
    );
}

#[test]
fn the_delivery_prefix_decodes_as_attributed() {
    let change = Change {
        seq: 7,
        source: attribution::Source {
            program: "chat".into(),
            object: attribution::Object {
                kind: "message".into(),
                id: "m1".into(),
            },
        },
        revision: 1,
        recipient: 4,
        reason: attribution::Reason::Mention,
        kind: attribution::Kind::Added,
        detail: Vec::new(),
        actor: attribution::Actor::Account(2),
        cause: attribution::Cause::Direct,
        height: 1,
    };
    let delivered = Change::payload(&attributed_as(), &change);
    assert_eq!(
        abi::decode::<Op>(&delivered).unwrap(),
        Op::Attributed(Box::new(change))
    );
}

#[test]
fn the_watch_prefix_decodes_as_heard() {
    let event = Event {
        id: 5,
        seq: 1,
        data: b"event: alive".to_vec(),
    };
    assert_eq!(
        abi::decode::<Op>(&event.payload(&heard_as(3))).unwrap(),
        Op::Heard { watch: 3, event }
    );
}

#[test]
fn the_stream_prefix_decodes_as_streamed() {
    let event = Event {
        id: 3,
        seq: 2,
        data: b"data: {}".to_vec(),
    };
    assert_eq!(
        abi::decode::<Op>(&event.payload(&streamed_as(9))).unwrap(),
        Op::Streamed { thread: 9, event }
    );
}
