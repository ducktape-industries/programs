use super::network::{pods_get, served};
use super::*;
use crate::config::{PromptConfig, ToolContinuation};

fn configure(chain: &MockChain, mode: ToolContinuation) {
    let prompt = PromptConfig {
        config: scout(),
        pin: pin_of(PROVIDER),
        compaction_instruction: None,
        tool_continuation: mode,
    };
    commit(chain, ADA, CONFIG, &prompt.write());
}

fn assert_waiting(chain: &MockChain, thread: u64, open: u32) {
    assert_eq!(thread_row(chain, thread).open_calls, open);
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);
    assert!(assigned(chain, SERVICE).is_empty());
}

fn sent(chain: &MockChain) -> String {
    let (request, _) = open_request(chain);
    provider_body(chain, &request)["messages"].to_string()
}

fn calls(chain: &MockChain, thread: u64) -> Vec<CallRow> {
    let Reply::Calls(page) = chain
        .query(
            PROGRAM,
            &Query::Calls {
                thread,
                page: PageRequest::default(),
            },
        )
        .unwrap()
    else {
        panic!()
    };
    page.items
}

#[test]
fn sync_waits_for_every_result_without_serializing_calls_or_steering() {
    let world = super::schedule::schemed();
    let chain = &world.chain;
    served(chain);
    configure(chain, ToolContinuation::Sync);
    let thread = world.opened("m1", "use both", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([
            tool_use("slow", "network_call", pods_get("/v1/pods")),
            tool_use("fast", "files_list", json!({"path":"/shared"}))
        ]),
        "tool_use",
    );
    assert_waiting(chain, thread, 2);
    assert_eq!(assigned(chain, RELAY).len(), 2);
    world.mention("steer", "keep this correction");
    chain
        .submit(signed(ADA), PROGRAM, &Op::Resume { thread })
        .unwrap();
    assert_waiting(chain, thread, 2);
    chain.at(4, 0);
    relay_back(chain, "slow");
    relay_back(chain, "fast");
    let [request]: [RequestRow; 1] = assigned(chain, "pods").try_into().unwrap();
    assert_waiting(chain, thread, 1);
    assert!(!result_of(chain, thread, "fast").1);
    chain.at(5, 0);
    assert_waiting(chain, thread, 1);
    answer(
        chain,
        request.id,
        Answer::Output(response(200, json!({"pods":["finished"]}))),
    );
    chain.at(7, 0);
    let messages = sent(chain);
    for wanted in [
        "Result of tool call slow",
        "Result of tool call fast",
        "finished",
        "keep this correction",
    ] {
        assert!(messages.contains(wanted), "{messages}");
    }
    assert_eq!(thread_row(chain, thread).open_calls, 0);
}

#[test]
fn an_unrelated_default_agent_progresses_while_sync_waits() {
    let world = world();
    let chain = &world.chain;
    configure(chain, ToolContinuation::Sync);
    let thread = world.opened("m1", "list", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([tool_use("pending", "files_list", json!({"path":"/shared"}))]),
        "tool_use",
    );
    assert_waiting(chain, thread, 1);
    let second = super::accounts::agent_on(&world, "Second", MODEL, SERVICE, PROVIDER);
    let second_thread = super::accounts::mention_of(&world, second, "s1", "keep working");
    chain.at(4, 0);
    let (request, _) = super::accounts::turn_open(chain, second_thread);
    let body = provider_body(chain, &request)["messages"].to_string();
    assert!(body.contains("keep working"));
    super::accounts::replied(
        chain,
        &request,
        vec![model_wire::Block::Text {
            text: "second finished".into(),
            citations: Vec::new(),
        }],
        model_wire::Usage::default(),
        Vec::new(),
    );
    chain.at(5, 0);
    assert_eq!(thread_row(chain, second_thread).asking, Asking::Idle);
    assert!(thread_row(chain, second_thread).last_stop.is_some());
    assert_waiting(chain, thread, 1);
    relay_back(chain, "pending");
    chain.at(7, 0);
    assert!(sent(chain).contains("Result of tool call pending"));
    assert_eq!(thread_row(chain, thread).open_calls, 0);
}

#[test]
fn sync_releases_after_refusals_and_terminal_relay_failures() {
    let world = world();
    let chain = &world.chain;
    configure(chain, ToolContinuation::Sync);
    let thread = world.opened("m1", "use tools", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([
            tool_use("refused", "not_a_tool", json!({})),
            tool_use("failed", "files_list", json!({"path":"/shared"}))
        ]),
        "tool_use",
    );
    assert_waiting(chain, thread, 1);
    assert!(
        frames(chain, thread)
            .iter()
            .any(|body| matches!(body, Body::Refused { call, .. } if call == "refused"))
    );
    chain.at(4, 0);
    let [relay]: [RequestRow; 1] = assigned(chain, RELAY).try_into().unwrap();
    answer(chain, relay.id, Answer::Failed("relay refused".into()));
    chain.at(6, 0);
    assert!(result_of(chain, thread, "failed").1);
    assert!(sent(chain).contains("relay failed"));
    assert_eq!(thread_row(chain, thread).open_calls, 0);
}

