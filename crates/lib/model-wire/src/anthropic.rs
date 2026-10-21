//! Anthropic's Messages API as the harness wire: a request's body, its server-sent events folded
//! into harness events, an answer that is no reply read as a failure, a thread's summary, and a
//! model's window.
use serde_json::{Map, Value, json};

use crate::budget::{Budget, Window, header};
use crate::{
    Block, Class, Event, Failure, Lane, Message, Model, Pictures, Reply, Request, ServerTool,
    Shown, Stop, Summarized, Thinking, Usage, UserBlock,
};

pub const PROVIDER: &str = "anthropic";
pub const MESSAGES: &str = "/v1/messages";
pub const VERSION: &str = "2023-06-01";
pub const COMPACTION_BETA: &str = "compact-2026-09-04";
const WEB_SEARCH: &str = "web_search_20250305";
const WEB_FETCH: &str = "web_fetch_20250910";
const OVERFLOWED: &str = "model_context_window_exceeded";
const RESET: &str = "anthropic-ratelimit-unified-5h-reset";
const RETRY_AFTER: &str = "retry-after";
const UNIFIED: &str = "anthropic-ratelimit-unified-";
const ORGANIZATION: &str = "anthropic-organization-id";
const IMAGE_BASE64: u64 = 5 * 1024 * 1024;
const IMAGE_SIDE: u32 = 8000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    Turn,
    Summary,
}

pub fn model_path(name: &str) -> String {
    format!("/v1/models/{name}")
}

pub fn headers(request: &Request, ask: Ask) -> Vec<(String, String)> {
    let mut headers = vec![
        ("anthropic-version".to_owned(), VERSION.to_owned()),
        ("content-type".to_owned(), "application/json".to_owned()),
    ];
    if ask == Ask::Summary || carries_compaction(request) {
        headers.push(("anthropic-beta".to_owned(), COMPACTION_BETA.to_owned()));
    }
    headers
}

pub fn model_headers() -> Vec<(String, String)> {
    vec![("anthropic-version".to_owned(), VERSION.to_owned())]
}

fn carries_compaction(request: &Request) -> bool {
    request.messages.iter().any(|message| match message {
        Message::Assistant { lane, blocks } => {
            lane.provider == PROVIDER
                && blocks.iter().any(|block| {
                    matches!(
                        block,
                        Block::Summary {
                            sealed: Some(_),
                            ..
                        }
                    )
                })
        }
        Message::User { .. } => false,
    })
}

fn cached() -> Value {
    json!({ "type": "ephemeral" })
}

fn mark(block: Option<&mut Value>) {
    if let Some(Value::Object(block)) = block {
        block.insert("cache_control".into(), cached());
    }
}

fn server_definition(tool: &ServerTool) -> Value {
    let kind = match tool {
        ServerTool::WebSearch => WEB_SEARCH,
        ServerTool::WebFetch => WEB_FETCH,
    };
    json!({ "type": kind, "name": tool.word() })
}

fn thought(body: &mut Map<String, Value>, thinking: Thinking) {
    let effort = match thinking {
        Thinking::Off => return,
        Thinking::Low => "low",
        Thinking::Medium => "medium",
        Thinking::High => "high",
        Thinking::Max => "max",
    };
    body.insert("thinking".into(), json!({ "type": "adaptive" }));
    body.insert("output_config".into(), json!({ "effort": effort }));
}

// the request's body: the prompt is cached up to the last tool, the last system block and, on a
// turn, the last block of the last message
pub fn body(request: &Request, ask: Ask) -> Value {
    let request = &crate::fit::fit(request, PROVIDER);
    let mut tools: Vec<Value> = request
        .tools
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": tool.schema,
                "eager_input_streaming": true,
            })
        })
        .collect();
    tools.extend(request.server_tools.iter().map(server_definition));
    mark(tools.last_mut());
    let mut body = Map::new();
    body.insert("model".into(), json!(request.model));
    body.insert("max_tokens".into(), json!(request.max_tokens));
    if let Some(thinking) = request.thinking {
        thought(&mut body, thinking);
    }
    body.insert("tools".into(), Value::Array(tools));
    let mut system: Vec<Value> = request
        .system
        .iter()
        .map(|text| json!({ "type": "text", "text": text }))
        .collect();
    mark(system.last_mut());
    if !system.is_empty() {
        body.insert("system".into(), Value::Array(system));
    }
    let mut messages: Vec<Value> = request.messages.iter().map(message_json).collect();
    if ask == Ask::Turn {
        let last = messages.last_mut();
        mark(last.and_then(|message| message["content"].as_array_mut()?.last_mut()));
    }
    body.insert("messages".into(), Value::Array(messages));
    match ask {
        Ask::Turn => body.insert("stream".into(), json!(true)),
        Ask::Summary => body.insert("compaction".into(), json!({ "type": "summarize" })),
    };
    Value::Object(body)
}

