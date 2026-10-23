//! OpenAI's Responses API on the Codex surface a ChatGPT plan opens (`/backend-api/`) as the
//! harness wire: a request's body under that surface's rules, its server-sent events folded into
//! harness events, an answer that is no reply read as a failure, and a model's window.
use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::budget::{self, Budget, Window, header};
use crate::{
    Block, Class, Event, Failure, Lane, Message, Reply, Request, Shown, Stop, Thinking, Usage,
    UserBlock,
};

pub const PROVIDER: &str = "openai";
pub const INSTRUCTIONS: &str = "The developer message that opens this conversation, when there is one, carries your instructions.";
pub const RESPONSES: &str = "/backend-api/codex/responses";
const RETRY_AFTER: &str = "retry-after";
const SESSION_HEADERS: [&str; 3] = ["session-id", "thread-id", "x-client-request-id"];

pub fn headers(request: &Request) -> Vec<(String, String)> {
    let fixed = [
        ("content-type", "application/json"),
        ("accept", "text/event-stream"),
        ("openai-beta", "responses=experimental"),
    ];
    let fixed = fixed
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()));
    let session = request.cache_key.iter().flat_map(|key| {
        SESSION_HEADERS
            .iter()
            .map(move |name| ((*name).to_owned(), key.clone()))
    });
    fixed.chain(session).collect()
}

fn effort(thinking: Thinking) -> &'static str {
    match thinking {
        Thinking::Off => "minimal",
        Thinking::Low => "low",
        Thinking::Medium => "medium",
        Thinking::High => "high",
        Thinking::Max => "xhigh",
    }
}

fn input_text(text: &str) -> Value {
    json!({ "type": "input_text", "text": text })
}

fn input_image(media: &str, data: &str) -> Value {
    json!({ "type": "input_image", "image_url": format!("data:{media};base64,{data}") })
}

fn said(role: &str, content: Vec<Value>) -> Option<Value> {
    (!content.is_empty()).then(|| json!({ "type": "message", "role": role, "content": content }))
}

fn assistant_text(text: &str) -> Option<Value> {
    let content = vec![json!({ "type": "output_text", "text": text })];
    (!text.is_empty())
        .then(|| said("assistant", content))
        .flatten()
}

pub fn body(request: &Request) -> Value {
    let request = &crate::fit::fit(request, PROVIDER);
    let mut input: Vec<Value> = Vec::new();
    let system: Vec<Value> = request.system.iter().map(|text| input_text(text)).collect();
    input.extend(said("developer", system));
    for message in &request.messages {
        match message {
            Message::User { blocks } => user(&mut input, blocks),
            Message::Assistant { blocks, .. } => assistant(&mut input, blocks),
        }
    }
    let tools: Vec<Value> = request
        .tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.schema,
                "strict": false,
            })
        })
        .collect();
    let mut body = json!({
        "model": request.model,
        "instructions": INSTRUCTIONS,
        "input": input,
        "tools": tools,
        "tool_choice": "auto",
        "parallel_tool_calls": true,
        "include": ["reasoning.encrypted_content"],
        "store": false,
        "stream": true,
    });
    if let Some(thinking) = request.thinking {
        body["reasoning"] = json!({ "effort": effort(thinking), "summary": "auto" });
    }
    if let Some(key) = &request.cache_key {
        body["prompt_cache_key"] = json!(key);
    }
    body
}

fn user(input: &mut Vec<Value>, blocks: &[UserBlock]) {
    let mut content: Vec<Value> = Vec::new();
    for block in blocks {
        match block {
            UserBlock::Text { text } => content.push(input_text(text)),
            UserBlock::Image { media, data } => content.push(input_image(media, data)),
            UserBlock::Result {
                call,
                content: shown,
                ..
            } => {
                let text: Vec<&str> = shown
                    .iter()
                    .filter_map(|shown| match shown {
                        Shown::Text { text } => Some(text.as_str()),
                        Shown::Image { .. } => None,
                    })
                    .collect();
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": call,
                    "output": text.join("\n"),
                }));
                let images = shown.iter().filter_map(|shown| match shown {
                    Shown::Image { media, data } => Some(input_image(media, data)),
                    Shown::Text { .. } => None,
                });
                content.extend(images);
            }
        }
    }
    input.extend(said("user", content));
}