#[test]
fn changing_continuation_refreshes_an_existing_kept_conversation() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "list", Some(WINDOW));
    configure(chain, ToolContinuation::Sync);
    chain.at(3, 0);
    model_says(
        chain,
        json!([tool_use("pending", "files_list", json!({"path":"/shared"}))]),
        "tool_use",
    );
    assert_waiting(chain, thread, 1);
    configure(chain, ToolContinuation::Async);
    chain
        .submit(signed(ADA), PROGRAM, &Op::Resume { thread })
        .unwrap();
    assert!(sent(chain).contains("Accepted as call pending"));
    assert_eq!(thread_row(chain, thread).open_calls, 1);
}

#[test]
fn recovery_runs_cut_off_relays_before_releasing_sync() {
    let world = world();
    let chain = &world.chain;
    configure(chain, ToolContinuation::Sync);
    let thread = world.opened("m1", "list", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([tool_use(
            "pending",
            "files_read",
            json!({"path":"/missing"})
        )]),
        "tool_use",
    );
    assert_waiting(chain, thread, 1);
    chain.cancel_parked(PROGRAM);
    let deadline = 3 + egress::DEFAULT_WAIT;
    chain.at(deadline + 2, 0);
    chain
        .submit(signed(ADA), PROGRAM, &Op::Resume { thread })
        .unwrap();
    assert_waiting(chain, thread, 1);
    chain.at(deadline + 3, 0);
    relay_back(chain, "pending");
    chain.at(deadline + 5, 0);
    assert!(result_of(chain, thread, "pending").1);
    assert!(sent(chain).contains("Result of tool call pending"));
    assert_eq!(thread_row(chain, thread).open_calls, 0);
}

#[test]
fn a_cut_off_whole_stream_records_its_calls_and_waits_for_their_results() {
    let world = world();
    let chain = &world.chain;
    configure(chain, ToolContinuation::Sync);
    let thread = world.opened("m1", "read", Some(WINDOW));
    chain.at(3, 0);
    let (request, _) = open_request(chain);
    let fixture = crate::wire::tests::TOOL_USE
        .replace("get_balance", "files_read")
        .replace("asset", "path")
        .replace("USDC", "/missing");
    for (seq, data) in (0u64..).zip(adapted::adapted(&crate::wire::tests::events(&fixture))) {
        chain
            .submit(
                signed(&NODE),
                egress::PROGRAM,
                &egress::Op::Events(vec![egress::Event {
                    id: request.id,
                    seq,
                    data,
                }]),
            )
            .unwrap();
    }
    assert_eq!(calls(chain, thread)[0].state, CallState::Relaying);
    chain.cancel_parked(PROGRAM);
    let deadline = 3 + egress::DEFAULT_WAIT;
    chain.at(deadline + 2, 0);
    assert_waiting(chain, thread, 1);
    assert_eq!(calls(chain, thread).len(), 1);
    chain.at(deadline + 3, 0);
    relay_back(chain, "toolu_1");
    chain.at(deadline + 5, 0);
    assert!(sent(chain).contains("Result of tool call toolu_1"));
    assert_eq!(thread_row(chain, thread).open_calls, 0);
}

#[test]
fn cut_off_writes_finish_failed_once_and_do_not_orphan_the_sync_wait() {
    use crate::schema::tests::{code_with, listed, module};
    use ::program::schema::interface;
    let mut world = world();
    let chain = &mut world.chain;
    chain.seat::<programs::Programs>(programs::PROGRAM);
    let codes = |build: &[u8]| {
        vec![
            (PROGRAM, module(&[("ducktape.build", build)])),
            (egress::PROGRAM, code_with(&interface::<Egress>())),
            ("valset", module(&[])),
        ]
    };
    listed(chain, &codes(b"first"));
    served(chain);
    configure(chain, ToolContinuation::Sync);
    let chain = &world.chain;
    let thread = world.opened("m1", "read pods", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([tool_use("pending", "network_call", pods_get("/v1/pods"))]),
        "tool_use",
    );
    chain.at(4, 0);
    relay_back(chain, "pending");
    let [request]: [RequestRow; 1] = assigned(chain, "pods").try_into().unwrap();
    assert_waiting(chain, thread, 1);
    chain.at(5, 0);
    listed(chain, &codes(b"second"));
    chain.cancel_parked(PROGRAM);
    chain
        .submit(signed(ADA), PROGRAM, &Op::Resume { thread })
        .unwrap();
    assert_eq!(thread_row(chain, thread).open_calls, 0);
    assert!(result_of(chain, thread, "pending").1);
    assert!(sent(chain).contains("It may have run"));
    chain.at(6, 0);
    answer(
        chain,
        request.id,
        Answer::Output(response(200, json!({"pods":[]}))),
    );
    let results = frames(chain, thread)
        .into_iter()
        .chain(held(chain, thread))
        .filter(|body| matches!(body, Body::Arrived { call, .. } if call == "pending"))
        .count();
    assert_eq!(results, 1);
}
