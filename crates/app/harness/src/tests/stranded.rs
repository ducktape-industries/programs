use ::program::schema::interface;
use programs::Programs;

use super::network::{pods_get, served};
use super::*;
use crate::schema::tests::{code_with, listed, module};
use crate::wire::tests::{TEXT, events};

const PODS: &str = "pods";
const AGAIN: &str = "It may have run: check, and run it again if it is still needed";

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

fn updated(chain: &MockChain, at: u64) {
    chain.at(at, 0);
    listed(chain, &codes(b"2"));
}

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
    let of_call = |body: &Body| matches!(body, Body::Result { call: done, .. } | Body::Arrived { call: done, .. } if done == call);
    let bodies = frames(chain, thread).into_iter().chain(held(chain, thread));
    bodies.filter(of_call).count()
}

fn pods_request(chain: &MockChain) -> RequestRow {
    let [row]: [RequestRow; 1] = assigned(chain, PODS).try_into().unwrap();
    row
}

fn sent(chain: &MockChain) -> String {
    let (request, head) = open_request(chain);
    assert_eq!(path(&head), model_wire::TURN);
    provider_body(chain, &request)["messages"].to_string()
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
    chain.at(5, 0);
    assert!(sent(chain).contains("Accepted as call n1"));
    model_says(
        chain,
        json!([{ "type": "text", "text": "Listing." }]),
        "end_turn",
    );
    let deadline = request.made + egress::DEFAULT_WAIT;
    chain.at(deadline + 1, 0);
    assert_eq!(
        state(chain, thread, "n1"),
        CallState::Writing,
        "egress answers by its deadline"
    );
    assert_eq!(results(chain, thread, "n1"), 0);
    chain.at(deadline + 2, 0);
    let (text, failed) = result_of(chain, thread, "n1");
    assert!(failed, "{text}");
    let why = format!(
        "harness heard no answer by block {deadline}, egress's deadline for what this call sent at block {}",
        request.made
    );
    assert!(text.starts_with(&why), "{text}");
    assert!(text.ends_with(AGAIN), "{text}");
    chain.at(deadline + 3, 0);
    let messages = sent(chain);
    assert!(messages.contains(&why), "{messages}");
    assert!(messages.contains("list the pods"), "the history is kept");
    assert!(assigned(chain, PODS).is_empty(), "nothing is sent again");
}

const RUN: &str = r#"name = "run"
description = "Run a command in the pod."

[input]
command = { type = "array", description = "the command and its arguments" }

[[step]]
service = "pods"
target = "/v1/pods/duckpond-7/exec"
body = { command = "$command" }
as = "ran"
"#;

const STREAM: &str = r#"name = "stream"
description = "Run a command in the pod, waking the conversation when it ends."

[input]
command = { type = "array", description = "the command and its arguments" }

[[step]]
service = "pods"
target = "/v1/pods/duckpond-7/exec"
body = { command = "$command" }
stream = true
"#;

fn tooled() -> World {
    let world = listed_world();
    for (name, text) in [("run", RUN), ("stream", STREAM)] {
        commit(
            &world.chain,
            ADA,
            &format!("/home/Scout/tools/{name}.toml"),
            text,
        );
    }
    world
}

#[test]
fn an_exec_an_update_cut_off_fails_once_a_resume_comes_and_is_not_run_again() {
    let world = tooled();
    let chain = &world.chain;
    let thread = world.opened("m1", "deploy it", Some(WINDOW));
    let input = json!({ "command": ["make", "deploy"] });
    turn(chain, 3, "x1", "run", input);
    assert_eq!(state(chain, thread, "x1"), CallState::Writing);
    let request = pods_request(chain);
    chain.at(5, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Deploying." }]),
        "end_turn",
    );
    updated(chain, 6);
    chain.cancel_parked(PROGRAM);
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);

    chain
        .submit(signed(ADA), PROGRAM, &Op::Resume { thread })
        .unwrap();
    let (text, failed) = result_of(chain, thread, "x1");
    assert!(failed, "{text}");
    let why = format!(
        "harness was updated while this call awaited the answer to what it sent at block {}",
        request.made
    );
    assert!(text.contains(&why), "{text}");
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
    let world = tooled();
    let chain = &world.chain;
    let thread = world.opened("m1", "deploy it", Some(WINDOW));
    let input = json!({ "command": ["make", "deploy"] });
    turn(chain, 3, "x1", "run", input);
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

