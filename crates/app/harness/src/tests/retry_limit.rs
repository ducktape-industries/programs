//! Finite automatic retries preserve a discoverable failure and stop background wake loops.
use super::*;

const RETRIES: u32 = 3;

fn errors(world: &World) -> Vec<FinalError> {
    let query = Query::FinalErrors {
        agent: world.agent,
        page: PageRequest::default(),
    };
    let Reply::FinalErrors(page) = world.chain.query(PROGRAM, &query).unwrap() else {
        panic!("final errors")
    };
    page.items
}

fn reject(chain: &MockChain) {
    let (request, _) = open_request(chain);
    let failure =
        json!({"type":"error","error":{"type":"rate_limit_error","message":"pool rejected"}});
    answer(chain, request.id, Answer::Output(response(429, failure)));
}

fn exhaust(world: &World) -> (u64, u64) {
    let chain = &world.chain;
    let thread = world.opened("m1", "preserve this input", Some(WINDOW));
    let mut height = 3;
    for failure in 1..=RETRIES + 1 {
        chain.at(height, 0);
        reject(chain);
        if failure <= RETRIES {
            height += (1u64 << (failure - 1)) + 1;
        }
    }
    (thread, height)
}

#[test]
fn initial_attempt_and_three_retries_stop_with_a_durable_failure_and_context() {
    let world = world();
    let chain = &world.chain;
    let (thread, height) = exhaust(&world);
    assert!(assigned(chain, SERVICE).is_empty());
    let listed = errors(&world);
    let [failure] = listed.as_slice() else {
        panic!("one durable failure: {listed:?}")
    };
    assert_eq!(failure.thread, thread);
    assert!(failure.text.contains("pool rejected"), "{}", failure.text);
    assert!(
        failure.text.contains("3 automatic retries"),
        "{}",
        failure.text
    );
    let query = Query::Frames {
        thread,
        page: PageRequest::default(),
    };
    let Reply::Frames(page) = chain.query(PROGRAM, &query).unwrap() else {
        panic!("frames")
    };
    assert!(page.items.iter().any(|frame| frame.seq == failure.frame
        && matches!(&frame.body, Body::Notice { text } if text == &failure.text)));
    assert!(page.items.iter().any(|frame| matches!(&frame.body, Body::Post { text, .. } if text.contains("preserve this input"))));
    chain.at(height + egress::DEFAULT_WAIT + 2, 0);
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "no fourth automatic retry"
    );
    assert_eq!(
        errors(&world),
        listed,
        "a wake must not overwrite the failure"
    );
}

#[test]
fn resume_and_new_notifications_cannot_restart_exhausted_unchanged_work() {
    let world = world();
    let chain = &world.chain;
    let (thread, height) = exhaust(&world);
    chain.at(height + 1, 0);
    chain
        .submit(signed(ADA), PROGRAM, &Op::Resume { thread })
        .unwrap();
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "Resume is also used by background programs"
    );
    world.mention("m2", "a later notification must remain available");
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "ordinary notifications do not replenish retries"
    );
    assert!(!errors(&world).is_empty());
}

#[test]
fn a_successful_turn_resets_the_allowance_for_the_next_turn() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    reject(chain);
    chain.at(5, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "back" }]),
        "end_turn",
    );
    world.mention("m2", "new turn");
    let mut height = 6;
    for failure in 1..=RETRIES + 1 {
        chain.at(height, 0);
        reject(chain);
        if failure <= RETRIES {
            height += (1u64 << (failure - 1)) + 1;
        }
    }
    chain.at(height + egress::DEFAULT_WAIT + 2, 0);
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "fresh allowance also ends"
    );
    assert_eq!(errors(&world)[0].thread, thread);
}