fn assistant(input: &mut Vec<Value>, blocks: &[Block]) {
    for block in blocks {
        let item = match block {
            Block::Text { text, .. } | Block::Summary { text, .. } => assistant_text(text),
            Block::Reasoning { sealed, .. } => sealed
                .as_deref()
                .and_then(|item| serde_json::from_str(item).ok()),
            Block::ToolUse { id, name, input } => Some(json!({
                "type": "function_call",
                "call_id": id,
                "name": name,
                "arguments": input,
            })),
            Block::Redacted { .. }
            | Block::ServerToolUse { .. }
            | Block::ServerToolResult { .. } => None,
        };
        input.extend(item);
    }
}

fn text_of(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}

fn read_item(item: &Value) -> Option<Block> {
    match item["type"].as_str()? {
        "message" => {
            let text: String = item["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|part| match part["type"].as_str()? {
                    "output_text" => Some(text_of(part, "text")),
                    "refusal" => Some(text_of(part, "refusal")),
                    _ => None,
                })
                .collect();
            Some(Block::Text {
                text,
                citations: Vec::new(),
            })
        }
        "function_call" => Some(Block::ToolUse {
            id: text_of(item, "call_id"),
            name: text_of(item, "name"),
            input: text_of(item, "arguments"),
        }),
        "reasoning" => {
            let text: Vec<String> = item["summary"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|part| text_of(part, "text"))
                .collect();
            let sealed = item["encrypted_content"].as_str();
            let sealed = sealed.filter(|sealed| !sealed.is_empty());
            Some(Block::Reasoning {
                text: text.join("\n\n"),
                sealed: sealed.map(|_| item.to_string()),
            })
        }
        _ => None,
    }
}

fn streamable(block: &Block) -> Option<&str> {
    match block {
        Block::Text { text, .. } | Block::Reasoning { text, .. } => Some(text),
        Block::ToolUse { input, .. } => Some(input),
        _ => None,
    }
}

fn refuses(item: &Value) -> bool {
    let mut parts = item["content"].as_array().into_iter().flatten();
    parts.any(|part| part["type"] == "refusal")
}

fn opened_item(item: &Value) -> Option<Block> {
    let block = read_item(item)?;
    Some(match block {
        Block::Text { .. } => Block::Text {
            text: String::new(),
            citations: Vec::new(),
        },
        Block::ToolUse { id, name, .. } => Block::ToolUse {
            id,
            name,
            input: String::new(),
        },
        Block::Reasoning { .. } => Block::Reasoning {
            text: String::new(),
            sealed: None,
        },
        other => other,
    })
}

#[derive(Clone, Debug)]
pub struct Fold {
    asked: String,
    model: String,
    places: BTreeMap<u64, u64>,
    streamed: BTreeMap<u64, String>,
    done: BTreeMap<u64, Block>,
    summaries: BTreeMap<u64, u64>,
    events: u64,
    refused: bool,
    ended: bool,
}

impl Fold {
    pub fn new(asked: &str) -> Fold {
        Fold {
            asked: asked.to_owned(),
            model: String::new(),
            places: BTreeMap::new(),
            streamed: BTreeMap::new(),
            done: BTreeMap::new(),
            summaries: BTreeMap::new(),
            events: 0,
            refused: false,
            ended: false,
        }
    }

