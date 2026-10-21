//! The agents on a `MockChain` with identity, attribution, awaits, scheduler, chat, files, egress
//! and valset seated: an agent is a program account whose script calls harness and names its
//! configuration in files; a mention, a direct message or a reply in a chat thread it took part
//! in reaches it, the node serving its model's egress service answers as the model would, whole
//! or one streamed event at a time, and every tool call runs once its relay comes back, as the
//! agent.
mod accounts;
mod blob_put;
mod budget;
mod chat_channels;
mod chat_messages;
mod chat_search;
mod compact_at;
mod config;
mod continued;
mod conversations;
mod file_tools;
mod final_errors;
mod forge_commit;
mod forge_copy;
mod forge_review;
mod forge_symbols;
mod forge_tools;
mod listen;
mod models;
mod network;
mod node_activity;
mod notices;
mod owned;
mod pictures;
mod pods;
mod reads;
mod resume;
mod routes;
mod rules;
mod schedule;
mod server_tools;
mod service_tools;
mod services;
mod stale;
mod stranded;
mod streams;
mod thinking;
mod wait;
mod wait_file;
mod wait_forge;
mod wakes;
mod woken;
mod workspace;

use ::program::{Error, MockChain, Origin, Principal, code};
use egress::{Answer, Egress, RELAY, Request, RequestRow, http};
use serde_json::{Value, json};
use valset::{Genesis, Member, Valset};

use crate::*;

const NODE: [u8; 32] = [1; 32];
const WINDOW: u64 = 200_000;
const ELSEWHERE: &str = "acme-harness";

thread_local! {
    static SEATED: std::cell::RefCell<String> = std::cell::RefCell::new(PROGRAM.to_owned());
}

fn harness() -> String {
    SEATED.with(|seated| seated.borrow().clone())
}
const SERVICE: &str = "models";
const ADA: &[u8] = b"ada-key";
const BO: &[u8] = b"bo-key";
const MODEL: &str = "claude-test";
const CONFIG: &str = "/home/Ada/agents/scout.md";

struct World {
    chain: MockChain,
    ada: AccountNumber,
    bo: AccountNumber,
    agent: AccountNumber,
}

fn signed(key: &[u8]) -> Origin {
    Origin::Signed(key.to_vec())
}

fn number(output: Vec<u8>) -> AccountNumber {
    abi::decode(&output).unwrap()
}

fn identity_op(chain: &MockChain, key: &[u8], op: identity::Op) -> Result<Vec<u8>, Error> {
    chain.submit(signed(key), identity::PROGRAM, &op)
}

const PROVIDER: &str = "anthropic";

fn pin_of(provider: &str) -> Pin {
    Pin {
        service: None,
        provider: Some(provider.into()),
    }
}

// the configuration as its file says it, naming the provider whose route reaches its model
fn md(config: &Config) -> String {
    config.write(&pin_of(PROVIDER))
}

fn set_route(services: &[(&str, Wire)]) -> Op {
    let services = services
        .iter()
        .map(|(service, wire)| RouteService {
            service: (*service).into(),
            wire: *wire,
        })
        .collect();
    Op::SetRoute {
        provider: PROVIDER.into(),
        services,
    }
}

fn scout() -> Config {
    Config {
        model: MODEL.into(),
        max_tokens: 1024,
        wait: None,
        thinking: None,
        server_tools: Vec::new(),
        listen: Vec::new(),
        settle: None,
        compact_at: None,
        system: "Keep notes in /shared.".into(),
    }
}

fn commit(chain: &MockChain, key: &[u8], path: &str, text: &str) -> BlobId {
    let base = match chain.query(files::PROGRAM, &files::Query::Head).unwrap() {
        files::Reply::Head(head) => head.map(|row| row.id),
        other => panic!("{other:?}"),
    };
    let commit = files::Op::Commit {
        base,
        message: "configure".into(),
        changes: vec![files::Change::Put {
            path: path.into(),
            exec: false,
            meta: Default::default(),
            content: files::Content::Chunks {
                size: text.len() as u64,
                chunks: vec![
                    chain
                        .put_blob(files::CHUNK, text.as_bytes().to_vec())
                        .unwrap(),
                ],
            },
        }],
    };
    abi::decode(&chain.submit(signed(key), files::PROGRAM, &commit).unwrap()).unwrap()
}

fn set_list(chain: &MockChain, key: &[u8], program: &str, at: String, list: ::program::Ownership) {
    let set = access::Op::Set {
        program: program.into(),
        key: at,
        list: Some(list),
    };
    chain.submit(signed(key), "access", &set).unwrap();
}

fn list(chain: &MockChain, key: &[u8], path: &str, write: ::program::Ownership) {
    set_list(chain, key, files::PROGRAM, path.into(), write);
}

fn listed(
    chain: &MockChain,
    path: &str,
) -> (Option<::program::Party>, Option<::program::Ownership>) {
    let ask = |query| chain.query::<_, access::Reply>("access", &query).unwrap();
    let owner = match ask(access::Query::Resource {
        program: files::PROGRAM.into(),
        key: path.into(),
    }) {
        access::Reply::Resource(resource) => resource.owner,
        other => panic!("{other:?}"),
    };
    let list = match ask(access::Query::List {
        program: files::PROGRAM.into(),
        key: path.into(),
    }) {
        access::Reply::List(list) => list,
        other => panic!("{other:?}"),
    };
    (owner, list)
}

fn scripted(chain: &MockChain, agent: AccountNumber, path: &str) {
    let script = identity::Op::SetScript {
        account: agent,
        script: Some(identity::Script {
            calls: harness(),
            config: path.into(),
        }),
    };
    identity_op(chain, ADA, script).unwrap();
}

fn world() -> World {
    world_as(PROGRAM)
}

fn world_as(id: &str) -> World {
    SEATED.with(|seated| *seated.borrow_mut() = id.to_owned());
    let mut chain = MockChain::default();
    chain.seat::<identity::Identity>(identity::PROGRAM);
    chain.seat::<Valset>("valset");
    chain.seat::<awaits::Awaits>(awaits::PROGRAM);
    chain.seat::<scheduler::Scheduler>(scheduler::PROGRAM);
    chain.seat_awaiting::<Egress>(egress::PROGRAM);
    chain.seat::<attribution::Attribution>(attribution::PROGRAM);
    chain.seat_awaiting::<Harness>(id);
    chain.seat::<files::Files>(files::PROGRAM);
    chain.seat::<chat::Chat>(chat::PROGRAM);
    chain.seat::<access::Access>("access");
    for program in [
        "access",
        "valset",
        egress::PROGRAM,
        attribution::PROGRAM,
        id,
        files::PROGRAM,
        chat::PROGRAM,
    ] {
        let register = identity::Op::RegisterProgram {
            program: program.into(),
        };
        chain
            .submit(Origin::Root, identity::PROGRAM, &register)
            .unwrap();
    }
    let genesis = Genesis {
        validators: vec![Member {
            key: NODE.to_vec(),
            address: None,
            halo: "node.example.org:9100".into(),
        }],
        member_cap: 8,
    };
    chain.init("valset", &genesis).unwrap();
    chain.init(id, &()).unwrap();
    let serve = egress::Op::Serve {
        services: vec![SERVICE.into(), RELAY.into()],
    };
    chain
        .submit(signed(&NODE), egress::PROGRAM, &serve)
        .unwrap();
    let person = |key, name: &str| {
        let create = identity::Op::Create {
            name: name.into(),
            scheme: ::program::Scheme::Ed25519,
        };
        number(identity_op(&chain, key, create).unwrap())
    };
    let ada = person(ADA, "Ada");
    let register = egress::Op::Register {
        service: SERVICE.into(),
    };
    let assign = egress::Op::Assign {
        service: SERVICE.into(),
        nodes: vec![NODE.to_vec()],
    };
    for op in [register, assign] {
        chain.submit(signed(ADA), egress::PROGRAM, &op).unwrap();
    }
    let bo = person(BO, "Bo");
    let create = identity::Op::CreateAgent {
        name: "Scout".into(),
    };
    let agent = number(identity_op(&chain, ADA, create).unwrap());
    let ccs = set_route(&[(SERVICE, Wire::AnthropicMessages)]);
    chain.submit(signed(ADA), id, &ccs).unwrap();
    commit(&chain, ADA, CONFIG, &md(&scout()));
    list(&chain, ADA, CONFIG, ::program::Ownership::Owner);
    scripted(&chain, agent, CONFIG);
    let general = chat::Op::CreateChannel {
        channel_id: "general".into(),
        name: "general".into(),
        post_policy: chat::PostPolicy::Open,
    };
    chain.submit(signed(ADA), chat::PROGRAM, &general).unwrap();
    World {
        chain,
        ada,
        bo,
        agent,
    }
}

