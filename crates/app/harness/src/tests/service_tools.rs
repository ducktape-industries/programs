//! Tool-file steps that call an egress service as the agent: answered whole, or streamed at a
//! heartbeat harness measures; a refusal in the service's words or named by its status and media
//! type, an absent optional input left out, and an image answer kept in files and shown.
use base64::Engine;

use super::*;

const LOCAL: &str = "local-harness";

const READ: &str = r#"name = "read"
description = "Read a file on the computer."

[input]
path = { type = "string", description = "an absolute path" }
offset = { type = "integer", description = "the first line", optional = true }

[[step]]
service = "local-harness"
target = "/v1/read"
body = { path = "$path", offset = "$offset" }
as = "read"
"#;

const BASH: &str = r#"name = "bash"
description = "Run a command on the computer."

[input]
command = { type = "string", description = "the shell text" }

[[step]]
service = "local-harness"
target = "/v1/bash"
body = { command = "$command" }
stream = true
"#;

fn local(world: World) -> World {
    let chain = &world.chain;
    let register = egress::Op::Register {
        service: LOCAL.into(),
    };
    let assign = egress::Op::Assign {
        service: LOCAL.into(),
        nodes: vec![NODE.to_vec()],
    };
    for op in [register, assign] {
        chain.submit(signed(ADA), egress::PROGRAM, &op).unwrap();
    }
    let serve = egress::Op::Serve {
        services: vec![SERVICE.into(), RELAY.into(), LOCAL.into()],
    };
    chain
        .submit(signed(&NODE), egress::PROGRAM, &serve)
        .unwrap();
    for (name, text) in [("read", READ), ("bash", BASH)] {
        commit(chain, ADA, &format!("/home/Scout/tools/{name}.toml"), text);
    }
    world
}

fn asked(chain: &MockChain) -> Vec<RequestRow> {
    assigned(chain, LOCAL)
}

fn head_of(row: &RequestRow) -> http::Head {
    match &row.request {
        Request::Http { head, .. } => head.clone(),
        Request::Relay { .. } | Request::Crosspond { .. } => {
            panic!("a service step asks over HTTP")
        }
    }
}

fn sender(chain: &MockChain, id: u64) -> Option<AccountNumber> {
    let asked = egress::Query::Sender { id };
    let egress::Reply::Sender(sender) = chain.query(egress::PROGRAM, &asked).unwrap() else {
        panic!("Sender answers Sender")
    };
    sender
}

fn raw(status: u16, media: &str, body: &[u8]) -> Answer {
    let headers = vec![("content-type".to_owned(), media.to_owned())];
    Answer::Output(abi::encode(&http::Response {
        status,
        headers,
        body: body.to_vec(),
    }))
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes.resize(96, 7);
    bytes
}

#[test]
fn a_service_step_asks_as_the_agent_leaves_an_absent_optional_input_out_and_binds_the_answer() {
    let world = local(world());
    let chain = &world.chain;
    let thread = world.opened("m1", "read it", Some(WINDOW));
    let (model, _) = open_request(chain);
    let offered = provider_body(chain, &model);
    let tools = offered["tools"].as_array().unwrap();
    let read = tools.iter().find(|tool| tool["name"] == "read").unwrap();
    assert_eq!(read["input_schema"]["required"], json!(["path"]));
    let calls = [
        ("r_all", "read", json!({ "path": "/etc/hosts" })),
        (
            "r_from",
            "read",
            json!({ "path": "/etc/hosts", "offset": 3 }),
        ),
    ];
    turn(chain, 3, &calls);
    chain.at(5, 0);
    let rows = asked(chain);
    assert_eq!(rows.len(), 2, "{rows:?}");
    for row in &rows {
        assert_eq!(target(row), "/v1/read");
        assert_eq!(row.delivery, egress::Delivery::Whole);
        let head = head_of(row);
        assert_eq!(head.method, http::Method::Post);
        let json = ("content-type".to_owned(), "application/json".to_owned());
        assert_eq!(head.headers, [json]);
        assert_eq!(sender(chain, row.id), Some(world.agent));
        let sent = body(chain, row);
        let out = match sent.get("offset") {
            None => {
                assert_eq!(sent, json!({ "path": "/etc/hosts" }));
                json!({ "text": "1\tlocalhost" })
            }
            Some(_) => {
                assert_eq!(sent, json!({ "path": "/etc/hosts", "offset": 3 }));
                json!({ "text": "3\tnothing" })
            }
        };
        answer(chain, row.id, Answer::Output(response(200, out)));
    }
    assert_eq!(
        json_result(chain, thread, "r_all"),
        json!({ "ok": true, "read": { "text": "1\tlocalhost" } })
    );
    assert_eq!(
        json_result(chain, thread, "r_from"),
        json!({ "ok": true, "read": { "text": "3\tnothing" } })
    );
}

