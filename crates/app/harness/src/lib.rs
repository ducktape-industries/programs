//! The `harness` program: agents that consensus runs. An agent is a program account whose script
//! calls harness and names its configuration in files (`config.rs`); what it hears reaches harness
//! through `attribution`, its model through the egress role (`wire/`, the account by `routes.rs`).
mod adopt;
mod agents;
mod bookings;
mod budget;
mod calls;
mod cards;
mod codec;
pub mod config;
mod conversations;
mod drafts;
mod examples;
mod failures;
mod fleet_runs;
mod gate;
mod listening;
mod pictures;
mod program;
mod providers;
mod queries;
mod relined;
mod replies;
mod resume;
mod room_rules;
mod routes;
mod runs;
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
mod wakes;
mod watches;
mod wire;
mod woken;

pub use ::program::{BlobId, Principal};
pub use abi::role::egress::Event;
pub use abi::role::identity::{AccountNumber, Standing};
pub use attribution::Change;
pub use config::{Config, Malformed, Pin, ServerTool, Thinking};
pub use program::Harness;
pub use store::{PageRequest, PageResponse};

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

pub use gate::{agent_key, conversation_key, thread_key};
pub use state::{Written, written};

// The owned tables: the keys ownership records a provider's route and an agent's conversations under.
pub mod tables {
    pub use crate::state::{CONVERSATIONS, PROVIDER_ROUTES};
}