impl World {
    fn post(&self, key: &[u8], channel: &str, id: &str, text: &str) {
        let op = chat::Op::PostMessage {
            channel_id: channel.into(),
            message_id: id.into(),
            blocks: chat::parse_message(text),
            thread: None,
        };
        self.chain.submit(signed(key), chat::PROGRAM, &op).unwrap();
    }

    fn mention(&self, id: &str, text: &str) -> u64 {
        self.post(ADA, "general", id, &format!("<@{}> {text}", self.agent));
        self.threads()[0].id
    }

    fn opened(&self, id: &str, text: &str, window: Option<u64>) -> u64 {
        let thread = self.mention(id, text);
        self.chain.at(2, 0);
        learn(&self.chain, window);
        thread
    }

    fn threads(&self) -> Vec<ThreadRow> {
        let asked = Query::Threads {
            agent: self.agent,
            page: PageRequest::default(),
        };
        let Reply::Threads(page) = self.chain.query(&harness(), &asked).unwrap() else {
            panic!()
        };
        page.items
    }
}

fn answering(world: &World, ownership: ::program::Ownership) {
    let set = identity::Op::SetOwnership {
        account: world.agent,
        ownership,
    };
    identity_op(&world.chain, ADA, set).unwrap();
}

fn manager_only(world: &World) {
    answering(world, ::program::Ownership::Owner);
}

fn new_thread_retiring_the_current_one(world: &World) {
    let fresh = Op::NewThread { agent: world.agent };
    world.chain.submit(signed(ADA), PROGRAM, &fresh).unwrap();
}

fn assigned(chain: &MockChain, service: &str) -> Vec<RequestRow> {
    let asked = egress::Query::Assigned { key: NODE.to_vec() };
    let egress::Reply::Requests(rows) = chain.query(egress::PROGRAM, &asked).unwrap() else {
        panic!()
    };
    rows.into_iter()
        .filter(|row| row.request.service() == service)
        .collect()
}

fn body(chain: &MockChain, row: &RequestRow) -> Value {
    let host = chain.host(egress::PROGRAM).borrow();
    let mut bytes = Vec::new();
    for part in row.request.parts() {
        bytes.extend(&host.blobs[part].body);
    }
    serde_json::from_slice(&bytes).expect("the parts join into one JSON request")
}

fn response(status: u16, body: Value) -> Vec<u8> {
    abi::encode(&http::Response {
        status,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    })
}

fn reply(content: Value, stop: &str, input_tokens: u64) -> Vec<u8> {
    let body = json!({
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "model": "claude-test",
        "content": content,
        "stop_reason": stop,
        "usage": { "input_tokens": input_tokens, "output_tokens": 5, "cache_read_input_tokens": 3 },
    });
    response(200, body)
}

fn open_request(chain: &MockChain) -> (RequestRow, http::Head) {
    let [request] = assigned(chain, SERVICE)
        .try_into()
        .expect("one model call open");
    let Request::Http { head, .. } = request.request.clone() else {
        panic!("the model is asked over http");
    };
    (request, head)
}

fn path(head: &http::Head) -> &str {
    &head.target
}

fn learn(chain: &MockChain, window: Option<u64>) {
    let (request, head) = open_request(chain);
    assert_eq!(
        (head.method, path(&head)),
        (http::Method::Get, "/v1/models/claude-test")
    );
    let info = json!({ "type": "model", "id": "claude-test", "max_input_tokens": window });
    answer(chain, request.id, Answer::Output(response(200, info)));
}

fn answer(chain: &MockChain, id: u64, answer: Answer) {
    let op = egress::Op::Answer { id, answer };
    chain
        .submit(Origin::Signed(NODE.to_vec()), egress::PROGRAM, &op)
        .unwrap();
}

fn model_says(chain: &MockChain, content: Value, stop: &str) {
    model_says_after(chain, content, stop, 10);
}

fn model_says_after(chain: &MockChain, content: Value, stop: &str, input_tokens: u64) {
    let (request, head) = open_request(chain);
    assert_eq!(path(&head), "/v1/messages");
    let said = reply(content, stop, input_tokens);
    answer(chain, request.id, Answer::Output(said));
}

fn relay_back(chain: &MockChain, call: &str) {
    let rows = assigned(chain, RELAY);
    let row = rows
        .iter()
        .find(|row| {
            row.request
                == Request::Relay {
                    note: call.as_bytes().to_vec(),
                }
        })
        .expect("the call is relaying");
    answer(chain, row.id, Answer::Output(Vec::new()));
}

fn frames(chain: &MockChain, thread: u64) -> Vec<Body> {
    let asked = Query::Frames {
        thread,
        page: PageRequest::default(),
    };
    let Reply::Frames(page) = chain.query(&harness(), &asked).unwrap() else {
        panic!()
    };
    page.items.into_iter().map(|frame| frame.body).collect()
}

fn held(chain: &MockChain, thread: u64) -> Vec<Body> {
    let Reply::Held(held) = chain.query(&harness(), &Query::Held { thread }).unwrap() else {
        panic!()
    };
    held
}

fn result_of(chain: &MockChain, thread: u64, call: &str) -> (String, bool) {
    frames(chain, thread)
        .into_iter()
        .chain(held(chain, thread))
        .find_map(|body| match body {
            Body::Result {
                call: done,
                text,
                failed,
                ..
            } if done == call => Some((text, failed)),
            _ => None,
        })
        .expect("the call has a result")
}

fn thread_row(chain: &MockChain, thread: u64) -> ThreadRow {
    let Reply::Thread(Some(row)) = chain.query(&harness(), &Query::Thread { thread }).unwrap()
    else {
        panic!()
    };
    row
}

fn tool_use(id: &str, name: &str, input: Value) -> Value {
    json!({ "type": "tool_use", "id": id, "name": name, "input": input })
}

