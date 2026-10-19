//! The Anthropic Messages API as JSON: a thread's request in parts (a header, one part per
//! message, separators, a close that asks for a streamed reply) so earlier messages are reused
//! blobs, a reply read back as a turn whole or one server-sent event at a time, an on-demand
//! compaction's summary, and a model's context window from the models endpoint.
use abi::role::egress::http::{Head, Method, Response};
use serde_json::{Map, Value, json};

use super::{Answered, Header, Reply, Summarized, Summary};
use crate::pictures::{Look, Looks};
use crate::{
    Awaited, Body, Flow, Forming, Frame, Partial, Role, Said, ServerTool, Stop, Thinking, Usage,
    tools,
};

pub(crate) const VERSION: &str = "2023-06-01";
pub(crate) const COMPACTION_BETA: &str = "compact-2026-09-04";
pub(crate) const WEB_SEARCH: &str = "web_search_20250305";
pub(crate) const WEB_FETCH: &str = "web_fetch_20250910";
pub(crate) const CLOSE: &str = r#"],"stream":true}"#;
pub(crate) const SUMMARIZE: &str = r#"],"compaction":{"type":"summarize"}}"#;
// Anthropic's own limits on one image, from its vision guide
// (https://docs.anthropic.com/en/docs/build-with-claude/vision): at most 5 MB as base64, and at
// most 8000 px on either side
const IMAGE_BASE64: u64 = 5 * 1024 * 1024;
const IMAGE_SIDE: u32 = 8000;

pub(crate) fn head(summarized: bool) -> Head {
    let mut headers = vec![
        ("anthropic-version".into(), VERSION.into()),
        ("content-type".into(), "application/json".into()),
    ];
    if summarized {
        headers.push(("anthropic-beta".into(), COMPACTION_BETA.into()));
    }
    Head {
        method: Method::Post,
        target: "/v1/messages".into(),
        headers,
    }
}

pub(crate) fn model(name: &str) -> Head {
    Head {
        method: Method::Get,
        target: format!("/v1/models/{name}"),
        headers: vec![("anthropic-version".into(), VERSION.into())],
    }
}

// what a whole request asks of the model, read back from its head: a model's window or a
// thread's summary
pub(crate) fn awaited(asked: &Head) -> Option<Awaited> {
    if let Some(name) = asked.target.strip_prefix("/v1/models/")
        && *asked == model(name)
    {
        return Some(Awaited::Window {
            name: name.to_owned(),
        });
    }
    (*asked == head(true)).then_some(Awaited::Summary)
}

fn cached() -> Value {
    json!({ "type": "ephemeral" })
}

// harness's own tools, then the agent's tool files, each naming the file it is read from
fn definitions(files: &[tools::file::FileTool]) -> Vec<Value> {
    let native = tools::CATALOG.iter().map(|entry| {
        json!({
            "name": entry.name,
            "description": entry.description,
            "input_schema": (entry.schema)(),
            "eager_input_streaming": true,
        })
    });
    let written = files.iter().map(|tool| {
        json!({
            "name": tool.name,
            "description": format!("{} (tool file {})", tool.description, tool.path),
            "input_schema": tool.schema(),
            "eager_input_streaming": true,
        })
    });
    native.chain(written).collect()
}

fn server_definition(tool: ServerTool) -> Value {
    let kind = match tool {
        ServerTool::WebSearch => WEB_SEARCH,
        ServerTool::WebFetch => WEB_FETCH,
    };
    json!({ "type": kind, "name": tool.word() })
}

pub(crate) fn header(header: &Header) -> String {
    let mut tools = definitions(header.file_tools);
    tools.extend(header.server_tools.iter().copied().map(server_definition));
    if let Some(Value::Object(last)) = tools.last_mut() {
        last.insert("cache_control".into(), cached());
    }
    let mut request = Map::new();
    request.insert("model".into(), json!(header.name));
    request.insert("max_tokens".into(), json!(header.max_tokens));
    if let Some(thinking) = header.thinking {
        thought(&mut request, thinking);
    }
    request.insert("tools".into(), Value::Array(tools));
    let mut system: Vec<Value> = header
        .system
        .iter()
        .map(|text| json!({ "type": "text", "text": text }))
        .collect();
    if let Some(Value::Object(last)) = system.last_mut() {
        last.insert("cache_control".into(), cached());
    }
    if !system.is_empty() {
        request.insert("system".into(), Value::Array(system));
    }
    let mut text = Value::Object(request).to_string();
    text.pop();
    text.push_str(",\"messages\":[");
    text
}