#[test]
fn explicit_retry_keeps_old_and_new_input_and_gets_a_fresh_allowance() {
    let world = world();
    let chain = &world.chain;
    let (thread, height) = exhaust(&world);
    world.mention("m2", "new instruction retained");
    chain.at(height + 1, 0);
    chain
        .submit(signed(BO), PROGRAM, &Op::Retry { thread })
        .unwrap();
    let (request, _) = open_request(chain);
    let messages = provider_body(chain, &request)["messages"].to_string();
    assert!(messages.contains("preserve this input"));
    assert!(messages.contains("new instruction retained"));
    let mut height = height + 2;
    for failure in 1..=RETRIES + 1 {
        chain.at(height, 0);
        reject(chain);
        if failure <= RETRIES {
            height += (1u64 << (failure - 1)) + 1;
        }
    }
    chain.at(height + egress::DEFAULT_WAIT + 2, 0);
    assert!(assigned(chain, SERVICE).is_empty(), "pickup is bounded too");
    chain
        .submit(signed(ADA), PROGRAM, &Op::Retry { thread })
        .unwrap();
    chain.at(height + egress::DEFAULT_WAIT + 3, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "picked up" }]),
        "end_turn",
    );
    assert!(errors(&world).is_empty());
}

#[test]
fn unrelated_success_does_not_replenish_exhaustion() {
    let world = world();
    let chain = &world.chain;
    let (_, height) = exhaust(&world);
    let old = errors(&world);
    let agent = accounts::agent_on(&world, "Other", MODEL, SERVICE, PROVIDER);
    accounts::mention_of(&world, agent, "m2", "hello");
    chain.at(height + 1, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "unrelated success" }]),
        "end_turn",
    );
    chain.at(height + 2, 0);
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "unrelated success asks no exhausted input"
    );
    assert_eq!(errors(&world), old);
}

#[test]
fn a_real_booked_resume_and_its_result_do_not_replenish_exhaustion() {
    let world = schedule::schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "book then exhaust", Some(WINDOW));
    let payload = json!({ "resume": { "thread": thread } });
    schedule::turn(
        &world,
        3,
        &[(
            "b1",
            "schedule_run",
            json!({
                "height": 50, "program": PROGRAM, "op": payload,
            }),
        )],
    );
    let booked = schedule::answered(chain, thread, "b1")["entry"]
        .as_u64()
        .unwrap();
    let mut height = 5;
    for failure in 1..=RETRIES + 1 {
        chain.at(height, 0);
        reject(chain);
        if failure <= RETRIES {
            height += (1u64 << (failure - 1)) + 1;
        }
    }
    let old = errors(&world);
    chain.at(50, 0);
    chain.at(51, 0);
    let Reply::Booking(Some(booking)) = chain
        .query(PROGRAM, &Query::Booking { entry: booked })
        .unwrap()
    else {
        panic!("booked run retained")
    };
    assert!(!booking.ran.expect("actually ran").failed);
    assert!(
        frames(chain, thread)
            .iter()
            .any(|body| matches!(body, Body::Post { text, .. } if text.contains("ran as you")))
    );
    assert!(assigned(chain, SERVICE).is_empty());
    assert_eq!(errors(&world), old);
}

#[test]
fn a_legacy_high_attempt_row_stops_before_another_request() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "legacy input", Some(WINDOW));
    chain.at(3, 0);
    reject(chain);
    {
        let mut host = chain.host(PROGRAM).borrow_mut();
        let state = &mut host.state;
        let key = crate::state::RETRIES.key(&thread);
        let mut row: crate::state::Retry = abi::decode(state.get(&key).unwrap()).unwrap();
        row.attempt = 183;
        state.insert(key, abi::encode(&row));
        state.remove(&crate::state::MODEL_FAILURES.key(&thread));
    }
    chain.at(5, 0);
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "already exhausted old rows ask nothing"
    );
    assert!(errors(&world)[0].text.contains("3 automatic retries"));
}