#[test]
fn a_refusal_reads_in_the_services_words_or_by_its_status_and_media_type() {
    let world = local(world());
    let chain = &world.chain;
    let thread = world.opened("m1", "read it", Some(WINDOW));
    let calls = [
        ("r_gone", "read", json!({ "path": "/nope" })),
        ("r_off", "read", json!({ "path": "/off" })),
        ("r_empty", "read", json!({ "path": "/empty" })),
    ];
    turn(chain, 3, &calls);
    chain.at(5, 0);
    for row in asked(chain) {
        let refused = match provider_body(chain, &row)["path"].as_str().unwrap() {
            "/nope" => Answer::Output(response(404, json!({ "error": "no file /nope" }))),
            "/off" => raw(530, "text/html; charset=UTF-8", b"<html>error 1033</html>"),
            _ => raw(502, "text/plain", b""),
        };
        answer(chain, row.id, refused);
    }
    let gone = refusal(chain, thread, "r_gone");
    let said = "step 1 (service local-harness): local-harness answered 404: no file /nope";
    assert!(
        gone.starts_with(code::NOT_FOUND) && gone.contains(said),
        "{gone}"
    );
    let off = refusal(chain, thread, "r_off");
    let said = "local-harness answered 530 with a text/html body, not JSON";
    assert!(off.contains(said) && !off.contains("<html>"), "{off}");
    let empty = refusal(chain, thread, "r_empty");
    assert!(
        empty.contains("local-harness answered 502 with no body"),
        "{empty}"
    );
}