fn thought(request: &mut Map<String, Value>, thinking: Thinking) {
    let effort = match thinking {
        Thinking::Off => {
            request.insert("thinking".into(), json!({ "type": "disabled" }));
            return;
        }
        Thinking::Low | Thinking::Medium | Thinking::High | Thinking::Max => thinking.word(),
    };
    request.insert("thinking".into(), json!({ "type": "adaptive" }));
    request.insert("output_config".into(), json!({ "effort": effort }));
}

pub(crate) fn image_size(size: u64) -> Result<(), String> {
    let encoded = size.div_ceil(3).saturating_mul(4);
    match encoded <= IMAGE_BASE64 {
        true => Ok(()),
        false => {
            Err("too large to show: Anthropic takes at most 5 MB of base64 per image".to_owned())
        }
    }
}

pub(crate) fn image_side(width: u32, height: u32) -> Result<(), String> {
    match width.max(height) <= IMAGE_SIDE {
        true => Ok(()),
        false => Err(format!(
            "{width}×{height} px, too large to show: Anthropic takes at most {IMAGE_SIDE} px on a side"
        )),
    }
}

fn blocks(body: &Body, looks: &[Look]) -> Vec<Value> {
    match body {
        Body::Post { text, .. } => {
            let mut blocks = vec![json!({ "type": "text", "text": text })];
            blocks.extend(looks.iter().flat_map(seen));
            blocks
        }
        Body::Turn { said, .. } => said.iter().filter_map(said_block).collect(),
        Body::Refused { call, problem, .. } => vec![json!({
            "type": "tool_result",
            "tool_use_id": call,
            "is_error": true,
            "content": problem,
        })],
        Body::Result {
            call, text, failed, ..
        } => vec![json!({
            "type": "tool_result",
            "tool_use_id": call,
            "is_error": failed,
            "content": result_content(text, looks),
        })],
        Body::Notice { .. } | Body::Compaction { .. } => Vec::new(),
    }
}

// a picture's blocks: the line naming it, then the image; or the sentence saying why not
fn seen(look: &Look) -> Vec<Value> {
    match look {
        Look::Image { label, media, data } => vec![
            json!({ "type": "text", "text": label }),
            json!({ "type": "image", "source": { "type": "base64", "media_type": media, "data": data } }),
        ],
        Look::Words(text) => vec![json!({ "type": "text", "text": text })],
    }
}

// a tool result's content: its text alone, or with what it shows after it
fn result_content(text: &str, looks: &[Look]) -> Value {
    if looks.is_empty() {
        return json!(text);
    }
    let mut content = vec![json!({ "type": "text", "text": text })];
    content.extend(looks.iter().flat_map(seen));
    Value::Array(content)
}

fn said_block(said: &Said) -> Option<Value> {
    match said {
        Said::Text(text) if text.is_empty() => None,
        Said::Text(text) => Some(json!({ "type": "text", "text": text })),
        Said::Thinking { text, signature } => Some(json!({
            "type": "thinking",
            "thinking": text,
            "signature": signature,
        })),
        Said::RedactedThinking(data) => Some(json!({ "type": "redacted_thinking", "data": data })),
        Said::ToolUse { id, name, input } => {
            let input: Value = serde_json::from_str(input).unwrap_or_else(|_| json!({}));
            Some(json!({ "type": "tool_use", "id": id, "name": name, "input": input }))
        }
        Said::ServerToolUse { id, name, input } => {
            let input: Value = serde_json::from_str(input).unwrap_or_else(|_| json!({}));
            Some(json!({ "type": "server_tool_use", "id": id, "name": name, "input": input }))
        }
        Said::ServerToolResult(block) => serde_json::from_str(block).ok(),
        Said::Cited { text, citations } => {
            let citations: Vec<Value> = citations
                .iter()
                .filter_map(|citation| serde_json::from_str(citation).ok())
                .collect();
            Some(json!({ "type": "text", "text": text, "citations": citations }))
        }
    }
}

