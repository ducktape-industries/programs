//! The harness wire (`model-wire`) to the service that runs each provider's adapter: a thread's
//! request in the same parts as any wire, each assistant message with the lane that wrote it, a
//! turn's events folded into a [`Partial`], and the reply the stream ends in taken whole.
use std::collections::BTreeMap;

use abi::role::egress::http::{Head, Method, Response};
use model_wire::{Block, Class, Event, Failure, Message, Shown, UserBlock};
use serde_json::{Map, Value, json};

use super::{Answered, Foreign, Header, Summarized, Summary, anthropic};
use crate::pictures::{Look, Looks};
use crate::{Body, Flow, Forming, Frame, Partial, Role, Said, tools};

pub(crate) type Lanes = BTreeMap<u64, model_wire::Lane>;

pub(crate) fn close(target: &model_wire::Target) -> String {
    let target = serde_json::to_string(target).expect("a target is JSON");
    format!("],\"target\":{target}}}")
}

fn json_head(method: Method, target: String) -> Head {
    let headers = match method {
        Method::Post => vec![("content-type".into(), "application/json".into())],
        Method::Get | Method::Delete | Method::Put | Method::Patch => Vec::new(),
    };
    Head {
        method,
        target,
        headers,
    }
}

pub(crate) fn turn_head() -> Head {
    json_head(Method::Post, model_wire::TURN.into())
}

pub(crate) fn summary_head() -> Head {
    json_head(Method::Post, model_wire::SUMMARY.into())
}

pub(crate) fn model(name: &str) -> Head {
    json_head(Method::Get, model_wire::model_path(name))
}

pub(crate) fn awaited(asked: &Head) -> Option<crate::Awaited> {
    if let Some(name) = asked.target.strip_prefix(model_wire::MODELS)
        && *asked == model(name)
    {
        return Some(crate::Awaited::Window {
            name: name.to_owned(),
        });
    }
    (*asked == summary_head()).then_some(crate::Awaited::Summary)
}

pub(crate) fn header(header: &Header) -> String {
    let native = tools::CATALOG.iter().map(|entry| {
        json!({
            "name": entry.name,
            "description": entry.description,
            "schema": (entry.schema)(),
        })
    });
    let written = header.file_tools.iter().map(|tool| {
        json!({
            "name": tool.name,
            "description": format!("{} (tool file {})", tool.description, tool.path),
            "schema": tool.schema(),
        })
    });
    let tools: Vec<Value> = native.chain(written).collect();
    let server_tools: Vec<&str> = header.server_tools.iter().map(|tool| tool.word()).collect();
    let mut request = Map::new();
    request.insert("model".into(), json!(header.name));
    request.insert("max_tokens".into(), json!(header.max_tokens));
    if let Some(thinking) = header.thinking {
        request.insert("thinking".into(), json!(thinking.word()));
    }
    request.insert("system".into(), json!(header.system));
    request.insert("tools".into(), Value::Array(tools));
    request.insert("server_tools".into(), json!(server_tools));
    let mut text = Value::Object(request).to_string();
    text.pop();
    text.push_str(",\"messages\":[");
    text
}

fn shown(look: &Look) -> Vec<Shown> {
    match look {
        Look::Image { label, media, data } => vec![
            Shown::Text {
                text: label.clone(),
            },
            Shown::Image {
                media: (*media).to_owned(),
                data: data.clone(),
            },
        ],
        Look::Words(text) => vec![Shown::Text { text: text.clone() }],
    }
}

fn user_block(shown: Shown) -> UserBlock {
    match shown {
        Shown::Text { text } => UserBlock::Text { text },
        Shown::Image { media, data } => UserBlock::Image { media, data },
    }
}

