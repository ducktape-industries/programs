use ::program::schema::interface;
use programs::Programs;

use super::*;
use crate::schema::tests::{code_with, listed, module};

fn schemed() -> World {
    let mut world = world();
    world.chain.seat::<Programs>(programs::PROGRAM);
    let codes = [
        (chat::PROGRAM, code_with(&interface::<chat::Chat>())),
        (files::PROGRAM, code_with(&interface::<files::Files>())),
        (egress::PROGRAM, code_with(&interface::<Egress>())),
        ("valset", module(&[])),
    ];
    listed(&world.chain, &codes);
    let closed = chat::Op::CreateChannel {
        channel_id: "closed".into(),
        name: "closed".into(),
        post_policy: chat::PostPolicy::MembersOnly,
    };
    world
        .chain
        .submit(signed(BO), chat::PROGRAM, &closed)
        .unwrap();
    world
}

fn called(world: &World, calls: &[(&str, &str, Value)]) -> u64 {
    let chain = &world.chain;
    let thread = world.opened("m1", "use the network", Some(WINDOW));
    chain.at(3, 0);
    let uses: Vec<Value> = calls
        .iter()
        .map(|(id, tool, input)| tool_use(id, tool, input.clone()))
        .collect();
    model_says(chain, Value::Array(uses), "tool_use");
    chain.at(4, 0);
    for (id, _, _) in calls {
        relay_back(chain, id);
    }
    thread
}

fn json_result(chain: &MockChain, thread: u64, call: &str) -> Value {
    let (text, failed) = result_of(chain, thread, call);
    assert!(!failed, "{call}: {text}");
    serde_json::from_str(&text).unwrap()
}

fn refusal(chain: &MockChain, thread: u64, call: &str) -> String {
    let (text, failed) = result_of(chain, thread, call);
    assert!(failed, "{call}: {text}");
    text
}

fn head(chain: &MockChain) -> BlobId {
    let files::Reply::Head(Some(head)) = chain.query(files::PROGRAM, &files::Query::Head).unwrap()
    else {
        panic!("files has a head")
    };
    head.id
}

#[test]
fn the_model_reads_a_programs_shapes_asks_its_queries_and_reads_the_replies() {
    let world = schemed();
    let chain = &world.chain;
    let calls = [
        ("s_chat", "network_schema", json!({ "program": "chat" })),
        ("s_files", "network_schema", json!({ "program": "files" })),
        (
            "q_chat",
            "network_query",
            json!({ "program": "chat", "query": { "Channel": { "channel_id": "general" } } }),
        ),
        (
            "q_files",
            "network_query",
            json!({ "program": "files", "query": { "Stat": { "path": CONFIG } } }),
        ),
    ];
    let thread = called(&world, &calls);

    let shape = json_result(chain, thread, "s_chat");
    assert_eq!(
        (
            &shape["program"],
            &shape["op"],
            &shape["query"],
            &shape["reply"]
        ),
        (
            &json!("chat"),
            &json!("Op"),
            &json!("Query"),
            &json!("Reply")
        )
    );
    assert_eq!(
        shape["types"]["Op"]["enum"]["PostMessage"]["struct"]["thread"],
        json!({ "option": "u64" })
    );
    let files_shape = json_result(chain, thread, "s_files");
    assert_eq!(files_shape["types"]["Query"]["enum"]["Head"], json!(null));

    let channel = json_result(chain, thread, "q_chat");
    let info = &channel["reply"]["Channel"];
    assert_eq!(info["channel"]["name"], "general");
    let stat = json_result(chain, thread, "q_files");
    let snapshot = &stat["reply"]["Stat"]["snapshot"];
    assert_eq!(
        *snapshot,
        json!({ "Sha256": abi::hex(head(chain).digest()) })
    );
}

