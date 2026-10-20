//! A recorded Anthropic stream read through the adapter service and the harness wire, against
//! the same stream read by harness's own Anthropic wire: the same turns, the same failure, and a
//! request that reaches Anthropic the same.
use ::program::{BlobId, Principal};
use model_wire::anthropic::{self as adapter, Ask, Fold};

use super::*;
use crate::{Body, Frame, ServerTool, Thinking};

const MODEL: &str = "claude-sonnet-4-5";

fn adapted(fixture: &str) -> Vec<Vec<u8>> {
    let mut fold = Fold::new(MODEL);
    let mut events: Vec<model_wire::Event> = fixture
        .lines()
        .filter(|line| !line.is_empty())
        .flat_map(|line| fold.event(line))
        .collect();
    events.extend(fold.end());
    events
        .iter()
        .map(|event| model_wire::sse(event).into_bytes())
        .collect()
}

fn read_through_the_adapter(fixture: &str) -> Answered {
    let mut partial = Partial::new(Wire::Harness);
    for (seq, data) in adapted(fixture).iter().enumerate() {
        partial.next_seq = seq as u64 + 1;
        fold(&mut partial, data);
    }
    folded(partial)
}

#[test]
fn every_recorded_stream_reads_through_the_adapter_as_anthropics_own_wire_reads_it() {
    for fixture in [
        TEXT,
        THINKING,
        TOOL_USE,
        SERVER_TOOLS,
        FORGE_COMMIT,
        CUT_OFF,
    ] {
        let Answered::Reply(own) = folded(streamed(fixture)) else {
            panic!("anthropic's own wire reads a turn");
        };
        let Answered::Reply(adapted) = read_through_the_adapter(fixture) else {
            panic!("the harness wire reads a turn");
        };
        assert_eq!(adapted, own);
    }
    let Answered::Failed(own) = folded(streamed(ERROR)) else {
        panic!("anthropic's own wire fails");
    };
    let Answered::Failed(adapted) = read_through_the_adapter(ERROR) else {
        panic!("the harness wire fails");
    };
    assert_eq!(adapted, own);
}

#[test]
fn a_stream_cut_short_fails_through_the_adapter() {
    let cut: String = TEXT
        .lines()
        .take(4)
        .map(|line| format!("{line}\n"))
        .collect();
    let Answered::Failed(text) = read_through_the_adapter(&cut) else {
        panic!("a cut stream fails");
    };
    assert_eq!(
        text,
        "the model's reply ended after 4 events, before the model finished it"
    );
}

#[test]
fn a_reply_that_ran_server_tools_reaches_the_harness_wire_with_the_context_of_its_last_pass() {
    let delta = json!({
        "type": "message_delta",
        "delta": { "stop_reason": "end_turn", "stop_sequence": null },
        "usage": {
            "input_tokens": 21006,
            "output_tokens": 316,
            "iterations": [
                { "type": "message", "input_tokens": 2824, "output_tokens": 104 },
                { "type": "message", "input_tokens": 18182, "output_tokens": 212 }
            ]
        }
    });
    let last = TEXT.lines().nth(6).unwrap();
    let fixture = TEXT.replace(last, &delta.to_string());
    let own = &events(&fixture)[6];
    let adapted = adapted(&fixture);
    let reply = adapted.last().unwrap();
    assert_eq!(last_pass(Wire::AnthropicMessages, own), Some(18394));
    assert_eq!(
        last_pass(Wire::Harness, reply),
        Some(18394),
        "the adapter's reply names what anthropic's own wire reads"
    );
}

fn frame(seq: u64, body: Body) -> Frame {
    Frame {
        seq,
        height: seq,
        time: seq,
        body,
    }
}

fn post(seq: u64, text: &str) -> Frame {
    let body = Body::Post {
        author: Principal::Account(15),
        text: text.into(),
    };
    frame(seq, body)
}

fn conversation() -> Vec<(Role, Vec<Frame>)> {
    let turn = Body::Turn {
        model: MODEL.into(),
        config: BlobId::Sha256([0; 32]),
        said: vec![
            Said::Thinking {
                text: "let me check".into(),
                signature: "sig1".into(),
            },
            Said::ToolUse {
                id: "toolu_1".into(),
                name: "files_read".into(),
                input: r#"{"path":"/a"}"#.into(),
            },
        ],
        stop: Stop::ToolUse,
        usage: Usage::default(),
    };
    let result = Body::Result {
        call: "toolu_1".into(),
        tool: "files_read".into(),
        text: "read".into(),
        failed: false,
    };
    vec![
        (Role::User, vec![post(1, "hi")]),
        (Role::Assistant, vec![frame(2, turn)]),
        (Role::User, vec![frame(3, result), post(4, "next")]),
    ]
}

fn request(wire: Wire, header: &Header) -> String {
    let groups = conversation();
    let mut text = super::header(wire, header);
    let count = groups.len();
    for (at, (role, frames)) in groups.into_iter().enumerate() {
        let (first, last) = (at == 0, at + 1 == count);
        let shown = (&Looks::new(), &Foreign::new());
        text.push_str(&message(
            wire,
            role,
            &frames,
            shown,
            &Lanes::new(),
            first,
            last,
        ));
    }
    text.push_str(close(wire));
    text
}

#[test]
fn a_turn_reaches_anthropic_through_the_adapter_as_anthropics_own_wire_writes_it() {
    let system = ["You keep notes.".to_owned()];
    let header = Header {
        name: MODEL,
        max_tokens: 1024,
        thinking: Some(Thinking::High),
        server_tools: &[ServerTool::WebSearch],
        system: &system,
        file_tools: &[],
    };
    let own: serde_json::Value =
        serde_json::from_str(&request(Wire::AnthropicMessages, &header)).unwrap();
    let asked: model_wire::Request =
        serde_json::from_str(&request(Wire::Harness, &header)).unwrap();
    assert_eq!(adapter::body(&asked, Ask::Turn), own);
}