fn user_blocks(body: &Body, looks: &[Look]) -> Vec<UserBlock> {
    let shows = || looks.iter().flat_map(shown);
    match body {
        Body::Post { text, .. } => {
            let mut blocks = vec![UserBlock::Text { text: text.clone() }];
            blocks.extend(shows().map(user_block));
            blocks
        }
        Body::Refused { call, problem, .. } => vec![UserBlock::Result {
            call: call.clone(),
            failed: true,
            content: vec![Shown::Text {
                text: problem.clone(),
            }],
        }],
        Body::Result {
            call, text, failed, ..
        } => {
            let mut content = vec![Shown::Text { text: text.clone() }];
            content.extend(shows());
            vec![UserBlock::Result {
                call: call.clone(),
                failed: *failed,
                content,
            }]
        }
        Body::Turn { .. } | Body::Notice { .. } | Body::Compaction { .. } => Vec::new(),
    }
}

pub(crate) fn block(said: &Said) -> Block {
    match said {
        Said::Text(text) => Block::Text {
            text: text.clone(),
            citations: Vec::new(),
        },
        Said::Cited { text, citations } => Block::Text {
            text: text.clone(),
            citations: citations.clone(),
        },
        Said::Thinking { text, signature } => Block::Reasoning {
            text: text.clone(),
            sealed: (!signature.is_empty()).then(|| signature.clone()),
        },
        Said::RedactedThinking(data) => Block::Redacted {
            sealed: data.clone(),
        },
        Said::ToolUse { id, name, input } => Block::ToolUse {
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
        },
        Said::ServerToolUse { id, name, input } => Block::ServerToolUse {
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
        },
        Said::ServerToolResult(block) => Block::ServerToolResult {
            sealed: block.clone(),
        },
    }
}

fn said(block: Block) -> Option<Said> {
    match block {
        Block::Text { text, citations } if citations.is_empty() => Some(Said::Text(text)),
        Block::Text { text, citations } => Some(Said::Cited { text, citations }),
        Block::Reasoning { text, sealed } => Some(Said::Thinking {
            text,
            signature: sealed.unwrap_or_default(),
        }),
        Block::Redacted { sealed } => Some(Said::RedactedThinking(sealed)),
        Block::ToolUse { id, name, input } => {
            let input = untyped(&name, input);
            Some(Said::ToolUse { id, name, input })
        }
        Block::ServerToolUse { id, name, input } => Some(Said::ServerToolUse { id, name, input }),
        Block::ServerToolResult { sealed } => Some(Said::ServerToolResult(sealed)),
        Block::Summary { .. } => None,
    }
}

fn untyped(tool: &str, input: String) -> String {
    match serde_json::from_str::<Value>(&input) {
        Ok(mut value) => {
            tools::untyped_read(tool, &mut value);
            value.to_string()
        }
        Err(_) => input,
    }
}

fn plain(said: &Said) -> Option<Block> {
    match said {
        Said::ServerToolUse { .. } | Said::ServerToolResult(_) | Said::Cited { .. } => {
            let plain = anthropic::plain_block(said)?;
            let text = plain["text"].as_str()?.to_owned();
            Some(Block::Text {
                text,
                citations: Vec::new(),
            })
        }
        Said::Text(_)
        | Said::Thinking { .. }
        | Said::RedactedThinking(_)
        | Said::ToolUse { .. } => Some(block(said)),
    }
}

pub(crate) fn message(
    role: Role,
    frames: &[Frame],
    (looks, foreign): (&Looks, &Foreign),
    lanes: &Lanes,
    first: bool,
) -> String {
    let none: Vec<Look> = Vec::new();
    let messages: Vec<Message> = match role {
        Role::User => vec![Message::User {
            blocks: frames
                .iter()
                .flat_map(|frame| user_blocks(&frame.body, looks.get(&frame.seq).unwrap_or(&none)))
                .collect(),
        }],
        Role::Assistant => frames
            .iter()
            .filter_map(|frame| {
                let Body::Turn { said, model, .. } = &frame.body else {
                    return None;
                };
                let lane = lanes.get(&frame.seq).cloned().unwrap_or_else(|| {
                    model_wire::Lane::of(model_wire::anthropic::PROVIDER, model)
                });
                let blocks = match foreign.contains(&frame.seq) {
                    true => said.iter().filter_map(plain).collect(),
                    false => said.iter().map(block).collect(),
                };
                Some(Message::Assistant { lane, blocks })
            })
            .collect(),
    };
    let text = messages
        .iter()
        .map(|message| serde_json::to_string(message).expect("a message is JSON"))
        .collect::<Vec<_>>()
        .join(",");
    match first {
        true => text,
        false => format!(",{text}"),
    }
}

