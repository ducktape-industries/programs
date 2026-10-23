use super::*;

fn compacting(world: &World) -> u64 {
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
    assert!(matches!(
        thread_row(chain, thread).asking,
        Asking::Compacting { .. }
    ));
    thread
}

fn errors(world: &World) -> Vec<FinalError> {
    let Reply::FinalErrors(page) = world
        .chain
        .query(
            PROGRAM,
            &Query::FinalErrors {
                agent: world.agent,
                page: PageRequest::default(),
            },
        )
        .unwrap()
    else {
        panic!()
    };
    page.items
}

fn retryable_summary(answered: Answer) {
    let world = world();
    let chain = &world.chain;
    let thread = compacting(&world);
    let before = thread_row(chain, thread);
    let (request, _) = open_request(chain);
    let failed_at = request.made + 1;
    chain.at(failed_at, 0);
    answer(chain, request.id, answered);
    let waiting = thread_row(chain, thread);
    assert_eq!(
        waiting.seen, before.seen,
        "retry preserves unanswered input"
    );
    assert_eq!(waiting.base, before.base);
    assert_eq!(waiting.summary, before.summary);
    assert_eq!(waiting.compact_before, before.compact_before);
    assert!(errors(&world).is_empty(), "transient failure is not final");
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "retry is not immediate"
    );
    chain.at(failed_at + 2, 0);
    let (retry, head) = open_request(chain);
    assert_eq!(path(&head), model_wire::SUMMARY);
    assert_ne!(retry.id, request.id);
    assert_eq!(body(chain, &retry), body(chain, &request));
    assert!(matches!(
        thread_row(chain, thread).asking,
        Asking::Compacting { .. }
    ));
    chain.at(retry.made + 1, 0);
    summarized(chain, "summary after retry");
    assert!(thread_row(chain, thread).summary.is_some());
    assert!(errors(&world).is_empty());
}

#[test]
fn a_rate_limited_summary_retries_the_same_prefix() {
    retryable_summary(Answer::Output(response(
        429,
        json!({
            "type": "error", "error": {"type": "rate_limit_error", "message": "busy"}
        }),
    )));
}

#[test]
fn an_overloaded_summary_retries_the_same_prefix() {
    retryable_summary(Answer::Output(response(
        503,
        json!({
            "type": "error", "error": {"type": "overloaded_error", "message": "busy"}
        }),
    )));
}

#[test]
fn a_summary_the_service_failed_retries_the_same_prefix() {
    retryable_summary(Answer::Failed("temporary service outage".into()));
}

#[test]
fn an_expired_summary_reports_once_instead_of_backing_off_and_retrying() {
    let world = world();
    let chain = &world.chain;
    let thread = compacting(&world);
    let before = thread_row(chain, thread);
    let (request, _) = open_request(chain);
    world.mention("held", "a held mention must not restart a failed summary");
    assert_eq!(thread_row(chain, thread).held, 1);
    chain.at(request.made + egress::DEFAULT_WAIT + 1, 0);
    let failed = thread_row(chain, thread);
    assert_eq!(failed.asking, Asking::Idle);
    assert_eq!(failed.base, before.base);
    assert_eq!(failed.summary, before.summary);
    assert_eq!(failed.compact_before, before.compact_before);
    assert_eq!(failed.held, 0);
    assert_eq!(failed.seen, failed.last_input);
    assert!(assigned(chain, SERVICE).is_empty());
    let failures = errors(&world);
    assert_eq!(failures.len(), 1, "the failed compaction is visible");
    assert_eq!((failures[0].thread, failures[0].count), (thread, 1));
    chain.at(request.made + 2 * egress::DEFAULT_WAIT + 1, 0);
    assert_eq!(errors(&world), failures);
    assert!(assigned(chain, SERVICE).is_empty());
}

