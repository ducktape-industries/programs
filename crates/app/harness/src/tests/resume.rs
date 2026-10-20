use super::*;
use crate::state::THREADS;
use crate::wire::tests::{TOOL_USE, events};

fn turns(chain: &MockChain, thread: u64) -> usize {
    let frames = frames(chain, thread);
    let turn = |body: &&Body| matches!(body, Body::Turn { .. });
    frames.iter().filter(turn).count()
}

fn calls(chain: &MockChain, thread: u64) -> Vec<CallRow> {
    let asked = Query::Calls {
        thread,
        page: PageRequest::default(),
    };
    let Reply::Calls(page) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!()
    };
    page.items
}

fn hello() -> Value {
    json!([{ "type": "text", "text": "hello back" }])
}

fn fail(chain: &MockChain, request: &RequestRow) {
    let failed = Answer::Failed("this node does not serve ccs".into());
    answer(chain, request.id, failed);
}

#[test]
fn a_model_call_the_node_failed_is_asked_again_at_the_next_block_and_answered_once() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    let (request, _) = open_request(chain);
    fail(chain, &request);
    assert!(assigned(chain, SERVICE).is_empty(), "not at once");
    let row = thread_row(chain, thread);
    assert_eq!(
        (row.asking, row.seen, row.last_stop),
        (Asking::Idle, 0, None)
    );
    let [told]: [String; 1] = notices(chain, thread).try_into().unwrap();
    let again = "harness asks the model again at block 4 (attempt 1)";
    assert!(told.ends_with(again), "{told}");
    chain.at(4, 0);
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "the wake runs as 4 ends"
    );
    chain.at(5, 0);
    model_says(chain, hello(), "end_turn");
    let row = thread_row(chain, thread);
    assert_eq!(
        (row.asking, row.last_stop),
        (Asking::Idle, Some(Stop::EndTurn))
    );
    assert_eq!(turns(chain, thread), 1);
    assert_eq!(notices(chain, thread).len(), 1);
    assert!(assigned(chain, SERVICE).is_empty());
}

#[test]
fn the_wait_doubles_with_each_attempt_and_a_new_input_asks_at_once() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    fail(chain, &open_request(chain).0);
    chain.at(5, 0);
    fail(chain, &open_request(chain).0);
    let told = notices(chain, thread);
    let again = "again at block 7 (attempt 2)";
    assert!(told[1].ends_with(again), "{}", told[1]);
    world.mention("m2", "are you back?");
    chain.at(6, 0);
    model_says(chain, hello(), "end_turn");
    assert_eq!(turns(chain, thread), 1);
    chain.at(8, 0);
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "the old wake finds nothing to ask"
    );
}

#[test]
fn a_call_the_model_refused_is_not_asked_again_and_held_input_asks_next() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    world.mention("m2", "and this");
    chain.at(3, 0);
    let (request, _) = open_request(chain);
    let invalid =
        json!({ "type": "error", "error": { "type": "invalid_request_error", "message": "bad" } });
    answer(chain, request.id, Answer::Output(response(400, invalid)));
    let [told]: [String; 1] = notices(chain, thread).try_into().unwrap();
    assert!(!told.contains("asks the model again"), "{told}");
    let (request, _) = open_request(chain);
    let sent = body(chain, &request)["messages"].to_string();
    assert!(sent.contains("and this"), "{sent}");
}

#[test]
fn a_swap_that_cut_off_an_unanswered_call_asks_again_once_egress_has_answered_it() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    let (request, _) = open_request(chain);
    chain.cancel_parked(PROGRAM);
    let deadline = request.made + egress::DEFAULT_WAIT;
    chain.at(deadline + 1, 0);
    assert!(notices(chain, thread).is_empty(), "egress answers by then");
    chain.at(deadline + 2, 0);
    let [told]: [String; 1] = notices(chain, thread).try_into().unwrap();
    let cut = format!("the model call made at block {} was cut off", request.made);
    assert!(told.starts_with(&cut), "{told}");
    let again = format!("again at block {} (attempt 1)", deadline + 2);
    assert!(told.ends_with(&again), "{told}");
    chain.at(deadline + 3, 0);
    model_says(chain, hello(), "end_turn");
    assert_eq!(turns(chain, thread), 1);
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);
}

fn read_tool() -> Vec<Vec<u8>> {
    let fixture = TOOL_USE
        .replace("get_balance", "files_read")
        .replace("asset", "path")
        .replace("USDC", "/x");
    events(&fixture)
}