#[test]
fn a_streamed_step_answers_at_once_and_wakes_its_conversation_with_the_streams_end() {
    let world = local(world());
    let chain = &world.chain;
    let thread = world.opened("m1", "build it", Some(WINDOW));
    turn(
        chain,
        3,
        &[("b1", "bash", json!({ "command": "cargo test" }))],
    );
    let streams = json_result(chain, thread, "b1");
    assert_eq!(streams["streaming"], json!(true), "{streams}");
    assert!(asked(chain).is_empty(), "the stream opens a block later");
    chain.at(5, 1000);
    chain.at(6, 2000);
    chain.at(7, 3000);
    let [row]: [RequestRow; 1] = asked(chain).try_into().unwrap();
    assert_eq!(target(&row), "/v1/bash");
    assert_eq!(
        row.delivery,
        egress::Delivery::Events {
            deliver: heard_as(1)
        }
    );
    let every = ("every".to_owned(), "32".to_owned());
    assert!(head_of(&row).headers.contains(&every), "{row:?}");
    assert_eq!(
        provider_body(chain, &row),
        json!({ "command": "cargo test" })
    );
    assert_eq!(sender(chain, row.id), Some(world.agent));
    event(chain, &row, 0, "event: alive\ndata: {}");
    event(chain, &row, 1, ": ping");
    assert!(told(chain, thread).is_empty(), "a heartbeat wakes no one");
    let ended =
        "event: ended\ndata: {\"id\": 7, \"exit_code\": 0, \"stdout\": \"ok\", \"stderr\": \"\"}";
    event(chain, &row, 2, ended);
    let [woke]: [String; 1] = told(chain, thread).try_into().unwrap();
    assert!(
        woke.starts_with("bash call b1 (local-harness /v1/bash) ended: "),
        "{woke}"
    );
    assert!(woke.contains(r#""stdout":"ok""#), "{woke}");
}

#[test]
fn an_image_answer_is_kept_in_the_agents_home_and_shown_to_the_model_in_its_result() {
    let world = local(world());
    let chain = &world.chain;
    let thread = world.opened("m1", "look", Some(WINDOW));
    turn(
        chain,
        3,
        &[("i1", "read", json!({ "path": "/tmp/shot.png" }))],
    );
    chain.at(5, 0);
    let [row]: [RequestRow; 1] = asked(chain).try_into().unwrap();
    let bytes = png(64, 48);
    answer(chain, row.id, raw(200, "image/png", &bytes));
    let seen = json_result(chain, thread, "i1");
    let path = format!("/home/Scout/seen/{thread}-i1-1.png");
    let read = &seen["read"];
    assert_eq!(read["path"], json!(path), "{seen}");
    assert_eq!(
        (
            &read["media_type"],
            &read["size"],
            &read["width"],
            &read["height"]
        ),
        (&json!("image/png"), &json!(96), &json!(64), &json!(48)),
        "{seen}"
    );
    let stat = files::Query::Stat {
        path: path.clone(),
        at: None,
    };
    let files::Reply::Stat { entry, .. } = chain.query(files::PROGRAM, &stat).unwrap() else {
        panic!("Stat answers Stat")
    };
    assert!(entry.is_some(), "files holds {path}");
    let messages = sent_messages(chain);
    let content = messages.last().unwrap()["content"].as_array().unwrap();
    let result = content
        .iter()
        .find(|block| block["tool_use_id"] == "i1")
        .unwrap_or_else(|| panic!("no result for i1: {content:?}"));
    let shown = result["content"].as_array().unwrap();
    let images: Vec<&Value> = shown
        .iter()
        .filter(|block| block["type"] == "image")
        .collect();
    let [image] = images[..] else {
        panic!("one image in the result: {shown:?}");
    };
    let data = image["source"]["data"].as_str().unwrap();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(data)
        .unwrap();
    assert_eq!(decoded, bytes);
}

#[test]
fn a_service_step_that_streams_before_the_end_or_names_no_target_or_service_is_left_out() {
    let world = local(world());
    let chain = &world.chain;
    let early = BASH.replace("name = \"bash\"", "name = \"early\"").replace(
        "stream = true\n",
        "stream = true\n\n[[step]]\nservice = \"local-harness\"\ntarget = \"/v1/after\"\n",
    );
    let aimless = READ
        .replace("name = \"read\"", "name = \"aimless\"")
        .replace("target = \"/v1/read\"\n", "");
    let reserved = READ
        .replace("name = \"read\"", "name = \"reserved\"")
        .replace("service = \"local-harness\"", "service = \"http\"");
    let verb = READ.replace("name = \"read\"", "name = \"verb\"").replace(
        "target = \"/v1/read\"",
        "method = \"FETCH\"\ntarget = \"/v1/read\"",
    );
    for (name, text) in [
        ("early", early),
        ("aimless", aimless),
        ("reserved", reserved),
        ("verb", verb),
    ] {
        commit(chain, ADA, &format!("/home/Scout/tools/{name}.toml"), &text);
    }
    let asked = Query::AgentTools { agent: world.agent };
    let Reply::AgentTools(tools) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!("AgentTools answers AgentTools")
    };
    let said = [
        "tool early: step 1: only the last step streams",
        "tool aimless: step 1: a service step names its target",
        "tool reserved: step 1: http is no egress service's name",
        "tool verb: step 1: method FETCH is none of GET, POST, PUT, PATCH and DELETE",
    ];
    for line in said {
        assert!(
            tools
                .problems
                .iter()
                .any(|problem| problem.starts_with(line)),
            "{line} in {:?}",
            tools.problems
        );
    }
}

#[test]
fn a_refused_stream_wakes_its_conversation_with_the_services_refusal() {
    let world = local(world());
    let chain = &world.chain;
    let thread = world.opened("m1", "build it", Some(WINDOW));
    turn(chain, 3, &[("b1", "bash", json!({ "command": "nope" }))]);
    chain.at(5, 1000);
    chain.at(6, 2000);
    chain.at(7, 3000);
    let [row]: [RequestRow; 1] = asked(chain).try_into().unwrap();
    event(chain, &row, 0, "event: alive\ndata: {}");
    let refused = "event: refused\ndata: {\"status\": 500, \"error\": \"no shell\"}";
    event(chain, &row, 1, refused);
    let [woke]: [String; 1] = told(chain, thread).try_into().unwrap();
    assert!(
        woke.starts_with("bash call b1 (local-harness /v1/bash) ended: {\"refused\":"),
        "{woke}"
    );
    assert!(woke.contains("no shell") && woke.contains("500"), "{woke}");
}

fn put(chain: &MockChain, path: &str, bytes: &[u8]) -> BlobId {
    let base = match chain.query(files::PROGRAM, &files::Query::Head).unwrap() {
        files::Reply::Head(head) => head.map(|row| row.id),
        other => panic!("{other:?}"),
    };
    let chunk = chain.put_blob(files::CHUNK, bytes.to_vec()).unwrap();
    let put = files::Change::Put {
        path: path.into(),
        exec: false,
        meta: Default::default(),
        content: files::Content::Chunks {
            size: bytes.len() as u64,
            chunks: vec![chunk],
        },
    };
    let commit = files::Op::Commit {
        base,
        message: "seen before".into(),
        changes: vec![put],
    };
    abi::decode(&chain.submit(signed(ADA), files::PROGRAM, &commit).unwrap()).unwrap()
}

#[test]
fn an_image_a_rerun_brings_again_keeps_its_file_and_one_that_changed_is_written_over() {
    let world = local(world());
    let chain = &world.chain;
    let thread = world.opened("m1", "look", Some(WINDOW));
    let same = png(64, 48);
    put(chain, &format!("/home/Scout/seen/{thread}-i1-1.png"), &same);
    let older = put(chain, &format!("/home/Scout/seen/{thread}-i2-1.png"), &same);
    let calls = [
        ("i1", "read", json!({ "path": "/tmp/same.png" })),
        ("i2", "read", json!({ "path": "/tmp/new.png" })),
    ];
    turn(chain, 3, &calls);
    chain.at(5, 0);
    let (same_rows, new_rows): (Vec<RequestRow>, Vec<RequestRow>) = asked(chain)
        .into_iter()
        .partition(|row| body(chain, row)["path"] == "/tmp/same.png");
    let ([same_row], [new_row]) = (&same_rows[..], &new_rows[..]) else {
        panic!("one read of each: {same_rows:?} {new_rows:?}")
    };
    answer(chain, same_row.id, raw(200, "image/png", &same));
    let again = json_result(chain, thread, "i1");
    assert_eq!(
        again["read"]["snapshot"],
        json!(abi::hex(older.digest())),
        "the same bytes keep the file and commit nothing: {again}"
    );
    let newer = png(32, 16);
    answer(chain, new_row.id, raw(200, "image/png", &newer));
    let changed = json_result(chain, thread, "i2");
    let snapshot = changed["read"]["snapshot"].as_str().unwrap();
    assert_ne!(snapshot, abi::hex(older.digest()), "{changed}");
    assert_eq!(
        (&changed["read"]["width"], &changed["read"]["height"]),
        (&json!(32), &json!(16)),
        "{changed}"
    );
}

fn second_stream(world: &World, measured_at: u64) -> Vec<(String, String)> {
    let chain = &world.chain;
    world.opened("m1", "build it", Some(WINDOW));
    turn(
        chain,
        3,
        &[("b1", "bash", json!({ "command": "cargo test" }))],
    );
    chain.at(5, 1000);
    model_says(
        chain,
        json!([{ "type": "text", "text": "running" }]),
        "end_turn",
    );
    chain.at(6, 2000);
    chain.at(7, 3000);
    let [first]: [RequestRow; 1] = asked(chain).try_into().unwrap();
    let ended =
        "event: ended\ndata: {\"id\": 1, \"exit_code\": 0, \"stdout\": \"\", \"stderr\": \"\"}";
    event(chain, &first, 0, ended);
    chain.at(8, 4000);
    model_says(
        chain,
        json!([{ "type": "text", "text": "done" }]),
        "end_turn",
    );
    chain.at(75, 71_000);
    world.mention("m2", "again");
    chain.at(76, 72_000);
    let uses = json!([tool_use("b2", "bash", json!({ "command": "cargo build" }))]);
    model_says(chain, uses, "tool_use");
    chain.at(77, 73_000);
    relay_back(chain, "b2");
    chain.at(78, measured_at);
    chain.at(79, measured_at + 1000);
    let rows: Vec<RequestRow> = asked(chain)
        .into_iter()
        .filter(|row| row.id != first.id)
        .collect();
    let [second] = &rows[..] else {
        panic!("one more stream: {rows:?}")
    };
    head_of(second).headers
}

fn steady_beat() -> (String, String) {
    let steady = (73_000 - 1000) / (77 - 5);
    let every = egress::DEFAULT_WAIT * steady / 2 / 1000;
    ("every".to_owned(), every.to_string())
}

#[test]
fn a_slow_block_under_a_streams_measure_paces_its_heartbeat_from_the_steadier_clock() {
    let world = local(world());
    let headers = second_stream(&world, 84_600);
    let beat = steady_beat();
    assert!(
        headers.contains(&beat),
        "every {} from the clock up to the call, not from the slow block: {headers:?}",
        beat.1
    );
}

#[test]
fn a_halt_on_the_block_that_measures_a_stream_counts_once_and_leaves_its_heartbeat_steady() {
    let world = local(world());
    let headers = second_stream(&world, 73_000 + 90_000);
    let beat = steady_beat();
    assert!(
        headers.contains(&beat),
        "every {} from the clock up to the call, not from the halt: {headers:?}",
        beat.1
    );
}

#[test]
fn a_first_stream_with_a_halt_on_its_measuring_block_waits_a_block_and_paces_from_the_other() {
    let world = local(world());
    let chain = &world.chain;
    world.opened("m1", "build it", Some(WINDOW));
    turn(
        chain,
        3,
        &[("b1", "bash", json!({ "command": "cargo test" }))],
    );
    chain.at(5, 90_000);
    chain.at(6, 91_000);
    assert!(
        asked(chain).is_empty(),
        "with no block measured before the call, the request waits a block"
    );
    chain.at(7, 92_000);
    let [row]: [RequestRow; 1] = asked(chain).try_into().unwrap();
    let every = egress::DEFAULT_WAIT * 1000 / 2 / 1000;
    let beat = ("every".to_owned(), every.to_string());
    let headers = head_of(&row).headers;
    assert!(
        headers.contains(&beat),
        "every {every} from the block after the halt: {headers:?}"
    );
}

const FETCH: &str = r#"name = "fetch"
description = "Fetch a file by its path."

[input]
path = { type = "string", description = "the file's path" }
tag = { type = "string", description = "a query value" }

[[step]]
service = "local-harness"
method = "GET"
target = "/v1/files$path?tag=$tag"
as = "file"
"#;

#[test]
fn a_target_fills_each_value_percent_encoded_keeping_slashes_only_before_its_query() {
    let world = local(world());
    let chain = &world.chain;
    commit(chain, ADA, "/home/Scout/tools/fetch.toml", FETCH);
    world.opened("m1", "fetch it", Some(WINDOW));
    let input = json!({ "path": "/home/a b?c#d&e%f", "tag": "x y&z=/#%" });
    turn(chain, 3, &[("f1", "fetch", input)]);
    chain.at(5, 0);
    let [row]: [RequestRow; 1] = asked(chain).try_into().unwrap();
    assert_eq!(
        target(&row),
        "/v1/files/home/a%20b%3Fc%23d%26e%25f?tag=x%20y%26z%3D%2F%23%25"
    );
}

#[test]
fn a_lost_service_stream_says_what_it_ran_may_still_be_running_on_that_service() {
    let world = local(world());
    let chain = &world.chain;
    let thread = world.opened("m1", "build it", Some(WINDOW));
    turn(
        chain,
        3,
        &[("b1", "bash", json!({ "command": "cargo test" }))],
    );
    chain.at(5, 1000);
    chain.at(6, 2000);
    chain.at(7, 3000);
    let [row]: [RequestRow; 1] = asked(chain).try_into().unwrap();
    event(chain, &row, 0, "event: alive\ndata: {}");
    let end = egress::Op::EndStream {
        id: row.id,
        events: 1,
        end: egress::StreamEnd::Complete {
            status: 200,
            headers: vec![("content-type".into(), "text/event-stream".into())],
        },
    };
    chain.submit(signed(&NODE), egress::PROGRAM, &end).unwrap();
    let [woke]: [String; 1] = told(chain, thread).try_into().unwrap();
    let lost = "bash call b1 (local-harness /v1/bash) lost its stream, which closed with status 200 \
                before its end event";
    assert!(woke.starts_with(lost), "{woke}");
    let said =
        "what it ran may still be running on local-harness, so check there before running it again";
    assert!(woke.contains(said), "{woke}");
    assert!(
        !woke.contains("build.sh") && !woke.contains("pod"),
        "{woke}"
    );
}

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
        Request::Relay { .. } | Request::Crosspond { .. } => String::new(),
    }
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

