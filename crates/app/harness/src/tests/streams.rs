use super::*;
use crate::wire::tests::{ERROR, TEXT, events};

fn node(chain: &MockChain, op: &egress::Op) -> Result<Vec<u8>, Error> {
    chain.submit(signed(&NODE), egress::PROGRAM, op)
}

fn stream(chain: &MockChain, request: &RequestRow, events: &[Vec<u8>], seqs: &[u64]) {
    for &seq in seqs {
        let event = egress::Event {
            id: request.id,
            seq,
            data: events[seq as usize].clone(),
        };
        node(chain, &egress::Op::Events(vec![event])).unwrap();
    }
}

fn handed(chain: &MockChain, thread: u64, request: &RequestRow, events: &[Vec<u8>], seqs: &[u64]) {
    for &seq in seqs {
        let event = egress::Event {
            id: request.id,
            seq,
            data: events[seq as usize].clone(),
        };
        let streamed = Op::Streamed { thread, event };
        chain
            .submit(Origin::Program(egress::PROGRAM.into()), PROGRAM, &streamed)
            .unwrap();
    }
}

fn end(chain: &MockChain, request: &RequestRow, events: u64) {
    let ended = egress::Op::EndStream {
        id: request.id,
        events,
        end: egress::StreamEnd::Complete {
            status: 200,
            headers: vec![("content-type".into(), "text/event-stream".into())],
        },
    };
    node(chain, &ended).unwrap();
}

fn partial(chain: &MockChain, thread: u64) -> Option<Partial> {
    let Reply::Partial(partial) = chain.query(PROGRAM, &Query::Partial { thread }).unwrap() else {
        panic!()
    };
    partial
}

fn turns(chain: &MockChain, thread: u64) -> Vec<Body> {
    frames(chain, thread)
        .into_iter()
        .filter(|body| matches!(body, Body::Turn { .. }))
        .collect()
}

fn asked(world: &World) -> (u64, RequestRow) {
    let thread = world.opened("m1", "hello", Some(WINDOW));
    world.chain.at(3, 0);
    let (request, _) = open_request(&world.chain);
    (thread, request)
}

#[test]
fn a_turn_asks_for_a_streamed_reply_delivered_as_its_threads_streamed_op() {
    let world = world();
    let (thread, request) = asked(&world);
    assert_eq!(
        request.delivery,
        egress::Delivery::Events {
            deliver: streamed_as(thread)
        }
    );
    assert_eq!(body(&world.chain, &request)["stream"], true);
    assert_eq!(
        partial(&world.chain, thread),
        Some(Partial::new(Wire::AnthropicMessages))
    );
}

#[test]
fn the_reply_grows_event_by_event_and_its_turn_is_the_whole_replys() {
    let streamed = world();
    let chain = &streamed.chain;
    let (thread, request) = asked(&streamed);
    let events = events(TEXT);
    stream(chain, &request, &events, &[0, 1, 2, 3]);
    let growing = partial(chain, thread).unwrap();
    assert_eq!(growing.said, [Said::Text("Hello".into())]);
    assert_eq!(growing.flow, Flow::Open);
    assert!(turns(chain, thread).is_empty());
    stream(chain, &request, &events, &[4, 5, 6, 7]);
    end(chain, &request, 8);
    assert_eq!(partial(chain, thread), None);

    let whole = world();
    let (other, request) = asked(&whole);
    let said = json!({
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4-5",
        "content": [{ "type": "text", "text": "Hello, world" }],
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 10,
            "output_tokens": 5,
            "cache_creation_input_tokens": 0,
            "cache_read_input_tokens": 0
        },
    });
    answer(
        &whole.chain,
        request.id,
        Answer::Output(response(200, said)),
    );
    let [turn] = turns(chain, thread).try_into().unwrap();
    assert_eq!(turns(&whole.chain, other), [turn]);
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);
}