#[test]
fn a_mention_by_its_manager_asks_the_model_in_the_agents_conversation() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "note that the network is up", Some(WINDOW));
    let (request, head) = open_request(chain);
    assert_eq!(
        (head.method, path(&head)),
        (http::Method::Post, "/v1/messages")
    );
    head.check_path().unwrap();
    let sent = body(chain, &request);
    assert_eq!(sent["model"], "claude-test");
    let system = sent["system"].as_array().unwrap();
    assert_eq!(system.len(), 2, "{system:?}");
    assert_eq!(system[0]["text"], "Keep notes in /shared.");
    assert!(system[0].get("cache_control").is_none(), "{system:?}");
    let manual = system[1]["text"].as_str().unwrap();
    assert!(
        manual.starts_with(&format!(
            "ducktape resource manual\n- Account: Scout, account {}, managed by account {}.",
            world.agent, world.ada
        )),
        "{manual}"
    );
    assert_eq!(system[1]["cache_control"]["type"], "ephemeral");
    let defined = sent["tools"].as_array().unwrap();
    assert_eq!(defined.len(), tools::CATALOG.len());
    assert!(
        defined
            .iter()
            .all(|tool| tool["eager_input_streaming"] == true),
        "every tool's input streams as the model writes it: {defined:?}"
    );
    let text = sent["messages"][0]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.starts_with(&format!(
            "Ada (account {}) mentioned you in chat channel general, message 1",
            world.ada
        )),
        "{text}"
    );
    assert!(text.ends_with("note that the network is up"), "{text}");
    let row = thread_row(chain, thread);
    assert_eq!(
        (row.agent, row.manager, row.place.as_str()),
        (world.agent, world.ada, "chat/general")
    );

    world.mention("m2", "still there?");
    assert_eq!(world.threads().len(), 1, "one conversation");
    assert_eq!(
        assigned(chain, SERVICE).len(),
        1,
        "one model call at a time"
    );
    assert!(
        matches!(&held(chain, thread)[..], [Body::Post { text, .. }] if text.ends_with("still there?"))
    );
    chain.at(3, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Hi." }]),
        "end_turn",
    );
    let order: Vec<Option<Role>> = frames(chain, thread).iter().map(Body::role).collect();
    assert_eq!(
        order,
        [Some(Role::User), Some(Role::Assistant), Some(Role::User)]
    );
    assert_eq!(
        assigned(chain, SERVICE).len(),
        1,
        "the held mention asks again"
    );
}