#[test]
fn an_op_is_the_agents_and_the_program_allows_or_refuses_it() {
    let world = schemed();
    let chain = &world.chain;
    let base = json!({ "Sha256": abi::hex(head(chain).digest()) });
    let post = |channel: &str, id: &str| {
        json!({ "program": "chat", "op": { "PostMessage": {
            "channel_id": channel,
            "message_id": id,
            "blocks": [{ "Paragraph": [{ "text": "hello", "marks": [] }] }],
            "thread": null,
        } } })
    };
    let commit = |path: &str| {
        json!({ "program": "files", "op": { "Commit": {
            "base": base,
            "message": "notes",
            "changes": [{ "Mkdir": { "path": path } }],
        } } })
    };
    let calls = [
        ("c_post", "network_call", post("general", "net-1")),
        ("c_closed", "network_call", post("closed", "net-2")),
        ("c_home", "network_call", commit("/home/Scout/notes")),
        ("c_root", "network_call", commit("/home/Ada")),
    ];
    let thread = called(&world, &calls);

    assert_eq!(json_result(chain, thread, "c_post"), json!({ "ok": true }));
    let asked = chat::Query::MessageById {
        message_id: "net-1".into(),
    };
    let chat::Reply::Message(Some(posted)) = chain.query(chat::PROGRAM, &asked).unwrap() else {
        panic!("the post landed")
    };
    assert_eq!(posted.author, Principal::Account(world.agent));
    assert_eq!(posted.text, "hello");
    let closed = refusal(chain, thread, "c_closed");
    assert!(closed.starts_with(code::UNAUTHORIZED), "{closed}");

    let home = json_result(chain, thread, "c_home");
    let snapshot = abi::decode::<BlobId>(&abi::unhex(home["returned"].as_str().unwrap()).unwrap());
    assert_eq!(snapshot.unwrap(), head(chain));
    let unread = home["unread"].as_str().unwrap();
    assert!(unread.contains("schema names no type"), "{unread}");
    let root = refusal(chain, thread, "c_root");
    assert!(root.starts_with(code::UNAUTHORIZED), "{root}");
}

#[test]
fn an_unknown_program_or_input_that_fits_no_shape_fails_with_its_reason() {
    let world = schemed();
    let chain = &world.chain;
    let calls = [
        (
            "u_schema",
            "network_schema",
            json!({ "program": "nowhere" }),
        ),
        (
            "u_query",
            "network_query",
            json!({ "program": "nowhere", "query": "Head" }),
        ),
        ("u_plain", "network_schema", json!({ "program": "valset" })),
        (
            "m_variant",
            "network_query",
            json!({ "program": "chat", "query": { "Shout": {} } }),
        ),
        (
            "m_field",
            "network_call",
            json!({ "program": "chat", "op": { "AddReaction": { "channel_id": "general", "seq": "first", "emoji": "+1" } } }),
        ),
        (
            "m_missing",
            "network_call",
            json!({ "program": "files", "op": { "Pin": { "name": "v1" } } }),
        ),
    ];
    let thread = called(&world, &calls);

    for call in ["u_schema", "u_query"] {
        let text = refusal(chain, thread, call);
        assert!(text.starts_with(code::UNKNOWN_PROGRAM), "{text}");
    }
    let plain = refusal(chain, thread, "u_plain");
    assert!(plain.starts_with(code::UNSUPPORTED), "{plain}");
    let variant = refusal(chain, thread, "m_variant");
    assert!(
        variant.starts_with(code::INVALID_INPUT) && variant.contains("Shout"),
        "{variant}"
    );
    let field = refusal(chain, thread, "m_field");
    assert!(field.contains("op.AddReaction.seq"), "{field}");
    let missing = refusal(chain, thread, "m_missing");
    assert!(missing.contains("snapshot"), "{missing}");
}

#[test]
fn a_call_whose_input_is_not_an_object_of_its_fields_is_refused_before_it_runs() {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "use the network", Some(WINDOW));
    chain.at(3, 0);
    let calls = json!([
        tool_use("b_none", "network_call", json!({ "op": "Leave" })),
        tool_use("b_text", "network_query", json!("chat")),
    ]);
    model_says(chain, calls, "tool_use");
    let refused: Vec<String> = frames(chain, thread)
        .into_iter()
        .filter_map(|body| match body {
            Body::Refused { call, problem, .. } => Some(format!("{call}: {problem}")),
            _ => None,
        })
        .collect();
    assert_eq!(refused.len(), 2, "{refused:?}");
    assert!(
        refused[0].starts_with("b_none: network_call input"),
        "{refused:?}"
    );
    assert!(
        refused[1].starts_with("b_text: network_query input"),
        "{refused:?}"
    );
    assert!(assigned(chain, RELAY).is_empty());
}