#[test]
fn repeats_are_dropped_and_a_restart_at_zero_starts_the_reply_over() {
    let world = world();
    let chain = &world.chain;
    let (thread, request) = asked(&world);
    let events = events(TEXT);
    stream(chain, &request, &events, &[0, 1, 2, 3, 4]);
    handed(chain, thread, &request, &events, &[4, 3]);
    assert_eq!(
        partial(chain, thread).unwrap().said,
        [Said::Text("Hello, world".into())]
    );
    stream(chain, &request, &events, &[0, 1, 2, 3]);
    assert_eq!(
        partial(chain, thread).unwrap().said,
        [Said::Text("Hello".into())]
    );
    stream(chain, &request, &events, &[4, 5, 6, 7]);
    end(chain, &request, 8);
    let [Body::Turn { said, .. }] = &turns(chain, thread)[..] else {
        panic!("one turn");
    };
    assert_eq!(said, &[Said::Text("Hello, world".into())]);
}

#[test]
fn a_gap_in_the_stream_is_noticed_once_and_the_thread_goes_idle() {
    let world = world();
    let chain = &world.chain;
    let (thread, request) = asked(&world);
    let events = events(TEXT);
    stream(chain, &request, &events, &[0, 1]);
    handed(chain, thread, &request, &events, &[3]);
    stream(chain, &request, &events, &[2, 3, 4, 5, 6, 7]);
    end(chain, &request, 8);
    assert!(turns(chain, thread).is_empty());
    assert_eq!(
        notices(chain, thread),
        ["the model's streamed reply broke: event 3 arrived where event 2 was due"]
    );
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);
    assert_eq!(partial(chain, thread), None);
    assert!(assigned(chain, SERVICE).is_empty());
}

#[test]
fn an_error_event_is_noticed_once_and_the_next_input_asks_again() {
    let world = world();
    let chain = &world.chain;
    let (thread, request) = asked(&world);
    let events = events(ERROR);
    stream(chain, &request, &events, &[0, 1, 2, 3]);
    end(chain, &request, 4);
    assert_eq!(
        notices(chain, thread),
        ["the model's reply failed as it streamed, overloaded_error: Overloaded"]
    );
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);
    world.mention("m2", "again");
    let (_, head) = open_request(chain);
    assert_eq!(path(&head), "/v1/messages");
}

#[test]
fn a_whole_reply_to_a_streamed_request_is_read_as_today() {
    let world = world();
    let chain = &world.chain;
    let (thread, request) = asked(&world);
    let limited =
        json!({ "type": "error", "error": { "type": "rate_limit_error", "message": "Slow down" } });
    answer(chain, request.id, Answer::Output(response(429, limited)));
    assert_eq!(
        notices(chain, thread),
        ["the model answered 429, rate_limit_error: Slow down"]
    );
}

#[test]
fn only_the_egress_role_streams_and_only_into_a_thread_waiting_on_its_model() {
    let world = world();
    let chain = &world.chain;
    let (thread, request) = asked(&world);
    let event = egress::Event {
        id: request.id,
        seq: 0,
        data: events(TEXT)[0].clone(),
    };
    let forged = Op::Streamed {
        thread,
        event: event.clone(),
    };
    let refused = chain.submit(signed(ADA), PROGRAM, &forged).unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
    let from_files = chain
        .submit(Origin::Program(files::PROGRAM.into()), PROGRAM, &forged)
        .unwrap_err();
    assert_eq!(from_files.code, code::UNAUTHORIZED);
    assert_eq!(
        partial(chain, thread),
        Some(Partial::new(Wire::AnthropicMessages))
    );
    let elsewhere = Op::Streamed {
        thread: thread + 1,
        event,
    };
    let idle = chain
        .submit(Origin::Program(egress::PROGRAM.into()), PROGRAM, &elsewhere)
        .unwrap_err();
    assert_eq!(idle.code, code::WRONG_STATE);
}

mod live;
