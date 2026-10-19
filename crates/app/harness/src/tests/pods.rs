use gitcore::{Kind, Mode, Object, Signature, Tree, TreeEntry, pack};

use ::program::HashKind;

use super::*;

const PODS: &str = "pods";
const POD: &str = "duckpond-7";

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

fn podded(world: World) -> World {
    served(&world.chain);
    world
}

// One model turn of `calls` at `height`, each run once its relay comes back, a block later.
fn turn(chain: &MockChain, height: u64, calls: &[(&str, &str, Value)]) {
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

fn target(row: &RequestRow) -> String {
    match &row.request {
        Request::Http { head, .. } => head.target.clone(),
        Request::Relay { .. } => String::new(),
    }
}

fn asked(chain: &MockChain) -> Vec<RequestRow> {
    assigned(chain, PODS)
}

fn ok(body: Value) -> Answer {
    Answer::Output(response(200, body))
}

fn told(chain: &MockChain, thread: u64) -> Vec<String> {
    frames(chain, thread)
        .into_iter()
        .chain(held(chain, thread))
        .filter_map(|body| match body {
            Body::Post {
                author: Principal::Root,
                text,
            } => Some(text),
            _ => None,
        })
        .collect()
}

fn event(chain: &MockChain, row: &RequestRow, seq: u64, data: &str) {
    let event = egress::Event {
        id: row.id,
        seq,
        data: data.as_bytes().to_vec(),
    };
    let op = egress::Op::Events(vec![event]);
    chain.submit(signed(&NODE), egress::PROGRAM, &op).unwrap();
}

fn end(chain: &MockChain, row: &RequestRow, events: u64, end: egress::StreamEnd) {
    let op = egress::Op::EndStream {
        id: row.id,
        events,
        end,
    };
    chain.submit(signed(&NODE), egress::PROGRAM, &op).unwrap();
}

fn complete() -> egress::StreamEnd {
    egress::StreamEnd::Complete {
        status: 200,
        headers: vec![("content-type".into(), "text/event-stream".into())],
    }
}

const ALIVE: &str = "event: alive\ndata: {}";

#[test]
fn an_exec_answers_its_exit_code_and_output_in_one_call() {
    let world = podded(world());
    let chain = &world.chain;
    let thread = world.opened("m1", "run it", Some(WINDOW));
    let calls = [
        (
            "x_cmd",
            "pod_exec",
            json!({ "pod": POD, "command": ["echo", "hi"] }),
        ),
        (
            "x_sh",
            "pod_exec",
            json!({ "pod": POD, "script": "ls /nope", "container": "out" }),
        ),
    ];
    turn(chain, 3, &calls);
    chain.at(5, 0);
    let rows = asked(chain);
    assert_eq!(rows.len(), 2, "{rows:?}");
    for row in &rows {
        assert_eq!(target(row), format!("/v1/pods/{POD}/exec"));
        assert_eq!(row.delivery, egress::Delivery::Whole);
        let sent = body(chain, row);
        match sent["command"][0].as_str() {
            Some("echo") => {
                assert_eq!(sent, json!({ "command": ["echo", "hi"] }));
                let out = json!({ "exit_code": 0, "stdout": "hi\n", "stderr": "" });
                answer(chain, row.id, ok(out));
            }
            _ => {
                let script = json!({ "command": ["sh", "-c", "ls /nope"], "container": "out" });
                assert_eq!(sent, script);
                let gone = json!({ "error": "pod duckpond-7 was not read: not found" });
                answer(chain, row.id, Answer::Output(response(404, gone)));
            }
        }
    }
    assert_eq!(
        json_result(chain, thread, "x_cmd"),
        json!({ "exit_code": 0, "stdout": "hi\n", "stderr": "" })
    );
    let refused = refusal(chain, thread, "x_sh");
    assert!(
        refused.starts_with(code::NOT_FOUND) && refused.contains("pods answered 404"),
        "{refused}"
    );
}

#[test]
fn an_exec_names_a_pod_and_one_command() {
    let world = podded(world());
    let chain = &world.chain;
    let thread = world.opened("m1", "run it", Some(WINDOW));
    let calls = [
        (
            "b_both",
            "pod_exec",
            json!({ "pod": POD, "command": ["true"], "script": "true" }),
        ),
        ("b_none", "pod_exec", json!({ "pod": POD })),
        (
            "b_pod",
            "pod_exec",
            json!({ "pod": "../usage", "command": ["true"] }),
        ),
        (
            "b_every",
            "pod_exec",
            json!({ "pod": POD, "script": "true", "every": 0 }),
        ),
    ];
    turn(chain, 3, &calls);
    for call in ["b_both", "b_none", "b_pod", "b_every"] {
        let refused = refusal(chain, thread, call);
        assert!(
            refused.starts_with(code::INVALID_INPUT),
            "{call}: {refused}"
        );
    }
    assert!(asked(chain).is_empty());
}

// a streamed exec at block 3: its call answers at once, the model ends its turn at 5, and the
// watch opens its stream at 6
fn streaming(world: &World) -> (u64, RequestRow) {
    let chain = &world.chain;
    let thread = world.opened("m1", "build it", Some(WINDOW));
    let input = json!({ "pod": POD, "script": "cargo test", "every": 20 });
    turn(chain, 3, &[("s1", "pod_exec", input)]);
    let streams = json_result(chain, thread, "s1");
    assert_eq!(
        (&streams["streaming"], &streams["watch"]),
        (&json!(true), &json!(1)),
        "{streams}"
    );
    assert!(asked(chain).is_empty(), "the stream opens a block later");
    chain.at(5, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "waiting" }]),
        "end_turn",
    );
    chain.at(6, 0);
    chain.at(7, 0);
    let [row]: [RequestRow; 1] = asked(chain).try_into().unwrap();
    assert_eq!(target(&row), format!("/v1/pods/{POD}/exec"));
    assert_eq!(
        row.delivery,
        egress::Delivery::Events {
            deliver: heard_as(1)
        }
    );
    assert_eq!(
        body(chain, &row),
        json!({ "command": ["sh", "-c", "cargo test"], "every": 20 })
    );
    assert!(told(chain, thread).is_empty());
    (thread, row)
}