pub(crate) fn message(
    role: Role,
    frames: &[Frame],
    looks: &Looks,
    first: bool,
    last: bool,
) -> String {
    let none: Vec<Look> = Vec::new();
    let mut content: Vec<Value> = frames
        .iter()
        .flat_map(|frame| blocks(&frame.body, looks.get(&frame.seq).unwrap_or(&none)))
        .collect();
    if content.is_empty() {
        content.push(json!({ "type": "text", "text": "(nothing)" }));
    }
    if let (true, Some(Value::Object(block))) = (last, content.last_mut()) {
        block.insert("cache_control".into(), cached());
    }
    let role = match role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    let text = json!({ "role": role, "content": content }).to_string();
    match first {
        true => text,
        false => format!(",{text}"),
    }
}

pub(crate) fn summary_message(block: &str) -> String {
    format!(r#"{{"role":"assistant","content":[{block}]}}"#)
}

pub(crate) fn summary_words(block: &str) -> String {
    serde_json::from_str::<Value>(block)
        .ok()
        .and_then(|value| value["content"].as_str().map(str::to_owned))
        .unwrap_or_else(|| block.to_owned())
}

pub(crate) fn summary_text(text: &str) -> String {
    let content = json!([{ "type": "text", "text": text }]);
    json!({ "role": "assistant", "content": content }).to_string()
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

fn text_of(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}

fn read_said(block: &Value) -> Option<Said> {
    match block["type"].as_str()? {
        "text" => Some(text_said(block)),
        "thinking" => Some(Said::Thinking {
            text: text_of(block, "thinking"),
            signature: text_of(block, "signature"),
        }),
        "redacted_thinking" => Some(Said::RedactedThinking(text_of(block, "data"))),
        "tool_use" => Some(Said::ToolUse {
            id: text_of(block, "id"),
            name: text_of(block, "name"),
            input: block["input"].to_string(),
        }),
        "server_tool_use" => Some(Said::ServerToolUse {
            id: text_of(block, "id"),
            name: text_of(block, "name"),
            input: block["input"].to_string(),
        }),
        result if result.ends_with("_tool_result") => {
            Some(Said::ServerToolResult(block.to_string()))
        }
        _ => None,
    }
}

fn text_said(block: &Value) -> Said {
    let text = text_of(block, "text");
    let citations: Vec<String> = block["citations"]
        .as_array()
        .into_iter()
        .flatten()
        .map(Value::to_string)
        .collect();
    match citations.is_empty() {
        true => Said::Text(text),
        false => Said::Cited { text, citations },
    }
}

fn json(body: &[u8]) -> Result<Value, String> {
    serde_json::from_slice(body).map_err(|error| format!("the model's answer is not JSON: {error}"))
}

fn reply(value: &Value) -> Result<Reply, String> {
    let content = value["content"]
        .as_array()
        .ok_or("the model's reply has no content")?;
    let mut usage = Usage::default();
    counted(&mut usage, &value["usage"]);
    Ok(Reply {
        model: text_of(value, "model"),
        said: content.iter().filter_map(read_said).collect(),
        stop: stop(value["stop_reason"].as_str()),
        usage,
    })
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

const OVERFLOWED: &str = "model_context_window_exceeded";

pub(crate) fn fold(partial: &mut Partial, data: &str) {
    let Ok(event) = serde_json::from_str::<Value>(data) else {
        partial.flow = Flow::Broken {
            reason: format!("event {} is not JSON", partial.next_seq),
        };
        return;
    };
    match event["type"].as_str().unwrap_or_default() {
        "message_start" => started(partial, &event["message"]),
        "content_block_start" => block_started(partial, &event),
        "content_block_delta" => block_delta(partial, &event),
        "content_block_stop" => block_stopped(partial, &event),
        "message_delta" => message_delta(partial, &event),
        "message_stop" => partial.flow = Flow::Ended,
        "error" => {
            partial.flow = Flow::Failed {
                text: stream_failure(&event["error"]),
            }
        }
        "ping" => {}
        _unread => {}
    }
}

fn started(partial: &mut Partial, message: &Value) {
    partial.model = text_of(message, "model");
    partial.usage = Usage::default();
    counted(&mut partial.usage, &message["usage"]);
}

fn index_of(event: &Value) -> u64 {
    event["index"].as_u64().unwrap_or_default()
}

fn block_started(partial: &mut Partial, event: &Value) {
    let index = index_of(event);
    let Some(mut said) = read_said(&event["content_block"]) else {
        partial.forming = Forming::Skipped { index };
        return;
    };
    if let Said::ToolUse { input, .. } | Said::ServerToolUse { input, .. } = &mut said {
        input.clear();
    }
    partial.said.push(said);
    partial.forming = Forming::Kept { index };
}

fn broken(partial: &mut Partial, reason: String) {
    partial.flow = Flow::Broken { reason };
}

enum Block {
    Kept,
    Skipped,
    NotForming,
}

fn block(forming: Forming, index: u64) -> Block {
    let (at, block) = match forming {
        Forming::Kept { index } => (index, Block::Kept),
        Forming::Skipped { index } => (index, Block::Skipped),
        Forming::Nothing => return Block::NotForming,
    };
    match at == index {
        true => block,
        false => Block::NotForming,
    }
}

fn block_delta(partial: &mut Partial, event: &Value) {
    let index = index_of(event);
    let delta = &event["delta"];
    let kind = delta["type"].as_str().unwrap_or_default();
    let said = match block(partial.forming, index) {
        Block::Kept => partial.said.last_mut(),
        Block::Skipped => return,
        Block::NotForming => None,
    };
    let Some(said) = said else {
        let reason = format!("a {kind} for block {index}, which is not the block forming");
        return broken(partial, reason);
    };
    match applied(said, kind, delta) {
        Applied::Fits => {}
        Applied::DoesNotFit => broken(
            partial,
            format!("a {kind} for block {index}, which it does not fit"),
        ),
    }
}

enum Applied {
    Fits,
    DoesNotFit,
}

fn applied(said: &mut Said, kind: &str, delta: &Value) -> Applied {
    if kind == "citations_delta" {
        return cited(said, &delta["citation"]);
    }
    let (grown, key) = match (kind, said) {
        ("text_delta", Said::Text(text) | Said::Cited { text, .. }) => (text, "text"),
        ("thinking_delta", Said::Thinking { text, .. }) => (text, "thinking"),
        ("signature_delta", Said::Thinking { signature, .. }) => (signature, "signature"),
        ("input_json_delta", Said::ToolUse { input, .. } | Said::ServerToolUse { input, .. }) => {
            (input, "partial_json")
        }
        (_, _) => return Applied::DoesNotFit,
    };
    grown.push_str(delta[key].as_str().unwrap_or_default());
    Applied::Fits
}

fn cited(said: &mut Said, citation: &Value) -> Applied {
    let citation = citation.to_string();
    match said {
        Said::Text(text) => {
            let text = std::mem::take(text);
            *said = Said::Cited {
                text,
                citations: vec![citation],
            };
        }
        Said::Cited { citations, .. } => citations.push(citation),
        _ => return Applied::DoesNotFit,
    }
    Applied::Fits
}

fn block_stopped(partial: &mut Partial, event: &Value) {
    let index = index_of(event);
    match block(partial.forming, index) {
        Block::Kept => partial.forming = Forming::Nothing,
        Block::Skipped => return partial.forming = Forming::Nothing,
        Block::NotForming => {
            let reason = format!("block {index} stopped, which is not the block forming");
            return broken(partial, reason);
        }
    }
    let (tool, input) = match partial.said.last_mut() {
        Some(Said::ToolUse { name, input, .. }) => (Some(name.as_str()), input),
        Some(Said::ServerToolUse { input, .. }) => (None, input),
        _ => return,
    };
    let written = match input.is_empty() {
        true => Ok(json!({})),
        false => serde_json::from_str::<Value>(input),
    };
    if let Ok(mut value) = written {
        // a field its tool gives no type streams as JSON text: it is read as the JSON it holds
        if let Some(tool) = tool {
            tools::untyped_read(tool, &mut value);
        }
        *input = value.to_string();
    }
}

fn message_delta(partial: &mut Partial, event: &Value) {
    counted(&mut partial.usage, &event["usage"]);
    let reason = event["delta"]["stop_reason"].as_str();
    match reason {
        Some(OVERFLOWED) => partial.flow = Flow::Overflowed,
        Some(_) | None => partial.stop = Some(stop(reason)),
    }
}

fn stream_failure(error: &Value) -> String {
    let kind = error["type"].as_str().unwrap_or("error");
    let message = error["message"].as_str().unwrap_or_default();
    format!("the model's reply failed as it streamed, {kind}: {message}")
}

fn overflowed(response: &Response) -> bool {
    let value: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    let invalid = value["error"]["type"] == "invalid_request_error";
    let too_long = value["error"]["message"]
        .as_str()
        .is_some_and(|message| message.starts_with("prompt is too long"));
    response.status == 400 && invalid && too_long
}

// a request the model refused over its images, in its words: an image, or a request of many, too
// large, or a request too large in all
fn refused_pictures(response: &Response) -> bool {
    let value: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    let invalid = value["error"]["type"] == "invalid_request_error";
    let image = value["error"]["message"]
        .as_str()
        .is_some_and(|message| message.to_lowercase().contains("image"));
    (response.status == 400 && invalid && image) || response.status == 413
}

pub(crate) fn answered(response: &Response) -> Answered {
    if response.status != 200 {
        let text = failure(response.status, &response.body);
        return match (overflowed(response), refused_pictures(response)) {
            (true, _) => Answered::Overflow,
            (false, true) => Answered::Pictures(text),
            (false, false) => Answered::Failed(text),
        };
    }
    let value = match json(&response.body) {
        Ok(value) => value,
        Err(text) => return Answered::Failed(text),
    };
    if value["stop_reason"] == OVERFLOWED {
        return Answered::Overflow;
    }
    match reply(&value) {
        Ok(reply) => Answered::Reply(reply),
        Err(text) => Answered::Failed(text),
    }
}

pub(crate) fn summarized(response: &Response) -> Summarized {
    if overflowed(response) {
        return Summarized::Overflow;
    }
    if refused_pictures(response) {
        return Summarized::Pictures(failure(response.status, &response.body));
    }
    match summary(response) {
        Ok(summary) => Summarized::Summary(summary),
        Err(text) => Summarized::Failed(text),
    }
}

fn summary(response: &Response) -> Result<Summary, String> {
    if response.status != 200 {
        return Err(failure(response.status, &response.body));
    }
    let value = json(&response.body)?;
    let stopped = value["stop_reason"].as_str().unwrap_or_default();
    if stopped != "compaction" {
        return Err(format!(
            "the model did not summarize the thread: it stopped with {stopped}"
        ));
    }
    let block = &value["content"][0];
    let summarized = block["type"] == "compaction" && block["content"].is_string();
    let Some(fields) = block.as_object().filter(|_| summarized) else {
        return Err("the model answered no summary".to_owned());
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
    Ok(Summary {
        block: Value::Object(present).to_string(),
        tokens,
    })
}

pub(crate) fn window(response: &Response) -> Result<Option<u64>, String> {
    if response.status != 200 {
        return Err(failure(response.status, &response.body));
    }
    let value = json(&response.body)?;
    let tokens = value["max_input_tokens"].as_u64();
    Ok(tokens.filter(|tokens| *tokens > 0))
}

pub(crate) fn failure(status: u16, body: &[u8]) -> String {
    let value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let kind = value["error"]["type"].as_str();
    let message = value["error"]["message"].as_str();
    match (kind, message) {
        (Some(kind), Some(message)) => format!("the model answered {status}, {kind}: {message}"),
        _ => format!("the model answered {status}"),
    }
}