#[test]
fn a_cut_summary_stream_reports_once_without_committing_its_terminal() {
    let world = world();
    let chain = &world.chain;
    let thread = compacting(&world);
    let before = thread_row(chain, thread);
    let (request, _) = open_request(chain);
    chain.at(request.made + 1, 0);
    event(chain, &request, 0, summary("must not commit after cut"));
    world.mention("held", "held input must not restart the cut summary");
    chain
        .submit(
            signed(&NODE),
            egress::PROGRAM,
            &egress::Op::EndStream {
                id: request.id,
                events: 1,
                end: egress::StreamEnd::Cut {
                    reason: "upstream disconnected".into(),
                },
            },
        )
        .unwrap();
    let stopped = thread_row(chain, thread);
    assert_eq!(stopped.summary, before.summary);
    assert_eq!(stopped.base, before.base);
    assert_eq!(stopped.compact_before, before.compact_before);
    assert_eq!(stopped.asking, Asking::Idle);
    assert_eq!(stopped.held, 0);
    assert_eq!(stopped.seen, stopped.last_input);
    assert_eq!(errors(&world).len(), 1);
    chain.at(request.made + 3, 0);
    assert!(assigned(chain, SERVICE).is_empty());
    assert_eq!(errors(&world).len(), 1);
}

#[test]
fn a_summary_asks_for_event_delivery_so_progress_refreshes_its_wait() {
    let world = world();
    compacting(&world);
    let (request, _) = open_request(&world.chain);
    assert!(matches!(request.delivery, egress::Delivery::Events { .. }));
}

fn event(chain: &MockChain, request: &RequestRow, seq: u64, data: Vec<u8>) {
    let event = egress::Event {
        id: request.id,
        seq,
        data,
    };
    chain
        .submit(
            signed(&NODE),
            egress::PROGRAM,
            &egress::Op::Events(vec![event]),
        )
        .unwrap();
}

fn finish(chain: &MockChain, request: &RequestRow, events: u64) {
    chain
        .submit(
            signed(&NODE),
            egress::PROGRAM,
            &egress::Op::EndStream {
                id: request.id,
                events,
                end: egress::StreamEnd::Complete {
                    status: 200,
                    headers: vec![("content-type".into(), "text/event-stream".into())],
                },
            },
        )
        .unwrap();
}

fn summary(text: &str) -> Vec<u8> {
    model_wire::summary_sse(&model_wire::SummaryEvent::Summary(model_wire::Summarized {
        text: text.into(),
        sealed: None,
        lane: model_wire::Lane::of(model_wire::anthropic::PROVIDER, MODEL),
        tokens: 5,
    }))
    .unwrap()
    .into_bytes()
}

#[test]
fn upstream_summary_progress_outlives_the_original_wait_and_only_the_completed_stream_commits() {
    let world = world();
    let chain = &world.chain;
    let thread = compacting(&world);
    let before = thread_row(chain, thread);
    let (request, _) = open_request(chain);
    let mut height = request.made;
    let events = [
        b": upstream thinking\n\n".to_vec(),
        b": upstream delta\n\n".to_vec(),
        summary("the plan"),
    ];
    for (seq, data) in events.into_iter().enumerate() {
        height += egress::DEFAULT_WAIT - 1;
        chain.at(height, 0);
        event(chain, &request, seq as u64, data);
        assert_eq!(thread_row(chain, thread).summary, before.summary);
        assert_eq!(thread_row(chain, thread).base, before.base);
    }
    assert!(height - request.made > 2 * egress::DEFAULT_WAIT);
    world.mention("held", "carry on after the summary");
    finish(chain, &request, 3);
    let after = thread_row(chain, thread);
    assert!(after.summary.is_some());
    assert!(after.base > before.base);
    assert_eq!(after.held, 0);
    assert!(errors(&world).is_empty());
    assert_eq!(path(&open_request(chain).1), model_wire::TURN);
    chain.at(height + 1, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "finished" }]),
        "end_turn",
    );
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);
}

#[test]
fn a_gap_in_a_summary_stream_is_a_final_error_without_a_partial_summary() {
    let world = world();
    let chain = &world.chain;
    let thread = compacting(&world);
    let before = thread_row(chain, thread);
    let (request, _) = open_request(chain);
    chain.at(request.made + 1, 0);
    event(chain, &request, 0, b": progress\n\n".to_vec());
    let gapped = Op::Streamed {
        thread,
        event: egress::Event {
            id: request.id,
            seq: 2,
            data: summary("must not commit"),
        },
    };
    chain
        .submit(Origin::Program(egress::PROGRAM.into()), PROGRAM, &gapped)
        .unwrap();
    event(chain, &request, 1, b": missing event\n\n".to_vec());
    event(chain, &request, 2, summary("must not commit"));
    finish(chain, &request, 3);
    let after = thread_row(chain, thread);
    assert_eq!((after.summary, after.base), (before.summary, before.base));
    assert_eq!(after.asking, Asking::Idle);
    assert_eq!(errors(&world).len(), 1);
    assert!(assigned(chain, SERVICE).is_empty());
}