fn streaming(world: &World) -> (u64, RequestRow) {
    let chain = &world.chain;
    let thread = world.opened("m1", "build it", Some(WINDOW));
    let input = json!({ "command": ["cargo", "test"] });
    turn(chain, 3, "s1", "stream", input);
    let (streams, failed) = result_of(chain, thread, "s1");
    assert!(
        !failed && streams.contains("\"streaming\":true"),
        "{streams}"
    );
    chain.at(5, 1000);
    let waiting = json!([{ "type": "text", "text": "waiting" }]);
    model_says(chain, waiting, "end_turn");
    chain.at(6, 2000);
    chain.at(7, 3000);
    (thread, pods_request(chain))
}

#[test]
fn a_streamed_exec_still_wakes_its_conversation_once_after_an_update() {
    let world = tooled();
    let chain = &world.chain;
    let (thread, row) = streaming(&world);
    updated(chain, 8);
    chain.cancel_parked(PROGRAM);
    let ended = r#"event: ended
data: {"exit_code": 0, "stdout": "test result: ok", "stderr": ""}"#;
    event(chain, &row, 0, ended);
    let [woke]: [String; 1] = told(chain, thread).try_into().unwrap();
    let from = "stream call s1 (pods /v1/pods/duckpond-7/exec) ended: ";
    assert!(woke.starts_with(from), "{woke}");
    chain.at(9, 0);
    let done = json!([{ "type": "text", "text": "green" }]);
    model_says(chain, done, "end_turn");

    chain.at(8 + egress::DEFAULT_WAIT + 2, 0);
    assert_eq!(told(chain, thread).len(), 1, "it wakes once");
    assert!(assigned(chain, SERVICE).is_empty());
}

fn resumed(chain: &MockChain, thread: u64) {
    chain
        .submit(signed(ADA), PROGRAM, &Op::Resume { thread })
        .unwrap();
}

#[test]
fn a_stream_whose_end_an_update_cut_off_wakes_its_conversation_once_egress_no_longer_holds_it() {
    let world = tooled();
    let chain = &world.chain;
    let (thread, row) = streaming(&world);
    updated(chain, 8);
    chain.cancel_parked(PROGRAM);
    event(chain, &row, 0, "event: alive\ndata: {}");
    resumed(chain, thread);
    assert!(told(chain, thread).is_empty(), "egress still holds it");
    chain.at(9, 0);
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
    chain.at(10, 0);
    resumed(chain, thread);
    let [woke]: [String; 1] = told(chain, thread).try_into().unwrap();
    let unheard = "stream call s1 (pods /v1/pods/duckpond-7/exec) ended unheard: egress no longer holds its stream";
    assert!(woke.starts_with(unheard), "{woke}");
    assert!(woke.contains("this is not its result"), "{woke}");
    assert!(woke.contains("still be running on pods"), "{woke}");
    assert!(assigned(chain, PODS).is_empty(), "nothing runs again");
    chain.at(8 + egress::DEFAULT_WAIT + 2, 0);
    assert_eq!(told(chain, thread).len(), 1, "it wakes once");
}

fn reply_streams(chain: &MockChain, request: &RequestRow, fixture: &str) {
    for (seq, data) in (0u64..).zip(adapted::adapted(&events(fixture))) {
        let event = egress::Event {
            id: request.id,
            seq,
            data,
        };
        let op = egress::Op::Events(vec![event]);
        chain.submit(signed(&NODE), egress::PROGRAM, &op).unwrap();
    }
}

fn turns(chain: &MockChain, thread: u64) -> usize {
    let bodies = frames(chain, thread).into_iter();
    bodies
        .filter(|body| matches!(body, Body::Turn { .. }))
        .count()
}

fn partial(chain: &MockChain, thread: u64) -> Option<Partial> {
    let Reply::Partial(partial) = chain.query(PROGRAM, &Query::Partial { thread }).unwrap() else {
        panic!()
    };
    partial
}

fn wakes_of(chain: &MockChain, thread: u64) -> Vec<Wake> {
    let Reply::Wakes(wakes) = chain.query(PROGRAM, &Query::Wakes { thread }).unwrap() else {
        panic!()
    };
    wakes
}

#[test]
fn a_reply_that_streams_whole_after_a_switch_is_recorded_as_it_ends() {
    let world = listed_world();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    let (request, _) = open_request(chain);
    updated(chain, 3);
    chain.cancel_parked(PROGRAM);
    reply_streams(chain, &request, TEXT);
    assert_eq!(turns(chain, thread), 1, "its end records it");
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);
    let [open]: [RequestRow; 1] = assigned(chain, SERVICE).try_into().unwrap();
    assert_eq!(open.id, request.id, "nothing is asked again");
    chain.at(5, 0);
    assert_eq!(turns(chain, thread), 1, "the switch's wake adds nothing");
    assert_eq!(assigned(chain, SERVICE).len(), 1);
}

