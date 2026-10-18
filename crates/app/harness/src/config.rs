//! An agent's configuration file: Markdown whose TOML front matter, between `+++` lines, names
//! the model, `max_tokens`, `wait`, `thinking`, `server_tools`, `listen`, `settle`, `compact_at`;
//! its body is the system prompt. Harness parses it at each model call; harness-view writes it.
use std::fmt;

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use serde::{Deserialize, Serialize};

pub const FENCE: &str = "+++";

// `listen` names the chat channels the agent hears every message of, and `settle` how long, in
// milliseconds, its general input waits for more before a turn, and `compact_at` the tokens of
// context at which its conversations start compacting
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Config {
    pub model: String,
    pub max_tokens: u32,
    pub wait: Option<u64>,
    pub thinking: Option<Thinking>,
    pub server_tools: Vec<ServerTool>,
    pub listen: Vec<String>,
    pub settle: Option<u64>,
    pub compact_at: Option<u64>,
    pub system: String,
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Thinking {
    Off,
    Low,
    Medium,
    High,
    Max,
}

impl Thinking {
    pub const ALL: &'static [Thinking] = &[
        Thinking::Off,
        Thinking::Low,
        Thinking::Medium,
        Thinking::High,
        Thinking::Max,
    ];

    pub fn word(self) -> &'static str {
        match self {
            Thinking::Off => "off",
            Thinking::Low => "low",
            Thinking::Medium => "medium",
            Thinking::High => "high",
            Thinking::Max => "max",
        }
    }
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ServerTool {
    WebSearch,
    WebFetch,
}

impl ServerTool {
    pub const ALL: &'static [ServerTool] = &[ServerTool::WebSearch, ServerTool::WebFetch];

    pub fn word(self) -> &'static str {
        match self {
            ServerTool::WebSearch => "web_search",
            ServerTool::WebFetch => "web_fetch",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Malformed {
    NotText,
    NoOpeningFence,
    NoClosingFence,
    FrontMatter(String),
    NoTokens,
    NoWait,
    NoSettle(String),
    NoCompactAt,
}

impl fmt::Display for Malformed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Malformed::NotText => f.write_str("it is not UTF-8 text"),
            Malformed::NoOpeningFence => {
                write!(
                    f,
                    "it does not open with a {FENCE} line before its front matter"
                )
            }
            Malformed::NoClosingFence => {
                write!(f, "its front matter has no closing {FENCE} line")
            }
            Malformed::FrontMatter(reason) => write!(f, "its front matter is not valid: {reason}"),
            Malformed::NoTokens => f.write_str("max_tokens is at least 1"),
            Malformed::NoWait => f.write_str("wait, when given, is at least 1 block"),
            Malformed::NoSettle(given) => write!(
                f,
                "settle, when given, is a whole number of ms, s or m above zero, like \"10s\", not {given:?}"
            ),
            Malformed::NoCompactAt => f.write_str("compact_at, when given, is at least 1 token"),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FrontMatter {
    model: String,
    max_tokens: u32,
    wait: Option<u64>,
    thinking: Option<Thinking>,
    #[serde(default)]
    server_tools: Vec<ServerTool>,
    #[serde(default)]
    listen: Vec<String>,
    settle: Option<String>,
    compact_at: Option<u64>,
}

fn fence(line: &str) -> bool {
    line.trim_end() == FENCE
}

const UNITS: [(&str, u64); 3] = [("ms", 1), ("s", 1_000), ("m", 60_000)];

fn millis(text: &str) -> Option<u64> {
    let (number, unit) = UNITS
        .iter()
        .find_map(|(suffix, unit)| Some((text.trim().strip_suffix(suffix)?, *unit)))?;
    let millis = number.trim().parse::<u64>().ok()?.checked_mul(unit)?;
    (millis > 0).then_some(millis)
}

fn duration(millis: u64) -> String {
    let (suffix, unit) = UNITS
        .iter()
        .rev()
        .find(|(_, unit)| millis.is_multiple_of(*unit))
        .copied()
        .unwrap_or(UNITS[0]);
    format!("{}{suffix}", millis / unit)
}

impl Config {
    pub fn parse(bytes: &[u8]) -> Result<Config, Malformed> {
        let text = std::str::from_utf8(bytes).map_err(|_| Malformed::NotText)?;
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        let opens = lines.first().is_some_and(|line| fence(line));
        if !opens {
            return Err(Malformed::NoOpeningFence);
        }
        let after = &lines[1..];
        let Some(close) = after.iter().position(|line| fence(line)) else {
            return Err(Malformed::NoClosingFence);
        };
        let front = after[..close].concat();
        let matter: FrontMatter = toml::from_str(&front)
            .map_err(|error| Malformed::FrontMatter(error.message().to_owned()))?;
        if matter.max_tokens == 0 {
            return Err(Malformed::NoTokens);
        }
        if matter.wait == Some(0) {
            return Err(Malformed::NoWait);
        }
        if matter.compact_at == Some(0) {
            return Err(Malformed::NoCompactAt);
        }
        let mut server_tools: Vec<ServerTool> = Vec::new();
        for tool in matter.server_tools {
            if !server_tools.contains(&tool) {
                server_tools.push(tool);
            }
        }
        let settle = match matter.settle {
            None => None,
            Some(given) => Some(millis(&given).ok_or(Malformed::NoSettle(given))?),
        };
        Ok(Config {
            model: matter.model,
            max_tokens: matter.max_tokens,
            wait: matter.wait,
            thinking: matter.thinking,
            server_tools,
            listen: matter.listen,
            settle,
            compact_at: matter.compact_at,
            system: after[close + 1..].concat().trim().to_owned(),
        })
    }

    pub fn paragraphs(&self) -> Vec<String> {
        let mut paragraphs = Vec::new();
        let mut lines: Vec<&str> = Vec::new();
        for line in self.system.lines().chain([""]) {
            let blank = line.trim().is_empty();
            match blank {
                false => lines.push(line),
                true if lines.is_empty() => {}
                true => paragraphs.push(std::mem::take(&mut lines).join("\n")),
            }
        }
        paragraphs
    }

    pub fn write(&self) -> String {
        let model = toml::Value::String(self.model.clone());
        let mut front = format!("model = {model}\nmax_tokens = {}\n", self.max_tokens);
        if let Some(wait) = self.wait {
            front.push_str(&format!("wait = {wait}\n"));
        }
        if let Some(thinking) = self.thinking {
            front.push_str(&format!("thinking = \"{}\"\n", thinking.word()));
        }
        if !self.server_tools.is_empty() {
            let words: Vec<String> = self
                .server_tools
                .iter()
                .map(|tool| format!("\"{}\"", tool.word()))
                .collect();
            front.push_str(&format!("server_tools = [{}]\n", words.join(", ")));
        }
        if !self.listen.is_empty() {
            let channels = self.listen.iter().cloned().map(toml::Value::String);
            let listen = toml::Value::Array(channels.collect());
            front.push_str(&format!("listen = {listen}\n"));
        }
        if let Some(settle) = self.settle {
            front.push_str(&format!("settle = \"{}\"\n", duration(settle)));
        }
        if let Some(compact_at) = self.compact_at {
            front.push_str(&format!("compact_at = {compact_at}\n"));
        }
        format!("{FENCE}\n{front}{FENCE}\n{}\n", self.system)
    }
}

#[cfg(test)]
mod tests;
