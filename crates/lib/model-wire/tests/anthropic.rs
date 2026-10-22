//! The Anthropic adapter against the streams harness's own Anthropic wire was recorded from: each
//! folds into the reply harness reads today, a request goes out as harness writes it today, and what
//! another model or provider sealed is left out or goes as its words.
use model_wire::anthropic::{self, Ask, Fold};
use model_wire::budget::{Budget, Window};
use model_wire::{
    Block, Class, Event, Failure, Lane, Message, Pictures, Reply, Request, Shown, Stop, Thinking,
    Tool, Usage, UserBlock,
};
use serde_json::{Value, json};

const TEXT: &str = include_str!("../../../app/harness/src/wire/fixtures/text.jsonl");
const THINKING: &str = include_str!("../../../app/harness/src/wire/fixtures/thinking.jsonl");
const TOOL_USE: &str = include_str!("../../../app/harness/src/wire/fixtures/tool_use.jsonl");
const ERROR: &str = include_str!("../../../app/harness/src/wire/fixtures/error.jsonl");
const SERVER_TOOLS: &str =
    include_str!("../../../app/harness/src/wire/fixtures/server_tools.jsonl");
const CUT_OFF: &str = include_str!("../../../app/harness/src/wire/fixtures/cut_off.jsonl");
const MODEL: &str = "claude-sonnet-4-5";

fn folded(fixture: &str) -> Vec<Event> {
    let mut fold = Fold::new(MODEL);
    let mut events: Vec<Event> = fixture
        .lines()
        .filter(|line| !line.is_empty())
        .flat_map(|line| fold.event(line))
        .collect();
    events.extend(fold.end());
    events
}

fn reply(fixture: &str) -> Reply {
    match folded(fixture).pop() {
        Some(Event::Reply(reply)) => reply,
        other => panic!("the stream ends in a reply: {other:?}"),
    }
}

fn lane() -> Lane {
    Lane::of(anthropic::PROVIDER, MODEL)
}

fn usage(output: u64) -> Usage {
    Usage {
        input: 10,
        output,
        cache_read: 0,
        cache_write: 0,
    }
}

fn text(text: &str) -> Block {
    Block::Text {
        text: text.into(),
        citations: Vec::new(),
    }
}

#[test]
fn a_text_reply_streams_its_pieces_and_ends_in_the_whole_turn() {
    let events = folded(TEXT);
    let streamed = &events[..events.len() - 1];
    let expected = [
        Event::Block {
            index: 0,
            block: text(""),
        },
        Event::Delta {
            index: 0,
            text: "Hello".into(),
        },
        Event::Delta {
            index: 0,
            text: ", world".into(),
        },
        Event::End { index: 0 },
    ];
    assert_eq!(streamed, expected);
    let whole = Reply {
        model: MODEL.into(),
        lane: lane(),
        said: vec![text("Hello, world")],
        stop: Stop::EndTurn,
        usage: usage(5),
        prompt: Some(10),
        context: None,
    };
    assert_eq!(reply(TEXT), whole);
}

#[test]
fn a_reply_that_ran_server_tools_names_the_context_its_last_pass_left() {
    let delta = json!({
        "type": "message_delta",
        "delta": { "stop_reason": "end_turn", "stop_sequence": null },
        "usage": {
            "input_tokens": 21006,
            "output_tokens": 316,
            "iterations": [
                { "type": "message", "input_tokens": 2824, "output_tokens": 104 },
                {
                    "type": "message",
                    "input_tokens": 18182,
                    "output_tokens": 212,
                    "cache_read_input_tokens": 0,
                    "cache_creation_input_tokens": 0
                }
            ]
        }
    });
    let last = TEXT.lines().nth(6).unwrap();
    let fixture = TEXT.replace(last, &delta.to_string());
    let reply = reply(&fixture);
    assert_eq!(reply.context, Some(18182 + 212));
    assert_eq!(
        (reply.usage.input, reply.usage.output),
        (21006, 316),
        "usage stays what the reply cost"
    );
}