pub const PROGRAM: &str = "harness";
pub const PART: &str = "part";

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Described,
)]
#[tool_about(
    "Agents that consensus runs: each agent's conversations (threads) with its model, the routes harness reaches each provider's models through egress by, the runs agents book, and the waits that wake them. An agent is a program account whose script calls harness and names its configuration file in files."
)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum Op {
    #[tool_op(description = "Close a conversation; it stays readable. Its agent's manager, or whom the manager lists; a conversation bound to a place (OpenThread) anyone may close. Closing one already closed is refused as wrong_state, whoever asks.", example = examples::close)]
    Close { thread: u64 },
    #[tool_op(
        description = "From attribution only (and harness itself for one it held back): a change about an agent, such as a mention, a direct message or a reply in its thread, which reaches its conversation when the agent's list allows the author."
    )]
    Attributed(Box<Change>),
    #[tool_op(
        description = "From the program bound to the egress role only: one event of a model's streamed reply for a conversation waiting on it."
    )]
    Streamed { thread: u64, event: Event } = 4,
    #[tool_op(description = "Start a fresh conversation for an agent, which becomes its current one. Its manager, or whom its conversation list allows. Returns the conversation's id.", returns = u64, example = examples::new_thread)]
    NewThread { agent: AccountNumber },
    #[tool_op(description = "Start a conversation that begins from the agent's current one (its frames, summary included), which becomes current. Its manager, or whom its conversation list allows. Returns the conversation's id.", returns = u64, example = examples::fork_thread)]
    ForkThread { agent: AccountNumber },
    #[tool_op(description = "Open a conversation for an agent bound to a place (a chat channel or a thread): what's heard there goes to it. Any account; refused while the agent has one open there. Returns the conversation's id.", returns = u64, example = examples::open_thread)]
    OpenThread { agent: AccountNumber, place: Place },
    #[tool_op(description = "As an agent harness runs: run an op of target as yourself at a later height, waking thread with what it did. Returns the booking's entry.", example = examples::book)]
    Book {
        height: u64,
        target: String,
        payload: Vec<u8>,
        thread: Option<u64>,
    },
    #[tool_op(description = "Cancel a booked run before it runs. The agent that booked it.", example = examples::unbook)]
    Unbook { entry: u64 },
    #[tool_op(description = "Harness's own scheduled entry only: runs a booked op at its block.")]
    RunBooked,
    #[tool_op(description = "Kick a conversation that stopped: it asks its model again where a lost or failed call left it idle. Anyone may.", example = examples::resume)]
    Resume { thread: u64 },
    #[tool_op(
        description = "Harness's own: posts the lines it kept for a manager about its agents' conversations."
    )]
    Report { manager: AccountNumber },
    #[tool_op(description = "Harness's own scheduled entry: ends a conversation's settle window.")]
    Settle { thread: u64 },
    #[tool_op(
        description = "Harness's own scheduled run: opens the egress stream a streamed tool call waits on."
    )]
    Watch { watch: u64 },
    #[tool_op(
        description = "From the program bound to the egress role only: one event of a watched tool call's stream."
    )]
    Heard { watch: u64, event: Event },
    #[tool_op(description = "As an agent harness runs: wake thread once when the event happens (a height, a chat message, a forge change or ref, a files path, a chat place clearing); each but a chat message is a wake harness arms as the agent. Returns the wait's id.", returns = u64)]
    Wait { thread: u64, event: WaitFor } = 18,
    #[tool_op(description = "Cancel an open wait. The agent that made it.", example = examples::unwait)]
    Unwait { wait: u64 },
    #[tool_op(description = "Claim who may start an agent's conversations: the ownership role records the row as yours, and its policy decides each later NewThread and ForkThread. The agent's manager only.", example = examples::claim)]
    ClaimConversations { agent: AccountNumber } = 24,
    // one-time (J-011 ⑥): records owners for harness's owned rows from before the guard
    #[tool_op(description = "One-time backfill of owners for harness's rows from before ownership was recorded, a page at a time; returns the key to resume after, or null when done.", returns = Option<Vec<u8>>, example = examples::adopt)]
    AdoptOwners {
        kind: String,
        after: Option<Vec<u8>>,
        limit: u64,
    },
    #[tool_op(description = "Set every egress service, in order and each with the wire it speaks, that reaches a provider's models: an agent whose configuration names the provider (provider = \"anthropic\") is reached through them. The first account to set a provider's route owns it; after that its owner, or whom the owner lists.", example = examples::set_route)]
    SetRoute {
        provider: String,
        services: Vec<RouteService>,
    } = 29,
    #[tool_op(description = "Remove a provider's route. Its owner, or whom the owner lists.", example = examples::remove_route)]
    RemoveRoute { provider: String },
    #[tool_op(description = "One-time backfill of tallies: walks at most limit conversations after the cursor and tallies the first one that has none, added up from its frames, so a run reads one transcript at most, as Tally and ForkThread do. While a conversation may be left, it books itself again for the next block after that one, so one call tallies every conversation, one a block. Anyone may; a conversation already tallied is left as it is, so chains started together never tally one twice. Returns the key it resumes after, or null when done.", returns = Option<Vec<u8>>, example = examples::adopt_tallies)]
    AdoptTallies { after: Option<Vec<u8>>, limit: u64 },
    #[tool_op(description = "One-time: put every part an open conversation sealed before harness held sealed parts on lines on its thread's line, and let a closed idle conversation go of its sealed parts, one thread a block from harness's own scheduled run until none is left. Anyone may start it; a start while it runs is refused.", example = examples::hold_sealed)]
    HoldSealed = 33,
    #[tool_op(
        description = "From fleet only: a run an agent asked for with the fleet_run tool ended or was lost; the conversation that asked is told, when the run's asker is that conversation's agent."
    )]
    RunEnded { thread: u64, ended: fleet::RunEnd },
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
    Harness,
}

impl Wire {
    pub const ALL: &'static [Wire] = &[Wire::AnthropicMessages, Wire::Harness];