#[test]
fn a_swap_mid_reply_records_what_streamed_whole_and_its_call_runs_once() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    let (request, _) = open_request(chain);
    for (seq, data) in (0u64..).zip(read_tool()) {
        let id = request.id;
        let event = egress::Event { id, seq, data };
        let op = egress::Op::Events(vec![event]);
        chain.submit(signed(&NODE), egress::PROGRAM, &op).unwrap();
    }
    let [call]: [CallRow; 1] = calls(chain, thread).try_into().unwrap();
    assert_eq!(call.state, CallState::Relaying);
    chain.cancel_parked(PROGRAM);
    let deadline = 3 + egress::DEFAULT_WAIT;
    chain.at(deadline + 2, 0);
    assert_eq!(turns(chain, thread), 1, "what streamed whole is the turn");
    assert_eq!(calls(chain, thread).len(), 1, "its call is placed once");
    assert_eq!(thread_row(chain, thread).asking, Asking::Running);
    let relays = deadline + 1 + egress::DEFAULT_WAIT;
    chain.at(relays + 2, 0);
    relay_back(chain, "toolu_1");
    let results = frames(chain, thread)
        .into_iter()
        .filter(|body| matches!(body, Body::Result { call, .. } if call == "toolu_1"))
        .count();
    assert_eq!(results, 1);
    let (_, head) = open_request(chain);
    assert_eq!(path(&head), "/v1/messages");
}

#[test]
fn the_manager_hears_one_line_for_a_blocks_cut_turns_and_a_bound_room_one_post() {
    let world = world();
    let chain = &world.chain;
    let create = identity::Op::CreateAgent {
        name: "Chief".into(),
    };
    let chief = number(identity_op(chain, ADA, create).unwrap());
    scripted(chain, chief, CONFIG);
    let room = chat::Op::CreateChannel {
        channel_id: "j-1".into(),
        name: "j-1".into(),
        post_policy: chat::PostPolicy::Open,
    };
    chain.submit(signed(ADA), chat::PROGRAM, &room).unwrap();
    let open = Op::OpenThread {
        agent: world.agent,
        place: Place::Channel {
            channel: "j-1".into(),
        },
    };
    let bound: u64 = abi::decode(&chain.submit(signed(ADA), PROGRAM, &open).unwrap()).unwrap();
    let canonical = world.opened("m1", "hello", Some(WINDOW));
    world.post(ADA, "j-1", "m2", &format!("<@{}> and here", world.agent));
    let host = chain.host(PROGRAM);
    let ctx = host.exec(chain.env(PROGRAM));
    for id in [canonical, bound] {
        let managed = ThreadRow {
            manager: chief,
            ..thread_row(chain, id)
        };
        THREADS.put(&ctx, &id, &managed);
    }
    chain.at(3, 0);
    let cut = assigned(chain, SERVICE);
    assert_eq!(cut.len(), 2);
    for request in &cut {
        fail(chain, request);
    }
    chain.at(5, 0);
    let asked = Query::Threads {
        agent: chief,
        page: PageRequest::default(),
    };
    let Reply::Threads(page) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!()
    };
    let [told]: [ThreadRow; 1] = page.items.try_into().unwrap();
    let posts: Vec<String> = frames(chain, told.id)
        .into_iter()
        .filter_map(|body| match body {
            Body::Post { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    let [post]: [String; 1] = posts.try_into().unwrap();
    assert!(post.starts_with("harness picked up 2 turns"), "{post}");
    for id in [canonical, bound] {
        assert!(post.contains(&format!("- thread {id} (")), "{post}");
    }
    let message_id = format!("harness-resume-{bound}-3");
    let asked = chat::Query::MessageById { message_id };
    let chat::Reply::Message(Some(line)) = chain.query(chat::PROGRAM, &asked).unwrap() else {
        panic!("the bound room has the line")
    };
    let resumed = format!("conversation {bound}'s turn resumed after block 3 (egress failed)");
    assert!(line.text.contains(&resumed), "{}", line.text);
}

#[test]
fn a_mention_from_someone_the_agent_does_not_answer_leaves_a_notice() {
    let world = world();
    let chain = &world.chain;
    manager_only(&world);
    world.post(BO, "general", "m1", &format!("<@{}> psst", world.agent));
    let [row]: [ThreadRow; 1] = world.threads().try_into().unwrap();
    let [told]: [String; 1] = notices(chain, row.id).try_into().unwrap();
    let from = format!("dropped a mention from account {} ", world.bo);
    assert!(told.starts_with(&from), "{told}");
    assert!(
        told.ends_with("this agent doesn't answer it (identity: Answers)"),
        "{told}"
    );
    assert!(assigned(chain, SERVICE).is_empty(), "a notice asks nothing");
}

fn limited_until(chain: &MockChain, reset: &str) {
    let (request, _) = open_request(chain);
    let limited =
        json!({ "type": "error", "error": { "type": "rate_limit_error", "message": "Slow down" } });
    let busy = http::Response {
        status: 429,
        headers: vec![
            (
                "anthropic-ratelimit-unified-5h-utilization".into(),
                "1.0".into(),
            ),
            ("anthropic-ratelimit-unified-5h-reset".into(), reset.into()),
        ],
        body: limited.to_string().into_bytes(),
    };
    answer(chain, request.id, Answer::Output(abi::encode(&busy)));
}

#[test]
fn a_429_holds_until_the_usage_window_resets_or_an_input_comes_and_the_window_is_kept() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 1_000_000);
    limited_until(chain, "1100");
    let [told]: [String; 1] = notices(chain, thread).try_into().unwrap();
    let held = "again once the model's wait ends at 1100 (seconds since the epoch) (attempt 1)";
    assert!(told.ends_with(held), "{told}");
    let asked = Query::UsageWindow {
        service: SERVICE.into(),
    };
    let Reply::UsageWindow(Some(window)) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!("the window is kept")
    };
    assert_eq!((window.utilization.as_str(), window.reset), ("1.0", 1100));
    chain.at(4, 1_010_000);
    assert!(assigned(chain, SERVICE).is_empty(), "held for the reset");
    chain.at(5, 1_020_000);
    world.mention("m2", "still there?");
    assert_eq!(
        assigned(chain, SERVICE).len(),
        1,
        "a new input ends the wait for the reset"
    );
}

