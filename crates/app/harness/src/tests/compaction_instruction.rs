use super::*;
use crate::config::PromptConfig;

fn configure(chain: &MockChain, instruction: Option<&str>, provider: &str) {
    let parsed = PromptConfig {
        config: Config {
            compact_at: Some(1000),
            ..scout()
        },
        pin: pin_of(provider),
        compaction_instruction: instruction.map(str::to_owned),
        tool_continuation: crate::config::ToolContinuation::default(),
    };
    commit(chain, ADA, CONFIG, &parsed.write());
}

fn codex(world: &World) {
    let route = Op::SetRoute {
        provider: model_wire::codex::PROVIDER.into(),
        services: vec![RouteService {
            service: SERVICE.into(),
            wire: Wire::Harness,
        }],
    };
    world.chain.submit(signed(ADA), PROGRAM, &route).unwrap();
}

fn complete_turn(chain: &MockChain) {
    model_says_after(
        chain,
        json!([{ "type":"text", "text":"Noted." }]),
        "end_turn",
        1000,
    );
}

fn summary(world: &World, id: &str) -> RequestRow {
    world.mention(id, "continue");
    let (request, head) = open_request(&world.chain);
    assert_eq!(path(&head), model_wire::SUMMARY);
    request
}

#[test]
fn a_summary_refreshes_the_exact_instruction_without_leaking_it_into_turns() {
    let world = world();
    let chain = &world.chain;
    codex(&world);
    configure(
        chain,
        Some("A: preserve decisions"),
        model_wire::codex::PROVIDER,
    );
    let thread = world.mention("m1", "remember this");
    chain.at(2, 0);
    super::accounts::model_read_on(chain, SERVICE, MODEL, model_wire::codex::PROVIDER);
    chain.at(3, 0);
    let (turn, _) = open_request(chain);
    assert!(body(chain, &turn).get("compaction_instruction").is_none());
    complete_turn(chain);
    let exact = "B: \"quoted\"\n한국어 ";
    configure(chain, Some(exact), model_wire::codex::PROVIDER);
    chain.at(4, 0);
    let request = summary(&world, "m2");
    let sent = body(chain, &request);
    assert_eq!(sent["compaction_instruction"], exact);
    assert!(!sent["system"].to_string().contains("B:"));
    let tail = chain
        .host(PROGRAM)
        .borrow()
        .lines
        .get(&(
            PROGRAM.into(),
            crate::turns::held_on(thread, b"summary-tail"),
        ))
        .copied()
        .unwrap();
    let original = chain.host(PROGRAM).borrow().blobs[&tail].body.clone();
    configure(chain, None, model_wire::codex::PROVIDER);
    assert_eq!(
        body(chain, &request),
        sent,
        "a file edit cannot rewrite an in-flight request"
    );
    assert_eq!(chain.host(PROGRAM).borrow().blobs[&tail].body, original);
    chain.at(5, 0);
    summarized(chain, "Summary.");
    let (turn, _) = open_request(chain);
    assert!(body(chain, &turn).get("compaction_instruction").is_none());
    chain.at(6, 0);
    complete_turn(chain);
    chain.at(7, 0);
    let request = summary(&world, "m3");
    assert!(
        body(chain, &request)
            .get("compaction_instruction")
            .is_none()
    );
    chain.at(8, 0);
    summarized(chain, "Default summary.");
    chain.at(9, 0);
    complete_turn(chain);
    configure(
        chain,
        Some("C: new exact directive"),
        model_wire::codex::PROVIDER,
    );
    chain.at(10, 0);
    let request = summary(&world, "m4");
    assert_eq!(
        body(chain, &request)["compaction_instruction"],
        "C: new exact directive"
    );
    let current_tail = chain.host(PROGRAM).borrow().lines[&(
        PROGRAM.into(),
        crate::turns::held_on(thread, b"summary-tail"),
    )];
    assert_ne!(current_tail, tail);
}

#[test]
fn overflow_recovery_uses_the_current_instruction_only_on_the_summary() {
    let world = world();
    let chain = &world.chain;
    codex(&world);
    configure(
        chain,
        Some("Overflow exact directive"),
        model_wire::codex::PROVIDER,
    );
    let thread = world.mention("m1", "one");
    chain.at(2, 0);
    super::accounts::model_read_on(chain, SERVICE, MODEL, model_wire::codex::PROVIDER);
    chain.at(3, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "One." }]),
        "end_turn",
    );
    world.mention("m2", "two");
    chain.at(4, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Two." }]),
        "end_turn",
    );
    world.mention("m3", "three");
    chain.at(5, 0);
    let (turn, head) = open_request(chain);
    assert_eq!(path(&head), model_wire::TURN);
    assert!(body(chain, &turn).get("compaction_instruction").is_none());
    refused_as_too_long(chain);
    let (request, head) = open_request(chain);
    assert_eq!(path(&head), model_wire::SUMMARY);
    assert_eq!(
        body(chain, &request)["compaction_instruction"],
        "Overflow exact directive"
    );
    assert!(matches!(
        thread_row(chain, thread).asking,
        Asking::Compacting { .. }
    ));
}

