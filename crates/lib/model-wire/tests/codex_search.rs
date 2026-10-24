//! Codex hosted search request, returned evidence and account-safe continuation fixtures.
use model_wire::{Block, Event, Lane, Message, Request, ServerTool, Stop, Target, Tool, UserBlock};
use model_wire::{anthropic, codex};
use serde_json::{Value, json};

fn request() -> Request {
    Request {
        model: "gpt-test".into(),
        tools: vec![Tool {
            name: "files_read".into(),
            description: "Read".into(),
            schema: json!({"type":"object"}),
        }],
        server_tools: vec![ServerTool::WebSearch, ServerTool::WebFetch],
        target: Target::Pinned {
            service: "codex".into(),
            account: "a".into(),
        },
        ..Request::default()
    }
}

fn lane() -> Lane {
    Lane {
        provider: codex::PROVIDER.into(),
        model: "gpt-test".into(),
        service: Some("codex".into()),
        account: Some("a".into()),
    }
}

fn reply(items: &[Value]) -> model_wire::Reply {
    let mut fold = codex::Fold::new("gpt-test");
    let mut events = Vec::new();
    for (index, item) in items.iter().enumerate() {
        events.extend(
            fold.event(
                &json!({
                    "type":"response.output_item.added", "output_index":index,
                    "item":item,
                })
                .to_string(),
            ),
        );
        events.extend(
            fold.event(
                &json!({
                    "type":"response.output_item.done", "output_index":index,
                    "item":item,
                })
                .to_string(),
            ),
        );
    }
    events.extend(
        fold.event(
            &json!({
                "type":"response.completed",
                "response":{"model":"gpt-test","output":items},
            })
            .to_string(),
        ),
    );
    assert!(fold.end().is_none());
    assert!(!events.iter().any(|event| matches!(event, Event::Failed(_))));
    events
        .into_iter()
        .find_map(|event| match event {
            Event::Reply(reply) => Some(reply),
            _ => None,
        })
        .unwrap()
}

fn search(action: Value) -> Value {
    json!({"type":"web_search_call", "id":"ws_1", "status":"completed",
        "action":action, "encrypted_content":"private-search-state"})
}

fn cited() -> Value {
    json!({"type":"message", "role":"assistant", "content":[{
        "type":"output_text", "text":"Result", "annotations":[{
            "type":"url_citation", "start_index":0, "end_index":6,
            "title":"Source title", "url":"https://example.org/source"
        }]
    }]})
}

#[test]
fn search_is_hosted_beside_ordinary_functions_and_only_when_requested() {
    let mut request = request();
    let body = codex::body(&request);
    assert_eq!(
        body["tools"],
        json!([
            {"type":"function", "name":"files_read", "description":"Read",
             "parameters":{"type":"object"}, "strict":false},
            {"type":"web_search"}
        ])
    );
    request.server_tools = vec![ServerTool::WebFetch];
    assert_eq!(codex::body(&request)["tools"], json!([body["tools"][0]]));
    request.server_tools.clear();
    assert_eq!(codex::body(&request)["tools"], json!([body["tools"][0]]));
}

#[test]
fn hosted_search_returns_citations_and_replays_whole_before_continuation() {
    let item = search(json!({"type":"search", "query":"news", "queries":["news"]}));
    let message = cited();
    let reply = reply(&[item.clone(), message.clone()]);
    assert_eq!(reply.stop, Stop::EndTurn);
    assert_eq!(
        reply.said[0],
        Block::ServerToolResult {
            sealed: item.to_string()
        }
    );
    assert_eq!(
        reply.said[1],
        Block::Text {
            text: "Result".into(),
            citations: vec![message["content"][0]["annotations"][0].to_string()],
        }
    );
    let mut request = request();
    request.messages = vec![
        Message::Assistant {
            lane: lane(),
            blocks: reply.said,
        },
        Message::User {
            blocks: vec![UserBlock::Text {
                text: "Continue from the source".into(),
            }],
        },
    ];
    let body = codex::body(&request);
    assert_eq!(body["input"][0], item);
    assert_eq!(body["input"][1]["content"][0], message["content"][0]);
    assert_eq!(
        body["input"][1]["content"],
        json!([
            message["content"][0],
            {"type":"output_text", "text":"Source title https://example.org/source", "annotations":[]}
        ])
    );
    assert_eq!(
        body["input"][2]["content"][0]["text"],
        "Continue from the source"
    );
}

