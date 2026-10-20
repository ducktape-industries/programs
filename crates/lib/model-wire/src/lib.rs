//! The harness wire: the one vocabulary harness speaks to a model, and an adapter per provider
//! that translates it to that provider's own wire and back. A provider's field names appear only
//! in its adapter's module.
pub mod anthropic;
pub mod codex;

use serde::{Deserialize, Serialize};
use serde_json::Value;

// where the service that runs the adapters answers each ask
pub const TURN: &str = "/turn";
pub const SUMMARY: &str = "/summary";
pub const MODELS: &str = "/models/";

pub fn model_path(name: &str) -> String {
    format!("{MODELS}{name}")
}

pub fn ccs_provider(word: &str) -> Option<&'static str> {
    let known = [
        (anthropic::CCS_PROVIDER, anthropic::PROVIDER),
        (codex::CCS_PROVIDER, codex::PROVIDER),
    ];
    let found = known.into_iter().find(|(ccs, _)| *ccs == word);
    found.map(|(_, provider)| provider)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub model: String,
    pub max_tokens: u32,
    #[serde(default)]
    pub thinking: Option<Thinking>,
    #[serde(default)]
    pub system: Vec<String>,
    #[serde(default)]
    pub tools: Vec<Tool>,
    #[serde(default)]
    pub server_tools: Vec<ServerTool>,
    pub messages: Vec<Message>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Thinking {
    Off,
    Low,
    Medium,
    High,
    Max,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerTool {
    WebSearch,
    WebFetch,
}

impl ServerTool {
    pub fn word(self) -> &'static str {
        match self {
            ServerTool::WebSearch => "web_search",
            ServerTool::WebFetch => "web_fetch",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

// the provider and the model that wrote an assistant message: what it sealed goes back only to
// the same provider, and its reasoning's seal only to the same model
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lane {
    pub provider: String,
    pub model: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Message {
    User { blocks: Vec<UserBlock> },
    Assistant { lane: Lane, blocks: Vec<Block> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UserBlock {
    Text {
        text: String,
    },
    Image {
        media: String,
        data: String,
    },
    Result {
        call: String,
        failed: bool,
        content: Vec<Shown>,
    },
}

// what a tool result shows: its text, and the images it carries
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Shown {
    Text { text: String },
    Image { media: String, data: String },
}

// an assistant's block; `sealed` and `citations` hold what the provider issued and takes back
// verbatim, which nothing on harness's side reads
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Block {
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        citations: Vec<String>,
    },
    Reasoning {
        text: String,
        #[serde(default)]
        sealed: Option<String>,
    },
    Redacted {
        sealed: String,
    },
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
    ServerToolResult {
        sealed: String,
    },
    Summary {
        text: String,
        #[serde(default)]
        sealed: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stop {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
    Pause,
    Refusal,
}

// tokens as harness counts them: `input` is the prompt's uncached share, `output` includes any
// reasoning
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

// a model's whole answer, the authority on the turn: `model` as the provider named it, `prompt`
// the prompt's own size where the provider reports it before answering
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reply {
    pub model: String,
    pub lane: Lane,
    pub said: Vec<Block>,
    pub stop: Stop,
    pub usage: Usage,
    #[serde(default)]
    pub prompt: Option<u64>,
    #[serde(default)]
    pub context: Option<u64>,
}

// one server-sent event of a turn: a block opens at its place in the reply, grows by text (its
// text, its reasoning, or its input's JSON), and ends; the stream ends with a reply or a failure
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    Block { index: u64, block: Block },
    Delta { index: u64, text: String },
    End { index: u64 },
    Reply(Reply),
    Failed(Failure),
}

impl Event {
    pub fn ends(&self) -> bool {
        matches!(self, Event::Reply(_) | Event::Failed(_))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    pub class: Class,
    pub message: String,
    #[serde(default)]
    pub reset: Option<u64>,
}

impl Failure {
    pub fn new(class: Class, message: impl Into<String>) -> Failure {
        Failure {
            class,
            message: message.into(),
            reset: None,
        }
    }
}

// why a call failed: the stream broke or ended early (`dropped`), the provider refused or failed
// it (`provider`), it was busy (`overloaded`), its answer could not be read (`wire`), the prompt
// does not fit the model's window (`overflow`), the model refused its images (`pictures`), or the
// account is out of its limit until `reset`, in seconds since the epoch (`limited`)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    Dropped,
    Provider,
    Overloaded,
    Wire,
    Overflow,
    Pictures,
    Limited,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    pub provider: String,
    pub window: Option<u64>,
}

// a thread summarized: what a model of any provider reads, what its own provider takes back, and
// the tokens the summary took
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summarized {
    pub text: String,
    #[serde(default)]
    pub sealed: Option<String>,
    pub lane: Lane,
    pub tokens: u64,
}

pub fn event_data(event: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(event);
    let data: Vec<&str> = text
        .split(['\n', '\r'])
        .filter_map(|line| line.strip_prefix("data:"))
        .map(|value| value.strip_prefix(' ').unwrap_or(value))
        .collect();
    let has_data = !data.is_empty();
    has_data.then(|| data.join("\n"))
}

pub fn sse(event: &Event) -> String {
    let data = serde_json::to_string(event).expect("an event is JSON");
    format!("data: {data}\n\n")
}
