//! The Codex adapter against streams in the shape of OpenAI's documented Responses events (no live
//! Codex stream is recorded yet): text, a function call, reasoning summaries, usage with a cached
//! share, a refusal, an incomplete reply, a failure, a stream cut short, and a request under the surface's rules.
use model_wire::budget::{Budget, Window};
use model_wire::codex::{self, Fold};
use model_wire::{
    Block, Class, Event, Lane, Message, Reply, Request, Shown, Stop, Target, Thinking, Tool, Usage,
    UserBlock,
};
use serde_json::{Value, json};

const MODEL: &str = "gpt-test";

fn lines(events: &[Value]) -> Vec<String> {
    events.iter().map(Value::to_string).collect()
}

fn folded(events: &[Value]) -> Vec<Event> {
    let mut fold = Fold::new(MODEL);
    let mut out: Vec<Event> = lines(events)
        .iter()
        .flat_map(|line| fold.event(line))
        .collect();
    out.extend(fold.end());
    out
}

fn created() -> Value {
    json!({ "type": "response.created", "response": { "id": "resp_1", "model": MODEL, "status": "in_progress" } })
}

fn usage(input: u64, cached: u64, output: u64) -> Value {
    json!({
        "input_tokens": input,
        "input_tokens_details": { "cached_tokens": cached },
        "output_tokens": output,
        "output_tokens_details": { "reasoning_tokens": 0 },
        "total_tokens": input + output,
    })
}

fn completed(output: Value, usage: Value) -> Value {
    json!({
        "type": "response.completed",
        "response": { "id": "resp_1", "model": MODEL, "status": "completed", "output": output, "usage": usage },
    })
}

fn message_item(text: &str) -> Value {
    json!({
        "type": "message", "id": "msg_1", "role": "assistant", "status": "completed",
        "content": [{ "type": "output_text", "text": text, "annotations": [] }],
    })
}

fn call_item(arguments: &str) -> Value {
    json!({
        "type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "files_read",
        "arguments": arguments, "status": "completed",
    })
}

fn text_stream() -> Vec<Value> {
    let opened = json!({ "type": "message", "id": "msg_1", "role": "assistant", "status": "in_progress", "content": [] });
    vec![
        created(),
        json!({ "type": "response.output_item.added", "output_index": 0, "item": opened }),
        json!({ "type": "response.output_text.delta", "output_index": 0, "content_index": 0, "item_id": "msg_1", "delta": "Hello" }),
        json!({ "type": "response.output_text.delta", "output_index": 0, "content_index": 0, "item_id": "msg_1", "delta": ", world" }),
        json!({ "type": "response.output_item.done", "output_index": 0, "item": message_item("Hello, world") }),
        completed(json!([message_item("Hello, world")]), usage(1000, 800, 5)),
    ]
}

fn text(text: &str) -> Block {
    Block::Text {
        text: text.into(),
        citations: Vec::new(),
    }
}

#[test]
fn a_text_reply_streams_its_pieces_and_ends_in_the_whole_turn() {
    let events = folded(&text_stream());
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
        lane: Lane::of(codex::PROVIDER, MODEL),
        said: vec![text("Hello, world")],
        stop: Stop::EndTurn,
        usage: Usage {
            input: 200,
            output: 5,
            cache_read: 800,
            cache_write: 0,
        },
        prompt: Some(1000),
        context: None,
    };
    assert_eq!(events.last(), Some(&Event::Reply(whole)));
}