#[test]
fn thinking_keeps_its_signature_in_the_reply_and_never_streams_it() {
    let events = folded(THINKING);
    let opened = Block::Reasoning {
        text: String::new(),
        sealed: None,
    };
    assert_eq!(
        events[0],
        Event::Block {
            index: 0,
            block: opened
        }
    );
    let streamed = serde_json::to_string(&events[..events.len() - 1]).unwrap();
    assert!(!streamed.contains("sig1"), "{streamed}");
    let thought = Block::Reasoning {
        text: "let me check".into(),
        sealed: Some("sig1".into()),
    };
    assert_eq!(reply(THINKING).said, [thought, text("ok")]);
}

#[test]
fn a_tool_calls_input_streams_in_pieces_and_ends_whole() {
    let events = folded(TOOL_USE);
    let pieces: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            Event::Delta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(pieces, ["{\"asset\":", "\"USDC\"}"]);
    let reply = reply(TOOL_USE);
    assert_eq!(reply.stop, Stop::ToolUse);
    let call = Block::ToolUse {
        id: "toolu_1".into(),
        name: "get_balance".into(),
        input: json!({ "asset": "USDC" }).to_string(),
    };
    assert_eq!(reply.said, [call]);
}

#[test]
fn a_call_cut_off_keeps_its_input_as_written() {
    let reply = reply(CUT_OFF);
    assert_eq!(reply.stop, Stop::MaxTokens);
    let [Block::ToolUse { input, .. }] = &reply.said[..] else {
        panic!("one call: {:?}", reply.said);
    };
    assert!(input.ends_with("pub fn "), "{input}");
    assert!(serde_json::from_str::<Value>(input).is_err());
}