    pub fn event(&mut self, data: &str) -> Vec<Event> {
        if self.ended {
            return Vec::new();
        }
        self.events += 1;
        let events = self.read(data);
        self.ended = events.iter().any(Event::ends);
        events
    }

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
        let output = event["output_index"].as_u64().unwrap_or_default();
        match event["type"].as_str().unwrap_or_default() {
            "response.created" | "response.in_progress" => {
                self.model = text_of(&event["response"], "model");
                Vec::new()
            }
            "response.output_item.added" => self.added(output, &event["item"]),
            "response.output_text.delta" | "response.function_call_arguments.delta" => {
                self.delta(output, &event["delta"])
            }
            "response.reasoning_summary_text.delta" => {
                let part = event["summary_index"].as_u64().unwrap_or_default();
                let seen = self.summaries.entry(output).or_insert(part);
                let mut events = Vec::new();
                if *seen != part {
                    *seen = part;
                    events.extend(self.delta(output, &json!("\n\n")));
                }
                events.extend(self.delta(output, &event["delta"]));
                events
            }
            "response.refusal.delta" => {
                self.refused = true;
                self.delta(output, &event["delta"])
            }
            "response.refusal.done" => {
                self.refused = true;
                Vec::new()
            }
            "response.output_item.done" => self.item_done(output, &event["item"]),
            "response.completed" => vec![self.completed(&event["response"])],
            "response.incomplete" => vec![self.incomplete(&event["response"])],
            "response.failed" => vec![Event::Failed(failed(&event["response"]["error"]))],
            "error" => vec![Event::Failed(failed(&event))],
            _ => Vec::new(),
        }
    }

    fn added(&mut self, output: u64, item: &Value) -> Vec<Event> {
        let Some(block) = opened_item(item) else {
            return Vec::new();
        };
        let index = self.places.len() as u64;
        self.places.insert(output, index);
        vec![Event::Block { index, block }]
    }

    fn delta(&mut self, output: u64, delta: &Value) -> Vec<Event> {
        let Some(index) = self.places.get(&output).copied() else {
            return Vec::new();
        };
        let text = delta.as_str().unwrap_or_default().to_owned();
        self.streamed.entry(index).or_default().push_str(&text);
        vec![Event::Delta { index, text }]
    }

    fn item_done(&mut self, output: u64, item: &Value) -> Vec<Event> {
        let (Some(index), Some(block)) = (self.places.get(&output).copied(), read_item(item))
        else {
            return Vec::new();
        };
        self.refused |= refuses(item);
        let streamed = self.streamed.remove(&index).unwrap_or_default();
        let rest = streamable(&block)
            .and_then(|whole| whole.strip_prefix(streamed.as_str()))
            .filter(|rest| !rest.is_empty())
            .map(|rest| Event::Delta {
                index,
                text: rest.to_owned(),
            });
        self.done.insert(index, block);
        rest.into_iter().chain([Event::End { index }]).collect()
    }

    fn said(&mut self) -> Vec<Block> {
        std::mem::take(&mut self.done).into_values().collect()
    }

    fn reply(&mut self, response: &Value, stop: Stop) -> Event {
        let said = self.said();
        let model = match text_of(response, "model") {
            model if model.is_empty() => self.model.clone(),
            model => model,
        };
        let (usage, prompt) = usage_of(&response["usage"]);
        Event::Reply(Reply {
            model,
            lane: Lane::of(PROVIDER, &self.asked),
            said,
            stop,
            usage,
            prompt,
            context: None,
        })
    }

    fn completed(&mut self, response: &Value) -> Event {
        let calls = self
            .done
            .values()
            .any(|block| matches!(block, Block::ToolUse { .. }));
        let stop = match (calls, self.refused) {
            (true, _) => Stop::ToolUse,
            (false, true) => Stop::Refusal,
            (false, false) => Stop::EndTurn,
        };
        self.reply(response, stop)
    }

    fn incomplete(&mut self, response: &Value) -> Event {
        match response["incomplete_details"]["reason"].as_str() {
            Some("max_output_tokens") => self.reply(response, Stop::MaxTokens),
            Some("content_filter") => self.reply(response, Stop::Refusal),
            reason => {
                let message = format!(
                    "the model's reply ended incomplete: {}",
                    reason.unwrap_or("no reason given")
                );
                Event::Failed(Failure::new(Class::Provider, message))
            }
        }
    }
}