#[test]
fn a_direct_message_reaches_the_agent_and_only_those_it_answers_are_heard() {
    let world = world();
    let chain = &world.chain;
    manager_only(&world);
    world.post(
        BO,
        "general",
        "b1",
        &format!("<@{}> do my bidding", world.agent),
    );
    let [dropped]: [ThreadRow; 1] = world.threads().try_into().unwrap();
    let [told]: [String; 1] = notices(chain, dropped.id).try_into().unwrap();
    assert!(
        told.starts_with("dropped a mention"),
        "Scout does not answer Bo: {told}"
    );
    assert!(assigned(chain, SERVICE).is_empty(), "a notice asks nothing");

    let open = chat::Op::CreateDmChannel {
        counterpart: world.agent,
        name: "scout".into(),
    };
    chain.submit(signed(ADA), chat::PROGRAM, &open).unwrap();
    let room = chat::dm_channel_id(world.ada, world.agent);
    world.post(ADA, &room, "d1", &format!("a word, <@{}>", world.agent));
    let [thread] = world.threads().try_into().unwrap();
    assert_eq!(
        (thread.id, thread.place.as_str()),
        (dropped.id, "chat/general"),
        "the one conversation opened where Bo's mention was dropped"
    );
    let posts: Vec<String> = frames(chain, thread.id)
        .into_iter()
        .filter_map(|body| match body {
            Body::Post { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(
        posts.len(),
        1,
        "a mention in a direct message is heard once"
    );
    assert!(posts[0].contains("mentioned you"), "{}", posts[0]);

    answering(
        &world,
        ::program::Ownership::Listed(vec![::program::Party::Account(world.bo)]),
    );
    world.post(BO, "general", "b2", &format!("<@{}> now?", world.agent));
    let [same] = world.threads().try_into().unwrap();
    assert_eq!(
        same.id, thread.id,
        "another person continues the one conversation"
    );
    let bo = format!(
        "Bo (account {}) mentioned you in chat channel general",
        world.bo
    );
    assert!(
        frames(chain, thread.id)
            .into_iter()
            .chain(held(chain, thread.id))
            .any(|body| matches!(body, Body::Post { text, .. } if text.starts_with(&bo))),
        "{bo}"
    );
}

#[test]
fn a_reply_in_a_thread_the_agent_took_part_in_reaches_it_unmentioned_and_is_answered_there() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "start a thread", Some(WINDOW));
    let in_thread = |id: &str, text: &str| {
        tool_use(
            id,
            "chat_post",
            json!({ "channel": "general", "text": text, "reply_to": 1 }),
        )
    };
    chain.at(3, 0);
    model_says(chain, json!([in_thread("toolu_a", "on it")]), "tool_use");
    chain.at(4, 0);
    relay_back(chain, "toolu_a");
    chain.at(5, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Posted." }]),
        "end_turn",
    );
    chain.at(6, 0);
    let follow_up = chat::Op::PostMessage {
        channel_id: "general".into(),
        message_id: "a3".into(),
        blocks: chat::parse_message("and the second part?"),
        thread: Some(1),
    };
    chain
        .submit(signed(ADA), chat::PROGRAM, &follow_up)
        .unwrap();
    assert_eq!(
        world.threads().len(),
        1,
        "the agent's one conversation continues"
    );
    let posts: Vec<String> = frames(chain, thread)
        .into_iter()
        .filter_map(|body| match body {
            Body::Post { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    let [_, heard] = &posts[..] else {
        panic!("{posts:?}")
    };
    assert!(
        heard.starts_with(&format!(
            "Ada (account {}) continued a thread with you in chat channel general, message 3, \
             in the thread of message 1 (reply_to 1):",
            world.ada
        )),
        "{heard}"
    );
    assert!(heard.ends_with("and the second part?"), "{heard}");
    chain.at(7, 0);
    model_says(
        chain,
        json!([in_thread("toolu_b", "the second part")]),
        "tool_use",
    );
    chain.at(8, 0);
    relay_back(chain, "toolu_b");
    let asked = chat::Query::MessageById {
        message_id: format!("harness-{thread}-toolu_b"),
    };
    let chat::Reply::Message(Some(answered)) = chain.query(chat::PROGRAM, &asked).unwrap() else {
        panic!()
    };
    assert_eq!(
        (answered.author, answered.thread),
        (Principal::Account(world.agent), Some(1))
    );
}

#[test]
fn an_agent_that_answers_a_program_hears_what_that_program_posts_as_itself() {
    let world = world();
    let chain = &world.chain;
    manager_only(&world);
    let post = |id: &str| chat::Op::PostMessage {
        channel_id: "general".into(),
        message_id: id.into(),
        blocks: chat::parse_message(&format!("<@{}> from egress", world.agent)),
        thread: None,
    };
    let by_egress = Origin::Program(egress::PROGRAM.into());
    chain
        .submit(by_egress.clone(), chat::PROGRAM, &post("e1"))
        .unwrap();
    let [row]: [ThreadRow; 1] = world.threads().try_into().unwrap();
    let told = notices(chain, row.id);
    assert_eq!(told.len(), 1, "Scout answers its manager only");
    answering(
        &world,
        ::program::Ownership::Listed(vec![::program::Party::Program(egress::PROGRAM.into())]),
    );
    world.post(BO, "general", "b1", &format!("<@{}> me too?", world.agent));
    let told = notices(chain, row.id);
    assert_eq!(told.len(), 2, "a listed program is not Bo");
    chain.submit(by_egress, chat::PROGRAM, &post("e2")).unwrap();
    assert_eq!(world.threads().len(), 1);
    let heard = frames(chain, row.id)
        .into_iter()
        .filter(|body| matches!(body, Body::Post { .. }))
        .count();
    assert_eq!(heard, 1);
}

// a runtime off the chain asks identity `Hears`, the one rule harness delivers by: a stranger is
// heard until the manager narrows whom the agent answers, dropped then, and heard again once listed
#[test]
fn identity_hears_whom_harness_delivers() {
    let world = world();
    let chain = &world.chain;
    let hears = |from: AccountNumber| {
        let asked = identity::Query::Hears {
            account: world.agent,
            from,
        };
        match chain.query(identity::PROGRAM, &asked).unwrap() {
            identity::Reply::Hears(hears) => hears,
            other => panic!("{other:?}"),
        }
    };
    assert!(hears(world.bo) && hears(world.ada));
    manager_only(&world);
    assert!(!hears(world.bo) && hears(world.ada));
    world.post(BO, "general", "b1", &format!("<@{}> psst", world.agent));
    let [row]: [ThreadRow; 1] = world.threads().try_into().unwrap();
    assert_eq!(notices(chain, row.id).len(), 1, "Bo's mention is dropped");
    answering(
        &world,
        ::program::Ownership::Listed(vec![::program::Party::Account(world.bo)]),
    );
    assert!(hears(world.bo));
    world.post(BO, "general", "b2", &format!("<@{}> now?", world.agent));
    let heard = frames(chain, row.id)
        .into_iter()
        .chain(held(chain, row.id))
        .filter(|body| matches!(body, Body::Post { .. }))
        .count();
    assert_eq!(heard, 1, "Bo is heard once listed");
}

#[test]
fn an_agent_taken_back_from_harness_or_suspended_hears_nothing() {
    let world = world();
    let chain = &world.chain;
    let suspend = identity::Op::Suspend {
        account: world.agent,
    };
    identity_op(chain, ADA, suspend).unwrap();
    world.post(ADA, "general", "m1", &format!("<@{}> hello?", world.agent));
    assert!(
        world.threads().is_empty(),
        "a suspended agent hears nothing"
    );
    let resume = identity::Op::Resume {
        account: world.agent,
    };
    identity_op(chain, ADA, resume).unwrap();
    let taken_back = identity::Op::SetScript {
        account: world.agent,
        script: None,
    };
    identity_op(chain, ADA, taken_back).unwrap();
    world.post(ADA, "general", "m2", &format!("<@{}> hello?", world.agent));
    assert!(
        world.threads().is_empty(),
        "an agent with no script hears nothing"
    );
    scripted(chain, world.agent, CONFIG);
    world.post(ADA, "general", "m3", &format!("<@{}> hello?", world.agent));
    assert_eq!(world.threads().len(), 1);
}

#[test]
fn every_tool_call_runs_a_block_later_and_the_model_is_asked_once_all_results_are_back() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "note it", Some(WINDOW));
    chain.at(3, 0);
    let calls = json!([
        { "type": "text", "text": "On it." },
        tool_use("toolu_w", "files_write", json!({ "path": "/shared/note.md", "content": "hello" })),
        tool_use("toolu_l", "files_list", json!({ "path": "/shared" })),
        tool_use("toolu_x", "no_such_tool", json!({})),
        tool_use("toolu_bad", "files_read", json!({ "offset": 3 })),
    ]);
    model_says(chain, calls, "tool_use");
    let bodies = frames(chain, thread);
    assert!(matches!(&bodies[2], Body::Refused { call, .. } if call == "toolu_x"));
    assert!(matches!(&bodies[3], Body::Refused { problem, .. } if problem.contains("path")));
    assert_eq!(bodies.len(), 4);
    assert_eq!(assigned(chain, RELAY).len(), 2);
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "the model waits for the results"
    );

    let same_block = egress::Op::Answer {
        id: assigned(chain, RELAY)[0].id,
        answer: Answer::Output(Vec::new()),
    };
    let refused = chain.submit(signed(&NODE), egress::PROGRAM, &same_block);
    assert_eq!(refused.unwrap_err().code, code::WRONG_STATE);

    chain.at(4, 0);
    relay_back(chain, "toolu_l");
    let (listed, failed) = result_of(chain, thread, "toolu_l");
    assert!(!failed);
    assert_eq!(
        serde_json::from_str::<Value>(&listed).unwrap()["entries"],
        json!([])
    );
    assert!(assigned(chain, SERVICE).is_empty());
    relay_back(chain, "toolu_w");
    let (written, failed) = result_of(chain, thread, "toolu_w");
    assert!(!failed, "{written}");
    let asked = files::Query::Read {
        path: "/shared/note.md".into(),
        at: None,
        offset: 0,
        len: 100,
    };
    let files::Reply::Contents(contents) = chain.query(files::PROGRAM, &asked).unwrap() else {
        panic!()
    };
    assert_eq!(contents.bytes, b"hello");
    let files::Reply::Head(Some(head)) = chain.query(files::PROGRAM, &files::Query::Head).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        head.author,
        Principal::Account(world.agent),
        "the agent is the writer"
    );
    assert_eq!(
        serde_json::from_str::<Value>(&written).unwrap()["snapshot"],
        abi::hex(head.id.digest())
    );
    assert_eq!(thread_row(chain, thread).open_calls, 0);

    let [next] = assigned(chain, SERVICE).try_into().unwrap();
    let sent = body(chain, &next);
    let messages = sent["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[1]["content"][1]["type"], "tool_use");
    let results = messages[2]["content"].as_array().unwrap();
    let answered: Vec<(&str, bool)> = results
        .iter()
        .map(|block| {
            assert_eq!(block["type"], "tool_result");
            (
                block["tool_use_id"].as_str().unwrap(),
                block["is_error"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        answered,
        [
            ("toolu_x", true),
            ("toolu_bad", true),
            ("toolu_l", false),
            ("toolu_w", false)
        ]
    );
}

#[test]
fn input_that_arrives_while_a_call_runs_is_held_and_follows_its_result() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "look", Some(WINDOW));
    chain.at(3, 0);
    let call = tool_use("toolu_l", "files_list", json!({ "path": "/" }));
    model_says(chain, json!([call]), "tool_use");
    world.mention("m2", "and also");
    assert_eq!(held(chain, thread).len(), 1);
    chain.at(4, 0);
    relay_back(chain, "toolu_l");
    let [again] = assigned(chain, SERVICE).try_into().unwrap();
    let sent = body(chain, &again);
    let last = sent["messages"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["role"], "user");
    assert_eq!(last["content"][0]["type"], "tool_result");
    let text = last["content"][1]["text"].as_str().unwrap();
    assert!(text.ends_with("and also"), "{text}");
    chain.at(5, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Done." }]),
        "end_turn",
    );
    assert!(assigned(chain, SERVICE).is_empty());
    let row = thread_row(chain, thread);
    assert_eq!((row.asking, row.due()), (Asking::Idle, false));
    assert_eq!(chain.frames(), 0);
}

#[test]
fn a_call_whose_relay_fails_fails_once_and_the_model_hears_it() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "look", Some(WINDOW));
    chain.at(3, 0);
    let call = tool_use("toolu_l", "files_list", json!({ "path": "/" }));
    model_says(chain, json!([call]), "tool_use");
    chain.at(4, 0);
    let [row] = assigned(chain, RELAY).try_into().unwrap();
    answer(chain, row.id, Answer::Failed("no route".into()));
    assert!(assigned(chain, RELAY).is_empty(), "a relay is not repeated");
    let (text, failed) = result_of(chain, thread, "toolu_l");
    assert!(failed && text.contains("the relay failed"), "{text}");
    assert!(text.contains("no route"), "{text}");
    assert_eq!(assigned(chain, SERVICE).len(), 1);
}