fn on_its_line(chain: &MockChain, thread: u64, call: &str) -> Option<BlobId> {
    let line = [
        thread.to_be_bytes().as_slice(),
        b"/tool/",
        call.as_bytes(),
        b"/",
    ]
    .concat();
    let host = chain.host(PROGRAM).borrow();
    host.lines
        .iter()
        .find(|((holder, at), _)| holder == PROGRAM && *at == line)
        .map(|(_, id)| *id)
}

fn body_part(row: &RequestRow) -> BlobId {
    match &row.request {
        Request::Http { body, .. } => body[0],
        Request::Relay { .. } | Request::Crosspond { .. } => panic!("an HTTP request"),
    }
}

#[test]
fn a_service_steps_request_body_is_held_on_its_call_line_until_its_answer_comes_back() {
    let world = local(world());
    let chain = &world.chain;
    let thread = world.opened("m1", "read it", Some(WINDOW));
    turn(chain, 3, &[("h1", "read", json!({ "path": "/etc/hosts" }))]);
    chain.at(5, 0);
    let [row]: [RequestRow; 1] = asked(chain).try_into().unwrap();
    assert_eq!(on_its_line(chain, thread, "h1"), Some(body_part(&row)));
    let out = json!({ "text": "1\tlocalhost" });
    answer(chain, row.id, Answer::Output(response(200, out.clone())));
    assert_eq!(json_result(chain, thread, "h1")["read"], out);
    assert_eq!(on_its_line(chain, thread, "h1"), None);
}