#[test]
fn search_actions_and_optional_metadata_survive_without_becoming_client_calls() {
    let items = vec![
        json!({"type":"web_search_call", "action":{"type":"search", "queries":["a","b"]}}),
        search(json!({"type":"open_page", "url":"https://example.org"})),
        search(json!({"type":"find_in_page", "url":"https://example.org", "pattern":"word"})),
    ];
    let reply = reply(&items);
    assert_eq!(reply.stop, Stop::EndTurn);
    let mut request = request();
    request.messages = vec![Message::Assistant {
        lane: lane(),
        blocks: reply.said,
    }];
    assert_eq!(codex::body(&request)["input"], json!(items));
}

#[test]
fn foreign_accounts_and_providers_keep_portable_evidence_without_native_seals() {
    let reply = reply(&[search(json!({"type":"search", "query":"news"})), cited()]);
    let mut request = request();
    request.messages = vec![Message::Assistant {
        lane: lane(),
        blocks: reply.said,
    }];
    for target in [
        Target::Unpinned,
        Target::Pinned {
            service: "codex".into(),
            account: "b".into(),
        },
    ] {
        request.target = target;
        for body in [
            codex::body(&request),
            anthropic::body(&request, anthropic::Ask::Turn),
        ] {
            let wire = body.to_string();
            assert!(wire.contains("news"));
            assert!(wire.contains("Source title"));
            assert!(wire.contains("https://example.org/source"));
            assert!(!wire.contains("private-search-state"));
            assert!(!wire.contains("url_citation"));
            assert!(!wire.contains("web_search_call"));
        }
    }
}

#[test]
fn citations_from_multiple_text_parts_follow_the_joined_text() {
    let mut item = cited();
    item["content"].as_array_mut().unwrap().insert(
        0,
        json!({"type":"output_text", "text":"Prefix ", "annotations":[]}),
    );
    let reply = reply(&[item]);
    let Block::Text { text, citations } = &reply.said[0] else {
        panic!("text")
    };
    assert_eq!(text, "Prefix Result");
    let citation: Value = serde_json::from_str(&citations[0]).unwrap();
    assert_eq!(citation["start_index"], 7);
    assert_eq!(citation["end_index"], 13);
    let original = json!({"type":"output_text", "text":text, "annotations":[citation]});
    let mut request = request();
    request.messages = vec![Message::Assistant {
        lane: lane(),
        blocks: reply.said,
    }];
    let body = codex::body(&request);
    assert_eq!(body["input"][0]["content"][0], original);
    assert_eq!(body["input"][0]["content"][1]["annotations"], json!([]));
}

#[test]
fn retained_sources_are_separate_deduplicated_text_without_changing_annotations() {
    let mut item = cited();
    let annotation = item["content"][0]["annotations"][0].clone();
    item["content"][0]["annotations"] = json!([
        annotation, annotation,
        {"type":"url_citation", "title":"No address"},
        {"type":"url_citation", "url":"https://example.org/other"}
    ]);
    let reply = reply(&[item.clone()]);
    let mut request = request();
    request.messages = vec![Message::Assistant {
        lane: lane(),
        blocks: reply.said,
    }];
    assert_eq!(
        codex::body(&request)["input"][0]["content"],
        json!([
            item["content"][0],
            {"type":"output_text", "text":"Source title https://example.org/source\nhttps://example.org/other", "annotations":[]}
        ])
    );
}

#[test]
fn uncited_text_and_unpinned_portable_sources_keep_their_existing_wire_shape() {
    let mut request = request();
    request.messages = vec![Message::Assistant {
        lane: lane(),
        blocks: vec![Block::Text {
            text: "Uncited".into(),
            citations: vec![],
        }],
    }];
    assert_eq!(
        codex::body(&request)["input"],
        json!([{"type":"message", "role":"assistant", "content":[
            {"type":"output_text", "text":"Uncited", "annotations":[]}
        ]}])
    );
    let reply = reply(&[cited()]);
    request.messages = vec![Message::Assistant {
        lane: lane(),
        blocks: reply.said,
    }];
    request.target = Target::Unpinned;
    assert_eq!(
        codex::body(&request)["input"],
        json!([{"type":"message", "role":"assistant", "content":[
            {"type":"output_text", "text":"Result\nSource title https://example.org/source", "annotations":[]}
        ]}])
    );
}

#[test]
fn ordinary_function_calls_still_end_as_client_tool_use_beside_search() {
    let call =
        json!({"type":"function_call", "call_id":"call_1", "name":"files_read", "arguments":"{}"});
    let reply = reply(&[search(json!({"type":"search", "query":"news"})), call]);
    assert_eq!(reply.stop, Stop::ToolUse);
    assert!(matches!(reply.said[1], Block::ToolUse { .. }));
}
