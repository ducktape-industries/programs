//! `blob_put`: an agent puts its own text as a part blob and names it as an egress request's
//! body, puts a file of files whole at a snapshot or at its head, and gives content or source,
//! never both or neither; the hash and the kind are the network's.
use ::program::HashKind;
use ::program::schema::interface;
use programs::Programs;

use super::*;
use crate::schema::tests::{code_with, listed};

const PODS: &str = "pods";

fn pods() -> World {
    let mut world = world();
    world.chain.seat::<Programs>(programs::PROGRAM);
    listed(
        &world.chain,
        &[(egress::PROGRAM, code_with(&interface::<Egress>()))],
    );
    let chain = &world.chain;
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
    world
}

fn ran(chain: &MockChain, height: u64, calls: &[(&str, &str, Value)]) {
    chain.at(height, 0);
    let uses: Vec<Value> = calls
        .iter()
        .map(|(id, tool, input)| tool_use(id, tool, input.clone()))
        .collect();
    model_says(chain, Value::Array(uses), "tool_use");
    chain.at(height + 1, 0);
    for (id, _, _) in calls {
        relay_back(chain, id);
    }
}

fn json_of(chain: &MockChain, thread: u64, call: &str) -> Value {
    let (text, failed) = result_of(chain, thread, call);
    assert!(!failed, "{call}: {text}");
    serde_json::from_str(&text).unwrap()
}

fn part(text: &str) -> BlobId {
    ::program::blob_id(HashKind::Sha256, PART, text.as_bytes()).unwrap()
}

#[test]
fn the_agents_text_is_put_as_a_part_and_an_egress_request_names_it_as_its_body() {
    let world = pods();
    let chain = &world.chain;
    let thread = world.opened("m1", "start a pod", Some(WINDOW));
    let spec = json!({ "image": "toolchain", "command": ["sleep", "infinity"] });
    let text = spec.to_string();
    let put = json!({ "kind": "part", "content": text });
    ran(chain, 3, &[("b1", "blob_put", put)]);
    let id = part(&text);
    let digest = abi::hex(id.digest());
    let put = json_of(chain, thread, "b1");
    assert_eq!(put["blob"], format!("sha256:{digest}"));
    assert_eq!(put["size"], text.len());
    assert_eq!(put["kind"], "part");
    assert_eq!(put["in_network_call"], json!({ "Sha256": digest }));
    let held = &chain.host(egress::PROGRAM).borrow().blobs[&id];
    assert_eq!((held.kind.as_str(), held.body.clone()), (PART, text.into_bytes()));

    let request = json!({ "program": "egress", "op": { "Request": {
        "request": { "Http": {
            "service": PODS,
            "head": {
                "method": "Post",
                "target": "/v1/pods",
                "headers": [["content-type", "application/json"]],
            },
            "body": [put["in_network_call"]],
        } },
        "delivery": "Whole",
        "wait": null,
    } } });
    ran(chain, 5, &[("c1", "network_call", request)]);
    let [row] = assigned(chain, PODS).try_into().expect("one pods request");
    let Request::Http { head, body: parts, .. } = &row.request else {
        panic!("pods is asked over http");
    };
    assert_eq!(
        (head.method, head.target.as_str()),
        (http::Method::Post, "/v1/pods")
    );
    assert_eq!(*parts, [id]);
    assert_eq!(body(chain, &row), spec);

    chain.at(7, 0);
    let created = response(201, json!({ "pod": { "name": "duckpond-1" } }));
    answer(chain, row.id, Answer::Output(created.clone()));
    let called = json_of(chain, thread, "c1");
    assert_eq!(
        called,
        json!({ "ok": true, "returned": abi::hex(&created) })
    );
}

#[test]
fn a_file_is_put_whole_as_files_holds_it_at_a_snapshot_or_at_its_head() {
    let world = world();
    let chain = &world.chain;
    let path = "/shared/pod.json";
    let (older, newer) = ("{\"command\":[\"true\"]}", "{\"command\":[\"false\"]}");
    let first = commit(chain, ADA, path, older);
    commit(chain, ADA, path, newer);
    let thread = world.opened("m1", "put the spec", Some(WINDOW));
    let at = abi::hex(first.digest());
    ran(
        chain,
        3,
        &[
            (
                "s_at",
                "blob_put",
                json!({ "kind": "part", "source": path, "snapshot": at }),
            ),
            (
                "s_head",
                "blob_put",
                json!({ "kind": "part", "source": path }),
            ),
            (
                "s_none",
                "blob_put",
                json!({ "kind": "part", "source": "/shared/none.json" }),
            ),
        ],
    );
    let named = |text: &str| format!("sha256:{}", abi::hex(part(text).digest()));
    let put = json_of(chain, thread, "s_at");
    assert_eq!((&put["blob"], &put["size"]), (&json!(named(older)), &json!(older.len())));
    assert_eq!(json_of(chain, thread, "s_head")["blob"], named(newer));
    let (missing, failed) = result_of(chain, thread, "s_none");
    assert!(failed && missing.starts_with(code::NOT_FOUND), "{missing}");
}

#[test]
fn content_or_source_is_given_alone_and_the_hash_and_the_kind_are_the_networks() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "put blobs", Some(WINDOW));
    chain.at(3, 0);
    let snapshot = "00".repeat(32);
    let calls = json!([
        tool_use("r_both", "blob_put", json!({ "kind": "part", "content": "{}", "source": CONFIG })),
        tool_use("r_neither", "blob_put", json!({ "kind": "part" })),
        tool_use("r_at", "blob_put", json!({ "kind": "part", "content": "{}", "snapshot": snapshot })),
        tool_use("p_sha1", "blob_put", json!({ "kind": "blob", "content": "hello\n", "hash": "sha1" })),
        tool_use("p_words", "blob_put", json!({ "kind": "two words", "content": "x" })),
    ]);
    model_says(chain, calls, "tool_use");
    let refused: Vec<(String, String)> = frames(chain, thread)
        .into_iter()
        .filter_map(|body| match body {
            Body::Refused { call, problem, .. } => Some((call, problem)),
            _ => None,
        })
        .collect();
    let ids: Vec<&str> = refused.iter().map(|(call, _)| call.as_str()).collect();
    assert_eq!(ids, ["r_both", "r_neither", "r_at"]);
    for (call, problem) in &refused {
        assert!(problem.starts_with("blob_put input"), "{call}: {problem}");
    }
    assert_eq!(assigned(chain, RELAY).len(), 2);

    chain.at(4, 0);
    relay_back(chain, "p_sha1");
    relay_back(chain, "p_words");
    let git = "ce013625030ba8dba906f756967f9e9ca394464a";
    let sha1 = json_of(chain, thread, "p_sha1");
    assert_eq!(sha1["blob"], format!("sha1:{git}"));
    assert_eq!(sha1["in_network_call"], json!({ "Sha1": git }));
    let (words, failed) = result_of(chain, thread, "p_words");
    assert!(failed && words.starts_with(code::INVALID_INPUT), "{words}");
}