#[test]
fn a_resume_someone_sends_ends_a_429s_wait_for_the_reset() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 1_000_000);
    limited_until(chain, "1100");
    chain.at(5, 1_020_000);
    assert!(assigned(chain, SERVICE).is_empty(), "held for the reset");
    chain
        .submit(signed(ADA), PROGRAM, &Op::Resume { thread })
        .unwrap();
    assert_eq!(
        assigned(chain, SERVICE).len(),
        1,
        "a resume ends the wait for the reset"
    );
}

#[test]
fn a_429s_wait_ends_by_itself_at_the_reset() {
    let world = world();
    let chain = &world.chain;
    world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 1_000_000);
    limited_until(chain, "1100");
    chain.at(4, 1_010_000);
    assert!(assigned(chain, SERVICE).is_empty(), "held for the reset");
    chain.at(13, 1_100_000);
    chain.at(14, 1_100_000);
    assert_eq!(assigned(chain, SERVICE).len(), 1, "asked at the reset");
}

fn refused(chain: &MockChain) {
    let (request, _) = open_request(chain);
    let invalid =
        json!({ "type": "error", "error": { "type": "invalid_request_error", "message": "bad" } });
    answer(chain, request.id, Answer::Output(response(400, invalid)));
}

#[test]
fn a_resume_someone_sends_asks_again_about_the_input_a_turn_gave_up_on() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    refused(chain);
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "a refusal ends the turn"
    );
    chain.at(4, 0);
    let resume = Op::Resume { thread };
    chain.submit(signed(ADA), PROGRAM, &resume).unwrap();
    let (request, _) = open_request(chain);
    let sent = body(chain, &request)["messages"].to_string();
    assert!(sent.contains("hello"), "{sent}");
    let told = notices(chain, thread);
    let last = told.last().unwrap();
    assert!(last.starts_with("re-asked by account "), "{last}");
    assert!(last.ends_with(" after a turn that gave up"), "{last}");
    chain.at(5, 0);
    model_says(chain, hello(), "end_turn");
    assert_eq!(turns(chain, thread), 1);
    chain.at(6, 0);
    chain.submit(signed(ADA), PROGRAM, &resume).unwrap();
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "an answered input is not asked again"
    );
    assert_eq!(notices(chain, thread).len(), told.len());
}

#[test]
fn harness_own_wakes_never_ask_again_about_input_a_turn_gave_up_on() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    refused(chain);
    let told = notices(chain, thread).len();
    chain.at(3 + egress::DEFAULT_WAIT + 3, 0);
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "the watch asks nothing"
    );
    assert_eq!(notices(chain, thread).len(), told);
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);
}
