use std::collections::BTreeMap;

use ::program::schema::{Return, interface, returns, roots};
use ::program::{Error, Program, QueryCtx, invalid};
use borsh::schema::{BorshSchemaContainer, Definition, Fields};
use programs::Programs;

use super::*;
use crate::schema::tests::{code_with, described_code, listed, module};

fn schemed() -> World {
    schemed_with(code_with(&interface::<files::Files>()))
}

fn schemed_with(files_code: Vec<u8>) -> World {
    let mut world = world();
    world.chain.seat::<Programs>(programs::PROGRAM);
    let codes = [
        (chat::PROGRAM, code_with(&interface::<chat::Chat>())),
        (files::PROGRAM, files_code),
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
    let thread = world.opened("m1", "use the network", Some(WINDOW));
    turn(&world.chain, 3, calls);
    thread
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

pub(super) fn head(chain: &MockChain) -> BlobId {
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
        (
            "s_chat",
            "network_schema",
            json!({ "program": "chat", "whole": true }),
        ),
        (
            "s_files",
            "network_schema",
            json!({ "program": "files", "whole": true }),
        ),
        (
            "q_chat",
            "network_query",
            json!({ "program": "chat", "query": { "channel": { "channel_id": "general" } } }),
        ),
        (
            "q_files",
            "network_query",
            json!({ "program": "files", "query": { "stat": { "path": CONFIG } } }),
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
        shape["types"]["Op"]["enum"]["post_message"]["struct"]["thread"],
        json!({ "option": "u64" })
    );
    let files_shape = json_result(chain, thread, "s_files");
    assert_eq!(files_shape["types"]["Query"]["enum"]["head"], json!(null));

    let channel = json_result(chain, thread, "q_chat");
    let info = &channel["reply"]["channel"];
    assert_eq!(info["channel"]["name"], "general");
    let stat = json_result(chain, thread, "q_files");
    let snapshot = &stat["reply"]["stat"]["snapshot"];
    assert_eq!(
        *snapshot,
        json!({ "sha256": abi::hex(head(chain).digest()) })
    );
}

#[test]
fn an_op_is_the_agents_and_the_program_allows_or_refuses_it() {
    let world = schemed();
    let chain = &world.chain;
    let base = json!({ "sha256": abi::hex(head(chain).digest()) });
    let post = |channel: &str, id: &str| {
        json!({ "program": "chat", "op": { "post_message": {
            "channel_id": channel,
            "message_id": id,
            "blocks": [{ "paragraph": [{ "text": "hello", "marks": [] }] }],
            "thread": null,
        } } })
    };
    let commit = |path: &str| {
        json!({ "program": "files", "op": { "commit": {
            "base": base,
            "message": "notes",
            "changes": [{ "mkdir": { "path": path } }],
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
    let snapshot = json!({ "sha256": abi::hex(head(chain).digest()) });
    assert_eq!(home, json!({ "ok": true, "returned": snapshot }));
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
            json!({ "program": "chat", "query": { "shout": {} } }),
        ),
        (
            "m_field",
            "network_call",
            json!({ "program": "chat", "op": { "add_reaction": { "channel_id": "general", "seq": "first", "emoji": "+1" } } }),
        ),
        (
            "m_missing",
            "network_call",
            json!({ "program": "files", "op": { "pin": { "name": "v1" } } }),
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
        variant.starts_with(code::INVALID_INPUT) && variant.contains("shout"),
        "{variant}"
    );
    let field = refusal(chain, thread, "m_field");
    assert!(field.contains("op.add_reaction.seq"), "{field}");
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

// files' interface as a schema packed before interfaces named what ops return: op, query and
// reply alone.
fn before_returns(schema: BorshSchemaContainer) -> BorshSchemaContainer {
    let roots = roots(&schema).unwrap();
    let fields = [
        ("op", roots.op),
        ("query", roots.query),
        ("reply", roots.reply),
    ]
    .map(|(field, declaration)| (field.to_owned(), declaration.to_owned()));
    let mut definitions: BTreeMap<_, _> = schema
        .definitions()
        .map(|(declaration, definition)| (declaration.clone(), definition.clone()))
        .collect();
    let interface = Definition::Struct {
        fields: Fields::NamedFields(fields.to_vec()),
    };
    definitions.insert("Interface".to_owned(), interface);
    BorshSchemaContainer::new("Interface".to_owned(), definitions)
}

// files' op, query and reply, with what `RETURNS` says its ops return.
pub(super) struct Unsaid;

impl Program for Unsaid {
    const NAME: &'static str = files::PROGRAM;
    type Op = files::Op;
    type Query = files::Query;
    type Reply = files::Reply;

    fn query(_: &QueryCtx, _: files::Query) -> Result<files::Reply, Error> {
        Err(invalid("only its schema is listed"))
    }
}

struct Misread;

impl Program for Misread {
    const NAME: &'static str = files::PROGRAM;
    type Op = files::Op;
    type Query = files::Query;
    type Reply = files::Reply;
    const RETURNS: &'static [Return] = &[returns::<u8>("Commit")];

    fn query(_: &QueryCtx, _: files::Query) -> Result<files::Reply, Error> {
        Err(invalid("only its schema is listed"))
    }
}

pub(super) fn notes(chain: &MockChain) -> Value {
    let base = json!({ "sha256": abi::hex(head(chain).digest()) });
    json!({ "program": "files", "op": { "commit": {
        "base": base,
        "message": "notes",
        "changes": [{ "mkdir": { "path": "/home/Scout/notes" } }],
    } } })
}

// What the files `Commit` the agent calls answers, beside files' shapes, with `files_code` listed
// as files' code.
fn committed(files_code: Vec<u8>) -> (Value, Value, BlobId) {
    let world = schemed_with(files_code);
    let chain = &world.chain;
    let calls = [
        ("c_notes", "network_call", notes(chain)),
        (
            "s_files",
            "network_schema",
            json!({ "program": "files", "whole": true }),
        ),
    ];
    let thread = called(&world, &calls);
    let answer = json_result(chain, thread, "c_notes");
    let shape = json_result(chain, thread, "s_files");
    (answer, shape, head(chain))
}

fn returned_bytes(answer: &Value) -> BlobId {
    let bytes = design::unhex(answer["returned"].as_str().unwrap()).unwrap();
    abi::decode(&bytes).unwrap()
}

#[test]
fn an_output_a_schema_from_before_returns_names_no_type_for_stays_hex_and_says_why() {
    let before = before_returns(interface::<files::Files>());
    let (answer, shape, head) = committed(code_with(&before));
    assert_eq!(returned_bytes(&answer), head);
    let unread = answer["unread"].as_str().unwrap();
    let why = "the schema of program files names no type for what its ops return";
    assert!(unread.starts_with(why), "{unread}");
    assert!(shape.get("returns").is_none(), "{shape}");
}

#[test]
fn an_op_whose_program_names_no_return_for_it_answers_hex_and_names_the_op() {
    let (answer, shape, head) = committed(code_with(&interface::<Unsaid>()));
    assert_eq!(returned_bytes(&answer), head);
    let unread = answer["unread"].as_str().unwrap();
    assert!(
        unread.contains("names no type for what its op commit returns"),
        "{unread}"
    );
    assert_eq!(shape["returns"], json!({}));
}

#[test]
fn an_output_that_does_not_read_as_its_declared_type_falls_back_to_hex_and_says_why() {
    let (answer, shape, head) = committed(code_with(&interface::<Misread>()));
    assert_eq!(returned_bytes(&answer), head);
    let unread = answer["unread"].as_str().unwrap();
    let why = "the output does not read as u8, the type the schema of program files names for its op commit: ";
    assert!(
        unread.starts_with(why) && unread.ends_with("bytes are left after the value"),
        "{unread}"
    );
    assert_eq!(shape["returns"], json!({ "commit": "u8" }));
}

fn branch_oids(chain: &MockChain, branch: &str) -> Vec<String> {
    let asked = forge::Query::Log {
        repo: "notes".into(),
        from: forge::Revision::Ref(format!("refs/heads/{branch}").into_bytes()),
        exclude: None,
        page: PageRequest::first(16),
    };
    let forge::Reply::Log { page, .. } = chain.query(forge::PROGRAM, &asked).unwrap() else {
        panic!("a log answers Log")
    };
    page.items.into_iter().map(|commit| commit.oid).collect()
}

#[test]
fn a_forge_merge_reads_back_as_the_reply_forges_schema_names_for_it() {
    let mut world = super::forge_commit::forged(10_000);
    world.chain.seat::<Programs>(programs::PROGRAM);
    let forge_code = code_with(&interface::<forge::Forge>());
    listed(&world.chain, &[(forge::PROGRAM, forge_code)]);
    let chain = &world.chain;
    let thread = world.opened("m1", "merge a branch", Some(WINDOW));
    let commit = |id: &'static str, branch: &str| {
        let file = json!({ "path": format!("{id}.md"), "content": id });
        let input = json!({ "repo": "notes", "branch": branch, "files": [file], "message": id });
        (id, "forge_commit", input)
    };
    turn(chain, 3, &[commit("c_main", "main")]);
    turn(chain, 5, &[commit("c_side", "side")]);
    // both commits carry one time, so a log may list them in either order
    let [main]: [String; 1] = branch_oids(chain, "main").try_into().unwrap();
    let side = branch_oids(chain, "side")
        .into_iter()
        .find(|oid| *oid != main)
        .unwrap();
    let merge = json!({ "program": "forge", "op": { "merge": {
        "repo": "notes",
        "into": abi::hex(b"refs/heads/main"),
        "from": { "ref": abi::hex(b"refs/heads/side") },
        "expected_into": main,
        "expected_from": side,
        "result": side,
        "change": null,
    } } });
    let log = |branch: &str| {
        let from = json!({ "ref": { "utf8": format!("refs/heads/{branch}") } });
        let page = json!({ "after": null, "limit": 1 });
        let log = json!({ "repo": "notes", "from": from, "exclude": null, "page": page });
        json!({ "program": "forge", "query": { "log": log } })
    };
    let calls = [
        ("c_merge", "network_call", merge),
        (
            "s_forge",
            "network_schema",
            json!({ "program": "forge", "whole": true }),
        ),
        ("q_side", "network_query", log("side")),
        ("q_gone", "network_query", log("gone")),
    ];
    turn(chain, 7, &calls);
    // a ref written as text inside an enum reads as its bytes, and one forge lacks is named
    let read = json_result(chain, thread, "q_side");
    assert_eq!(read["reply"]["log"]["tip"], json!(side), "{read}");
    let gone = refusal(chain, thread, "q_gone");
    assert!(gone.contains("no ref refs/heads/gone"), "{gone}");

    let merged = json_result(chain, thread, "c_merge");
    let reply = &merged["returned"]["merged"];
    assert_eq!(reply["oid"], json!(side), "{merged}");
    assert_eq!(reply["change"], json!(null), "{merged}");
    assert!(reply["height"].is_u64(), "{merged}");
    assert!(merged.get("unread").is_none(), "{merged}");
    let merged_main = branch_oids(chain, "main");
    assert!(
        merged_main.len() == 2 && merged_main.contains(&side),
        "{merged_main:?}"
    );
    let shape = json_result(chain, thread, "s_forge");
    assert_eq!(shape["returns"]["merge"], json!("OpReply"));
    assert!(shape["returns"].get("push").is_none(), "{shape}");
    let reply_shape = &shape["types"]["OpReply"]["enum"]["merged"];
    assert!(reply_shape.is_object(), "{shape}");
}

const PODS: &str = "pods";

pub(super) fn served(chain: &MockChain) {
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

pub(super) fn pods_get(target: &str) -> Value {
    json!({ "program": "egress", "op": { "request": {
        "request": { "http": {
            "service": PODS,
            "head": { "method": "Get", "target": target, "headers": [] },
            "body": [],
        } },
        "delivery": "Whole",
        "wait": null,
    } } })
}

pub(super) fn pods_answer(chain: &MockChain, target: &str, output: Vec<u8>) {
    let asked = |row: &RequestRow| match &row.request {
        Request::Http { head, .. } => head.target == target,
        Request::Relay { .. } | Request::Crosspond { .. } => false,
    };
    let rows = assigned(chain, PODS);
    let row = rows.iter().find(|row| asked(row)).expect("pods is asked");
    answer(chain, row.id, Answer::Output(output));
}

pub(super) fn http_answer(status: u16, content_type: &str, body: &[u8]) -> Vec<u8> {
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

#[test]
fn an_op_or_query_written_as_json_text_is_read_as_the_json_it_holds() {
    let world = schemed();
    let chain = &world.chain;
    let query = json!({ "channel": { "channel_id": "general" } }).to_string();
    let op = json!({ "post_message": {
        "channel_id": "general",
        "message_id": "net-text",
        "blocks": [{ "paragraph": [{ "text": "hello", "marks": [] }] }],
        "thread": null,
    } })
    .to_string();
    let calls = [
        (
            "q_text",
            "network_query",
            json!({ "program": "chat", "query": query }),
        ),
        (
            "c_text",
            "network_call",
            json!({ "program": "chat", "op": op }),
        ),
        (
            "q_unit",
            "network_query",
            json!({ "program": "files", "query": "Head" }),
        ),
    ];
    let thread = called(&world, &calls);

    let channel = json_result(chain, thread, "q_text");
    assert_eq!(channel["reply"]["channel"]["channel"]["name"], "general");
    assert_eq!(json_result(chain, thread, "c_text"), json!({ "ok": true }));
    let asked = chat::Query::MessageById {
        message_id: "net-text".into(),
    };
    let chat::Reply::Message(Some(posted)) = chain.query(chat::PROGRAM, &asked).unwrap() else {
        panic!("the post landed")
    };
    assert_eq!(posted.text, "hello");
    let head = json_result(chain, thread, "q_unit");
    assert!(head["reply"]["head"].is_object(), "{head}");
}

// A world whose chat code tells of its ops and queries as founding packs it, beside files' code,
// which carries only its schema, valset's, which carries neither, and odd's, whose tools section
// is no JSON object.
fn described() -> World {
    let mut world = world();
    world.chain.seat::<Programs>(programs::PROGRAM);
    let chat_code = described_code(
        &interface::<chat::Chat>(),
        &<chat::Chat as Program>::tools(),
    );
    let codes = [
        (chat::PROGRAM, chat_code),
        (files::PROGRAM, code_with(&interface::<files::Files>())),
        ("valset", module(&[])),
        ("odd", module(&[(::program::tools::SECTION, b"[]")])),
    ];
    listed(&world.chain, &codes);
    world
}

#[test]
fn the_model_finds_a_program_unfolds_one_op_and_sends_its_example_edited() {
    let world = described();
    let chain = &world.chain;
    let told = <chat::Chat as Program>::tools();
    let post = told.ops.iter().find(|tool| tool.variant == "PostMessage");
    let post = post.unwrap().description;
    let calls = [
        ("d_all", "network_schema", json!({})),
        ("d_chat", "network_schema", json!({ "program": "chat" })),
        (
            "d_files",
            "network_schema",
            json!({ "program": "files", "whole": null }),
        ),
        (
            "d_post",
            "network_schema",
            json!({ "program": "chat", "op": "PostMessage" }),
        ),
        (
            "d_roots",
            "network_schema",
            json!({ "program": "chat", "query": "Roots" }),
        ),
        (
            "d_commit",
            "network_schema",
            json!({ "program": "files", "op": "Commit" }),
        ),
    ];
    let thread = called(&world, &calls);

    let all = json_result(chain, thread, "d_all");
    let about = |program: &str| {
        let rows = all["programs"].as_array().unwrap();
        let row = rows.iter().find(|row| row["program"] == program);
        row.map(|row| row["about"].clone())
    };
    assert_eq!(about("chat"), Some(json!(told.about)), "{all}");
    assert_eq!(about("files"), Some(Value::Null), "{all}");
    assert_eq!(about("valset"), Some(Value::Null), "{all}");
    let rows = all["programs"].as_array().unwrap();
    let odd = rows.iter().find(|row| row["program"] == "odd").unwrap();
    let unread = odd["unread"].as_str().unwrap();
    assert!(
        unread.contains("the tools section is no JSON object"),
        "{all}"
    );

    let index = json_result(chain, thread, "d_chat");
    assert_eq!(index["about"], json!(told.about));
    assert_eq!(index["ops"]["post_message"], json!(post));
    assert!(index["queries"]["roots"].is_string(), "{index}");
    assert!(
        index.get("note").is_none() && index.get("types").is_none(),
        "{index}"
    );
    let plain = json_result(chain, thread, "d_files");
    assert_eq!(plain["ops"]["commit"], Value::Null, "{plain}");
    let note = plain["note"].as_str().unwrap();
    assert!(note.contains("describes none of its ops"), "{note}");

    let unfolded = json_result(chain, thread, "d_post");
    assert_eq!(unfolded["description"], json!(post));
    let thread_field = &unfolded["shape"]["post_message"]["struct"]["thread"];
    assert_eq!(*thread_field, json!({ "option": "u64" }), "{unfolded}");
    let types = unfolded["types"].as_object().unwrap();
    assert!(
        !types.is_empty() && !types.contains_key("Op") && !types.contains_key("Query"),
        "{unfolded}"
    );
    assert!(unfolded.get("returns").is_none(), "{unfolded}");
    let roots = json_result(chain, thread, "d_roots");
    assert!(roots["description"].is_string(), "{roots}");
    assert!(roots["example"]["roots"].is_object(), "{roots}");
    assert!(roots["returns"].is_object(), "{roots}");
    let commit = json_result(chain, thread, "d_commit");
    assert!(commit.get("description").is_none(), "{commit}");
    assert!(commit["example"]["commit"].is_object(), "{commit}");
    let returned = commit["returns"].as_str().unwrap();
    assert!(commit["types"][returned].is_object(), "{commit}");

    let mut example = unfolded["example"].clone();
    example["post_message"]["channel_id"] = json!("general");
    example["post_message"]["message_id"] = json!("from-example");
    let send = json!({ "program": "chat", "op": example });
    turn(chain, 5, &[("c_example", "network_call", send)]);
    assert_eq!(
        json_result(chain, thread, "c_example"),
        json!({ "ok": true })
    );
    let asked = chat::Query::MessageById {
        message_id: "from-example".into(),
    };
    let chat::Reply::Message(Some(posted)) = chain.query(chat::PROGRAM, &asked).unwrap() else {
        panic!("the example's post landed")
    };
    assert_eq!(posted.author, Principal::Account(world.agent));
}

#[test]
fn a_variant_the_program_lacks_or_a_mixed_ask_is_refused_with_what_there_is() {
    let world = described();
    let chain = &world.chain;
    let calls = [
        (
            "r_variant",
            "network_schema",
            json!({ "program": "chat", "op": "Shout" }),
        ),
        (
            "r_side",
            "network_schema",
            json!({ "program": "chat", "query": "PostMessage" }),
        ),
        (
            "r_both",
            "network_schema",
            json!({ "program": "chat", "op": "PostMessage", "whole": true }),
        ),
        ("r_alone", "network_schema", json!({ "op": "PostMessage" })),
    ];
    let thread = called(&world, &calls);

    let variant = refusal(chain, thread, "r_variant");
    assert!(
        variant.starts_with(code::INVALID_INPUT)
            && variant.contains("program chat has no op Shout; its ops are ")
            && variant.contains("post_message"),
        "{variant}"
    );
    let side = refusal(chain, thread, "r_side");
    assert!(side.contains("has no query PostMessage"), "{side}");
    for call in ["r_both", "r_alone"] {
        let text = refusal(chain, thread, call);
        assert!(text.starts_with(code::INVALID_INPUT), "{text}");
    }
}

fn a_dm_with_the_agent(world: &World) -> String {
    let open = chat::Op::CreateDmChannel {
        counterpart: world.agent,
        name: "bo and scout".into(),
    };
    world
        .chain
        .submit(signed(BO), chat::PROGRAM, &open)
        .unwrap();
    let room = chat::dm_channel_id(world.bo, world.agent);
    let post = chat::Op::PostMessage {
        channel_id: room.clone(),
        message_id: "dm-1".into(),
        blocks: chat::parse_message("only for the two of us"),
        thread: None,
    };
    world
        .chain
        .submit(signed(BO), chat::PROGRAM, &post)
        .unwrap();
    room
}

#[test]
fn an_agent_reads_its_direct_room_through_its_tools() {
    let world = schemed();
    let room = a_dm_with_the_agent(&world);
    let roots = json!({ "roots": { "channel_id": room, "viewer": [], "page": {} } });
    let calls = [
        ("m_dm", "chat_messages", json!({ "channel": room })),
        (
            "q_dm",
            "network_query",
            json!({ "program": "chat", "query": roots }),
        ),
    ];
    let thread = called(&world, &calls);
    let read = json_result(&world.chain, thread, "m_dm");
    assert_eq!(read["messages"][0]["text"], "only for the two of us");
    let queried = json_result(&world.chain, thread, "q_dm");
    let first = &queried["reply"]["roots"]["items"][0];
    assert_eq!(first["text"], "only for the two of us");
}

#[test]
fn an_agents_own_as_reaches_chat_inside_harnesss_and_is_refused() {
    let world = schemed();
    let room = a_dm_with_the_agent(&world);
    let roots = chat::Query::Roots {
        channel_id: room,
        viewer: vec![],
        page: PageRequest::default(),
    };
    let reader = json!({ "account": world.bo });
    let asked = json!({ "as": { "reader": reader, "query": chat::hex(&abi::encode(&roots)) } });
    let calls = [(
        "q_as",
        "network_query",
        json!({ "program": "chat", "query": asked }),
    )];
    let thread = called(&world, &calls);
    let refused = refusal(&world.chain, thread, "q_as");
    assert!(refused.contains("an As inside an As"), "{refused}");
}

#[test]
fn an_agent_starts_itself_a_fresh_conversation_its_managers_list_and_policy_refuse_others() {
    let mut world = world();
    world.chain.seat::<Programs>(programs::PROGRAM);
    world.chain.seat::<ownership::Ownership>(ownership::PROGRAM);
    listed(
        &world.chain,
        &[(PROGRAM, code_with(&interface::<Harness>()))],
    );
    let chain = &world.chain;
    let agent = world.agent;
    let claim = Op::ClaimConversations { agent };
    chain.submit(signed(ADA), PROGRAM, &claim).unwrap();
    let strict = ownership::Op::Create {
        name: "mine".into(),
        write: ownership::Writers::Owner,
    };
    let made = chain
        .submit(signed(ADA), ownership::PROGRAM, &strict)
        .unwrap();
    let relink = ownership::Op::Relink {
        program: PROGRAM.into(),
        key: crate::state::CONVERSATIONS.key(&agent),
        policy: abi::decode(&made).unwrap(),
    };
    chain
        .submit(signed(ADA), ownership::PROGRAM, &relink)
        .unwrap();
    let refused = chain
        .submit(signed(BO), PROGRAM, &Op::NewThread { agent })
        .unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED, "anyone else still asks");

    let calls = [
        (
            "n_self",
            "network_call",
            json!({ "program": PROGRAM, "op": { "new_thread": { "agent": agent } } }),
        ),
        (
            "f_self",
            "network_call",
            json!({ "program": PROGRAM, "op": { "fork_thread": { "agent": agent } } }),
        ),
    ];
    let thread = called(&world, &calls);
    let renewed = json_result(chain, thread, "n_self");
    let fresh = renewed["returned"]
        .as_u64()
        .expect("the fresh conversation's id");
    assert_ne!(fresh, thread);
    let forked = refusal(chain, thread, "f_self");
    assert!(forked.starts_with(code::UNAUTHORIZED), "{forked}");
}
