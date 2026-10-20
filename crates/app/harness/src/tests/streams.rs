use super::*;
use crate::wire::tests::{CUT_OFF, ERROR, FORGE_COMMIT, TEXT, events, pieces};

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
        [
            "the model's streamed reply broke: event 3 arrived where event 2 was due; harness asks the model again at block 4 (attempt 1)"
        ]
    );
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);
    assert_eq!(partial(chain, thread), None);
    assert!(assigned(chain, SERVICE).is_empty());
}

#[test]
fn a_reply_slower_than_the_wait_lands_while_each_event_comes_within_it() {
    let world = world();
    let chain = &world.chain;
    let (thread, request) = asked(&world);
    let text = events(TEXT);
    let alive = b": ping\n".to_vec();
    let mut flowing = vec![text[0].clone(), alive.clone(), alive];
    flowing.extend(text[1..].iter().cloned());
    let mut height = request.made;
    for seq in 0..flowing.len() as u64 {
        height += egress::DEFAULT_WAIT - 1;
        chain.at(height, 0);
        stream(chain, &request, &flowing, &[seq]);
    }
    end(chain, &request, flowing.len() as u64);
    assert!(height - request.made > 2 * egress::DEFAULT_WAIT);
    assert_eq!(notices(chain, thread), Vec::<String>::new());
    let [Body::Turn { said, .. }] = &turns(chain, thread)[..] else {
        panic!("one turn");
    };
    assert_eq!(said, &[Said::Text("Hello, world".into())]);
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
        [
            "the model's reply failed as it streamed, overloaded_error: Overloaded; harness asks the model again at block 4 (attempt 1)"
        ]
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
        [
            "the model answered 429, rate_limit_error: Slow down; harness asks the model again at block 4 (attempt 1)"
        ]
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

fn kept_rows(chain: &MockChain, thread: u64) -> (Option<Partial>, Vec<String>) {
    let host = chain.host(PROGRAM).borrow();
    let head = host
        .state
        .get(&crate::state::PARTIALS.key(&thread))
        .map(|bytes| borsh::from_slice(bytes).unwrap());
    let under = crate::state::PIECES.key(&thread);
    let pieces = host
        .state
        .range(under.clone()..)
        .take_while(|(key, _)| key.starts_with(&under))
        .map(|(_, bytes)| borsh::from_slice(bytes).unwrap())
        .collect();
    (head, pieces)
}

#[test]
fn a_streaming_reply_keeps_each_events_text_once_beside_a_head_without_it() {
    let world = world();
    let chain = &world.chain;
    let (thread, request) = asked(&world);
    let events = events(TEXT);
    stream(chain, &request, &events, &[0, 1, 2, 3, 4]);
    let (head, pieces) = kept_rows(chain, thread);
    assert_eq!(head.unwrap().said, [Said::Text(String::new())]);
    assert_eq!(pieces, ["Hello", ", world"]);
    let growing = partial(chain, thread).unwrap();
    assert_eq!(growing.said, [Said::Text("Hello, world".into())]);
    stream(chain, &request, &events, &[5, 6, 7]);
    end(chain, &request, 8);
    assert_eq!(kept_rows(chain, thread), (None, Vec::new()));
}

#[test]
fn a_head_that_kept_its_text_whole_reads_whole_and_keeps_growing() {
    let world = world();
    let chain = &world.chain;
    let (thread, request) = asked(&world);
    let events = events(TEXT);
    stream(chain, &request, &events, &[0, 1, 2, 3]);
    let (head, _) = kept_rows(chain, thread);
    let whole = Partial {
        said: vec![Said::Text("Hello".into())],
        ..head.unwrap()
    };
    {
        let mut host = chain.host(PROGRAM).borrow_mut();
        let under = crate::state::PIECES.key(&thread);
        host.state.retain(|key, _| !key.starts_with(&under));
        let key = crate::state::PARTIALS.key(&thread);
        host.state.insert(key, borsh::to_vec(&whole).unwrap());
    }
    assert_eq!(partial(chain, thread).unwrap().said, whole.said);
    stream(chain, &request, &events, &[4]);
    let (head, pieces) = kept_rows(chain, thread);
    assert_eq!(head.unwrap().said, [Said::Text(String::new())]);
    assert_eq!(pieces, ["Hello, world"]);
}

#[test]
fn a_reply_that_ran_server_tools_measures_its_context_from_its_last_pass() {
    let world = world();
    let chain = &world.chain;
    let (thread, request) = asked(&world);
    let Request::Http { head, .. } = request.request.clone() else {
        panic!("the model is asked over http");
    };
    let delta = json!({
        "type": "message_delta",
        "delta": { "stop_reason": "end_turn", "stop_sequence": null },
        "usage": {
            "input_tokens": 21006,
            "cache_creation_input_tokens": 0,
            "cache_read_input_tokens": 0,
            "output_tokens": 316,
            "iterations": [
                {
                    "type": "message",
                    "input_tokens": 2824,
                    "output_tokens": 104,
                    "cache_read_input_tokens": 0,
                    "cache_creation_input_tokens": 0
                },
                {
                    "type": "message",
                    "input_tokens": 18182,
                    "output_tokens": 212,
                    "cache_read_input_tokens": 0,
                    "cache_creation_input_tokens": 0
                }
            ]
        }
    });
    let last = TEXT.lines().nth(6).unwrap();
    let fixture = TEXT.replace(last, &delta.to_string());
    let events = events(&fixture);
    stream(chain, &request, &events, &[0, 1, 2, 3, 4, 5, 6, 7]);
    end(chain, &request, 8);
    assert_eq!(
        thread_row(chain, thread).context,
        Context::Measured(18182 + 212),
        "the last pass's input and output, not the sum over both passes"
    );
    assert_eq!(
        beta(&head),
        Some(wire::anthropic::COMPACTION_BETA),
        "every turn asks for the usage of each pass"
    );
}