#[test]
fn a_call_in_flight_when_its_agent_is_taken_back_does_not_run() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "write", Some(WINDOW));
    chain.at(3, 0);
    let write = tool_use(
        "toolu_w",
        "files_write",
        json!({ "path": "/shared/late.md", "content": "x" }),
    );
    model_says(chain, json!([write]), "tool_use");
    let taken_back = identity::Op::SetScript {
        account: world.agent,
        script: None,
    };
    identity_op(chain, ADA, taken_back).unwrap();
    chain.at(4, 0);
    relay_back(chain, "toolu_w");
    let (text, failed) = result_of(chain, thread, "toolu_w");
    assert!(failed && text.contains("stopped running here"), "{text}");
    let asked = files::Query::Stat {
        path: "/shared/late.md".into(),
        at: None,
    };
    let files::Reply::Stat { entry, .. } = chain.query(files::PROGRAM, &asked).unwrap() else {
        panic!()
    };
    assert_eq!(entry, None, "the agent taken back wrote nothing");
    assert!(assigned(chain, SERVICE).is_empty());
    assert_eq!(chain.frames(), 0);
}

#[test]
fn a_write_is_the_agents_and_reaches_only_where_the_agent_may_write() {
    let world = world();
    let chain = &world.chain;
    let closed = chat::Op::CreateChannel {
        channel_id: "closed".into(),
        name: "closed".into(),
        post_policy: chat::PostPolicy::MembersOnly,
    };
    chain.submit(signed(BO), chat::PROGRAM, &closed).unwrap();
    let thread = world.opened("m1", "post", Some(WINDOW));
    chain.at(3, 0);
    let calls = json!([
        tool_use(
            "toolu_p",
            "chat_post",
            json!({ "channel": "general", "text": "hi **all**", "reply_to": 1 })
        ),
        tool_use(
            "toolu_c",
            "chat_post",
            json!({ "channel": "closed", "text": "hi" })
        ),
        tool_use(
            "toolu_e",
            "files_write",
            json!({ "path": "/etc/motd", "content": "x" })
        ),
        tool_use(
            "toolu_h",
            "files_write",
            json!({ "path": "/home/Scout/notes.md", "content": "mine" })
        ),
        tool_use(
            "toolu_m",
            "files_write",
            json!({ "path": CONFIG, "content": "x" })
        ),
        tool_use(
            "toolu_r",
            "files_write",
            json!({ "path": "/home/Ada", "content": "x" })
        ),
    ]);
    model_says(chain, calls, "tool_use");
    chain.at(4, 0);
    for call in [
        "toolu_p", "toolu_c", "toolu_e", "toolu_h", "toolu_m", "toolu_r",
    ] {
        relay_back(chain, call);
    }
    assert!(!result_of(chain, thread, "toolu_p").1);
    let (closed, failed) = result_of(chain, thread, "toolu_c");
    assert!(failed && closed.starts_with(code::UNAUTHORIZED), "{closed}");
    let (etc, failed) = result_of(chain, thread, "toolu_e");
    assert!(!failed, "a path no list covers is anyone's: {etc}");
    let (root, failed) = result_of(chain, thread, "toolu_r");
    assert!(failed && root.starts_with(code::UNAUTHORIZED), "{root}");
    let (home, failed) = result_of(chain, thread, "toolu_h");
    assert!(!failed, "{home}");
    let (manager, failed) = result_of(chain, thread, "toolu_m");
    assert!(
        failed && manager.starts_with(code::UNAUTHORIZED),
        "{manager}"
    );
    let asked = chat::Query::MessageById {
        message_id: format!("harness-{thread}-toolu_p"),
    };
    let chat::Reply::Message(Some(posted)) = chain.query(chat::PROGRAM, &asked).unwrap() else {
        panic!()
    };
    assert_eq!(posted.author, Principal::Account(world.agent));
    assert_eq!((posted.text.as_str(), posted.thread), ("hi all", Some(1)));

    let take_back = identity::Op::SetScript {
        account: world.agent,
        script: None,
    };
    identity_op(chain, ADA, take_back).unwrap();
    chain.at(5, 0);
    let again = tool_use(
        "toolu_q",
        "chat_post",
        json!({ "channel": "general", "text": "still me?" }),
    );
    model_says(chain, json!([again]), "tool_use");
    chain.at(6, 0);
    relay_back(chain, "toolu_q");
    let (taken, failed) = result_of(chain, thread, "toolu_q");
    assert!(failed && taken.contains("stopped running here"), "{taken}");
}

#[test]
fn an_agent_writes_a_file_and_lists_who_else_writes_it() {
    let mut world = world();
    world.chain.seat::<ownership::Ownership>(ownership::PROGRAM);
    let chain = &world.chain;
    let thread = world.opened("m1", "write", Some(WINDOW));
    chain.at(3, 0);
    let calls = json!([
        tool_use(
            "toolu_o",
            "files_write",
            json!({ "path": "/shared/mine.md", "content": "a" })
        ),
        tool_use(
            "toolu_p",
            "files_set_list",
            json!({ "path": "/shared/mine.md", "write": { "who": "creator" } })
        ),
        tool_use(
            "toolu_h",
            "files_write",
            json!({ "path": "/shared/agents.md", "content": "b" })
        ),
        tool_use(
            "toolu_i",
            "files_set_list",
            json!({
                "path": "/shared/agents.md",
                "write": { "who": "listed", "accounts": [world.bo], "programs": [PROGRAM] }
            })
        ),
        tool_use(
            "toolu_m",
            "files_set_list",
            json!({ "path": CONFIG, "write": { "who": "anyone" } })
        ),
    ]);
    model_says(chain, calls, "tool_use");
    chain.at(4, 0);
    for call in ["toolu_o", "toolu_p", "toolu_h", "toolu_i", "toolu_m"] {
        relay_back(chain, call);
    }
    for call in ["toolu_o", "toolu_p", "toolu_h", "toolu_i"] {
        let (text, failed) = result_of(chain, thread, call);
        assert!(!failed, "{call}: {text}");
    }
    let (manager, failed) = result_of(chain, thread, "toolu_m");
    assert!(
        failed && manager.starts_with(code::UNAUTHORIZED),
        "{manager}"
    );
    assert_eq!(
        listed(chain, "/shared/mine.md"),
        (
            Some(::program::Party::Account(world.agent)),
            Some(::program::Ownership::Owner)
        )
    );
    assert_eq!(
        listed(chain, "/shared/agents.md").1,
        Some(::program::Ownership::Listed(vec![
            ::program::Party::Account(world.bo),
            ::program::Party::Program(PROGRAM.into()),
        ]))
    );
    let guard = |path: &str| {
        let asked = ownership::Query::Effective {
            program: files::PROGRAM.into(),
            key: files::guard_key(path),
        };
        let ownership::Reply::Policy(Some(policy)) =
            chain.query(ownership::PROGRAM, &asked).unwrap()
        else {
            panic!("{path} is guarded");
        };
        (policy.owner, policy.write)
    };
    let agent = ownership::Party::Account(world.agent);
    assert_eq!(
        guard("/shared/mine.md"),
        (agent.clone(), ownership::Writers::Owner)
    );
    let listed_too = ownership::Writers::Listed(vec![
        ownership::Party::Account(world.bo),
        ownership::Party::Program(PROGRAM.into()),
    ]);
    assert_eq!(guard("/shared/agents.md"), (agent, listed_too));
    let by_ada = files::Op::Commit {
        base: None,
        message: "ada".into(),
        changes: vec![files::Change::Remove {
            path: "/shared/mine.md".into(),
        }],
    };
    let refused = chain.submit(signed(ADA), files::PROGRAM, &by_ada);
    assert_eq!(refused.unwrap_err().code, code::UNAUTHORIZED);
}

fn notices(chain: &MockChain, thread: u64) -> Vec<String> {
    frames(chain, thread)
        .into_iter()
        .filter_map(|body| match body {
            Body::Notice { text } => Some(text),
            _ => None,
        })
        .collect()
}