#[test]
fn searches_fetches_and_citations_come_back_sealed() {
    let said = reply(SERVER_TOOLS).said;
    let [
        Block::ServerToolUse { name, input, .. },
        Block::ServerToolResult { sealed },
        Block::ServerToolUse { .. },
        Block::ServerToolResult { .. },
        Block::Text { text, citations },
    ] = &said[..]
    else {
        panic!("a search, a fetch and cited text: {said:?}");
    };
    assert_eq!(
        (name.as_str(), input.as_str()),
        ("web_search", r#"{"query":"ducktape"}"#)
    );
    let result: Value = serde_json::from_str(sealed).unwrap();
    assert_eq!(result["type"], "web_search_tool_result");
    assert_eq!(text, "Ducktape is a network.");
    let citation: Value = serde_json::from_str(&citations[0]).unwrap();
    assert_eq!(citation["encrypted_index"], "Eo8BCioIAhgB");
}

#[test]
fn an_error_part_way_fails_the_stream_as_overloaded() {
    let failed = Failure::new(
        Class::Overloaded,
        "the model's reply failed as it streamed, overloaded_error: Overloaded",
    );
    assert_eq!(folded(ERROR).pop(), Some(Event::Failed(failed)));
}

#[test]
fn a_stream_that_ends_before_message_stop_is_dropped() {
    let mut fold = Fold::new(MODEL);
    for line in TEXT.lines().take(5) {
        fold.event(line);
    }
    let Some(Event::Failed(failure)) = fold.end() else {
        panic!("an unfinished stream fails");
    };
    assert_eq!(failure.class, Class::Dropped);
    assert_eq!(
        failure.message,
        "the model's reply ended after 5 events, before the model finished it"
    );
}

#[test]
fn a_delta_for_a_block_not_forming_breaks_the_stream() {
    let delta = json!({
        "type": "content_block_delta",
        "index": 3,
        "delta": { "type": "text_delta", "text": "x" },
    });
    let mut fold = Fold::new(MODEL);
    let events = fold.event(&delta.to_string());
    let broke = "a text_delta for block 3, which is not the block forming";
    assert_eq!(events, [Event::Failed(Failure::new(Class::Wire, broke))]);
    assert!(fold.end().is_none(), "the failure ended it");
}

#[test]
fn an_overflow_mid_reply_fails_as_overflow() {
    let lines = [
        json!({ "type": "message_start", "message": { "model": MODEL, "usage": {} } }),
        json!({ "type": "message_delta", "delta": { "stop_reason": "model_context_window_exceeded" }, "usage": {} }),
        json!({ "type": "message_stop" }),
    ];
    let mut fold = Fold::new(MODEL);
    let events: Vec<Event> = lines
        .iter()
        .flat_map(|line| fold.event(&line.to_string()))
        .collect();
    let [Event::Failed(failure)] = &events[..] else {
        panic!("overflow: {events:?}");
    };
    assert_eq!(failure.class, Class::Overflow);
}

fn request(messages: Vec<Message>) -> Request {
    Request {
        model: MODEL.into(),
        max_tokens: 1024,
        thinking: Some(Thinking::High),
        system: vec!["You keep notes.".into(), "You are tech.".into()],
        tools: vec![Tool {
            name: "files_read".into(),
            description: "Read a file.".into(),
            schema: json!({ "type": "object" }),
        }],
        server_tools: Vec::new(),
        messages,
        ..Request::default()
    }
}

fn assistant(lane: Lane, blocks: Vec<Block>) -> Message {
    Message::Assistant { lane, blocks }
}

fn said() -> Vec<Block> {
    vec![
        Block::Reasoning {
            text: "let me check".into(),
            sealed: Some("sig1".into()),
        },
        Block::Redacted {
            sealed: "opaque".into(),
        },
        Block::ServerToolUse {
            id: "srvtoolu_1".into(),
            name: "web_search".into(),
            input: r#"{"query":"x"}"#.into(),
        },
        Block::ServerToolResult {
            sealed: json!({ "type": "web_search_tool_result", "tool_use_id": "srvtoolu_1" })
                .to_string(),
        },
        Block::Text {
            text: "cited".into(),
            citations: vec![json!({ "encrypted_index": "e" }).to_string()],
        },
        Block::ToolUse {
            id: "toolu_1".into(),
            name: "files_read".into(),
            input: r#"{"path":"/a"}"#.into(),
        },
    ]
}

fn user() -> Message {
    Message::User {
        blocks: vec![
            UserBlock::Result {
                call: "toolu_1".into(),
                failed: false,
                content: vec![Shown::Text {
                    text: "read".into(),
                }],
            },
            UserBlock::Text {
                text: "next".into(),
            },
        ],
    }
}

#[test]
fn a_turn_goes_out_as_harness_writes_it_today() {
    let body = anthropic::body(&request(vec![assistant(lane(), said()), user()]), Ask::Turn);
    let expected = json!({
        "model": MODEL,
        "max_tokens": 1024,
        "thinking": { "type": "adaptive" },
        "output_config": { "effort": "high" },
        "tools": [{
            "name": "files_read",
            "description": "Read a file.",
            "input_schema": { "type": "object" },
            "eager_input_streaming": true,
            "cache_control": { "type": "ephemeral" },
        }],
        "system": [
            { "type": "text", "text": "You keep notes." },
            { "type": "text", "text": "You are tech.", "cache_control": { "type": "ephemeral" } },
        ],
        "messages": [
            {
                "role": "assistant",
                "content": [
                    { "type": "thinking", "thinking": "let me check", "signature": "sig1" },
                    { "type": "redacted_thinking", "data": "opaque" },
                    { "type": "server_tool_use", "id": "srvtoolu_1", "name": "web_search", "input": { "query": "x" } },
                    { "type": "web_search_tool_result", "tool_use_id": "srvtoolu_1" },
                    { "type": "text", "text": "cited", "citations": [{ "encrypted_index": "e" }] },
                    { "type": "tool_use", "id": "toolu_1", "name": "files_read", "input": { "path": "/a" } },
                ],
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "toolu_1", "is_error": false, "content": "read" },
                    { "type": "text", "text": "next", "cache_control": { "type": "ephemeral" } },
                ],
            },
        ],
        "stream": true,
    });
    assert_eq!(body, expected);
}