#[test]
fn explicit_retry_cannot_reopen_a_closed_conversation_or_bypass_an_inactive_agent() {
    let world = world();
    let chain = &world.chain;
    let (thread, _) = exhaust(&world);
    identity_op(
        chain,
        ADA,
        identity::Op::Suspend {
            account: world.agent,
        },
    )
    .unwrap();
    assert_eq!(
        chain
            .submit(signed(BO), PROGRAM, &Op::Retry { thread })
            .unwrap_err()
            .code,
        code::WRONG_STATE
    );
    identity_op(
        chain,
        ADA,
        identity::Op::Resume {
            account: world.agent,
        },
    )
    .unwrap();
    identity_op(
        chain,
        ADA,
        identity::Op::SetScript {
            account: world.agent,
            script: None,
        },
    )
    .unwrap();
    assert_eq!(
        chain
            .submit(signed(BO), PROGRAM, &Op::Retry { thread })
            .unwrap_err()
            .code,
        code::WRONG_STATE
    );
    scripted(chain, world.agent, CONFIG);
    chain
        .submit(signed(ADA), PROGRAM, &Op::Close { thread })
        .unwrap();
    let before = assigned(chain, SERVICE);
    assert!(
        before.iter().all(|request| request.delivery
            != egress::Delivery::Events {
                deliver: streamed_as(thread)
            }),
        "the closed conversation asks nothing; its unanswered input goes to an heir"
    );
    assert_eq!(
        chain
            .submit(signed(BO), PROGRAM, &Op::Retry { thread })
            .unwrap_err()
            .code,
        code::WRONG_STATE
    );
    assert!(thread_row(chain, thread).closed);
    assert_eq!(
        assigned(chain, SERVICE),
        before,
        "Retry cannot alter the heir or reopen the closed thread"
    );
}

#[test]
fn transport_errors_and_5xx_share_the_same_finite_allowance() {
    for transport in [false, true] {
        let world = world();
        let chain = &world.chain;
        world.opened("m1", "retain failed request", Some(WINDOW));
        let mut height = 3;
        for failure in 1..=RETRIES + 1 {
            chain.at(height, 0);
            let (request, _) = open_request(chain);
            let answer_value = if transport {
                Answer::Failed("transport unavailable".into())
            } else {
                Answer::Output(response(
                    503,
                    json!({"error":{"type":"overloaded_error","message":"service unavailable"}}),
                ))
            };
            answer(chain, request.id, answer_value);
            if failure <= RETRIES {
                height += (1u64 << (failure - 1)) + 1;
            }
        }
        chain.at(height + egress::DEFAULT_WAIT + 2, 0);
        assert!(assigned(chain, SERVICE).is_empty());
        assert!(errors(&world)[0].text.contains("3 automatic retries"));
    }
}

#[test]
fn legacy_adoption_keeps_the_rejection_instead_of_the_notification_that_woke_it() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "legacy input", Some(WINDOW));
    chain.at(3, 0);
    reject(chain);
    let failure_frame = thread_row(chain, thread).frames;
    {
        let mut host = chain.host(PROGRAM).borrow_mut();
        let key = crate::state::RETRIES.key(&thread);
        let mut row: crate::state::Retry = abi::decode(host.state.get(&key).unwrap()).unwrap();
        row.attempt = 183;
        host.state.insert(key, abi::encode(&row));
        host.state
            .remove(&crate::state::MODEL_FAILURES.key(&thread));
    }
    world.mention("m2", "unrelated board update");
    assert!(assigned(chain, SERVICE).is_empty());
    let listed = errors(&world);
    assert_eq!(listed.len(), 1);
    let failure = &listed[0];
    assert!(failure.text.contains("pool rejected"), "{}", failure.text);
    assert!(
        !failure.text.contains("unrelated board update"),
        "{}",
        failure.text
    );
    assert!(
        failure
            .text
            .contains(&format!("model failure notice frame {failure_frame}")),
        "{}",
        failure.text
    );
    assert!(frames(chain, thread).iter().any(
        |body| matches!(body, Body::Post { text, .. } if text.contains("unrelated board update"))
    ));
}
