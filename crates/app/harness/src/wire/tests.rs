//! Each wire's streamed reply folded event by event, against the whole reply of the same
//! content: recorded Anthropic streams of text, thinking with its signature, a tool call whose
//! input arrives in pieces, a commit whose files arrive as the model writes them, a call cut off
//! mid-input by max_tokens, and an error part way.
mod server_tools;
mod thinking;

use serde_json::{Value, json};

use super::*;
use crate::{Flow, Forming};

pub(crate) const TEXT: &str = include_str!("fixtures/text.jsonl");
pub(crate) const THINKING: &str = include_str!("fixtures/thinking.jsonl");
pub(crate) const TOOL_USE: &str = include_str!("fixtures/tool_use.jsonl");
pub(crate) const ERROR: &str = include_str!("fixtures/error.jsonl");
pub(crate) const SERVER_TOOLS: &str = include_str!("fixtures/server_tools.jsonl");
pub(crate) const FORGE_COMMIT: &str = include_str!("fixtures/forge_commit.jsonl");
pub(crate) const CUT_OFF: &str = include_str!("fixtures/cut_off.jsonl");

pub(crate) fn events(fixture: &str) -> Vec<Vec<u8>> {
    fixture
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let kind = serde_json::from_str::<Value>(line).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_owned();
            format!("event: {kind}\r\ndata: {line}").into_bytes()
        })
        .collect()
}

fn streamed(fixture: &str) -> Partial {
    let mut partial = Partial::new(Wire::AnthropicMessages);
    for (seq, data) in events(fixture).iter().enumerate() {
        partial.next_seq = seq as u64 + 1;
        fold(&mut partial, data);
    }
    partial
}

fn whole(content: Value, stop: &str, output: u64) -> Reply {
    let body = json!({
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4-5",
        "content": content,
        "stop_reason": stop,
        "usage": {
            "input_tokens": 10,
            "output_tokens": output,
            "cache_creation_input_tokens": 0,
            "cache_read_input_tokens": 0
        },
    });
    let response = Response {
        status: 200,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    };
    let Answered::Reply(reply) = answered(Wire::AnthropicMessages, &response) else {
        panic!("the whole reply reads as a turn");
    };
    reply
}

fn folded_reply(fixture: &str) -> Reply {
    let Answered::Reply(reply) = folded(streamed(fixture)) else {
        panic!("the stream folds into a turn");
    };
    reply
}

#[test]
fn a_streamed_text_reply_folds_into_the_turn_its_whole_reply_reads_as() {
    let content = json!([{ "type": "text", "text": "Hello, world" }]);
    assert_eq!(folded_reply(TEXT), whole(content, "end_turn", 5));
}

#[test]
fn streamed_thinking_keeps_its_signature() {
    let content = json!([
        { "type": "thinking", "thinking": "let me check", "signature": "sig1" },
        { "type": "text", "text": "ok" },
    ]);
    assert_eq!(folded_reply(THINKING), whole(content, "end_turn", 5));
}

#[test]
fn a_tool_calls_input_forms_from_its_pieces_and_reads_as_the_whole_replys() {
    let content = json!([{
        "type": "tool_use",
        "id": "toolu_1",
        "name": "get_balance",
        "input": { "asset": "USDC" },
    }]);
    assert_eq!(folded_reply(TOOL_USE), whole(content, "tool_use", 5));
}

#[test]
fn a_tool_call_forming_shows_the_input_so_far() {
    let events = events(TOOL_USE);
    let mut partial = Partial::new(Wire::AnthropicMessages);
    for data in &events[..3] {
        fold(&mut partial, data);
    }
    assert_eq!(partial.forming, Forming::Kept { index: 0 });
    assert_eq!(
        partial.said,
        [Said::ToolUse {
            id: "toolu_1".into(),
            name: "get_balance".into(),
            input: "{\"asset\":".into(),
        }]
    );
    assert_eq!(partial.flow, Flow::Open);
}

pub(crate) fn pieces(fixture: &str) -> Vec<String> {
    fixture
        .lines()
        .filter_map(|line| {
            let event: Value = serde_json::from_str(line).ok()?;
            event["delta"]["partial_json"].as_str().map(str::to_owned)
        })
        .collect()
}

