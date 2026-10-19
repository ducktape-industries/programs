use ::program::schema::interface;
use programs::Programs;

use super::network::{pods_get, served};
use super::*;
use crate::schema::tests::{code_with, listed, module};

const PODS: &str = "pods";
const POD: &str = "duckpond-7";
const AGAIN: &str = "It may have run: check, and run it again if it is still needed";

// the programs role lists harness's code beside egress's, and the node serves pods
fn listed_world() -> World {
    let mut world = world();
    world.chain.seat::<Programs>(programs::PROGRAM);
    listed(&world.chain, &codes(b"1"));
    served(&world.chain);
    world
}

fn codes(build: &[u8]) -> [(&'static str, Vec<u8>); 3] {
    [
        (PROGRAM, module(&[("ducktape.build", build)])),
        (egress::PROGRAM, code_with(&interface::<Egress>())),
        ("valset", module(&[])),
    ]
}

// harness's code changes at block `at`, as a deploy changes it: the programs role names new code
fn updated(chain: &MockChain, at: u64) {
    chain.at(at, 0);
    listed(chain, &codes(b"2"));
}

// the model calls `tool` at `height`, and its relay comes back a block later
fn turn(chain: &MockChain, height: u64, call: &str, tool: &str, input: Value) {
    chain.at(height, 0);
    model_says(chain, json!([tool_use(call, tool, input)]), "tool_use");
    chain.at(height + 1, 0);
    relay_back(chain, call);
}

fn state(chain: &MockChain, thread: u64, call: &str) -> CallState {
    let asked = Query::Calls {
        thread,
        page: PageRequest::default(),
    };
    let Reply::Calls(page) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!()
    };
    let row = page.items.into_iter().find(|row| row.id == call);
    row.expect("the call is placed").state
}

fn results(chain: &MockChain, thread: u64, call: &str) -> usize {
    let of_call = |body: &Body| matches!(body, Body::Result { call: done, .. } if done == call);
    let bodies = frames(chain, thread).into_iter().chain(held(chain, thread));
    bodies.filter(of_call).count()
}

fn pods_request(chain: &MockChain) -> RequestRow {
    let [row]: [RequestRow; 1] = assigned(chain, PODS).try_into().unwrap();
    row
}

// what the model is asked next
fn sent(chain: &MockChain) -> String {
    let (request, head) = open_request(chain);
    assert_eq!(path(&head), "/v1/messages");
    body(chain, &request)["messages"].to_string()
}