#[test]
fn a_failed_model_call_is_noticed_once_and_the_next_input_asks_again() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    let (request, _) = open_request(chain);
    let overloaded = json!({ "type": "error", "error": { "type": "overloaded_error", "message": "Overloaded" } });
    answer(chain, request.id, Answer::Output(response(529, overloaded)));
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "a failed call is asked again from the next block on"
    );
    let told = notices(chain, thread);
    assert_eq!(told.len(), 1);
    assert!(
        told[0].contains("overloaded_error: Overloaded"),
        "{}",
        told[0]
    );
    world.mention("m2", "again");
    let (_, head) = open_request(chain);
    assert_eq!(path(&head), "/v1/messages");
}

#[test]
fn a_model_call_waits_the_blocks_its_configuration_names_then_ends_with_the_expired_error() {
    let world = world();
    let chain = &world.chain;
    const WAIT: u64 = 30;
    let waits = Config {
        wait: Some(WAIT),
        ..scout()
    };
    commit(chain, ADA, CONFIG, &md(&waits));
    let thread = world.opened("m1", "hello", Some(WINDOW));
    let (request, head) = open_request(chain);
    assert_eq!(path(&head), "/v1/messages");
    chain.at(request.made + WAIT, 0);
    assert!(notices(chain, thread).is_empty());
    chain.at(request.made + WAIT + 1, 0);
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "an expired call is asked again from the next block on"
    );
    let told = notices(chain, thread);
    let expired = format!("request {} expired after {WAIT} blocks", request.id);
    assert_eq!(told.len(), 1);
    assert!(told[0].contains(&expired), "{}", told[0]);
    let late = egress::Op::Answer {
        id: request.id,
        answer: Answer::Output(reply(json!([]), "end_turn", 10)),
    };
    let refused = chain
        .submit(Origin::Signed(NODE.to_vec()), egress::PROGRAM, &late)
        .unwrap_err();
    assert_eq!(refused.code, code::NOT_FOUND);
    world.mention("m2", "again");
    let (_, head) = open_request(chain);
    assert_eq!(path(&head), "/v1/messages");
}

fn beta(head: &http::Head) -> Option<&str> {
    head.headers
        .iter()
        .find(|(name, _)| name == "anthropic-beta")
        .map(|(_, value)| value.as_str())
}

fn too_long() -> Vec<u8> {
    let error = json!({
        "type": "error",
        "error": { "type": "invalid_request_error", "message": "prompt is too long: 1000002 tokens > 1000000 maximum" },
    });
    abi::encode(&http::Response {
        status: 400,
        headers: Vec::new(),
        body: error.to_string().into_bytes(),
    })
}

fn refused_as_too_long(chain: &MockChain) {
    let (request, _) = open_request(chain);
    answer(chain, request.id, Answer::Output(too_long()));
}

fn sent_messages(chain: &MockChain) -> Vec<Value> {
    let (request, _) = open_request(chain);
    body(chain, &request)["messages"]
        .as_array()
        .unwrap()
        .clone()
}

fn summarized(chain: &MockChain, summary: &str) {
    let (request, head) = open_request(chain);
    assert_eq!(beta(&head), Some(wire::anthropic::COMPACTION_BETA));
    let body = json!({
        "type": "message",
        "role": "assistant",
        "content": [{ "type": "compaction", "content": summary, "signature": "sig", "citations": null }],
        "stop_reason": "compaction",
        "usage": {
            "input_tokens": 0,
            "output_tokens": 0,
            "iterations": [{ "type": "compaction", "input_tokens": 1500, "output_tokens": 276 }],
        },
    });
    answer(chain, request.id, Answer::Output(response(200, body)));
}

fn lines_of(chain: &MockChain, thread: u64) -> Vec<BlobId> {
    let mine = [thread.to_be_bytes().as_slice(), b"/"].concat();
    let host = chain.host(PROGRAM).borrow();
    host.lines
        .iter()
        .filter(|((holder, at), _)| holder == PROGRAM && at.starts_with(&mine))
        .map(|(_, id)| *id)
        .collect()
}

#[test]
fn an_idle_thread_closed_or_retired_holds_no_line() {
    let world = world();
    let chain = &world.chain;
    let closed = world.opened("m1", "remember the plan", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Noted." }]),
        "end_turn",
    );
    assert!(!lines_of(chain, closed).is_empty());
    let close = Op::Close { thread: closed };
    chain.submit(signed(ADA), PROGRAM, &close).unwrap();
    assert_eq!(
        lines_of(chain, closed),
        Vec::<BlobId>::new(),
        "an idle thread lets go of its lines when it closes"
    );

    let retired = world.mention("m2", "a fresh start");
    chain.at(5, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Noted." }]),
        "end_turn",
    );
    assert!(!lines_of(chain, retired).is_empty());
    let new = Op::NewThread { agent: world.agent };
    chain.submit(signed(ADA), PROGRAM, &new).unwrap();
    assert!(thread_row(chain, retired).closed);
    assert_eq!(
        lines_of(chain, retired),
        Vec::<BlobId>::new(),
        "an idle thread lets go of its lines when a new conversation retires it"
    );
}

#[test]
fn a_threads_request_parts_leave_its_lines_at_compaction_and_at_its_close() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "remember the plan", Some(2_000));
    chain.at(3, 0);
    model_says_after(
        chain,
        json!([{ "type": "text", "text": "Noted." }]),
        "end_turn",
        1000,
    );
    world.mention("m2", "and now?");
    let mine = [thread.to_be_bytes().as_slice(), b"/"].concat();
    let held = |line: &[u8]| -> Vec<BlobId> {
        let under = [mine.as_slice(), line].concat();
        let host = chain.host(PROGRAM).borrow();
        host.lines
            .iter()
            .filter(|((holder, at), _)| holder == PROGRAM && at.starts_with(&under))
            .map(|(_, id)| *id)
            .collect()
    };
    let sealed = held(b"sealed/");
    assert!(
        !sealed.is_empty(),
        "the summary request is made of sealed parts"
    );
    assert_eq!((held(b"header").len(), held(b"last").len()), (1, 1));

    chain.at(4, 0);
    summarized(chain, "Ada set out the plan.");
    assert_eq!(held(b"summary").len(), 1);
    assert!(
        !held(b"sealed/").contains(&sealed[0]),
        "the compaction lets go of the part it summarized"
    );

    let close = Op::Close { thread };
    chain.submit(signed(ADA), PROGRAM, &close).unwrap();
    assert!(
        !held(b"").is_empty(),
        "the open request still needs its parts"
    );
    chain.at(5, 0);
    model_says_after(
        chain,
        json!([{ "type": "text", "text": "Done." }]),
        "end_turn",
        300,
    );
    assert_eq!(
        held(b""),
        Vec::<BlobId>::new(),
        "a closed thread holds no part"
    );
}