#[test]
fn a_streamed_steps_request_body_is_held_until_its_stream_ends() {
    let world = local(world());
    let chain = &world.chain;
    let thread = world.opened("m1", "build it", Some(WINDOW));
    turn(
        chain,
        3,
        &[("b1", "bash", json!({ "command": "cargo test" }))],
    );
    chain.at(5, 1000);
    chain.at(6, 2000);
    chain.at(7, 3000);
    let [row]: [RequestRow; 1] = asked(chain).try_into().unwrap();
    assert_eq!(on_its_line(chain, thread, "b1"), Some(body_part(&row)));
    event(chain, &row, 0, "event: alive\ndata: {}");
    assert_eq!(on_its_line(chain, thread, "b1"), Some(body_part(&row)));
    event(chain, &row, 1, "event: ended\ndata: {\"exit_code\": 0}");
    assert_eq!(on_its_line(chain, thread, "b1"), None);
}

#[test]
fn a_note_on_a_stream_is_told_to_its_conversation_and_the_stream_goes_on_to_its_end() {
    let world = local(world());
    let chain = &world.chain;
    let thread = world.opened("m1", "build it", Some(WINDOW));
    turn(
        chain,
        3,
        &[("b1", "bash", json!({ "command": "cargo test" }))],
    );
    chain.at(5, 1000);
    chain.at(6, 2000);
    chain.at(7, 3000);
    let [row]: [RequestRow; 1] = asked(chain).try_into().unwrap();
    let line = "pod duckpond-7 is pending, waiting to be placed: 0/1 nodes are available";
    let note = format!("event: note\ndata: {{\"line\": \"{line}\"}}");
    event(chain, &row, 0, &note);
    let noted = told(chain, thread);
    assert_eq!(noted.len(), 1, "{noted:?}");
    let still = "bash call b1 (local-harness /v1/bash) is still running and notes: ";
    assert!(
        noted[0].starts_with(still) && noted[0].contains(line),
        "{noted:?}"
    );
    assert_eq!(on_its_line(chain, thread, "b1"), Some(body_part(&row)));
    event(chain, &row, 1, "event: ended\ndata: {\"exit_code\": 0}");
    let woke = told(chain, thread);
    assert_eq!(woke.len(), 2, "{woke:?}");
    let ended = "bash call b1 (local-harness /v1/bash) ended: ";
    assert!(woke[1].starts_with(ended), "{woke:?}");
    assert_eq!(on_its_line(chain, thread, "b1"), None);
}