#[test]
fn a_summary_restart_discards_the_old_terminal_and_repeats_do_not_replace_the_new_one() {
    let world = world();
    let chain = &world.chain;
    let thread = compacting(&world);
    let (request, _) = open_request(chain);
    chain.at(request.made + 1, 0);
    event(chain, &request, 0, summary("old"));
    event(chain, &request, 0, b": restart\n\n".to_vec());
    event(chain, &request, 1, summary("new"));
    event(chain, &request, 1, summary("duplicate"));
    finish(chain, &request, 2);
    let summaries: Vec<_> = frames(chain, thread)
        .into_iter()
        .filter_map(|body| match body {
            Body::Compaction { block, .. } => Some(block),
            _ => None,
        })
        .collect();
    assert_eq!(summaries.len(), 1);
    assert!(summaries[0].contains("new"));
    assert!(!summaries[0].contains("old"));
    assert!(errors(&world).is_empty());
}

#[test]
fn a_completed_stream_without_a_summary_reports_instead_of_retrying() {
    let world = world();
    let chain = &world.chain;
    let thread = compacting(&world);
    let (request, _) = open_request(chain);
    chain.at(request.made + 1, 0);
    event(chain, &request, 0, b": progress\n\n".to_vec());
    finish(chain, &request, 1);
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);
    assert_eq!(errors(&world).len(), 1);
    assert!(assigned(chain, SERVICE).is_empty());
}

#[test]
fn real_summary_overflow_still_backs_off_and_exhaustion_reports_once() {
    let world = world();
    let chain = &world.chain;
    let thread = compacting(&world);
    let (request, _) = open_request(chain);
    let through = match thread_row(chain, thread).asking {
        Asking::Compacting { through } => through,
        _ => panic!(),
    };
    chain.at(request.made + 1, 0);
    let overflow = model_wire::SummaryEvent::Failed(model_wire::Failure::new(
        model_wire::Class::Overflow,
        "too long",
    ));
    event(
        chain,
        &request,
        0,
        model_wire::summary_sse(&overflow).unwrap().into_bytes(),
    );
    finish(chain, &request, 1);
    assert_eq!(thread_row(chain, thread).compact_before, Some(through));
    assert!(errors(&world).is_empty());
    let (smaller, _) = open_request(chain);
    chain.at(smaller.made + 1, 0);
    refused_as_too_long(chain);
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);
    assert_eq!(errors(&world).len(), 1);
    assert!(assigned(chain, SERVICE).is_empty());
}

#[test]
fn a_failed_summary_stream_is_visible_and_a_later_input_can_try_again() {
    let world = world();
    let chain = &world.chain;
    let thread = compacting(&world);
    let (request, _) = open_request(chain);
    chain.at(request.made + 1, 0);
    let failure = model_wire::SummaryEvent::Failed(model_wire::Failure::new(
        model_wire::Class::Dropped,
        "upstream cut",
    ));
    event(
        chain,
        &request,
        0,
        model_wire::summary_sse(&failure).unwrap().into_bytes(),
    );
    finish(chain, &request, 1);
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);
    assert_eq!(errors(&world).len(), 1);
    assert!(assigned(chain, SERVICE).is_empty());
    world.mention("retry", "try again now");
    let (next, _) = open_request(chain);
    chain.at(next.made + 1, 0);
    summarized(chain, "recovered");
    let (next, _) = open_request(chain);
    chain.at(next.made + 1, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "answered" }]),
        "end_turn",
    );
    assert!(errors(&world).is_empty());
}

#[test]
fn data_after_a_summary_terminal_is_a_final_error_not_a_committed_summary() {
    let world = world();
    let chain = &world.chain;
    let thread = compacting(&world);
    let (request, _) = open_request(chain);
    chain.at(request.made + 1, 0);
    event(chain, &request, 0, summary("not committed"));
    event(chain, &request, 1, summary("extra terminal"));
    finish(chain, &request, 2);
    assert_eq!(thread_row(chain, thread).summary, None);
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);
    assert_eq!(errors(&world).len(), 1);
}
