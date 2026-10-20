//! An agent's configuration file: Markdown whose TOML front matter, between `+++` lines, names
//! the model, `max_tokens`, `wait`, `thinking`, `server_tools`, `listen`, `settle`, `compact_at`
//! and the `service` and `provider` it pins (`Pin`); its body is the system prompt (`with_front`, `with_body`).
use std::fmt;

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use serde::{Deserialize, Serialize};

pub const FENCE: &str = "+++";

// `listen` names the chat channels harness starts the agent hearing every message of, once each
// (chat's per-room choice stands after that), an entry ending in `*` every open room whose id
// starts with what comes before it, and `settle` how long, in
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

// the service an agent's front matter pins its model calls to (J-090); harness reads it beside
// `Config`, which keeps its shape
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pin {
    pub service: Option<String>,
    pub provider: Option<String>,
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
    service: Option<String>,
    provider: Option<String>,
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
        Config::parse_pinned(bytes).map(|(config, _)| config)
    }

    pub fn parse_pinned(bytes: &[u8]) -> Result<(Config, Pin), Malformed> {
        let text = std::str::from_utf8(bytes).map_err(|_| Malformed::NotText)?;
        let [_, front, _, body] = front_matter_parts(text)?;
        let matter: FrontMatter = toml::from_str(front)
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
        let named = |name: Option<String>| name.filter(|name| !name.is_empty());
        let pin = Pin {
            service: named(matter.service),
            provider: named(matter.provider),
        };
        let config = Config {
            model: matter.model,
            max_tokens: matter.max_tokens,
            wait: matter.wait,
            thinking: matter.thinking,
            server_tools,
            listen: matter.listen,
            settle,
            compact_at: matter.compact_at,
            system: body.trim().to_owned(),
        };
        Ok((config, pin))
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
        let front = self.front(&Pin::default());
        format!("{FENCE}\n{front}{FENCE}\n{}\n", self.system)
    }

    fn front(&self, pin: &Pin) -> String {
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
        if let Some(service) = &pin.service {
            let service = toml::Value::String(service.clone());
            front.push_str(&format!("service = {service}\n"));
        }
        if let Some(provider) = &pin.provider {
            let provider = toml::Value::String(provider.clone());
            front.push_str(&format!("provider = {provider}\n"));
        }
        front
    }
}

fn front_matter_parts(text: &str) -> Result<[&str; 4], Malformed> {
    let Some(open) = text.split_inclusive('\n').next().filter(|line| fence(line)) else {
        return Err(Malformed::NoOpeningFence);
    };
    let mut at = open.len();
    for line in text[at..].split_inclusive('\n') {
        if fence(line) {
            let body = at + line.len();
            return Ok([open, &text[open.len()..at], line, &text[body..]]);
        }
        at += line.len();
    }
    Err(Malformed::NoClosingFence)
}

// `file` with the front matter `config` names and the service the file pins, its
// body kept byte for byte; `config.system` is not read
pub fn with_front(file: &[u8], config: &Config) -> Result<String, Malformed> {
    let (_, pin) = Config::parse_pinned(file)?;
    let text = std::str::from_utf8(file).map_err(|_| Malformed::NotText)?;
    let [open, _, close, body] = front_matter_parts(text)?;
    Ok(format!("{open}{}{close}{body}", config.front(&pin)))
}

// `file` with `system` as its body, its front matter kept byte for byte
pub fn with_body(file: &[u8], system: &str) -> Result<String, Malformed> {
    let text = std::str::from_utf8(file).map_err(|_| Malformed::NotText)?;
    let [open, front, close, _] = front_matter_parts(text)?;
    let close = close.trim_end();
    Ok(format!("{open}{front}{close}\n{system}\n"))
}

#[cfg(test)]
mod tests;