fn message_json(message: &Message) -> Value {
    let (role, mut content) = match message {
        Message::User { blocks } => ("user", blocks.iter().map(user_block).collect()),
        Message::Assistant { blocks, .. } => {
            let content: Vec<Value> = blocks.iter().filter_map(block_json).collect();
            ("assistant", content)
        }
    };
    if content.is_empty() {
        content.push(json!({ "type": "text", "text": "(nothing)" }));
    }
    json!({ "role": role, "content": content })
}

fn image(media: &str, data: &str) -> Value {
    json!({ "type": "image", "source": { "type": "base64", "media_type": media, "data": data } })
}

fn shown(shown: &Shown) -> Value {
    match shown {
        Shown::Text { text } => json!({ "type": "text", "text": text }),
        Shown::Image { media, data } => image(media, data),
    }
}

fn user_block(block: &UserBlock) -> Value {
    match block {
        UserBlock::Text { text } => json!({ "type": "text", "text": text }),
        UserBlock::Image { media, data } => image(media, data),
        UserBlock::Result {
            call,
            failed,
            content,
        } => {
            let content = match &content[..] {
                [Shown::Text { text }] => json!(text),
                _ => Value::Array(content.iter().map(shown).collect()),
            };
            json!({
                "type": "tool_result",
                "tool_use_id": call,
                "is_error": failed,
                "content": content,
            })
        }
    }
}

fn input_of(input: &str) -> Value {
    serde_json::from_str(input).unwrap_or_else(|_| json!({}))
}

fn text_block(text: &str) -> Option<Value> {
    (!text.is_empty()).then(|| json!({ "type": "text", "text": text }))
}

fn block_json(block: &Block) -> Option<Value> {
    match block {
        Block::Text { text, citations } if !citations.is_empty() => {
            let citations: Vec<Value> = citations
                .iter()
                .filter_map(|citation| serde_json::from_str(citation).ok())
                .collect();
            Some(json!({ "type": "text", "text": text, "citations": citations }))
        }
        Block::Text { text, .. } => text_block(text),
        Block::Reasoning {
            text,
            sealed: Some(signature),
        } => Some(json!({
            "type": "thinking",
            "thinking": text,
            "signature": signature,
        })),
        Block::Reasoning { sealed: None, .. } => None,
        Block::Redacted { sealed } => Some(json!({ "type": "redacted_thinking", "data": sealed })),
        Block::ToolUse { id, name, input } => Some(json!({
            "type": "tool_use",
            "id": id,
            "name": name,
            "input": input_of(input),
        })),
        Block::ServerToolUse { id, name, input } => Some(json!({
            "type": "server_tool_use",
            "id": id,
            "name": name,
            "input": input_of(input),
        })),
        Block::ServerToolResult { sealed } => serde_json::from_str(sealed).ok(),
        Block::Summary {
            sealed: Some(sealed),
            ..
        } => serde_json::from_str(sealed).ok(),
        Block::Summary { text, .. } => text_block(text),
    }
}

pub fn tool_id(id: &str) -> String {
    let fits = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-';
    if !id.is_empty() && id.bytes().all(fits) {
        return id.to_owned();
    }
    id.bytes()
        .map(|byte| match byte {
            b'_' => "__".to_owned(),
            byte if fits(byte) => char::from(byte).to_string(),
            byte => format!("_{byte:02x}"),
        })
        .collect()
}