#[test]
fn a_thread_whose_next_answer_would_not_fit_the_window_is_summarized_first() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "remember the plan", Some(2_000));
    chain.at(3, 0);
    model_says_after(
        chain,
        json!([{ "type": "text", "text": "Noted." }]),
        "end_turn",
        1000,
    );
    assert_eq!(
        thread_row(chain, thread).context,
        Context::Measured(1008),
        "input, cache reads and output all count"
    );
    world.mention("m2", "and now?");
    let (request, head) = open_request(chain);
    assert_eq!(beta(&head), Some(wire::anthropic::COMPACTION_BETA));
    let sent = body(chain, &request);
    assert_eq!(sent["compaction"], json!({ "type": "summarize" }));
    let messages = sent["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1, "the summary covers what the model saw");
    assert!(
        messages[0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .ends_with("remember the plan")
    );
    assert_eq!(
        thread_row(chain, thread).asking,
        Asking::Compacting { through: 1 }
    );

    chain.at(4, 0);
    summarized(chain, "Ada set out the plan.");
    let row = thread_row(chain, thread);
    assert_eq!(row.context, Context::Measured(276));
    let summary = row.summary.expect("the thread keeps its summary");
    assert!(matches!(
        &frames(chain, thread)[summary as usize - 1],
        Body::Compaction { block, .. } if block.contains("Ada set out the plan.")
    ));
    let (request, head) = open_request(chain);
    assert_eq!(path(&head), "/v1/messages");
    assert_eq!(beta(&head), Some(wire::anthropic::COMPACTION_BETA));
    let sent = body(chain, &request);
    assert!(sent.get("compaction").is_none());
    let messages = sent["messages"].as_array().unwrap();
    assert_eq!(
        messages[0],
        json!({
            "role": "assistant",
            "content": [{ "type": "compaction", "content": "Ada set out the plan.", "signature": "sig" }],
        }),
        "the summary stands in for what it covers, with no null fields"
    );
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"][0]["text"], "Noted.");
    assert_eq!(messages[2]["role"], "user");
    assert!(
        messages[2]["content"][0]["text"]
            .as_str()
            .unwrap()
            .ends_with("and now?")
    );
    assert_eq!(messages.len(), 3);

    chain.at(5, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Still on it." }]),
        "end_turn",
    );
    world.mention("m3", "once more");
    let (request, _) = open_request(chain);
    let sent = body(chain, &request);
    let messages = sent["messages"].as_array().unwrap();
    assert_eq!(
        messages[0]["content"][0]["type"], "compaction",
        "every later request carries the summary"
    );
    assert_eq!(messages.len(), 5);
}

#[test]
fn a_request_the_model_finds_too_long_is_summarized_even_with_no_window_known() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "first", None);
    chain.at(3, 0);
    model_says_after(
        chain,
        json!([{ "type": "text", "text": "Ok." }]),
        "end_turn",
        1_000_000,
    );
    world.mention("m2", "second");
    let (request, head) = open_request(chain);
    assert_eq!(path(&head), "/v1/messages");
    assert!(
        body(chain, &request).get("compaction").is_none(),
        "no window, no early summary"
    );
    chain.at(4, 0);
    answer(chain, request.id, Answer::Output(too_long()));
    assert_eq!(thread_row(chain, thread).context, Context::Overflowed);
    let (request, _) = open_request(chain);
    assert_eq!(
        body(chain, &request)["compaction"],
        json!({ "type": "summarize" })
    );
    chain.at(5, 0);
    summarized(chain, "First, then Ok.");
    chain.at(6, 0);
    let (request, _) = open_request(chain);
    let cut_short = json!({
        "type": "message",
        "role": "assistant",
        "model": "claude-test",
        "content": [{ "type": "text", "text": "I was say" }],
        "stop_reason": "model_context_window_exceeded",
        "usage": { "input_tokens": 999_000, "output_tokens": 1000 },
    });
    answer(chain, request.id, Answer::Output(response(200, cut_short)));
    assert!(
        !frames(chain, thread)
            .iter()
            .any(|body| matches!(body, Body::Turn { said, .. } if said.contains(&Said::Text("I was say".into())))),
        "a turn cut short by the window is not kept"
    );
    let messages = sent_messages(chain);
    let last = &messages[messages.len() - 1];
    assert!(
        last["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("left out of your context: frame 3 is"),
        "with nothing new to summarize, the newest input is left out: {last}"
    );
    chain.at(7, 0);
    refused_as_too_long(chain);
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "with everything new left out and still too long, the thread gives up"
    );
    let told = notices(chain, thread);
    assert!(
        told.last().unwrap().contains("everything new left out"),
        "{told:?}"
    );
    let row = thread_row(chain, thread);
    assert_eq!(row.asking, Asking::Idle);
    assert_eq!(
        row.context,
        Context::Measured(0),
        "the next input is asked plainly"
    );
    world.mention("m3", "third");
    let (_, head) = open_request(chain);
    assert_eq!(path(&head), "/v1/messages");
    let messages = sent_messages(chain);
    let last = &messages[messages.len() - 1];
    assert!(
        last["content"][1]["text"]
            .as_str()
            .unwrap()
            .ends_with("third"),
        "{last}"
    );
}

#[test]
fn a_result_that_alone_overflows_the_window_is_left_out_and_the_model_searches_and_reads_it_in_parts()
 {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "read it", Some(WINDOW));
    chain.at(3, 0);
    let call = tool_use("toolu_l", "files_list", json!({ "path": "/" }));
    model_says(chain, json!([call]), "tool_use");
    chain.at(4, 0);
    relay_back(chain, "toolu_l");
    chain.at(5, 0);
    refused_as_too_long(chain);
    chain.at(6, 0);
    summarized(chain, "Ada asked to read it.");
    chain.at(7, 0);
    refused_as_too_long(chain);
    let messages = sent_messages(chain);
    let result = &messages[messages.len() - 1]["content"][0];
    assert_eq!(result["type"], "tool_result");
    assert_eq!(result["tool_use_id"], "toolu_l");
    assert_eq!(result["is_error"], false);
    let (text, failed) = result_of(chain, thread, "toolu_l");
    assert!(!failed, "the transcript keeps the result as it came");
    let note = format!(
        "left out of your context: frame 3 is {} bytes in 1 lines, too large for your context window alone. Find lines in it with thread_search (frame 3) and read parts of it with thread_read (frame 3, an offset and a length).",
        text.len()
    );
    assert_eq!(result["content"], note);
    let Reply::Withheld(left_out) = chain.query(PROGRAM, &Query::Withheld { thread }).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        left_out,
        [WithheldFrame {
            seq: 3,
            size: text.len() as u64
        }]
    );
    let told = notices(chain, thread);
    assert!(
        told.last()
            .unwrap()
            .starts_with("the result of files_list call toolu_l alone does not fit"),
        "{told:?}"
    );

    chain.at(8, 0);
    let wanted = &text[2..6];
    let calls = json!([
        tool_use(
            "toolu_s",
            "thread_search",
            json!({ "pattern": wanted, "frame": 3, "around": 1 })
        ),
        tool_use(
            "toolu_r",
            "thread_read",
            json!({ "frame": 3, "offset": 2, "length": 4 })
        ),
        tool_use("toolu_n", "thread_read", json!({ "frame": 99 })),
    ]);
    model_says(chain, calls, "tool_use");
    chain.at(9, 0);
    for call in ["toolu_s", "toolu_r", "toolu_n"] {
        relay_back(chain, call);
    }
    let (searched, failed) = result_of(chain, thread, "toolu_s");
    assert!(!failed, "{searched}");
    let searched: Value = serde_json::from_str(&searched).unwrap();
    assert_eq!(searched["hits"][0]["frame"], 3);
    assert_eq!(searched["hits"][0]["line"], 1);
    assert_eq!(searched["hits"][0]["offsets"][0], 2);
    assert_eq!(searched["hits"][0]["excerpts"][0], &text[1..7]);
    let (read, failed) = result_of(chain, thread, "toolu_r");
    assert!(!failed, "{read}");
    let read: Value = serde_json::from_str(&read).unwrap();
    assert_eq!(
        read,
        json!({ "frame": 3, "offset": 2, "size": text.len(), "text": wanted, "rest": text.len() - 6 })
    );
    let (missing, failed) = result_of(chain, thread, "toolu_n");
    assert!(failed && missing.contains("no frame 99"), "{missing}");
    chain.at(10, 0);

    model_says(
        chain,
        json!([{ "type": "text", "text": "Found it." }]),
        "end_turn",
    );
    let row = thread_row(chain, thread);
    assert_eq!(row.context, Context::Measured(18));
    assert_eq!(row.asking, Asking::Idle);
}