#[test]
fn unsupported_summary_routes_refuse_the_override_without_asking_a_summary_model() {
    for wire in [Wire::Harness, Wire::AnthropicMessages] {
        let world = world();
        let chain = &world.chain;
        let route = set_route(&[(SERVICE, wire)]);
        chain.submit(signed(ADA), PROGRAM, &route).unwrap();
        configure(chain, Some("Keep decisions"), PROVIDER);
        let thread = world.mention("m1", "remember");
        chain.at(2, 0);
        let (request, _) = open_request(chain);
        let info = json!({ "type": "model", "id": MODEL, "max_input_tokens": WINDOW });
        answer(chain, request.id, Answer::Output(response(200, info)));
        chain.at(3, 0);
        let (request, head) = open_request(chain);
        let expected = match wire {
            Wire::Harness => model_wire::TURN,
            Wire::AnthropicMessages => "/v1/messages",
        };
        assert_eq!(path(&head), expected);
        let said = reply(
            json!([{ "type": "text", "text": "Noted." }]),
            "end_turn",
            1000,
        );
        answer(chain, request.id, Answer::Output(said));
        chain.at(4, 0);
        world.mention("m2", "continue");
        assert!(assigned(chain, SERVICE).is_empty());
        assert!(
            notices(chain, thread)
                .iter()
                .any(|text| text.contains("compaction_instruction"))
        );
    }
}

fn summary_for(chain: &MockChain, thread: u64) -> RequestRow {
    assigned(chain, SERVICE)
        .into_iter()
        .find(|row| {
            let target = match &row.request {
                Request::Http { head, .. } => head.target.as_str(),
                _ => "",
            };
            let sent = body(chain, row);
            let expected = thread.to_string();
            target == model_wire::SUMMARY && sent["cache_key"].as_str() == Some(expected.as_str())
        })
        .unwrap()
}

fn finish_for(chain: &MockChain, thread: u64) {
    let (request, _) = super::accounts::turn_open(chain, thread);
    super::accounts::replied(
        chain,
        &request,
        vec![model_wire::Block::Text {
            text: "Noted".into(),
            citations: Vec::new(),
        }],
        model_wire::Usage {
            input: 1000,
            output: 8,
            cache_read: 0,
            cache_write: 0,
        },
        Vec::new(),
    );
}

#[test]
fn interleaved_agents_keep_their_own_in_flight_summary_tail_and_default() {
    for second_instruction in [None, Some("Second exact directive")] {
        interleaved_summaries(second_instruction);
    }
}

fn interleaved_summaries(second_instruction: Option<&str>) {
    let world = world();
    let chain = &world.chain;
    codex(&world);
    configure(
        chain,
        Some("Scout exact directive"),
        model_wire::codex::PROVIDER,
    );
    let second_path = "/agents/second.md";
    let second_config = PromptConfig {
        config: Config {
            compact_at: Some(1000),
            ..scout()
        },
        pin: pin_of(model_wire::codex::PROVIDER),
        compaction_instruction: second_instruction.map(str::to_owned),
        tool_continuation: crate::config::ToolContinuation::default(),
    };
    commit(chain, ADA, second_path, &second_config.write());
    let second = number(
        identity_op(
            chain,
            ADA,
            identity::Op::CreateAgent {
                name: "Second".into(),
            },
        )
        .unwrap(),
    );
    scripted(chain, second, second_path);
    let first_thread = world.mention("m1", "remember scout");
    chain.at(2, 0);
    super::accounts::model_read_on(chain, SERVICE, MODEL, model_wire::codex::PROVIDER);
    let post = chat::Op::PostMessage {
        channel_id: "general".into(),
        message_id: "s1".into(),
        blocks: chat::parse_message(&format!("<@{second}> remember second")),
        thread: None,
    };
    chain.submit(signed(ADA), chat::PROGRAM, &post).unwrap();
    let Reply::Thread(Some(second_row)) = chain
        .query(
            PROGRAM,
            &Query::ThreadAt {
                agent: second,
                place: "chat/general".into(),
            },
        )
        .unwrap()
    else {
        panic!("second conversation exists")
    };
    let second_thread = second_row.id;
    chain.at(3, 0);
    finish_for(chain, first_thread);
    finish_for(chain, second_thread);
    chain.at(4, 0);
    world.mention("m2", "summarize scout");
    let first = summary_for(chain, first_thread);
    let first_body = body(chain, &first);
    let post = chat::Op::PostMessage {
        channel_id: "general".into(),
        message_id: "s2".into(),
        blocks: chat::parse_message(&format!("<@{second}> summarize second")),
        thread: None,
    };
    chain.submit(signed(ADA), chat::PROGRAM, &post).unwrap();
    let second_request = summary_for(chain, second_thread);
    assert_eq!(body(chain, &first), first_body);
    assert_eq!(
        first_body["compaction_instruction"],
        "Scout exact directive"
    );
    assert_eq!(
        body(chain, &second_request)
            .get("compaction_instruction")
            .and_then(Value::as_str),
        second_instruction
    );
    let host = chain.host(PROGRAM);
    let host = host.borrow();
    assert!(host.lines.contains_key(&(
        PROGRAM.into(),
        crate::turns::held_on(first_thread, b"summary-tail")
    )));
    assert_eq!(
        host.lines.contains_key(&(
            PROGRAM.into(),
            crate::turns::held_on(second_thread, b"summary-tail")
        )),
        second_instruction.is_some()
    );
}