pub fn plain(block: &Block) -> Option<String> {
    let text = match block {
        Block::ServerToolUse { name, input, .. } => format!("[{name} {input}]"),
        Block::ServerToolResult { sealed } => plain_result(sealed),
        Block::Text { text, citations } => plain_cited(text, citations),
        _ => return None,
    };
    (!text.is_empty()).then_some(text)
}

fn plain_cited(text: &str, citations: &[String]) -> String {
    let mut lines = vec![text.to_owned()];
    for citation in citations {
        let Ok(value) = serde_json::from_str::<Value>(citation) else {
            continue;
        };
        let (title, url) = (text_of(&value, "title"), text_of(&value, "url"));
        let source = format!("{title} {url}").trim().to_owned();
        if !url.is_empty() && !lines.contains(&source) {
            lines.push(source);
        }
    }
    lines.join("\n")
}

fn plain_result(block: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(block) else {
        return String::new();
    };
    let kind = text_of(&value, "type");
    let content = &value["content"];
    let mut lines = vec![format!("[{kind}]")];
    let page = |item: &Value| {
        let (title, url) = (text_of(item, "title"), text_of(item, "url"));
        (!url.is_empty()).then(|| format!("{title} {url}").trim().to_owned())
    };
    match content {
        Value::Array(items) => lines.extend(items.iter().filter_map(page)),
        Value::Object(_) => {
            lines.extend(page(content));
            let source = &content["content"]["source"];
            lines.extend(
                source["data"]
                    .as_str()
                    .filter(|_| source["type"] == "text")
                    .map(str::to_owned),
            );
        }
        _ => {}
    }
    lines.join("\n")
}

pub fn answers(sealed: &str) -> Option<String> {
    let value: Value = serde_json::from_str(sealed).ok()?;
    value["tool_use_id"].as_str().map(str::to_owned)
}

pub fn budget(headers: &[(String, String)]) -> Budget {
    let mut windows: Vec<Window> = Vec::new();
    for (key, value) in headers {
        let key = key.to_ascii_lowercase();
        let named = key.strip_prefix(UNIFIED);
        let Some(name) = named.and_then(|rest| rest.strip_suffix("-utilization")) else {
            continue;
        };
        let reset = header(headers, &format!("{UNIFIED}{name}-reset"));
        let Some(reset) = reset.and_then(|reset| reset.parse().ok()) else {
            continue;
        };
        if name.is_empty() || windows.iter().any(|window| window.name == name) {
            continue;
        }
        let status = header(headers, &format!("{UNIFIED}{name}-status"));
        windows.push(Window {
            name: name.to_owned(),
            used: value.trim().to_owned(),
            reset,
            status: status.map(str::to_owned),
        });
    }
    let account = header(headers, ORGANIZATION).filter(|id| !id.is_empty());
    Budget {
        windows,
        status: header(headers, &format!("{UNIFIED}status")).map(str::to_owned),
        account: account.map(str::to_owned),
    }
}

fn text_of(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}

fn read_block(block: &Value) -> Option<Block> {
    match block["type"].as_str()? {
        "text" => Some(Block::Text {
            text: text_of(block, "text"),
            citations: block["citations"]
                .as_array()
                .into_iter()
                .flatten()
                .map(Value::to_string)
                .collect(),
        }),
        "thinking" => {
            let signature = text_of(block, "signature");
            Some(Block::Reasoning {
                text: text_of(block, "thinking"),
                sealed: (!signature.is_empty()).then_some(signature),
            })
        }
        "redacted_thinking" => Some(Block::Redacted {
            sealed: text_of(block, "data"),
        }),
        "tool_use" => Some(Block::ToolUse {
            id: text_of(block, "id"),
            name: text_of(block, "name"),
            input: block["input"].to_string(),
        }),
        "server_tool_use" => Some(Block::ServerToolUse {
            id: text_of(block, "id"),
            name: text_of(block, "name"),
            input: block["input"].to_string(),
        }),
        result if result.ends_with("_tool_result") => Some(Block::ServerToolResult {
            sealed: block.to_string(),
        }),
        _ => None,
    }
}