#[test]
fn a_thread_search_finds_what_a_summary_covers_and_reads_whole_lines_on_a_char_boundary() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "the key is é-42", Some(2_000));
    chain.at(3, 0);
    model_says_after(
        chain,
        json!([{ "type": "text", "text": "Noted." }]),
        "end_turn",
        1000,
    );
    world.mention("m2", "what was the key?");
    chain.at(4, 0);
    summarized(chain, "Ada gave a key.");
    chain.at(5, 0);
    let calls = json!([
        tool_use("toolu_s", "thread_search", json!({ "pattern": "key is" })),
        tool_use(
            "toolu_r",
            "thread_read",
            json!({ "frame": 1, "offset": 0, "length": 1 })
        ),
    ]);
    model_says(chain, calls, "tool_use");
    chain.at(6, 0);
    relay_back(chain, "toolu_s");
    relay_back(chain, "toolu_r");
    let (searched, _) = result_of(chain, thread, "toolu_s");
    let searched: Value = serde_json::from_str(&searched).unwrap();
    let hits = searched["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 2, "{searched}");
    assert_eq!(
        hits[1]["text"], "thread_search {\"pattern\":\"key is\"}",
        "the model's own turns are searched too"
    );
    assert_eq!(hits[0]["frame"], 1);
    assert!(
        hits[0]["text"]
            .as_str()
            .unwrap()
            .ends_with("the key is é-42"),
        "{searched}"
    );
    let (read, _) = result_of(chain, thread, "toolu_r");
    let read: Value = serde_json::from_str(&read).unwrap();
    assert_eq!(read["offset"], 0);
    assert_eq!(read["rest"], read["size"].as_u64().unwrap() - 1);
    let first = frames(chain, thread)[0].text();
    let middle = first.find('é').unwrap() + 1;
    let (got, _) = {
        chain.at(7, 0);
        let call = tool_use(
            "toolu_m",
            "thread_read",
            json!({ "frame": 1, "offset": middle, "length": 1 }),
        );
        model_says(chain, json!([call]), "tool_use");
        chain.at(8, 0);
        relay_back(chain, "toolu_m");
        result_of(chain, thread, "toolu_m")
    };
    let got: Value = serde_json::from_str(&got).unwrap();
    assert_eq!(got["offset"], middle - 1, "a read starts on a character");
    assert_eq!(got["text"], "é");
}

#[test]
fn a_summary_refused_as_too_long_covers_less_and_never_ends_on_the_models_turn() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "one", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Ok one." }]),
        "end_turn",
    );
    world.mention("m2", "two");
    chain.at(4, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Ok two." }]),
        "end_turn",
    );
    world.mention("m3", "three");
    chain.at(5, 0);
    refused_as_too_long(chain);
    assert_eq!(
        sent_messages(chain).len(),
        3,
        "the summary covers what the model answered"
    );
    chain.at(6, 0);
    refused_as_too_long(chain);
    assert_eq!(thread_row(chain, thread).compact_before, Some(3));
    let messages = sent_messages(chain);
    assert_eq!(
        messages.len(),
        1,
        "one message fewer would end on the model's turn, so the summary covers the first input alone"
    );
    assert_eq!(messages[0]["role"], "user");
    chain.at(7, 0);
    summarized(chain, "Ada said one.");
    assert_eq!(thread_row(chain, thread).compact_before, None);
    let messages = sent_messages(chain);
    let roles: Vec<&str> = messages
        .iter()
        .map(|message| message["role"].as_str().unwrap())
        .collect();
    assert_eq!(
        roles,
        ["assistant", "assistant", "user", "assistant", "user"]
    );
    assert_eq!(messages[0]["content"][0]["type"], "compaction");
}

#[test]
fn only_attribution_delivers_and_only_the_manager_closes() {
    let world = world();
    let chain = &world.chain;
    let thread = world.mention("m1", "hi");
    let change = attribution::Change {
        seq: 99,
        source: attribution::Source {
            program: chat::PROGRAM.into(),
            object: attribution::Object {
                kind: "message".into(),
                id: "m1".into(),
            },
        },
        revision: 9,
        recipient: world.agent,
        reason: attribution::Reason::Mention,
        kind: attribution::Kind::Added,
        detail: Vec::new(),
        actor: attribution::Actor::Account(world.ada),
        cause: attribution::Cause::Direct,
        height: 1,
    };
    let forged = Op::Attributed(Box::new(change));
    assert_eq!(
        chain
            .submit(signed(ADA), PROGRAM, &forged)
            .unwrap_err()
            .code,
        code::UNAUTHORIZED
    );
    let close = Op::Close { thread };
    assert_eq!(
        chain.submit(signed(BO), PROGRAM, &close).unwrap_err().code,
        code::UNAUTHORIZED
    );
    chain.submit(signed(ADA), PROGRAM, &close).unwrap();
    let thread = world.mention("m2", "a fresh start");
    assert_eq!(
        world.threads().len(),
        2,
        "a closed thread's place opens a new one"
    );
    assert_eq!(frames(chain, thread).len(), 1);
}

#[test]
fn with_no_node_serving_the_model_a_mention_says_so() {
    let world = world();
    let chain = &world.chain;
    let stop = egress::Op::Serve {
        services: Vec::new(),
    };
    chain.submit(signed(&NODE), egress::PROGRAM, &stop).unwrap();
    let thread = world.mention("m1", "hello");
    assert!(
        matches!(&frames(chain, thread)[1], Body::Notice { text } if text.contains("no member node serves"))
    );
}

#[test]
fn a_place_resolves_to_its_thread_a_model_to_its_window_and_a_thread_says_when_it_last_moved() {
    let world = world();
    let chain = &world.chain;
    let window = || {
        let asked = Query::Card { agent: world.agent };
        let Reply::Agent(Some(setup)) = chain.query(PROGRAM, &asked).unwrap() else {
            panic!()
        };
        setup.configured.reached().unwrap().window
    };
    chain.at(1, 10);
    let thread = world.mention("m1", "hello");
    assert_eq!(window(), Window::Unasked, "not learned yet");
    let row = thread_row(chain, thread);
    assert_eq!((row.last_height, row.last_time), (1, 10));
    chain.at(2, 20);
    learn(chain, Some(WINDOW));
    assert_eq!(window(), Window::Tokens(WINDOW));
    assert_eq!(
        thread_row(chain, thread).last_height,
        1,
        "asking adds no frame"
    );
    chain.at(3, 30);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Hi." }]),
        "end_turn",
    );
    let row = thread_row(chain, thread);
    assert_eq!((row.last_height, row.last_time), (3, 30));
    let at = |place: &str| {
        let asked = Query::ThreadAt {
            agent: world.agent,
            place: place.into(),
        };
        let Reply::Thread(row) = chain.query(PROGRAM, &asked).unwrap() else {
            panic!()
        };
        row
    };
    assert_eq!(at("chat/general"), Some(row));
    assert_eq!(at("chat/elsewhere"), None);
}

#[test]
fn every_tool_schema_is_an_object_and_names_its_required_fields() {
    for entry in tools::CATALOG.iter() {
        let schema = (entry.schema)();
        assert_eq!(schema["type"], "object", "{}", entry.name);
        let empty = (entry.check)(&json!({}));
        let required = schema["required"].as_array().unwrap();
        assert_eq!(empty.is_ok(), required.is_empty(), "{}", entry.name);
        assert!(entry.name.len() <= 64 && !entry.description.is_empty());
    }
}

#[test]
fn a_mention_reaches_the_agent_under_any_id_harness_is_seated_at() {
    let world = world_as(ELSEWHERE);
    world.post(ADA, "general", "m1", &format!("<@{}> hello", world.agent));
    assert_eq!(world.threads().len(), 1, "the agent hears its mention");
    world.chain.at(2, 0);
    learn(&world.chain, Some(WINDOW));
}