#[test]
fn another_models_reasoning_is_left_out_and_another_providers_seals_go_as_words() {
    let other_model = Lane::of(anthropic::PROVIDER, "claude-opus-5");
    let body = anthropic::body(&request(vec![assistant(other_model, said())]), Ask::Summary);
    let kinds: Vec<&str> = body["messages"][0]["content"]
        .as_array()
        .unwrap()
        .iter()
        .map(|block| block["type"].as_str().unwrap())
        .collect();
    let expected = [
        "server_tool_use",
        "web_search_tool_result",
        "text",
        "tool_use",
    ];
    assert_eq!(kinds, expected);
    assert!(
        !body.to_string().contains("let me check"),
        "another model's reasoning never goes as words"
    );
    assert_eq!(body["compaction"], json!({ "type": "summarize" }));
    assert!(body.get("stream").is_none());

    let openai = Lane::of("openai", "gpt-x");
    let body = anthropic::body(&request(vec![assistant(openai, said())]), Ask::Turn);
    let content = body["messages"][0]["content"].as_array().unwrap().clone();
    let words: Vec<Value> = content
        .iter()
        .map(|block| json!([block["type"], block["text"]]))
        .collect();
    assert_eq!(words, [json!(["text", "cited"]), json!(["tool_use", null])]);
    assert!(content[0].get("citations").is_none());
}

#[test]
fn a_refusal_reads_as_the_class_harness_acts_on() {
    let error = |kind: &str, message: &str| {
        json!({ "type": "error", "error": { "type": kind, "message": message } }).to_string()
    };
    let classes = [
        (
            400,
            error("invalid_request_error", "prompt is too long: 1 > 0"),
            Class::Overflow,
        ),
        (
            400,
            error("invalid_request_error", "image exceeds 5 MB"),
            Class::Pictures,
        ),
        (
            413,
            error("request_too_large", "too large"),
            Class::Pictures,
        ),
        (400, error("invalid_request_error", "bad"), Class::Provider),
        (
            529,
            error("overloaded_error", "Overloaded"),
            Class::Overloaded,
        ),
    ];
    for (status, body, class) in classes {
        let failure = anthropic::refused(status, &[], body.as_bytes(), 0);
        assert_eq!(failure.class, class, "{body}");
    }
    let headers = [
        ("retry-after".to_owned(), "30".to_owned()),
        (
            "anthropic-ratelimit-unified-5h-reset".to_owned(),
            "5000".to_owned(),
        ),
    ];
    let limited = anthropic::refused(
        429,
        &headers,
        error("rate_limit_error", "x").as_bytes(),
        1000,
    );
    assert_eq!((limited.class, limited.reset), (Class::Limited, Some(5000)));
    assert_eq!(
        limited.message,
        "the model answered 429, rate_limit_error: x"
    );
}

#[test]
fn a_summary_reads_back_its_compaction_block_and_its_words() {
    let answer = json!({
        "stop_reason": "compaction",
        "content": [{ "type": "compaction", "content": "We did things.", "encrypted": null }],
        "usage": { "iterations": [{ "type": "compaction", "output_tokens": 42 }] },
    });
    let summary = anthropic::summarized(MODEL, 200, &[], answer.to_string().as_bytes(), 0).unwrap();
    assert_eq!(summary.lane, lane());
    assert_eq!(summary.text, "We did things.");
    assert_eq!(summary.tokens, 42);
    let sealed = json!({ "type": "compaction", "content": "We did things." }).to_string();
    assert_eq!(summary.sealed, Some(sealed));
}