fn stop(reason: Option<&str>) -> Stop {
    match reason {
        Some("tool_use") => Stop::ToolUse,
        Some("max_tokens") => Stop::MaxTokens,
        Some("stop_sequence") => Stop::StopSequence,
        Some("pause_turn") => Stop::Pause,
        Some("refusal") => Stop::Refusal,
        _ => Stop::EndTurn,
    }
}

fn counted(usage: &mut Usage, value: &Value) {
    let counts = [
        ("input_tokens", &mut usage.input),
        ("output_tokens", &mut usage.output),
        ("cache_read_input_tokens", &mut usage.cache_read),
        ("cache_creation_input_tokens", &mut usage.cache_write),
    ];
    for (key, count) in counts {
        if let Some(reported) = value[key].as_u64() {
            *count = reported;
        }
    }
}

pub fn last_pass(usage: &Value) -> Option<u64> {
    let passes = usage["iterations"].as_array()?;
    let pass = passes.iter().rev().find(|pass| pass["type"] == "message")?;
    let counted = [
        "input_tokens",
        "cache_read_input_tokens",
        "cache_creation_input_tokens",
        "output_tokens",
    ];
    Some(counted.iter().filter_map(|key| pass[*key].as_u64()).sum())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Forming {
    Nothing,
    Kept { index: u64 },
    Skipped { index: u64 },
}

// a streamed reply as it folds: the blocks so far and the block forming, by the provider's index
#[derive(Clone, Debug)]
pub struct Fold {
    asked: String,
    model: String,
    said: Vec<Block>,
    forming: Forming,
    usage: Usage,
    prompt: Option<u64>,
    context: Option<u64>,
    stop: Option<Stop>,
    overflowed: bool,
    events: u64,
    ended: bool,
}

impl Fold {
    pub fn new(asked: &str) -> Fold {
        Fold {
            asked: asked.to_owned(),
            model: String::new(),
            said: Vec::new(),
            forming: Forming::Nothing,
            usage: Usage::default(),
            prompt: None,
            context: None,
            stop: None,
            overflowed: false,
            events: 0,
            ended: false,
        }
    }

    // the harness events one of the provider's events makes, the last of them the reply or the
    // failure that ends the stream; nothing once it ended
    pub fn event(&mut self, data: &str) -> Vec<Event> {
        if self.ended {
            return Vec::new();
        }
        self.events += 1;
        let events = self.read(data);
        self.ended = events.iter().any(Event::ends);
        events
    }

    // what ends the stream when the provider's stream ended without saying it was done
    pub fn end(&mut self) -> Option<Event> {
        if self.ended {
            return None;
        }
        self.ended = true;
        let message = format!(
            "the model's reply ended after {} events, before the model finished it",
            self.events
        );
        Some(Event::Failed(Failure::new(Class::Dropped, message)))
    }

    fn read(&mut self, data: &str) -> Vec<Event> {
        let Ok(event) = serde_json::from_str::<Value>(data) else {
            let message = format!("event {} is not JSON", self.events);
            return vec![Event::Failed(Failure::new(Class::Wire, message))];
        };
        match event["type"].as_str().unwrap_or_default() {
            "message_start" => {
                self.started(&event["message"]);
                Vec::new()
            }
            "content_block_start" => self.block_started(&event),
            "content_block_delta" => self.block_delta(&event),
            "content_block_stop" => self.block_stopped(&event),
            "message_delta" => {
                counted(&mut self.usage, &event["usage"]);
                self.context = last_pass(&event["usage"]).or(self.context);
                match event["delta"]["stop_reason"].as_str() {
                    Some(OVERFLOWED) => self.overflowed = true,
                    reason @ Some(_) => self.stop = Some(stop(reason)),
                    None => {}
                }
                Vec::new()
            }
            "message_stop" => vec![self.stopped()],
            "error" => vec![Event::Failed(stream_failure(&event["error"]))],
            _ => Vec::new(),
        }
    }

    fn started(&mut self, message: &Value) {
        self.model = text_of(message, "model");
        self.usage = Usage::default();
        counted(&mut self.usage, &message["usage"]);
        let usage = self.usage;
        self.prompt = Some(usage.input + usage.cache_read + usage.cache_write);
    }

    fn place(&self) -> u64 {
        self.said.len() as u64 - 1
    }

    fn broken(&self, message: String) -> Vec<Event> {
        vec![Event::Failed(Failure::new(Class::Wire, message))]
    }

    fn block_started(&mut self, event: &Value) -> Vec<Event> {
        let index = event["index"].as_u64().unwrap_or_default();
        let Some(mut block) = read_block(&event["content_block"]) else {
            self.forming = Forming::Skipped { index };
            return Vec::new();
        };
        if let Block::ToolUse { input, .. } | Block::ServerToolUse { input, .. } = &mut block {
            input.clear();
        }
        let opened = match &block {
            Block::Reasoning { text, .. } => Block::Reasoning {
                text: text.clone(),
                sealed: None,
            },
            block => block.clone(),
        };
        self.said.push(block);
        self.forming = Forming::Kept { index };
        vec![Event::Block {
            index: self.place(),
            block: opened,
        }]
    }

    fn forming(&self, index: u64) -> Option<bool> {
        match self.forming {
            Forming::Kept { index: at } if at == index => Some(true),
            Forming::Skipped { index: at } if at == index => Some(false),
            Forming::Kept { .. } | Forming::Skipped { .. } | Forming::Nothing => None,
        }
    }

    fn block_delta(&mut self, event: &Value) -> Vec<Event> {
        let index = event["index"].as_u64().unwrap_or_default();
        let delta = &event["delta"];
        let kind = delta["type"].as_str().unwrap_or_default();
        match self.forming(index) {
            Some(true) => {}
            Some(false) => return Vec::new(),
            None => {
                let message = format!("a {kind} for block {index}, which is not the block forming");
                return self.broken(message);
            }
        }
        let place = self.place();
        let Some(block) = self.said.last_mut() else {
            return Vec::new();
        };
        let piece = |key: &str| delta[key].as_str().unwrap_or_default().to_owned();
        let grown = match (kind, block) {
            ("text_delta", Block::Text { text, .. }) => Some((text, piece("text"))),
            ("thinking_delta", Block::Reasoning { text, .. }) => Some((text, piece("thinking"))),
            (
                "input_json_delta",
                Block::ToolUse { input, .. } | Block::ServerToolUse { input, .. },
            ) => Some((input, piece("partial_json"))),
            ("signature_delta", Block::Reasoning { sealed, .. }) => {
                sealed.get_or_insert_default().push_str(&piece("signature"));
                return Vec::new();
            }
            ("citations_delta", Block::Text { citations, .. }) => {
                citations.push(delta["citation"].to_string());
                return Vec::new();
            }
            (_, _) => None,
        };
        let Some((grown, text)) = grown else {
            return self.broken(format!("a {kind} for block {index}, which it does not fit"));
        };
        grown.push_str(&text);
        vec![Event::Delta { index: place, text }]
    }

    fn block_stopped(&mut self, event: &Value) -> Vec<Event> {
        let index = event["index"].as_u64().unwrap_or_default();
        match self.forming(index) {
            Some(true) => {}
            Some(false) => {
                self.forming = Forming::Nothing;
                return Vec::new();
            }
            None => {
                let message = format!("block {index} stopped, which is not the block forming");
                return self.broken(message);
            }
        }
        self.forming = Forming::Nothing;
        let place = self.place();
        if let Some(Block::ToolUse { input, .. } | Block::ServerToolUse { input, .. }) =
            self.said.last_mut()
        {
            let written = match input.is_empty() {
                true => Ok(json!({})),
                false => serde_json::from_str::<Value>(input),
            };
            if let Ok(value) = written {
                *input = value.to_string();
            }
        }
        vec![Event::End { index: place }]
    }

    fn stopped(&mut self) -> Event {
        if self.overflowed {
            let message = "the conversation does not fit the model's context window: summarize the conversation or use a model with a larger context window";
            return Event::Failed(Failure::new(Class::Overflow, message));
        }
        Event::Reply(Reply {
            model: self.model.clone(),
            lane: Lane::of(PROVIDER, &self.asked),
            said: std::mem::take(&mut self.said),
            stop: self.stop.unwrap_or(Stop::EndTurn),
            usage: self.usage,
            prompt: self.prompt,
            context: self.context,
        })
    }
}

fn stream_failure(error: &Value) -> Failure {
    let kind = error["type"].as_str().unwrap_or("error");
    let message = error["message"].as_str().unwrap_or_default();
    let class = match kind {
        "overloaded_error" | "api_error" | "rate_limit_error" => Class::Overloaded,
        _ => Class::Provider,
    };
    let message = format!("the model's reply failed as it streamed, {kind}: {message}");
    Failure::new(class, message)
}

// an answer that is no reply, as the class harness acts on: `now` (seconds since the epoch) turns a
// `retry-after` into the reset a limited account waits for
pub fn refused(status: u16, headers: &[(String, String)], body: &[u8], now: u64) -> Failure {
    let value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let kind = value["error"]["type"].as_str();
    let said = value["error"]["message"].as_str();
    let message = match (kind, said) {
        (Some(kind), Some(said)) => format!("the model answered {status}, {kind}: {said}"),
        _ => match crate::page_words(body) {
            Some(page) if value.is_null() => format!("the model answered {status}: {page}"),
            _ => format!("the model answered {status}"),
        },
    };
    let invalid = kind == Some("invalid_request_error");
    let lower = said.unwrap_or_default().to_lowercase();
    let seconds = |name| header(headers, name).and_then(|value| value.parse::<u64>().ok());
    let class = match status {
        400 if invalid && lower.starts_with("prompt is too long") => Class::Overflow,
        400 if invalid && lower.contains("image") => Class::Pictures,
        413 => Class::Pictures,
        429 => Class::Limited,
        500.. => Class::Overloaded,
        _ => Class::Provider,
    };
    let after = seconds(RETRY_AFTER).map(|after| now.saturating_add(after));
    let reset = match class {
        Class::Limited | Class::Overloaded => seconds(RESET).max(after),
        _ => None,
    };
    Failure {
        class,
        message,
        reset,
    }
}

// a summary's whole answer: the compaction block this provider takes back, and its words
pub fn summarized(
    asked: &str,
    status: u16,
    headers: &[(String, String)],
    body: &[u8],
    now: u64,
) -> Result<Summarized, Failure> {
    if status != 200 {
        return Err(refused(status, headers, body, now));
    }
    let value: Value = serde_json::from_slice(body).map_err(|error| {
        Failure::new(
            Class::Wire,
            format!("the model's answer is not JSON: {error}"),
        )
    })?;
    let stopped = value["stop_reason"].as_str().unwrap_or_default();
    if stopped != "compaction" {
        let message = format!("the model did not summarize the thread: it stopped with {stopped}");
        return Err(Failure::new(Class::Provider, message));
    }
    let block = &value["content"][0];
    let summarized = block["type"] == "compaction" && block["content"].is_string();
    let Some(fields) = block.as_object().filter(|_| summarized) else {
        return Err(Failure::new(
            Class::Provider,
            "the model answered no summary",
        ));
    };
    let present: Map<String, Value> = fields
        .iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let tokens = value["usage"]["iterations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|iteration| iteration["type"] == "compaction")
        .filter_map(|iteration| iteration["output_tokens"].as_u64())
        .sum();
    Ok(Summarized {
        text: text_of(block, "content"),
        sealed: Some(Value::Object(present).to_string()),
        lane: Lane::of(PROVIDER, asked),
        tokens,
    })
}

// a model's record: its context window, where the models endpoint names one
pub fn model(
    status: u16,
    headers: &[(String, String)],
    body: &[u8],
    now: u64,
) -> Result<Model, Failure> {
    if status != 200 {
        return Err(refused(status, headers, body, now));
    }
    let value: Value = serde_json::from_slice(body).map_err(|error| {
        Failure::new(
            Class::Wire,
            format!("the model's answer is not JSON: {error}"),
        )
    })?;
    let window = value["max_input_tokens"]
        .as_u64()
        .filter(|tokens| *tokens > 0);
    Ok(Model {
        provider: PROVIDER.to_owned(),
        window,
        pictures: Some(Pictures {
            most_base64: IMAGE_BASE64,
            most_side: IMAGE_SIDE,
        }),
    })
}