fn told(chain: &MockChain, thread: u64) -> Vec<String> {
    let bodies = frames(chain, thread).into_iter().chain(held(chain, thread));
    bodies
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

#[test]
fn a_whole_network_call_an_update_cut_off_fails_past_egress_deadline_and_the_model_hears_why() {
    let world = listed_world();
    let chain = &world.chain;
    let thread = world.opened("m1", "list the pods", Some(WINDOW));
    turn(chain, 3, "n1", "network_call", pods_get("/v1/pods"));
    assert_eq!(state(chain, thread, "n1"), CallState::Writing);
    let request = pods_request(chain);
    chain.cancel_parked(PROGRAM);
    let deadline = request.made + egress::DEFAULT_WAIT;
    chain.at(deadline + 1, 0);
    assert_eq!(
        state(chain, thread, "n1"),
        CallState::Writing,
        "egress answers by its deadline"
    );
    assert_eq!(thread_row(chain, thread).asking, Asking::Running);
    chain.at(deadline + 2, 0);
    let (text, failed) = result_of(chain, thread, "n1");
    assert!(failed, "{text}");
    let why = format!(
        "harness heard no answer by block {deadline}, egress's deadline for what this call sent at block {}",
        request.made
    );
    assert!(text.starts_with(&why), "{text}");
    assert!(text.ends_with(AGAIN), "{text}");
    let messages = sent(chain);
    assert!(messages.contains(&why), "{messages}");
    assert!(messages.contains("list the pods"), "the history is kept");
    assert!(assigned(chain, PODS).is_empty(), "nothing is sent again");
}

#[test]
fn an_exec_an_update_cut_off_fails_once_a_resume_comes_and_is_not_run_again() {
    let world = listed_world();
    let chain = &world.chain;
    let thread = world.opened("m1", "deploy it", Some(WINDOW));
    let input = json!({ "pod": POD, "command": ["make", "deploy"] });
    turn(chain, 3, "x1", "pod_exec", input);
    assert_eq!(state(chain, thread, "x1"), CallState::Writing);
    let request = pods_request(chain);
    updated(chain, 6);
    chain.cancel_parked(PROGRAM);
    assert_eq!(thread_row(chain, thread).asking, Asking::Running);

    chain
        .submit(signed(ADA), PROGRAM, &Op::Resume { thread })
        .unwrap();
    let (text, failed) = result_of(chain, thread, "x1");
    assert!(failed, "{text}");
    let why = format!(
        "harness was updated while this call awaited the answer to what it sent at block {}",
        request.made
    );
    assert!(text.starts_with(&why), "{text}");
    assert!(text.ends_with(AGAIN), "{text}");
    let messages = sent(chain);
    assert!(messages.contains(&why), "{messages}");
    assert!(messages.contains("deploy it"), "the history is kept");
    assert_eq!(pods_request(chain).id, request.id, "nothing is sent again");

    chain.at(request.made + egress::DEFAULT_WAIT + 2, 0);
    assert_eq!(
        results(chain, thread, "x1"),
        1,
        "harness's own wake adds nothing"
    );
    assert!(assigned(chain, PODS).is_empty());
}

#[test]
fn an_answer_after_its_call_was_reported_lost_is_a_notice_and_no_second_result() {
    let world = listed_world();
    let chain = &world.chain;
    let thread = world.opened("m1", "deploy it", Some(WINDOW));
    let input = json!({ "pod": POD, "command": ["make", "deploy"] });
    turn(chain, 3, "x1", "pod_exec", input);
    let request = pods_request(chain);
    updated(chain, 6);
    chain
        .submit(signed(ADA), PROGRAM, &Op::Resume { thread })
        .unwrap();
    assert!(result_of(chain, thread, "x1").1, "reported lost");

    let out = json!({ "exit_code": 0, "stdout": "deployed", "stderr": "" });
    answer(chain, request.id, Answer::Output(response(200, out)));
    assert_eq!(results(chain, thread, "x1"), 1);
    let notices = notices(chain, thread);
    let late = notices.last().unwrap();
    let after = "call x1 was answered after harness reported its answer lost";
    assert!(late.starts_with(after), "{late}");
    assert!(late.contains("deployed"), "{late}");
}

// a streamed exec whose stream opens at block 7, its call answered and its turn over
fn streaming(world: &World) -> (u64, RequestRow) {
    let chain = &world.chain;
    let thread = world.opened("m1", "build it", Some(WINDOW));
    let input = json!({ "pod": POD, "script": "cargo test", "every": 20 });
    turn(chain, 3, "s1", "pod_exec", input);
    let (streams, failed) = result_of(chain, thread, "s1");
    assert!(
        !failed && streams.contains("\"streaming\":true"),
        "{streams}"
    );
    chain.at(5, 0);
    let waiting = json!([{ "type": "text", "text": "waiting" }]);
    model_says(chain, waiting, "end_turn");
    chain.at(6, 0);
    chain.at(7, 0);
    (thread, pods_request(chain))
}

#[test]
fn a_streamed_exec_still_wakes_its_conversation_once_after_an_update() {
    let world = listed_world();
    let chain = &world.chain;
    let (thread, row) = streaming(&world);
    updated(chain, 8);
    chain.cancel_parked(PROGRAM);
    let ended = r#"event: ended
data: {"exit_code": 0, "stdout": "test result: ok", "stderr": ""}"#;
    event(chain, &row, 0, ended);
    let [woke]: [String; 1] = told(chain, thread).try_into().unwrap();
    let from = "pod_exec call s1 (pod duckpond-7: sh -c cargo test) ended: ";
    assert!(woke.starts_with(from), "{woke}");
    chain.at(9, 0);
    let done = json!([{ "type": "text", "text": "green" }]);
    model_says(chain, done, "end_turn");

    chain.at(8 + egress::DEFAULT_WAIT + 2, 0);
    assert_eq!(told(chain, thread).len(), 1, "it wakes once");
    assert!(assigned(chain, SERVICE).is_empty());
}

#[test]
fn a_stream_whose_end_an_update_cut_off_wakes_its_conversation_past_egress_deadline() {
    let world = listed_world();
    let chain = &world.chain;
    let (thread, row) = streaming(&world);
    updated(chain, 8);
    chain.cancel_parked(PROGRAM);
    event(chain, &row, 0, "event: alive\ndata: {}");
    let cut = egress::StreamEnd::Cut {
        reason: "stream_broken".into(),
    };
    let end = egress::Op::EndStream {
        id: row.id,
        events: 1,
        end: cut,
    };
    chain.submit(signed(&NODE), egress::PROGRAM, &end).unwrap();
    assert!(told(chain, thread).is_empty(), "no run heard the end");

    let deadline = 8 + egress::DEFAULT_WAIT;
    chain.at(deadline + 1, 0);
    assert!(
        told(chain, thread).is_empty(),
        "its events moved the deadline"
    );
    chain.at(deadline + 2, 0);
    let [woke]: [String; 1] = told(chain, thread).try_into().unwrap();
    let unheard = format!(
        "pod_exec call s1 (pod duckpond-7: sh -c cargo test) ended unheard: harness heard nothing more of it by block {deadline}"
    );
    assert!(woke.starts_with(&unheard), "{woke}");
    assert!(woke.ends_with("what it ran may still be running"), "{woke}");
    assert_eq!(assigned(chain, SERVICE).len(), 1, "the wake asks the model");
    assert!(assigned(chain, PODS).is_empty(), "nothing runs again");
}

#[test]
fn a_measured_stream_an_update_cut_off_before_its_stream_opened_wakes_its_conversation() {
    let world = listed_world();
    let chain = &world.chain;
    let thread = world.opened("m1", "build it", Some(WINDOW));
    let input = json!({ "pod": POD, "script": "cargo test", "stream": true });
    turn(chain, 3, "s1", "pod_exec", input);
    chain.at(5, 0);
    chain.at(6, 0);
    let read = pods_request(chain);
    updated(chain, 7);
    chain.cancel_parked(PROGRAM);

    let deadline = read.made + egress::DEFAULT_WAIT;
    chain.at(deadline + 1, 0);
    assert!(told(chain, thread).is_empty(), "egress answers by then");
    chain.at(deadline + 2, 0);
    let [woke]: [String; 1] = told(chain, thread).try_into().unwrap();
    let unheard = format!(
        "pod_exec call s1 (pod duckpond-7: sh -c cargo test) ended unheard: harness heard nothing more of it by block {deadline}"
    );
    assert!(woke.starts_with(&unheard), "{woke}");
    assert!(assigned(chain, PODS).is_empty(), "no exec is sent");
}