fn usage_of(value: &Value) -> (Usage, Option<u64>) {
    let input = value["input_tokens"].as_u64();
    let cached = value["input_tokens_details"]["cached_tokens"]
        .as_u64()
        .unwrap_or_default();
    let usage = Usage {
        input: input.unwrap_or_default().saturating_sub(cached),
        output: value["output_tokens"].as_u64().unwrap_or_default(),
        cache_read: cached,
        cache_write: 0,
    };
    (usage, input)
}

fn class_of(code: &str) -> Class {
    match code {
        "context_length_exceeded" => Class::Overflow,
        "rate_limit_exceeded" | "usage_limit_reached" => Class::Limited,
        "server_error" | "server_is_overloaded" | "slow_down" => Class::Overloaded,
        "invalid_image" | "image_too_large" => Class::Pictures,
        _ => Class::Provider,
    }
}

fn failed(error: &Value) -> Failure {
    let code = error["code"]
        .as_str()
        .or_else(|| error["type"].as_str())
        .unwrap_or("error");
    let message = error["message"].as_str().unwrap_or_default();
    let words = format!("the model's reply failed as it streamed, {code}: {message}");
    Failure::new(class_of(code), words)
}

fn window(headers: &[(String, String)], family: &str, place: &str) -> Option<Window> {
    let named = |part: &str| header(headers, &format!("x-{family}-{place}-{part}"));
    let used = budget::percent(named("used-percent")?)?;
    let reset = named("reset-at")?.parse().ok()?;
    let minutes: u64 = named("window-minutes")?.parse().ok()?;
    let length = budget::named(minutes);
    let name = match family {
        "codex" => length,
        family => format!("{family}_{length}"),
    };
    Some(Window {
        name,
        used,
        reset,
        status: None,
    })
}

pub fn budget(headers: &[(String, String)]) -> Budget {
    let mut windows: Vec<Window> = Vec::new();
    for (key, _) in headers {
        let key = key.to_ascii_lowercase();
        let named = key.strip_prefix("x-");
        let Some(family) = named.and_then(|rest| rest.strip_suffix("-primary-used-percent")) else {
            continue;
        };
        for place in ["primary", "secondary"] {
            windows.extend(window(headers, family, place));
        }
    }
    Budget {
        windows,
        ..Budget::default()
    }
}

pub fn refused(status: u16, headers: &[(String, String)], body: &[u8], now: u64) -> Failure {
    let value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let error = &value["error"];
    let code = error["code"].as_str().or_else(|| error["type"].as_str());
    let said = error["message"]
        .as_str()
        .or_else(|| error.as_str())
        .or_else(|| value["message"].as_str())
        .or_else(|| value["detail"].as_str())
        .or_else(|| value["detail"]["message"].as_str());
    let keys: Vec<&str> = value
        .as_object()
        .into_iter()
        .flat_map(|fields| fields.keys().map(String::as_str))
        .collect();
    let message = match (code, said) {
        (Some(code), Some(said)) => format!("the model answered {status}, {code}: {said}"),
        (None, Some(said)) => format!("the model answered {status}: {said}"),
        (Some(code), None) => format!("the model answered {status}, {code}"),
        _ if !keys.is_empty() => {
            let keys = keys.join(", ");
            format!("the model answered {status} in JSON of keys {keys}")
        }
        _ => match crate::page_words(body) {
            Some(page) if value.is_null() => format!("the model answered {status}: {page}"),
            _ => format!("the model answered {status}"),
        },
    };
    let class = match (status, code) {
        (_, Some(code)) if class_of(code) != Class::Provider => class_of(code),
        (413, _) => Class::Pictures,
        (429, _) => Class::Limited,
        (500.., _) => Class::Overloaded,
        _ => Class::Provider,
    };
    let after = header(headers, RETRY_AFTER).and_then(|value| value.parse::<u64>().ok());
    let after = after.map(|after| now.saturating_add(after));
    let reset = match class {
        Class::Limited | Class::Overloaded => after.max(error["resets_at"].as_u64()),
        _ => None,
    };
    Failure {
        class,
        message,
        reset,
    }
}