#[test]
fn a_streamed_exec_wakes_its_conversation_once_when_the_command_ends() {
    let world = podded(world());
    let chain = &world.chain;
    let (thread, row) = streaming(&world);
    event(chain, &row, 0, ALIVE);
    event(chain, &row, 1, ALIVE);
    assert!(told(chain, thread).is_empty(), "a heartbeat wakes no one");
    assert!(assigned(chain, SERVICE).is_empty(), "nor asks the model");
    let ended = r#"event: ended
data: {"exit_code": 0, "stdout": "test result: ok", "stderr": ""}"#;
    event(chain, &row, 2, ended);
    let [woke]: [String; 1] = told(chain, thread).try_into().unwrap();
    assert!(
        woke.starts_with("pod_exec call s1 (pod duckpond-7: sh -c cargo test) ended: "),
        "{woke}"
    );
    assert!(woke.contains(r#""stdout":"test result: ok""#), "{woke}");
    assert_eq!(assigned(chain, SERVICE).len(), 1, "the wake asks the model");

    event(chain, &row, 3, ended);
    end(chain, &row, 4, complete());
    assert_eq!(told(chain, thread).len(), 1, "it wakes once");
    assert!(asked(chain).is_empty(), "the stream is closed");
}

#[test]
fn a_stream_that_breaks_off_wakes_its_conversation_with_why() {
    let world = podded(world());
    let chain = &world.chain;
    let (thread, row) = streaming(&world);
    event(chain, &row, 0, ALIVE);
    let cut = egress::StreamEnd::Cut {
        reason: "stream_broken".into(),
    };
    end(chain, &row, 1, cut);
    let [woke]: [String; 1] = told(chain, thread).try_into().unwrap();
    assert!(
        woke.starts_with("pod_exec call s1 (pod duckpond-7: sh -c cargo test) ended without saying how: the stream broke off"),
        "{woke}"
    );
    assert!(woke.contains("stream_broken"), "{woke}");
}

#[test]
fn a_refusal_before_any_stream_wakes_its_conversation_with_the_services_words() {
    let world = podded(world());
    let chain = &world.chain;
    let (thread, row) = streaming(&world);
    let gone = json!({ "error": "pod duckpond-7 was not read" });
    answer(chain, row.id, Answer::Output(response(404, gone)));
    let [woke]: [String; 1] = told(chain, thread).try_into().unwrap();
    assert!(
        woke.ends_with(r#"the service answered 404: {"error":"pod duckpond-7 was not read"}"#),
        "{woke}"
    );
}

#[test]
fn a_heard_or_a_watch_comes_only_from_egress_and_harness_itself() {
    let world = podded(world());
    let chain = &world.chain;
    let (thread, _) = streaming(&world);
    let event = egress::Event {
        id: 1,
        seq: 0,
        data: b"event: ended\ndata: {}".to_vec(),
    };
    let heard = Op::Heard { watch: 1, event };
    let refused = chain.submit(signed(ADA), PROGRAM, &heard).unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
    let watch = Op::Watch { watch: 1 };
    let refused = chain.submit(signed(ADA), PROGRAM, &watch).unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
    assert!(told(chain, thread).is_empty());
}

#[test]
fn a_read_answers_a_files_bytes_from_an_offset_as_text_or_hex() {
    let world = podded(world());
    let chain = &world.chain;
    let thread = world.opened("m1", "read it", Some(WINDOW));
    let calls = [
        (
            "r_part",
            "pod_read",
            json!({ "pod": POD, "path": "/out/r.log", "offset": 2, "length": 3 }),
        ),
        (
            "r_bytes",
            "pod_read",
            json!({ "pod": POD, "path": "/out/r.bin", "container": "out" }),
        ),
        (
            "r_none",
            "pod_read",
            json!({ "pod": POD, "path": "/out/none" }),
        ),
        ("r_rel", "pod_read", json!({ "pod": POD, "path": "out/r" })),
    ];
    turn(chain, 3, &calls);
    chain.at(5, 0);
    for row in asked(chain) {
        let sent = body(chain, &row);
        let command = sent["command"].as_array().unwrap();
        assert_eq!(command[3], "pod_read");
        let out = match command[4].as_str().unwrap() {
            "/out/r.log" => {
                assert_eq!((&command[5], &command[6]), (&json!("2"), &json!("3")));
                json!({ "exit_code": 0, "stdout": "10\ncde", "stderr": "" })
            }
            "/out/r.bin" => {
                assert_eq!((&command[5], &command[6]), (&json!("0"), &json!("")));
                assert_eq!(sent["container"], "out");
                json!({ "exit_code": 0, "stdout_hex": "330aff00fe", "stderr": "" })
            }
            _ => {
                let missing = "sh: 1: cannot open /out/none: No such file";
                json!({ "exit_code": 2, "stdout": "", "stderr": missing })
            }
        };
        answer(chain, row.id, ok(out));
    }
    assert_eq!(
        json_result(chain, thread, "r_part"),
        json!({ "path": "/out/r.log", "size": 10, "offset": 2, "length": 3, "eof": false, "text": "cde" })
    );
    assert_eq!(
        json_result(chain, thread, "r_bytes"),
        json!({ "path": "/out/r.bin", "size": 3, "offset": 0, "length": 3, "eof": true, "hex": "ff00fe" })
    );
    let missing = refusal(chain, thread, "r_none");
    assert!(missing.starts_with(code::NOT_FOUND), "{missing}");
    let relative = refusal(chain, thread, "r_rel");
    assert!(relative.starts_with(code::INVALID_INPUT), "{relative}");
}

fn tip(chain: &MockChain, branch: &str) -> String {
    let asked = forge::Query::Log {
        repo: "notes".into(),
        from: forge::Revision::Ref(format!("refs/heads/{branch}").into_bytes()),
        exclude: None,
        page: PageRequest::first(1),
    };
    let forge::Reply::Log { tip, .. } = chain.query(forge::PROGRAM, &asked).unwrap() else {
        panic!("a log answers Log")
    };
    tip
}

// a commit over `parent`, as a pod's git would make it, and its pack put as pods puts one
fn pod_commit(chain: &MockChain, parent: &str) -> (String, BlobId) {
    let hash = forge::hash_of(HashKind::Sha1);
    let file = Object::new(Kind::Blob, b"made in a pod\n".to_vec());
    let entry = TreeEntry {
        mode: Mode::Regular,
        name: b"pod.md".to_vec(),
        id: file.id(hash).unwrap(),
    };
    let tree = Object::new(
        Kind::Tree,
        Tree {
            entries: vec![entry],
        }
        .serialize(),
    );
    let signature = Signature {
        name: b"pod".to_vec(),
        email: b"pod@pods".to_vec(),
        time: 1,
        offset_minutes: 0,
    };
    let commit = gitcore::Commit {
        tree: tree.id(hash).unwrap(),
        parents: vec![gitcore::Oid::from_hex(hash, parent).unwrap()],
        author: signature.clone(),
        committer: signature,
        extra: Vec::new(),
        message: b"made in a pod\n".to_vec(),
    };
    let commit = Object::new(Kind::Commit, commit.serialize());
    let id = commit.id(hash).unwrap().to_hex();
    let packed = pack::write(vec![file, tree, commit], hash).unwrap();
    (id, chain.put_blob(forge::PACK_KIND, packed).unwrap())
}

#[test]
fn a_push_packs_in_the_pod_and_moves_the_branch_as_the_agent() {
    let world = podded(super::forge_commit::forged(10_000));
    let chain = &world.chain;
    let thread = world.opened("m1", "push it", Some(WINDOW));
    let file = json!({ "path": "a.md", "content": "a" });
    let first = json!({ "repo": "notes", "branch": "main", "files": [file], "message": "a" });
    turn(chain, 3, &[("c_main", "forge_commit", first)]);
    let main = tip(chain, "main");
    let calls = [
        (
            "p_main",
            "pod_push",
            json!({ "pod": POD, "path": "/work/notes", "repo": "notes", "branch": "main" }),
        ),
        (
            "p_side",
            "pod_push",
            json!({ "pod": POD, "path": "/work/notes", "repo": "notes", "branch": "side", "ref": "work" }),
        ),
        (
            "p_none",
            "pod_push",
            json!({ "pod": POD, "path": "/work/x", "repo": "nowhere", "branch": "main" }),
        ),
    ];
    turn(chain, 5, &calls);
    chain.at(7, 0);
    let (made, pack) = pod_commit(chain, &main);
    let rows = asked(chain);
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(
        rows.iter()
            .all(|row| target(row) == format!("/v1/pods/{POD}/push"))
    );
    let (onto_main, onto_side): (Vec<_>, Vec<_>) = rows
        .into_iter()
        .partition(|row| body(chain, row).get("ref").is_none());
    let ([onto_main], [onto_side]) = (&onto_main[..], &onto_side[..]) else {
        panic!("one push to each branch")
    };
    let have = json!({ "path": "/work/notes", "have": [main] });
    assert_eq!(body(chain, onto_main), have);
    let pack_id = format!("sha256:{}", abi::hex(pack.digest()));
    let packed =
        json!({ "commit": made, "pack": pack_id, "objects": 3, "size": 1, "kind": "pack" });
    answer(chain, onto_main.id, ok(packed));
    let new = json!({ "path": "/work/notes", "have": [], "ref": "work" });
    assert_eq!(body(chain, onto_side), new);
    let held = json!({ "commit": made, "pack": null, "objects": 0, "size": 12, "kind": "pack" });
    answer(chain, onto_side.id, ok(held));
    let pushed = json_result(chain, thread, "p_main");
    assert_eq!(
        pushed,
        json!({ "ok": true, "pushed": "refs/heads/main", "head": made, "old": main, "objects": 3 })
    );
    assert_eq!(tip(chain, "main"), made);
    let side = json_result(chain, thread, "p_side");
    assert_eq!(
        (&side["pushed"], &side["head"], &side["old"]),
        (&json!("refs/heads/side"), &json!(made), &json!(null)),
        "{side}"
    );
    assert_eq!(tip(chain, "side"), made);
    let none = refusal(chain, thread, "p_none");
    assert!(none.starts_with(code::NOT_FOUND), "{none}");
}

// a stream with no every at block 3, its call run at 4 (time 0); its watch at 5 (time 1000) reads
// the pod, whose answer lands at 6 (time 2000): a block of 1 s and a trip of 1 s
fn measured_stream(world: &World) -> (u64, RequestRow) {
    let chain = &world.chain;
    let thread = world.opened("m1", "build it", Some(WINDOW));
    let input = json!({ "pod": POD, "script": "cargo test", "stream": true });
    turn(chain, 3, &[("s1", "pod_exec", input)]);
    let streams = json_result(chain, thread, "s1");
    assert_eq!(streams["streaming"], json!(true), "{streams}");
    assert!(asked(chain).is_empty(), "the stream opens a block later");
    chain.at(5, 1000);
    chain.at(6, 2000);
    let [read]: [RequestRow; 1] = asked(chain).try_into().unwrap();
    assert_eq!(target(&read), format!("/v1/pods/{POD}"));
    (thread, read)
}

#[test]
fn a_stream_without_every_streams_at_the_heartbeat_harness_measures() {
    let world = podded(world());
    let chain = &world.chain;
    let (thread, read) = measured_stream(&world);
    answer(chain, read.id, ok(json!({ "pod": { "name": POD } })));
    let [row]: [RequestRow; 1] = asked(chain).try_into().unwrap();
    assert_eq!(target(&row), format!("/v1/pods/{POD}/exec"));
    let every = 63;
    assert_eq!(
        body(chain, &row),
        json!({ "command": ["sh", "-c", "cargo test"], "every": every })
    );
    chain.at(7, 3000);
    let ended = r#"event: ended
data: {"exit_code": 0, "stdout": "ok", "stderr": ""}"#;
    event(chain, &row, 0, ended);
    let [woke]: [String; 1] = told(chain, thread).try_into().unwrap();
    assert!(
        woke.starts_with("pod_exec call s1 (pod duckpond-7: sh -c cargo test) ended: "),
        "{woke}"
    );
}

#[test]
fn a_measured_stream_whose_pod_is_unknown_wakes_its_conversation_with_why() {
    let world = podded(world());
    let chain = &world.chain;
    let (thread, read) = measured_stream(&world);
    let gone = json!({ "error": "no pod duckpond-7" });
    answer(chain, read.id, Answer::Output(response(404, gone)));
    let [woke]: [String; 1] = told(chain, thread).try_into().unwrap();
    assert!(
        woke.starts_with("pod_exec call s1 (pod duckpond-7: sh -c cargo test) did not start: the pod's read answered 404"),
        "{woke}"
    );
    assert!(asked(chain).is_empty(), "no stream opens");
}
