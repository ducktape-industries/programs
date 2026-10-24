//! A conversation's unanswered asks in a row: each input that ends with no answer counts one, as
//! does each busy retry from its backoff's ceiling on, kept with its notice's frame and the block
//! the run began; the next answered turn or a close clears them. `FinalErrors` reads them by agent.
use super::*;

fn refused_for_good() -> Vec<u8> {
    let error = json!({ "type": "error", "error": { "type": "invalid_request_error", "message": "malformed request" } });
    response(400, error)
}

fn rate_limited() -> Vec<u8> {
    let error =
        json!({ "type": "error", "error": { "type": "rate_limit_error", "message": "Slow down" } });
    response(429, error)
}

fn final_errors(world: &World) -> Vec<FinalError> {
    let asked = Query::FinalErrors {
        agent: world.agent,
        page: PageRequest::default(),
    };
    let Reply::FinalErrors(page) = world.chain.query(PROGRAM, &asked).unwrap() else {
        panic!()
    };
    page.items
}

fn only(world: &World) -> FinalError {
    let errors = final_errors(world);
    let [error] = &errors[..] else {
        panic!("one conversation in final errors: {errors:?}")
    };
    error.clone()
}

fn notice_at(chain: &MockChain, thread: u64, seq: u64) -> String {
    let asked = Query::Frames {
        thread,
        page: PageRequest::default(),
    };
    let Reply::Frames(page) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!()
    };
    let frame = page.items.into_iter().find(|frame| frame.seq == seq);
    match frame.map(|frame| frame.body) {
        Some(Body::Notice { text }) => text,
        other => panic!("frame {seq} is no notice: {other:?}"),
    }
}

fn fails(chain: &MockChain) {
    let (request, _) = open_request(chain);
    answer(chain, request.id, Answer::Output(refused_for_good()));
}

#[test]
fn a_final_error_is_kept_with_its_notice_and_a_second_in_a_row_counts_two() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    assert!(final_errors(&world).is_empty(), "nothing failed yet");
    fails(chain);
    let first = only(&world);
    assert_eq!((first.thread, first.count), (thread, 1), "{first:?}");
    assert_eq!(first.since, first.height, "{first:?}");
    assert!(first.text.contains("malformed request"), "{}", first.text);
    assert_eq!(notice_at(chain, thread, first.frame), first.text);
    chain.at(4, 0);
    world.mention("m2", "still there?");
    chain.at(5, 0);
    fails(chain);
    let second = only(&world);
    assert_eq!(
        (second.thread, second.count, second.since),
        (thread, 2, first.since),
        "{second:?}"
    );
    assert!(second.frame > first.frame, "{second:?}");
    assert!(second.height > first.height, "{second:?}");
    assert_eq!(notice_at(chain, thread, second.frame), second.text);
}

#[test]
fn an_answered_turn_clears_them_and_the_next_final_error_starts_a_new_run() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    fails(chain);
    assert_eq!(only(&world).count, 1);
    chain.at(4, 0);
    world.mention("m2", "again");
    chain.at(5, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Back." }]),
        "end_turn",
    );
    let cleared = final_errors(&world);
    assert!(
        cleared.is_empty(),
        "an answered turn clears them: {cleared:?}"
    );
    chain.at(6, 0);
    world.mention("m3", "once more");
    chain.at(7, 0);
    fails(chain);
    let run = only(&world);
    assert_eq!((run.thread, run.count), (thread, 1), "{run:?}");
    assert_eq!(run.since, run.height, "a new run begins: {run:?}");
}

#[test]
fn a_conversation_in_final_errors_that_is_closed_leaves_the_list() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    fails(chain);
    assert_eq!(only(&world).thread, thread);
    chain.at(4, 0);
    chain
        .submit(signed(ADA), PROGRAM, &Op::Close { thread })
        .unwrap();
    let left = final_errors(&world);
    assert!(left.is_empty(), "a closed conversation is listed: {left:?}");
}

#[test]
fn a_busy_answer_asked_again_is_no_final_error() {
    let world = world();
    let chain = &world.chain;
    world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    let (request, _) = open_request(chain);
    let overloaded = json!({ "type": "error", "error": { "type": "overloaded_error", "message": "Overloaded" } });
    answer(chain, request.id, Answer::Output(response(529, overloaded)));
    let none = final_errors(&world);
    assert!(none.is_empty(), "{none:?}");
}

#[test]
fn a_model_that_429s_forever_stops_and_explicit_retry_preserves_its_context() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    let mut height = 3;
    for attempt in 1..=crate::resume::AUTOMATIC_RETRIES + 1 {
        chain.at(height, 0);
        let (request, _) = open_request(chain);
        answer(chain, request.id, Answer::Output(rate_limited()));
        height += (1u64 << (attempt - 1)) + 1;
    }
    chain.at(height, 0);
    assert!(assigned(chain, SERVICE).is_empty());
    let stopped = only(&world);
    assert!(stopped.text.contains("3 automatic retries"));
    chain
        .submit(signed(ADA), PROGRAM, &Op::Retry { thread })
        .unwrap();
    chain.at(height + 1, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Back." }]),
        "end_turn",
    );
    assert!(final_errors(&world).is_empty());
}

#[test]
fn a_final_error_key_names_its_conversation() {
    let key = crate::state::FINAL_ERRORS.key(&(12u64, 7u64));
    assert_eq!(
        written(&key),
        Some(Written::FinalError {
            agent: 12,
            thread: 7
        })
    );
    let wrote = Written::FinalError {
        agent: 12,
        thread: 7,
    };
    assert_eq!(written(&wrote.key()), Some(wrote));
}
