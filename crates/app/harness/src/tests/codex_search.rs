//! Codex search evidence through the harness turn, persistence and unpinned continuation.
use super::accounts::{answer_headers, from_node, harness_wired, model_read_on, turn_open};
use super::*;
use model_wire::{Block, Event, Lane, Message, Stop, codex};

const GPT: &str = "gpt-test";

#[test]
fn codex_search_evidence_survives_the_actual_harness_continuation() {
    let world = world();
    let chain = &world.chain;
    harness_wired(&world);
    chain
        .submit(
            signed(ADA),
            PROGRAM,
            &Op::SetRoute {
                provider: codex::PROVIDER.into(),
                services: vec![RouteService {
                    service: SERVICE.into(),
                    wire: Wire::Harness,
                }],
            },
        )
        .unwrap();
    let config = Config {
        model: GPT.into(),
        server_tools: vec![ServerTool::WebSearch],
        ..scout()
    };
    let pin = Pin {
        provider: Some(codex::PROVIDER.into()),
        service: None,
    };
    commit(chain, ADA, CONFIG, &config.write(&pin));
    let thread = world.mention("search-1", "Search for ducktape");
    chain.at(2, 0);
    model_read_on(chain, SERVICE, GPT, codex::PROVIDER);
    let (row, _) = turn_open(chain, thread);
    let first: model_wire::Request = serde_json::from_value(body(chain, &row)).unwrap();
    assert_eq!(first.server_tools, vec![model_wire::ServerTool::WebSearch]);
    assert!(
        codex::body(&first)["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["type"] == "web_search")
    );
    let item = json!({"type":"web_search_call", "id":"ws_1", "status":"completed",
        "action":{"type":"search", "queries":["ducktape"]}, "encrypted_content":"secret-search-state"});
    let citation = json!({"type":"url_citation", "title":"Ducktape source", "url":"https://ducktape.example/source",
        "start_index":0, "end_index":6});
    let reply = model_wire::Reply {
        model: GPT.into(),
        lane: Lane::of(codex::PROVIDER, GPT),
        said: vec![
            Block::ServerToolResult {
                sealed: item.to_string(),
            },
            Block::Text {
                text: "Result".into(),
                citations: vec![citation.to_string()],
            },
        ],
        stop: Stop::EndTurn,
        usage: model_wire::Usage {
            input: 10,
            output: 5,
            cache_read: 0,
            cache_write: 0,
        },
        prompt: None,
        context: None,
    };
    chain.at(3, 0);
    from_node(
        chain,
        &egress::Op::Events(vec![egress::Event {
            id: row.id,
            seq: 0,
            data: model_wire::sse(&Event::Reply(reply)).into_bytes(),
        }]),
    );
    from_node(
        chain,
        &egress::Op::EndStream {
            id: row.id,
            events: 1,
            end: egress::StreamEnd::Complete {
                status: 200,
                headers: answer_headers("a", "0.1", "allowed"),
            },
        },
    );
    assert!(
        matches!(frames(chain, thread).pop(), Some(Body::Turn { said, .. })
        if matches!(&said[..], [Said::ServerToolResult(_), Said::Cited { .. }]))
    );
    let Reply::Answerers(answerers) = chain
        .query(
            PROGRAM,
            &Query::Answerers {
                thread,
                page: PageRequest::default(),
            },
        )
        .unwrap()
    else {
        panic!("answerers")
    };
    assert!(answerers.items.iter().any(|answerer| answerer.service == SERVICE
        && answerer.account.as_deref() == Some("a")));
    world.mention("search-2", "Continue from that source");
    let (next, _) = turn_open(chain, thread);
    let asked: model_wire::Request = serde_json::from_value(body(chain, &next)).unwrap();
    assert_eq!(asked.target, model_wire::Target::Unpinned);
    let history = asked
        .messages
        .iter()
        .filter_map(|message| match message {
            Message::Assistant { lane, blocks } => {
                assert_eq!(lane.provider, codex::PROVIDER);
                Some(blocks)
            }
            _ => None,
        })
        .flatten()
        .map(|block| match block {
            Block::Text { text, citations } => {
                assert!(citations.is_empty());
                text.as_str()
            }
            _ => panic!("foreign evidence must be portable text: {block:?}"),
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(history.contains("ducktape"), "{history}");
    assert!(history.contains("Ducktape source"), "{history}");
    assert!(
        history.contains("https://ducktape.example/source"),
        "{history}"
    );
    assert!(!history.contains("secret-search-state"), "{history}");
    for sent in [
        codex::body(&asked),
        model_wire::anthropic::body(&asked, model_wire::anthropic::Ask::Turn),
    ] {
        let sent = sent.to_string();
        assert!(sent.contains("Ducktape source"));
        assert!(sent.contains("https://ducktape.example/source"));
        assert!(!sent.contains("secret-search-state"));
        assert!(!sent.contains("url_citation"));
        assert!(!sent.contains("web_search_call"));
    }
}