pub(crate) fn summary_message(
    lane: model_wire::Lane,
    text: &str,
    sealed: Option<String>,
) -> String {
    let summary = Block::Summary {
        text: text.to_owned(),
        sealed,
    };
    let message = Message::Assistant {
        lane,
        blocks: vec![summary],
    };
    serde_json::to_string(&message).expect("a message is JSON")
}

pub(crate) fn own_summary(block: &str) -> String {
    let summary: model_wire::Summarized = match serde_json::from_str(block) {
        Ok(summary) => summary,
        Err(_) => model_wire::Summarized {
            text: block.to_owned(),
            sealed: None,
            lane: model_wire::Lane::default(),
            tokens: 0,
        },
    };
    summary_message(summary.lane, &summary.text, summary.sealed)
}

pub(crate) fn summary_words(block: &str) -> String {
    serde_json::from_str::<model_wire::Summarized>(block)
        .map(|summary| summary.text)
        .unwrap_or_else(|_| block.to_owned())
}

fn failure(response: &Response) -> Failure {
    serde_json::from_slice(&response.body).unwrap_or_else(|_| {
        let words = format!("the model answered {}", response.status);
        Failure::new(Class::Provider, words)
    })
}

pub(crate) fn answered(response: &Response) -> Answered {
    if response.status == 200 {
        return Answered::Failed(
            "the model's service answered a turn whole, not as a stream".into(),
        );
    }
    let failure = failure(response);
    if response.status == super::TOO_LARGE {
        return Answered::TooLarge(failure.message);
    }
    match failure.class {
        Class::Overflow => Answered::Overflow,
        Class::Pictures => Answered::Pictures(failure.message),
        Class::Dropped | Class::Provider | Class::Overloaded | Class::Wire | Class::Limited => {
            Answered::Failed(failure.message)
        }
    }
}

pub(crate) fn summarized(response: &Response) -> Summarized {
    if response.status != 200 {
        let failure = failure(response);
        if response.status == super::TOO_LARGE {
            return Summarized::TooLarge(failure.message);
        }
        return match failure.class {
            Class::Overflow => Summarized::Overflow,
            Class::Pictures => Summarized::Pictures(failure.message),
            Class::Dropped | Class::Provider | Class::Overloaded | Class::Wire | Class::Limited => {
                Summarized::Failed(failure.message)
            }
        };
    }
    match serde_json::from_slice::<model_wire::Summarized>(&response.body) {
        Ok(summary) => Summarized::Summary(Summary {
            tokens: summary.tokens,
            block: serde_json::to_string(&summary).expect("a summary is JSON"),
        }),
        Err(error) => Summarized::Failed(format!("the summary is not the harness wire: {error}")),
    }
}

pub(crate) fn learned(response: &Response) -> Result<(Option<u64>, String), String> {
    if response.status != 200 {
        return Err(failure(response).message);
    }
    let model: model_wire::Model = serde_json::from_slice(&response.body)
        .map_err(|error| format!("the model's record is not the harness wire: {error}"))?;
    Ok((model.window, model.provider))
}

fn broken(partial: &mut Partial, reason: String) {
    partial.flow = Flow::Broken { reason };
}