#[test]
fn a_call_egress_no_longer_holds_is_asked_again_from_the_first_frame_after_a_switch() {
    let world = listed_world();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    let (request, _) = open_request(chain);
    updated(chain, 3);
    chain.cancel_parked(PROGRAM);
    let failed = Answer::Failed("the node restarted".into());
    answer(chain, request.id, failed);
    assert!(assigned(chain, SERVICE).is_empty(), "no run hears it");
    world.mention("m2", "still there?");
    let said = notices(chain, thread);
    let cut = "the model call made at block 2 was cut off: harness's code changed by block 3";
    assert!(said.iter().any(|text| text.starts_with(cut)), "{said:?}");
    chain.at(5, 0);
    let (asked, _) = open_request(chain);
    assert_eq!(
        asked.made, 4,
        "asked a block after the switch, not at egress's deadline"
    );
    let messages = sent(chain);
    assert!(messages.contains("still there?"), "{messages}");
}

#[test]
fn closing_a_conversation_whose_call_a_switch_cut_off_ends_its_turn_and_its_records() {
    let world = listed_world();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    let (request, _) = open_request(chain);
    updated(chain, 3);
    chain.cancel_parked(PROGRAM);
    chain
        .submit(signed(ADA), PROGRAM, &Op::Close { thread })
        .unwrap();
    let row = thread_row(chain, thread);
    assert_eq!((row.closed, row.asking), (true, Asking::Idle));
    let wakes = wakes_of(chain, thread);
    let held = |wake: &Wake| matches!(wake, Wake::Turn | Wake::Flight);
    assert!(!wakes.iter().any(held), "{wakes:?}");
    assert_eq!(partial(chain, thread), None);
    chain.at(6, 0);
    let its_own = ::egress::Delivery::Events {
        deliver: streamed_as(thread),
    };
    let asked: Vec<_> = assigned(chain, SERVICE)
        .into_iter()
        .filter(|row| row.delivery == its_own)
        .map(|row| row.id)
        .collect();
    assert_eq!(asked, [request.id], "a closed conversation asks nothing");
}

#[test]
fn new_code_ends_the_turn_a_closed_conversation_lost_to_an_older_switch() {
    let world = listed_world();
    let chain = &world.chain;
    let stuck = world.opened("m1", "hello", Some(WINDOW));
    let close = Op::Close { thread: stuck };
    chain.submit(signed(ADA), PROGRAM, &close).unwrap();
    chain.cancel_parked(PROGRAM);
    let asking = thread_row(chain, stuck).asking;
    assert!(matches!(asking, Asking::Asked { .. }), "{asking:?}");
    updated(chain, 9);
    let fresh = world.mention("m2", "a fresh start");
    assert_ne!(fresh, stuck);
    chain.at(11, 0);
    let row = thread_row(chain, stuck);
    assert_eq!((row.closed, row.asking), (true, Asking::Idle));
    let wakes = wakes_of(chain, stuck);
    let held = |wake: &Wake| matches!(wake, Wake::Turn | Wake::Flight);
    assert!(!wakes.iter().any(held), "{wakes:?}");
    assert_eq!(partial(chain, stuck), None);
}

#[test]
fn an_acked_exec_an_update_cut_off_arrives_failed_as_new_input_and_is_not_run_again() {
    let world = tooled();
    let chain = &world.chain;
    let thread = world.opened("m1", "deploy it", Some(WINDOW));
    let input = json!({ "command": ["make", "deploy"] });
    turn(chain, 3, "x1", "run", input);
    let request = pods_request(chain);
    chain.at(5, 0);
    let messages = sent(chain);
    assert!(messages.contains("Accepted as call x1"), "{messages}");
    model_says(
        chain,
        json!([{ "type": "text", "text": "Deploying." }]),
        "end_turn",
    );
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);

    updated(chain, 6);
    chain.cancel_parked(PROGRAM);
    chain
        .submit(signed(ADA), PROGRAM, &Op::Resume { thread })
        .unwrap();
    let why = format!(
        "harness was updated while this call awaited the answer to what it sent at block {}",
        request.made
    );
    let messages = sent(chain);
    assert!(messages.contains("Result of tool call x1"), "{messages}");
    assert!(messages.contains(&why), "{messages}");
    assert!(messages.contains(AGAIN), "{messages}");
    assert_eq!(pods_request(chain).id, request.id, "nothing is sent again");
}