#[test]
fn a_function_call_streams_its_arguments_and_ends_the_turn_as_tool_use() {
    let opened = json!({ "type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "files_read", "arguments": "", "status": "in_progress" });
    let stream = vec![
        created(),
        json!({ "type": "response.output_item.added", "output_index": 0, "item": opened }),
        json!({ "type": "response.function_call_arguments.delta", "output_index": 0, "item_id": "fc_1", "delta": "{\"path\":" }),
        json!({ "type": "response.function_call_arguments.delta", "output_index": 0, "item_id": "fc_1", "delta": "\"/a\"}" }),
        json!({ "type": "response.function_call_arguments.done", "output_index": 0, "item_id": "fc_1", "arguments": "{\"path\":\"/a\"}" }),
        json!({ "type": "response.output_item.done", "output_index": 0, "item": call_item("{\"path\":\"/a\"}") }),
        completed(json!([call_item("{\"path\":\"/a\"}")]), usage(10, 0, 5)),
    ];
    let events = folded(&stream);
    let pieces: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            Event::Delta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(pieces, ["{\"path\":", "\"/a\"}"]);
    let Some(Event::Reply(reply)) = events.last() else {
        panic!("a reply: {events:?}");
    };
    assert_eq!(reply.stop, Stop::ToolUse);
    let call = Block::ToolUse {
        id: "call_1".into(),
        name: "files_read".into(),
        input: "{\"path\":\"/a\"}".into(),
    };
    assert_eq!(reply.said, [call]);
}

#[test]
fn reasoning_summaries_stream_as_reasoning_with_their_parts_apart() {
    let opened = json!({ "type": "reasoning", "id": "rs_1", "summary": [] });
    let done = json!({
        "type": "reasoning", "id": "rs_1",
        "summary": [{ "type": "summary_text", "text": "First." }, { "type": "summary_text", "text": "Second." }],
    });
    let delta = |part: u64, text: &str| json!({ "type": "response.reasoning_summary_text.delta", "output_index": 0, "item_id": "rs_1", "summary_index": part, "delta": text });
    let stream = vec![
        created(),
        json!({ "type": "response.output_item.added", "output_index": 0, "item": opened }),
        delta(0, "First."),
        delta(1, "Second."),
        json!({ "type": "response.output_item.done", "output_index": 0, "item": done }),
        completed(json!([]), usage(10, 0, 5)),
    ];
    let events = folded(&stream);
    let streamed: String = events
        .iter()
        .filter_map(|event| match event {
            Event::Delta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(streamed, "First.\n\nSecond.");
    let Some(Event::Reply(reply)) = events.last() else {
        panic!("a reply: {events:?}");
    };
    let thought = Block::Reasoning {
        text: "First.\n\nSecond.".into(),
        sealed: None,
    };
    assert_eq!(reply.said, [thought]);
}

#[test]
fn an_incomplete_reply_stops_at_max_tokens_and_a_failure_reads_by_its_code() {
    let incomplete = json!({
        "type": "response.incomplete",
        "response": { "model": MODEL, "status": "incomplete", "incomplete_details": { "reason": "max_output_tokens" }, "usage": usage(10, 0, 5) },
    });
    let Some(Event::Reply(reply)) = folded(&[created(), incomplete]).pop() else {
        panic!("an incomplete reply still answers");
    };
    assert_eq!(reply.stop, Stop::MaxTokens);
    let failed = |code: &str| {
        json!({
            "type": "response.failed",
            "response": { "status": "failed", "error": { "code": code, "message": "no" } },
        })
    };
    let classes = [
        ("context_length_exceeded", Class::Overflow),
        ("rate_limit_exceeded", Class::Limited),
        ("server_error", Class::Overloaded),
        ("invalid_prompt", Class::Provider),
    ];
    for (code, class) in classes {
        let Some(Event::Failed(failure)) = folded(&[created(), failed(code)]).pop() else {
            panic!("{code} fails");
        };
        assert_eq!(failure.class, class, "{code}");
    }
}

#[test]
fn a_stream_cut_short_is_dropped() {
    let stream = text_stream();
    let events = folded(&stream[..4]);
    let Some(Event::Failed(failure)) = events.last() else {
        panic!("a cut stream fails");
    };
    assert_eq!(failure.class, Class::Dropped);
}

#[test]
fn a_request_goes_out_under_the_codex_surfaces_rules() {
    let request = Request {
        model: MODEL.into(),
        max_tokens: 1024,
        thinking: Some(Thinking::High),
        system: vec!["You keep notes.".into()],
        tools: vec![Tool {
            name: "files_read".into(),
            description: "Read a file.".into(),
            schema: json!({ "type": "object" }),
        }],
        server_tools: Vec::new(),
        messages: vec![
            Message::User {
                blocks: vec![UserBlock::Text { text: "hi".into() }],
            },
            Message::Assistant {
                lane: Lane::of("anthropic", "claude"),
                blocks: vec![
                    Block::Reasoning {
                        text: "thought".into(),
                        sealed: Some("sig".into()),
                    },
                    Block::ToolUse {
                        id: "call_1".into(),
                        name: "files_read".into(),
                        input: "{\"path\":\"/a\"}".into(),
                    },
                ],
            },
            Message::User {
                blocks: vec![UserBlock::Result {
                    call: "call_1".into(),
                    failed: false,
                    content: vec![Shown::Text {
                        text: "read".into(),
                    }],
                }],
            },
        ],
        cache_key: Some("thread-7".into()),
        ..Request::default()
    };
    let body = codex::body(&request);
    assert_eq!(body["instructions"], codex::INSTRUCTIONS);
    assert_eq!(body["input"][0]["content"][0]["text"], "You keep notes.");
    assert_eq!(
        (body["store"].clone(), body["stream"].clone()),
        (json!(false), json!(true))
    );
    assert!(body.get("max_output_tokens").is_none());
    assert_eq!(body["reasoning"]["effort"], "high");
    assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
    assert_eq!(body["prompt_cache_key"], "thread-7");
    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["parameters"], json!({ "type": "object" }));
    let kinds: Vec<Value> = body["input"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| json!([item["type"], item["role"]]))
        .collect();
    let expected = [
        json!(["message", "developer"]),
        json!(["message", "user"]),
        json!(["function_call", null]),
        json!(["function_call_output", null]),
    ];
    assert_eq!(kinds, expected);
    let input = &body["input"];
    assert!(
        !body.to_string().contains("thought") && !body.to_string().contains("sig"),
        "another provider's reasoning never goes out, as words or sealed"
    );
    assert_eq!(input[3]["output"], "read");
}

#[test]
fn a_refusal_reads_as_the_class_harness_acts_on() {
    let headers = [("retry-after".to_owned(), "30".to_owned())];
    let limited = codex::refused(429, &headers, br#"{"detail":"Usage limit reached"}"#, 1000);
    assert_eq!((limited.class, limited.reset), (Class::Limited, Some(1030)));
    assert_eq!(
        limited.message,
        "the model answered 429: Usage limit reached"
    );
    let overflow = br#"{"error":{"code":"context_length_exceeded","message":"too long"}}"#;
    assert_eq!(codex::refused(400, &[], overflow, 0).class, Class::Overflow);
    assert_eq!(codex::refused(400, &[], b"{}", 0).class, Class::Provider);
    let reached = br#"{"error":{"type":"usage_limit_reached","message":"limit","resets_at":5000}}"#;
    let reached = codex::refused(429, &headers, reached, 1000);
    assert_eq!((reached.class, reached.reset), (Class::Limited, Some(5000)));
}

#[test]
fn a_refusal_streams_its_words_and_ends_the_turn_refused() {
    let opened = json!({ "type": "message", "id": "msg_1", "role": "assistant", "status": "in_progress", "content": [] });
    let refused = json!({
        "type": "message", "id": "msg_1", "role": "assistant", "status": "completed",
        "content": [{ "type": "refusal", "refusal": "I can't help with that." }],
    });
    let delta = |text: &str| json!({ "type": "response.refusal.delta", "output_index": 0, "content_index": 0, "item_id": "msg_1", "delta": text });
    let streamed = vec![
        created(),
        json!({ "type": "response.output_item.added", "output_index": 0, "item": opened.clone() }),
        json!({ "type": "response.content_part.added", "output_index": 0, "content_index": 0, "item_id": "msg_1", "part": { "type": "refusal", "refusal": "" } }),
        delta("I can't"),
        delta(" help with that."),
        json!({ "type": "response.refusal.done", "output_index": 0, "content_index": 0, "item_id": "msg_1", "refusal": "I can't help with that." }),
        json!({ "type": "response.output_item.done", "output_index": 0, "item": refused.clone() }),
        completed(json!([refused.clone()]), usage(10, 0, 5)),
    ];
    let events = folded(&streamed);
    let pieces: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            Event::Delta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(pieces, ["I can't", " help with that."]);
    let Some(Event::Reply(reply)) = events.last() else {
        panic!("a reply: {events:?}");
    };
    assert_eq!(reply.stop, Stop::Refusal);
    assert_eq!(reply.said, [text("I can't help with that.")]);
    let whole_only = vec![
        created(),
        json!({ "type": "response.output_item.added", "output_index": 0, "item": opened }),
        json!({ "type": "response.output_item.done", "output_index": 0, "item": refused.clone() }),
        completed(json!([refused]), usage(10, 0, 5)),
    ];
    let Some(Event::Reply(reply)) = folded(&whole_only).pop() else {
        panic!("a reply");
    };
    assert_eq!(reply.stop, Stop::Refusal);
}

#[test]
fn reasoning_keeps_its_encrypted_item_and_goes_back_whole_to_the_same_model() {
    let done = json!({
        "type": "reasoning", "id": "rs_1", "encrypted_content": "gAAAA-sealed",
        "summary": [{ "type": "summary_text", "text": "Plan." }],
    });
    let stream = vec![
        created(),
        json!({ "type": "response.output_item.added", "output_index": 0, "item": { "type": "reasoning", "id": "rs_1", "summary": [] } }),
        json!({ "type": "response.output_item.done", "output_index": 0, "item": done.clone() }),
        json!({ "type": "response.output_item.added", "output_index": 1, "item": call_item("") }),
        json!({ "type": "response.output_item.done", "output_index": 1, "item": call_item("{}") }),
        completed(json!([done.clone(), call_item("{}")]), usage(10, 0, 5)),
    ];
    let Some(Event::Reply(reply)) = folded(&stream).pop() else {
        panic!("a reply");
    };
    let Block::Reasoning { text, sealed } = &reply.said[0] else {
        panic!("reasoning first: {:?}", reply.said);
    };
    assert_eq!(text, "Plan.");
    let sealed: Value = serde_json::from_str(sealed.as_deref().unwrap()).unwrap();
    assert_eq!(sealed, done);
    let lane = Lane {
        service: Some("zk-codex".into()),
        account: Some("codex-a".into()),
        ..reply.lane.clone()
    };
    let request = |model: &str, target: Target| Request {
        model: model.into(),
        max_tokens: 1024,
        messages: vec![Message::Assistant {
            lane: lane.clone(),
            blocks: reply.said.clone(),
        }],
        target,
        ..Request::default()
    };
    let pinned = Target::Pinned {
        service: "zk-codex".into(),
        account: "codex-a".into(),
    };
    let same = codex::body(&request(MODEL, pinned.clone()));
    assert_eq!(same["input"][0], done);
    assert_eq!(same["input"][1]["type"], "function_call");
    for (model, target) in [("gpt-other", pinned), (MODEL, Target::Unnamed)] {
        let other = codex::body(&request(model, target));
        assert_eq!(other["input"][0]["type"], "function_call");
        assert!(!other.to_string().contains("gAAAA"), "{other}");
        assert!(!other.to_string().contains("Plan."), "{other}");
    }
}

#[test]
fn the_codex_windows_read_as_the_harness_budget() {
    let headers: Vec<(String, String)> = [
        ("x-codex-primary-used-percent", "12.5"),
        ("x-codex-primary-window-minutes", "300"),
        ("x-codex-primary-reset-at", "1792524600"),
        ("x-codex-secondary-used-percent", "100"),
        ("x-codex-secondary-window-minutes", "10080"),
        ("x-codex-secondary-reset-at", "1793084311"),
        ("x-codex-other-primary-used-percent", "7"),
        ("x-codex-other-primary-window-minutes", "90"),
        ("x-codex-other-primary-reset-at", "1792520000"),
    ]
    .iter()
    .map(|(name, value)| (name.to_string(), value.to_string()))
    .collect();
    let window = |name: &str, used: &str, reset: u64| Window {
        name: name.into(),
        used: used.into(),
        reset,
        status: None,
    };
    let expected = Budget {
        windows: vec![
            window("5h", "0.125", 1792524600),
            window("7d", "1", 1793084311),
            window("codex-other_90m", "0.07", 1792520000),
        ],
        ..Budget::default()
    };
    let budget = codex::budget(&headers);
    assert_eq!(budget, expected);
    assert_eq!(Budget::read(&budget.headers()), expected);
}

#[test]
fn a_refusal_that_is_no_json_names_its_page() {
    let page = b"<!DOCTYPE html>\n<html><head>\n<title>Just a\n  moment...</title></head><body>challenge</body></html>";
    let failure = codex::refused(403, &[], page, 0);
    assert_eq!(failure.message, "the model answered 403: Just a moment...");
    let plain = codex::refused(502, &[], b"\n  upstream connect error\nmore", 0);
    assert_eq!(plain.message, "the model answered 502");
    let long = format!("<html><body>{}</body></html>", "x".repeat(1 << 20));
    let long = codex::refused(403, &[], long.as_bytes(), 0);
    assert_eq!(long.message, "the model answered 403");
    assert_eq!(
        codex::refused(403, &[], b"", 0).message,
        "the model answered 403"
    );
    assert_eq!(
        codex::refused(403, &[], b"{}", 0).message,
        "the model answered 403"
    );
}