fn forming_input(partial: &Partial) -> Option<&str> {
    match (partial.forming, partial.said.last()) {
        (Forming::Kept { .. }, Some(Said::ToolUse { input, .. })) => Some(input),
        _ => None,
    }
}

#[test]
fn a_commits_files_grow_in_the_forming_call_as_each_piece_arrives() {
    let pieces = pieces(FORGE_COMMIT);
    let mut partial = Partial::new(Wire::AnthropicMessages);
    let mut seen = Vec::new();
    for data in &events(FORGE_COMMIT) {
        fold(&mut partial, data);
        if let Some(input) = forming_input(&partial) {
            seen.push(input.to_owned());
        }
    }
    let grown: Vec<String> = (0..=pieces.len())
        .map(|count| pieces[..count].concat())
        .collect();
    assert_eq!(seen, grown);
    assert!(seen[1].contains(r#""content": "pub fn "#), "{seen:?}");
    let content = json!([{
        "type": "tool_use",
        "id": "toolu_1",
        "name": "forge_commit",
        "input": serde_json::from_str::<Value>(&pieces.concat()).unwrap(),
    }]);
    assert_eq!(folded_reply(FORGE_COMMIT), whole(content, "tool_use", 60));
}

#[test]
fn a_call_cut_off_mid_input_keeps_its_input_as_written_and_the_reply_still_ends() {
    let partial = streamed(CUT_OFF);
    assert_eq!(partial.flow, Flow::Ended);
    let reply = folded_reply(CUT_OFF);
    assert_eq!(reply.stop, Stop::MaxTokens);
    assert_eq!(
        reply.said,
        [Said::ToolUse {
            id: "toolu_1".into(),
            name: "forge_commit".into(),
            input: pieces(CUT_OFF).concat(),
        }]
    );
    assert!(serde_json::from_str::<Value>(&pieces(CUT_OFF).concat()).is_err());
}

#[test]
fn an_error_event_fails_the_reply_with_what_the_model_said() {
    let partial = streamed(ERROR);
    assert_eq!(partial.said, [Said::Text("Hel".into())]);
    let Answered::Failed(text) = folded(partial) else {
        panic!("an error event fails the reply");
    };
    assert_eq!(
        text,
        "the model's reply failed as it streamed, overloaded_error: Overloaded"
    );
}

#[test]
fn a_stream_that_ends_before_message_stop_is_no_turn() {
    let events = events(TEXT);
    let mut partial = Partial::new(Wire::AnthropicMessages);
    for (seq, data) in events[..5].iter().enumerate() {
        partial.next_seq = seq as u64 + 1;
        fold(&mut partial, data);
    }
    let Answered::Failed(text) = folded(partial) else {
        panic!("an unfinished stream is no turn");
    };
    assert_eq!(
        text,
        "the model's reply ended after 5 events, before the model finished it"
    );
}

#[test]
fn a_delta_for_a_block_not_forming_breaks_the_reply() {
    let mut partial = Partial::new(Wire::AnthropicMessages);
    let delta = json!({
        "type": "content_block_delta",
        "index": 3,
        "delta": { "type": "text_delta", "text": "x" },
    });
    fold(&mut partial, format!("data: {delta}").as_bytes());
    assert_eq!(
        partial.flow,
        Flow::Broken {
            reason: "a text_delta for block 3, which is not the block forming".into()
        }
    );
}

#[test]
fn an_event_without_data_lines_changes_nothing() {
    let mut partial = Partial::new(Wire::AnthropicMessages);
    fold(&mut partial, b": keep-alive\n");
    assert_eq!(partial, Partial::new(Wire::AnthropicMessages));
}

#[test]
fn each_request_harness_makes_reads_back_as_what_it_waits_for() {
    use crate::{Awaited, Wire, attributed_as, awaited, streamed_as};
    use abi::role::egress::{Delivery, Request};
    let wire = Wire::AnthropicMessages;
    let http = |head| Request::Http {
        service: "anthropic".into(),
        head,
        body: Vec::new(),
    };
    let events = Delivery::Events {
        deliver: streamed_as(7),
    };
    let turn = http(super::turn_head(wire, true));
    assert_eq!(awaited(&turn, &events), Some(Awaited::Reply { thread: 7 }));
    let window = http(super::model_head(wire, "claude-sonnet-5"));
    let name = "claude-sonnet-5".to_owned();
    let learning = Some(Awaited::Window { name });
    assert_eq!(awaited(&window, &Delivery::Whole), learning);
    let summary = http(super::summary_head(wire));
    assert_eq!(awaited(&summary, &Delivery::Whole), Some(Awaited::Summary));
    let relay = Request::Relay {
        note: b"toolu_1".to_vec(),
    };
    let call = "toolu_1".to_owned();
    assert_eq!(
        awaited(&relay, &Delivery::Whole),
        Some(Awaited::Relay { call })
    );
    let whole_turn = http(super::turn_head(wire, false));
    assert_eq!(awaited(&whole_turn, &Delivery::Whole), None);
    let attributed = Delivery::Events {
        deliver: attributed_as(),
    };
    assert_eq!(awaited(&turn, &attributed), None);
}

fn streamed_call(tool: &str, pieces: &[&str]) -> Value {
    let started = json!({
        "type": "message_start",
        "message": {
            "id": "msg_1", "type": "message", "role": "assistant", "content": [],
            "model": "claude-sonnet-4-5", "stop_reason": null, "stop_sequence": null,
            "usage": { "input_tokens": 10, "output_tokens": 1,
                       "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0 }
        }
    });
    let block = json!({
        "type": "content_block_start", "index": 0,
        "content_block": { "type": "tool_use", "id": "toolu_1", "name": tool, "input": {} }
    });
    let mut lines = vec![started, block];
    lines.extend(pieces.iter().map(|piece| {
        json!({
            "type": "content_block_delta", "index": 0,
            "delta": { "type": "input_json_delta", "partial_json": piece }
        })
    }));
    lines.push(json!({ "type": "content_block_stop", "index": 0 }));
    let fixture: String = lines.iter().map(|line| format!("{line}\n")).collect();
    let partial = streamed(&fixture);
    let [Said::ToolUse { input, .. }] = &partial.said[..] else {
        panic!("one call: {:?}", partial.said);
    };
    serde_json::from_str(input).unwrap()
}

#[test]
fn a_streamed_call_reads_its_untyped_op_and_query_as_objects() {
    let pieces = [
        r#"{"program": "chat", "op": {"Post"#,
        r#"Message": {"channel_id": "general"}}}"#,
    ];
    let call = streamed_call("network_call", &pieces);
    let op = json!({ "PostMessage": { "channel_id": "general" } });
    assert_eq!(call["op"], op);
    let pieces = [
        r#"{"program": "chat", "query": {"Channel": "#,
        r#"{"channel_id": "general"}}}"#,
    ];
    let asked = streamed_call("network_query", &pieces);
    let query = json!({ "Channel": { "channel_id": "general" } });
    assert_eq!(asked["query"], query);
}

#[test]
fn a_streamed_untyped_field_written_as_json_text_is_read_as_the_json_it_holds() {
    let op = json!({ "PostMessage": { "channel_id": "general" } });
    let quoted = json!({ "program": "chat", "op": op.to_string() }).to_string();
    let (head, tail) = quoted.split_at(quoted.len() / 2);
    let call = streamed_call("network_call", &[head, tail]);
    assert_eq!((&call["program"], &call["op"]), (&json!("chat"), &op));
    let query = json!({ "Channel": { "channel_id": "general" } });
    let quoted = json!({ "program": "chat", "query": query.to_string() }).to_string();
    let asked = streamed_call("network_query", &[&quoted]);
    assert_eq!(asked["query"], query);
    let unit = streamed_call(
        "network_query",
        &[r#"{"program": "files", "query": "Head"}"#],
    );
    assert_eq!(
        unit["query"], "Head",
        "a variant's bare name is no JSON text"
    );
    let text = r#"{"path": "/a", "content": "{\"k\": 1}"}"#;
    let written = streamed_call("files_write", &[text]);
    assert_eq!(
        written["content"], "{\"k\": 1}",
        "a typed string stays text"
    );
}