#[test]
fn a_turn_after_this_providers_compaction_asks_under_the_compaction_beta() {
    let summary = |provider: &str| Message::Assistant {
        lane: Lane::of(provider, MODEL),
        blocks: vec![Block::Summary {
            text: "so far".into(),
            sealed: Some(r#"{"type":"compaction","content":"so far"}"#.into()),
        }],
    };
    let hi = Message::User {
        blocks: vec![UserBlock::Text { text: "hi".into() }],
    };
    let request = |messages: Vec<Message>| Request {
        model: MODEL.into(),
        max_tokens: 1024,
        messages,
        ..Request::default()
    };
    let beta = |request: &Request, ask: Ask| {
        let headers = anthropic::headers(request, ask);
        let found = headers
            .into_iter()
            .find(|(name, _)| name == "anthropic-beta");
        found.map(|(_, value)| value)
    };
    let beta_named = Some(anthropic::COMPACTION_BETA.to_owned());
    let fresh = request(vec![hi.clone()]);
    assert_eq!(beta(&fresh, Ask::Turn), None);
    assert_eq!(beta(&fresh, Ask::Summary), beta_named);
    let summarized = request(vec![summary(anthropic::PROVIDER), hi.clone()]);
    let body = anthropic::body(&summarized, Ask::Turn);
    assert_eq!(body["messages"][0]["content"][0]["type"], "compaction");
    assert_eq!(beta(&summarized, Ask::Turn), beta_named);
    let foreign = request(vec![summary("openai"), hi]);
    let body = anthropic::body(&foreign, Ask::Turn);
    assert_eq!(body["messages"][0]["content"][0]["type"], "text");
    assert_eq!(beta(&foreign, Ask::Turn), None);
    assert_eq!(
        anthropic::model_headers(),
        [(
            "anthropic-version".to_owned(),
            anthropic::VERSION.to_owned()
        )]
    );
}

#[test]
fn thinking_off_asks_no_thinking_at_all() {
    let mut off = request(vec![user()]);
    off.thinking = Some(Thinking::Off);
    let body = anthropic::body(&off, Ask::Turn);
    assert!(body.get("thinking").is_none(), "{body}");
    assert!(body.get("output_config").is_none(), "{body}");
}

#[test]
fn a_models_record_carries_anthropics_picture_limits() {
    let body = json!({ "max_input_tokens": 1000000 }).to_string();
    let model = anthropic::model(200, &[], body.as_bytes(), 0).unwrap();
    let pictures = Pictures {
        most_base64: 5 * 1024 * 1024,
        most_side: 8000,
    };
    assert_eq!(
        (model.window, model.pictures),
        (Some(1_000_000), Some(pictures))
    );
}

#[test]
fn the_unified_windows_read_as_the_harness_budget() {
    let headers: Vec<(String, String)> = [
        ("anthropic-ratelimit-unified-5h-utilization", "0.53"),
        ("anthropic-ratelimit-unified-5h-reset", "1792362000"),
        ("anthropic-ratelimit-unified-5h-status", "allowed"),
        ("anthropic-ratelimit-unified-7d-utilization", "0.27"),
        ("anthropic-ratelimit-unified-7d-reset", "1793048400"),
        ("anthropic-ratelimit-unified-1d-utilization", "0.1"),
        ("anthropic-ratelimit-unified-status", "allowed_warning"),
        ("anthropic-organization-id", "org-a"),
    ]
    .iter()
    .map(|(name, value)| (name.to_string(), value.to_string()))
    .collect();
    let budget = anthropic::budget(&headers);
    let expected = Budget {
        windows: vec![
            Window {
                name: "5h".into(),
                used: "0.53".into(),
                reset: 1792362000,
                status: Some("allowed".into()),
            },
            Window {
                name: "7d".into(),
                used: "0.27".into(),
                reset: 1793048400,
                status: None,
            },
        ],
        status: Some("allowed_warning".into()),
        account: Some("org-a".into()),
        identity: None,
        pool: None,
    };
    assert_eq!(budget, expected);
    assert_eq!(Budget::read(&budget.headers()), expected);
    assert_eq!(
        model_wire::budget_of(anthropic::PROVIDER, &headers),
        expected
    );
}

#[test]
fn a_refusal_that_is_no_json_names_its_page() {
    let page = b"<!DOCTYPE html>\n<html><head>\n<title>Just a\n  moment...</title></head><body>challenge</body></html>";
    let failure = anthropic::refused(403, &[], page, 0);
    assert_eq!(failure.message, "the model answered 403: Just a moment...");
    let plain = anthropic::refused(502, &[], b"\n  upstream connect error\nmore", 0);
    assert_eq!(plain.message, "the model answered 502");
    let long = format!("<html><body>{}</body></html>", "x".repeat(1 << 20));
    let long = anthropic::refused(403, &[], long.as_bytes(), 0);
    assert_eq!(long.message, "the model answered 403");
    assert_eq!(
        anthropic::refused(403, &[], b"", 0).message,
        "the model answered 403"
    );
    assert_eq!(
        anthropic::refused(403, &[], b"{}", 0).message,
        "the model answered 403"
    );
}