pub(crate) fn last_pass(data: &str) -> Option<u64> {
    match serde_json::from_str::<Event>(data).ok()? {
        Event::Reply(reply) => reply.context,
        _ => None,
    }
}

pub(crate) fn fold(partial: &mut Partial, data: &str) {
    let event = match serde_json::from_str::<Event>(data) {
        Ok(event) => event,
        Err(error) => {
            let reason = format!(
                "event {} is not the harness wire: {error}",
                partial.next_seq
            );
            return broken(partial, reason);
        }
    };
    match event {
        Event::Block { index, block } => opened(partial, index, block),
        Event::Delta { index, text } => grew(partial, index, &text),
        Event::End { index } => ended(partial, index),
        Event::Reply(reply) => {
            partial.model = reply.model;
            partial.said = reply.said.into_iter().filter_map(said).collect();
            partial.forming = Forming::Nothing;
            partial.stop = Some(stop(reply.stop));
            partial.usage = crate::Usage {
                input: reply.usage.input,
                output: reply.usage.output,
                cache_read: reply.usage.cache_read,
                cache_write: reply.usage.cache_write,
            };
            partial.flow = Flow::Ended;
        }
        Event::Failed(failure) => {
            partial.flow = match failure.class {
                Class::Overflow => Flow::Overflowed,
                Class::Wire => Flow::Broken {
                    reason: failure.message,
                },
                Class::Dropped
                | Class::Provider
                | Class::Overloaded
                | Class::Pictures
                | Class::Limited => Flow::Failed {
                    text: failure.message,
                },
            }
        }
    }
}

fn stop(stop: model_wire::Stop) -> crate::Stop {
    match stop {
        model_wire::Stop::EndTurn => crate::Stop::EndTurn,
        model_wire::Stop::ToolUse => crate::Stop::ToolUse,
        model_wire::Stop::MaxTokens => crate::Stop::MaxTokens,
        model_wire::Stop::StopSequence => crate::Stop::StopSequence,
        model_wire::Stop::Pause => crate::Stop::Pause,
        model_wire::Stop::Refusal => crate::Stop::Refusal,
    }
}

fn forming(partial: &Partial, index: u64) -> bool {
    partial.forming == Forming::Kept { index } && partial.said.len() as u64 == index + 1
}

fn opened(partial: &mut Partial, index: u64, block: Block) {
    if index != partial.said.len() as u64 {
        let reason = format!(
            "block {index} opened where block {} was due",
            partial.said.len()
        );
        return broken(partial, reason);
    }
    let Some(said) = said(block) else {
        return broken(
            partial,
            format!("block {index} is a summary, which no turn says"),
        );
    };
    partial.said.push(said);
    partial.forming = Forming::Kept { index };
}

fn grew(partial: &mut Partial, index: u64, piece: &str) {
    if !forming(partial, index) {
        let reason = format!("a delta for block {index}, which is not the block forming");
        return broken(partial, reason);
    }
    let grown = match partial.said.last_mut() {
        Some(
            Said::Text(text)
            | Said::Cited { text, .. }
            | Said::Thinking { text, .. }
            | Said::ToolUse { input: text, .. }
            | Said::ServerToolUse { input: text, .. },
        ) => text,
        Some(Said::RedactedThinking(_) | Said::ServerToolResult(_)) | None => {
            let reason = format!("a delta for block {index}, which it does not fit");
            return broken(partial, reason);
        }
    };
    grown.push_str(piece);
}

fn ended(partial: &mut Partial, index: u64) {
    if !forming(partial, index) {
        let reason = format!("block {index} ended, which is not the block forming");
        return broken(partial, reason);
    }
    partial.forming = Forming::Nothing;
    if let Some(Said::ToolUse { name, input, .. }) = partial.said.last_mut() {
        let written = match input.is_empty() {
            true => "{}".to_owned(),
            false => std::mem::take(input),
        };
        *input = untyped(name, written);
    }
}