const PODS: &str = "pods";

fn served(chain: &MockChain) {
    let register = egress::Op::Register {
        service: PODS.into(),
    };
    let assign = egress::Op::Assign {
        service: PODS.into(),
        nodes: vec![NODE.to_vec()],
    };
    for op in [register, assign] {
        chain.submit(signed(ADA), egress::PROGRAM, &op).unwrap();
    }
    let serve = egress::Op::Serve {
        services: vec![SERVICE.into(), RELAY.into(), PODS.into()],
    };
    chain
        .submit(signed(&NODE), egress::PROGRAM, &serve)
        .unwrap();
}

fn pods_get(target: &str) -> Value {
    json!({ "program": "egress", "op": { "Request": {
        "request": { "Http": {
            "service": PODS,
            "head": { "method": "Get", "target": target, "headers": [] },
            "body": [],
        } },
        "delivery": "Whole",
        "wait": null,
    } } })
}

fn pods_answer(chain: &MockChain, target: &str, output: Vec<u8>) {
    let asked = |row: &RequestRow| match &row.request {
        Request::Http { head, .. } => head.target == target,
        Request::Relay { .. } => false,
    };
    let rows = assigned(chain, PODS);
    let row = rows.iter().find(|row| asked(row)).expect("pods is asked");
    answer(chain, row.id, Answer::Output(output));
}

fn http_answer(status: u16, content_type: &str, body: &[u8]) -> Vec<u8> {
    abi::encode(&http::Response {
        status,
        headers: vec![("content-type".into(), content_type.into())],
        body: body.to_vec(),
    })
}

#[test]
fn an_http_answer_egress_delivers_whole_reads_as_its_status_headers_and_body() {
    let world = schemed();
    let chain = &world.chain;
    served(chain);
    let calls = [
        ("h_json", "network_call", pods_get("/v1/pods")),
        ("h_text", "network_call", pods_get("/v1/pods/p/logs")),
        ("h_bytes", "network_call", pods_get("/v1/pods/p/blob")),
        ("h_broken", "network_call", pods_get("/v1/usage")),
    ];
    let thread = called(&world, &calls);
    chain.at(5, 0);
    let pod = br#"{"pod": {"name": "duckpond-1"}}"#;
    let created = http_answer(201, "application/json", pod);
    let logs = http_answer(200, "text/plain", b"fetched\nbuilt\n");
    let bytes = http_answer(200, "application/octet-stream", &[0xff, 0, 0xfe]);
    pods_answer(chain, "/v1/pods", created);
    pods_answer(chain, "/v1/pods/p/logs", logs);
    pods_answer(chain, "/v1/pods/p/blob", bytes);
    pods_answer(chain, "/v1/usage", b"no response".to_vec());

    assert_eq!(
        json_result(chain, thread, "h_json"),
        json!({
            "ok": true,
            "status": 201,
            "headers": [["content-type", "application/json"]],
            "body": { "pod": { "name": "duckpond-1" } },
            "body_as": "json",
        })
    );
    assert_eq!(
        json_result(chain, thread, "h_text"),
        json!({
            "ok": true,
            "status": 200,
            "headers": [["content-type", "text/plain"]],
            "body": "fetched\nbuilt\n",
            "body_as": "text",
        })
    );
    let raw = json_result(chain, thread, "h_bytes");
    assert_eq!(
        (&raw["body"], &raw["body_as"]),
        (&json!("ff00fe"), &json!("hex"))
    );
    assert!(raw.get("returned").is_none(), "{raw}");
    let broken = json_result(chain, thread, "h_broken");
    assert_eq!(broken["returned"], abi::hex(b"no response"));
    assert!(broken.get("status").is_none(), "{broken}");
    let unread = broken["unread"].as_str().unwrap();
    assert!(
        unread.starts_with("the answer is no HTTP response"),
        "{unread}"
    );
}