    pub fn label(self) -> &'static str {
        match self {
            Wire::AnthropicMessages => "Anthropic Messages",
            Wire::Harness => "Harness",
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
pub struct RouteService {
    pub service: String,
    pub wire: Wire,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct ProviderRouteRow {
    pub provider: String,
    pub owner: AccountNumber,
    pub services: Vec<RouteService>,
}

// how an agent's model is reached, as its configuration and its provider's route say
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reached<'a> {
    pub name: &'a str,
    pub provider: &'a str,
    pub route: &'a [RouteService],
    pub service: &'a str,
    pub window: Window,
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

// An agent as a list of agents draws it (`Query::Cards`): its setup with the system prompt left
// out, its conversations counted, when one last moved, its newest and each open one asking, and
// how many member nodes serve its model's service (`None` while its configuration names no model).
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct AgentCard {
    pub setup: AgentSetup,
    pub open: u64,
    pub closed: u64,
    pub last_time: u64,
    pub newest: Option<Spoken>,
    pub asking: Vec<Spoken>,
    pub serving: Option<u64>,
}

// A conversation a card names, with the line its agent last set with `thread_topic`.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Spoken {
    pub thread: ThreadRow,
    pub topic: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum Configured {
    Unreadable {
        reason: String,
    },
    Malformed {
        snapshot: BlobId,
        malformed: Malformed,
    },
    Routed {
        snapshot: BlobId,
        config: Config,
        provider: String,
        route: Vec<RouteService>,
        window: Window,
    } = 4,
    NoRoute {
        snapshot: BlobId,
        config: Config,
        provider: String,
    },
    Unprovided {
        snapshot: BlobId,
        config: Config,
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
            Configured::NoRoute { provider, .. } => Some(format!(
                "the agent's configuration {path} names provider {provider}, which harness has no route for"
            )),
            Configured::Unprovided { .. } => Some(format!(
                "the agent's configuration {path} names no provider: add a line like provider = \"anthropic\", whose route reaches its model"
            )),
            Configured::Routed { .. } => None,
        }
    }

    pub fn config(&self) -> Option<(BlobId, &Config)> {
        match self {
            Configured::Routed {
                snapshot, config, ..
            }
            | Configured::NoRoute {
                snapshot, config, ..
            }
            | Configured::Unprovided { snapshot, config } => Some((*snapshot, config)),
            Configured::Unreadable { .. } | Configured::Malformed { .. } => None,
        }
    }

    // the tokens of context at which its conversations start compacting
    pub fn compact_at(&self) -> Option<u64> {
        self.config()?.1.compact_at
    }

    pub fn reached(&self) -> Option<Reached<'_>> {
        let Configured::Routed {
            config,
            provider,
            route,
            window,
            ..
        } = self
        else {
            return None;
        };
        Some(Reached {
            name: &config.model,
            provider,
            route,
            service: &route.first()?.service,
            window: *window,
        })
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

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Conversation {
    pub row: ThreadRow,
    pub topic: Option<String>,
    pub tally: Option<Tally>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct FinalError {
    pub thread: u64,
    pub text: String,
    pub frame: u64,
    pub height: u64,
    pub time: u64,
    pub since: u64,
    pub count: u64,
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
        unseen_input || self.paused()
    }

    // the model paused its turn (`pause_turn`, mid server tool): the next request goes on from
    // the turn's last block, with nothing after it
    pub fn paused(&self) -> bool {
        self.last_stop == Some(Stop::Pause)
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

// where a tool an agent gets comes from: harness's own code, or a tool file read at a snapshot
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum ToolSource {
    Native,
    File { path: String, snapshot: BlobId },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct AgentTool {
    pub name: String,
    pub source: ToolSource,
}

// every tool an agent gets, the shared tool files one of its own takes the place of, and why
// each tool file left out does not load
#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct AgentTools {
    pub tools: Vec<AgentTool>,
    pub shadowed: Vec<String>,
    pub problems: Vec<String>,
}

// a whole usage window, in the millionths `WindowOutlook::used` counts
pub const WHOLE_WINDOW: u64 = 1_000_000;

#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct BudgetReport {
    pub time: u64,
    pub turns: u64,
    pub services: Vec<ServicePace>,
    pub accounts: Vec<AccountBudget>,
    pub reads: Vec<StatusRead>,
}

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

#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct BudgetFactsReport {
    pub budget: BudgetReport,
    pub facts: Vec<BudgetFact>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct BudgetFact {
    pub service: String,
    pub account: String,
    pub provider: Option<String>,
    pub identity: Option<String>,
    pub previous: Option<String>,
    pub changed: Option<u64>,
}

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
    File {
        path: String,
    },
    Settled {
        repo: String,
        n: u64,
        until: Until,
    },
    Clear {
        channel: String,
        thread: Option<u64>,
    },
}

// how a change must end for a `Settled` wait to be over: merged, closed, or either
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Until {
    Merged,
    Closed,
    Either,
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
pub struct RunRow {
    pub channel: String,
    pub author: AccountNumber,
    pub mentioned: u64,
    pub last: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Ended {
    pub height: u64,
    pub text: String,
}

// what will wake a conversation again, each as harness holds it: an input no turn has seen yet
// (or a paused turn), a turn in progress, tool calls still open, a model call in flight, a retry
// asked again from block `at`, a settle window until time `until` (ms), an armed wait, a streamed
// tool call's watch, or a run booked from it that has not run; none means nothing will
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Wake {
    Unseen,
    Turn,
    Calls { open: u32 },
    Flight,
    Retry { at: u64 },
    Settle { until: u64 },
    Wait { wait: u64 },
    Stream { watch: u64 },
    Booking { entry: u64 },
    RetryAfter { at: u64, until: u64 },
    Run { run: u64 },
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    ::program::Ask,
    ::program::Described,
)]
#[ask(Harness)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum Query {
    #[ask(Reply::Agent(Option<AgentSetup>))]
    #[tool_query(description = "What harness resolves for an agent: its manager, standing, config path, the config and its model or what is wrong with them, and its current conversation.", example = examples::agent)]
    Agent { agent: AccountNumber },
    #[ask(Reply::Thread(Option<ThreadRow>))]
    #[reads(state::THREADS)]
    #[tool_query(description = "One conversation's row: its agent, manager, place, whether it's asking or idle, and its frame count.", example = examples::thread)]
    Thread { thread: u64 },
    #[ask(Reply::Threads(PageResponse<ThreadRow>))]
    #[reads(state::BY_AGENT, state::THREADS)]
    #[tool_query(description = "An agent's conversations, newest first.", example = examples::threads)]
    Threads {
        agent: AccountNumber,
        page: PageRequest,
    },
    #[ask(Reply::Frames(PageResponse<Frame>))]
    #[reads(state::FRAMES)]
    #[tool_query(description = "A conversation's frames in order: posts heard, turns with what the model said and its tool calls, results, notices.", example = examples::frames)]
    Frames { thread: u64, page: PageRequest },
    #[ask(Reply::Held(Vec<Body>))]
    #[reads(state::HELD)]
    #[tool_query(description = "The inputs a conversation holds behind its running loop.", example = examples::held)]
    Held { thread: u64 },
    #[ask(Reply::Calls(PageResponse<CallRow>))]
    #[reads(state::CALLS)]
    #[tool_query(description = "A conversation's tool calls with their state.", example = examples::calls)]
    Calls { thread: u64, page: PageRequest },
    #[ask(Reply::Tools(Vec<ToolInfo>))]
    #[tool_query(description = "Every tool harness offers its models: name, description, input schema and whether it writes.", example = examples::tools)]
    Tools,
    #[ask(Reply::Thread(Option<ThreadRow>))]
    #[reads(state::PLACES, state::THREADS)]
    #[tool_query(description = "The conversation an agent's input from a place (chat/<channel> or chat/<channel>/<root>) last reached.", example = examples::thread_at)]
    ThreadAt { agent: AccountNumber, place: String },
    #[ask(Reply::Withheld(Vec<WithheldFrame>))]
    #[reads(state::WITHHELD, state::FRAMES)]
    #[tool_query(description = "The inputs left out of a conversation's model view because the request was too long, by frame.", example = examples::withheld)]
    Withheld { thread: u64 } = 10,
    #[ask(Reply::Partial(Option<Partial>))]
    #[reads(state::PARTIALS, state::PIECES)]
    #[tool_query(description = "The reply a model is streaming into a conversation now, or null.", example = examples::partial)]
    Partial { thread: u64 },
    /// the line the thread's agent last set with `thread_topic`, if it set one
    #[ask(Reply::Topic(Option<String>))]
    #[reads(state::TOPICS)]
    #[tool_query(description = "The line a conversation's agent last set with thread_topic, or null.", example = examples::topic)]
    Topic { thread: u64 },
    #[ask(Reply::Threads(PageResponse<ThreadRow>))]
    #[reads(state::BOUND, state::THREADS)]
    #[tool_query(description = "An agent's conversations bound to a place.", example = examples::bound)]
    Bound {
        agent: AccountNumber,
        page: PageRequest,
    },
    #[ask(Reply::Tally(Tally))]
    #[reads(state::TALLIES, state::FRAMES)]
    #[tool_query(description = "A conversation's token and cost tally so far.", example = examples::tally)]
    Tally { thread: u64 },
    #[ask(Reply::Frames(PageResponse<Frame>))]
    #[reads(state::FRAMES)]
    #[tool_query(description = "A conversation's frames from seq from on.", example = examples::frames_from)]
    FramesFrom {
        thread: u64,
        from: u64,
        page: PageRequest,
    },
    #[ask(Reply::Call(Option<CallRow>))]
    #[reads(state::CALLS)]
    #[tool_query(description = "One tool call of a conversation, by its id, or null.", example = examples::call)]
    Call { thread: u64, call: String },
    #[ask(Reply::Bookings(PageResponse<Booking>))]
    #[reads(state::BOOKED, state::BOOKINGS)]
    #[tool_query(description = "The runs an agent booked.", example = examples::bookings)]
    Bookings {
        agent: AccountNumber,
        page: PageRequest,
    },
    #[ask(Reply::Booking(Option<Booking>))]
    #[reads(state::BOOKINGS)]
    #[tool_query(description = "One booked run by entry, and what it did once it ran, or null.", example = examples::booking)]
    Booking { entry: u64 },
    #[ask(Reply::Settling(Option<SettleWindow>))]
    #[reads(state::SETTLING)]
    #[tool_query(description = "A conversation's settle window, or null.", example = examples::settling)]
    Settling { thread: u64 } = 20,
    #[ask(Reply::Waits(PageResponse<WaitRow>))]
    #[reads(state::WAITING, state::WAITS)]
    #[tool_query(description = "An agent's waits, open and ended.", example = examples::waits)]
    Waits {
        agent: AccountNumber,
        page: PageRequest,
    } = 22,
    #[ask(Reply::Wait(Option<WaitRow>))]
    #[reads(state::WAITS)]
    #[tool_query(description = "One wait by id and how it ended, or null.", example = examples::wait)]
    Wait { wait: u64 },
    #[ask(Reply::Waits(PageResponse<WaitRow>))]
    #[reads(state::ARMED, state::WAITS)]
    #[tool_query(description = "The waits armed on one conversation.", example = examples::armed)]
    Armed { thread: u64, page: PageRequest },
    // `Agent` with the system prompt left out (an empty `system`): what a
    // list of agents, read again on every block, needs of each
    #[ask(Reply::Agent(Option<AgentSetup>))]
    #[tool_query(description = "Agent, with the system prompt left out: what a list of agents needs of each.", example = examples::card)]
    Card { agent: AccountNumber },
    #[ask(Reply::Route(Option<Route>))]
    #[reads(state::ROUTES)]
    #[tool_query(description = "The service and account a conversation's model calls go to, or null.", example = examples::route)]
    Route { thread: u64 } = 27,
    #[ask(Reply::Routes(PageResponse<Routed>))]
    #[reads(state::ROUTES)]
    #[tool_query(description = "Every conversation's route.", example = examples::routes)]
    Routes { page: PageRequest },
    #[ask(Reply::Answerers(PageResponse<Answerer>))]
    #[reads(state::ANSWERED)]
    #[tool_query(description = "Which service and account answered each turn and summary of a conversation, newest first, and whether it moved there.", example = examples::answerers)]
    Answerers { thread: u64, page: PageRequest },
    // what will wake a conversation again; none when nothing will
    #[ask(Reply::Wakes(Vec<Wake>))]
    #[reads(
        state::THREADS,
        state::FLIGHTS,
        state::RETRIES,
        state::SETTLING,
        state::ARMED,
        state::STREAMING,
        state::BOOKED,
        state::BOOKINGS
    )]
    #[tool_query(description = "What will wake a conversation again (an unseen input, a turn, open calls, a call in flight, a retry, a settle window, a wait, a stream, a booking); none means nothing will.", example = examples::wakes)]
    Wakes { thread: u64 },
    #[ask(Reply::AgentTools(AgentTools))]
    #[tool_query(description = "Every tool an agent gets on its next model call, each with where it comes from (harness's own, or a tool file's path and the snapshot it was read at), the shared tool files one of its own takes the place of, and why each tool file left out does not load.", example = examples::agent_tools)]
    AgentTools { agent: AccountNumber },
    // every agent harness runs, as a list of agents draws each, in one read
    #[ask(Reply::Cards(PageResponse<AgentCard>))]
    #[tool_query(description = "Every agent harness runs, as a list of agents draws each: its setup without the system prompt, how many conversations it has open and closed, when one last moved, its newest and each open one asking with their topics, and how many member nodes serve its model's service.", example = examples::cards)]
    Cards { page: PageRequest },
    #[ask(Reply::ProviderRoute(Option<ProviderRouteRow>))]
    #[reads(state::PROVIDER_ROUTES)]
    #[tool_query(description = "A provider's route: the egress services, in order and each with its wire, that reach its models, and who owns it; or null.", example = examples::provider_route)]
    ProviderRoute { provider: String },
    #[ask(Reply::ProviderRoutes(PageResponse<ProviderRouteRow>))]
    #[reads(state::PROVIDER_ROUTES)]
    #[tool_query(description = "Every provider's route.", example = examples::provider_routes)]
    ProviderRoutes { page: PageRequest },
    #[ask(Reply::FinalErrors(PageResponse<FinalError>))]
    #[reads(state::FINAL_ERRORS)]
    #[tool_query(description = "An agent's conversations whose inputs end in final errors, each with its newest error, that error's notice frame, the block the run of them began, and how many came in a row; a conversation leaves the list at its next answered turn or once it is closed.", example = examples::final_errors)]
    FinalErrors {
        agent: AccountNumber,
        page: PageRequest,
    },
    #[ask(Reply::Runs(Vec<RunRow>))]
    #[reads(state::THREADS, state::RUNS)]
    #[tool_query(description = "The posts a conversation's agent goes on hearing after a mention, one run per chat room: whose posts, the message that mentioned the agent, and the last post heard since. A post by anyone else ends a run.", example = examples::runs)]
    Runs { thread: u64 },
    #[ask(Reply::Conversations(PageResponse<Conversation>))]
    #[reads(state::BY_AGENT, state::THREADS, state::TOPICS, state::TALLIES)]
    #[tool_query(description = "An agent's conversations, newest first, each with its topic and its token and cost tally as harness keeps it, in one read; a conversation not tallied yet answers null, and AdoptTallies tallies those.", example = examples::conversations)]
    Conversations {
        agent: AccountNumber,
        page: PageRequest,
    },
    #[ask(Reply::BudgetFacts(BudgetFactsReport))]
    #[reads(state::BUDGETS, state::PACES, state::FLIGHTS, state::BUDGET_FACTS)]
    #[tool_query(description = "Every model service account's usage windows, calls per minute and when it runs out, with what harness learned of each: the provider its calls were for, the login its gate names behind it, and the login before that with when it changed.", example = examples::budget_facts)]
    BudgetFacts,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum Reply {
    Agent(Option<AgentSetup>),
    Thread(Option<ThreadRow>),
    Threads(PageResponse<ThreadRow>),
    Frames(PageResponse<Frame>),
    Held(Vec<Body>),
    Calls(PageResponse<CallRow>),
    Tools(Vec<ToolInfo>),
    Withheld(Vec<WithheldFrame>) = 9,
    Partial(Option<Partial>),
    Topic(Option<String>),
    Tally(Tally),
    Call(Option<CallRow>),
    Bookings(PageResponse<Booking>),
    Booking(Option<Booking>),
    Settling(Option<SettleWindow>) = 17,
    Waits(PageResponse<WaitRow>) = 19,
    Wait(Option<WaitRow>),
    Route(Option<Route>) = 22,
    Routes(PageResponse<Routed>),
    Answerers(PageResponse<Answerer>),
    Wakes(Vec<Wake>),
    AgentTools(AgentTools),
    Cards(PageResponse<AgentCard>),
    ProviderRoute(Option<ProviderRouteRow>),
    ProviderRoutes(PageResponse<ProviderRouteRow>),
    FinalErrors(PageResponse<FinalError>),
    Runs(Vec<RunRow>),
    Conversations(PageResponse<Conversation>),
    BudgetFacts(BudgetFactsReport),
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

pub fn run_ended_as(thread: u64) -> Vec<u8> {
    let ended = fleet::RunEnd {
        run: 0,
        attempt: 0,
        state: fleet::State::Queued,
    };
    let tail = abi::encode(&ended).len();
    let mut deliver = abi::encode(&Op::RunEnded { thread, ended });
    deliver.truncate(deliver.len() - tail);
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
        (Request::Http { head, .. }, Delivery::Whole) => wire::awaited(head),
    }
}

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let (title, fields) = match op {
        Op::Close { thread } => (format!("Close thread {thread}"), Vec::new()),
        Op::Attributed(change) => (
            format!("Account {} is related to an object", change.recipient),
            vec![field("change", Value::count(change.seq))],
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
        Op::ClaimConversations { agent } => (
            format!("Claim who starts agent {agent}'s conversations"),
            vec![field("agent", Value::count(*agent))],
        ),
        Op::AdoptOwners { kind, .. } => (
            format!("Record the owners of {kind} items"),
            vec![field("kind", Value::text(kind))],
        ),
        Op::SetRoute { provider, services } => {
            let listed: Vec<String> = services
                .iter()
                .map(|entry| format!("{} ({})", entry.service, entry.wire.label()))
                .collect();
            (
                format!("Set the services that reach provider {provider}"),
                vec![
                    field("provider", Value::text(provider)),
                    field("services", Value::text(listed.join(", "))),
                ],
            )
        }
        Op::RemoveRoute { provider } => (
            format!("Remove provider {provider}'s route"),
            vec![field("provider", Value::text(provider))],
        ),
        Op::AdoptTallies { limit, .. } => (
            "Tally the conversations that have no tally yet, one a block".to_owned(),
            vec![field("limit", Value::count(*limit))],
        ),
        Op::HoldSealed => (
            "Hold sealed parts on their threads' lines".to_owned(),
            Vec::new(),
        ),
        Op::RunEnded { thread, ended } => (
            format!(
                "Tell thread {thread} that run {} is {}",
                ended.run,
                ended.state.word()
            ),
            vec![
                field("thread", Value::count(*thread)),
                field("run", Value::count(ended.run)),
            ],
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
            "-",
            "-",
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
            "-",
            "-",
            "Wait",
            "Unwait",
            "-",
            "-",
            "-",
            "-",
            "ClaimConversations",
            "AdoptOwners",
            "-",
            "-",
            "-",
            "SetRoute",
            "RemoveRoute",
            "AdoptTallies",
            "-",
            "HoldSealed",
            "RunEnded",
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
            "-",
            "-",
            "Withheld",
            "Partial",
            "Topic",
            "Bound",
            "Tally",
            "FramesFrom",
            "Call",
            "Bookings",
            "Booking",
            "-",
            "Settling",
            "-",
            "Waits",
            "Wait",
            "Armed",
            "Card",
            "-",
            "Route",
            "Routes",
            "Answerers",
            "Wakes",
            "AgentTools",
            "Cards",
            "ProviderRoute",
            "ProviderRoutes",
            "FinalErrors",
            "Runs",
            "Conversations",
            "BudgetFacts"
        ]
    );
}

#[test]
fn wait_for_variants_only_append() {
    assert_eq!(
        describe::variants::<WaitFor>(),
        [
            "Height", "Pod", "Message", "Change", "Ref", "File", "Settled", "Clear"
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

#[test]
fn the_run_end_prefix_decodes_as_run_ended() {
    let ended = fleet::RunEnd {
        run: 7,
        attempt: 2,
        state: fleet::State::Passed,
    };
    let mut told = run_ended_as(9);
    told.extend(abi::encode(&ended));
    assert_eq!(
        abi::decode::<Op>(&told).unwrap(),
        Op::RunEnded { thread: 9, ended }
    );
}
