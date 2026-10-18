use super::*;
use crate::state::{EARLY, STREAMS, Stream};

fn event(seq: u64, data: &[u8]) -> Event {
    Event {
        id: 1,
        seq,
        data: data.to_vec(),
    }
}

fn events(chain: &MockChain, events: &[(u64, &[u8])]) {
    let events = events.iter().map(|(seq, data)| event(*seq, data)).collect();
    submit(chain, &ADA, &Op::Events(events)).unwrap();
}

fn end(chain: &MockChain, events: u64) {
    let end = StreamEnd::Complete {
        status: 200,
        headers: Vec::new(),
    };
    submit(chain, &ADA, &Op::EndStream { id: 1, events, end }).unwrap();
}

fn seqs(chain: &MockChain) -> Vec<u64> {
    heard(chain).iter().map(|event| event.seq).collect()
}

fn kept(chain: &MockChain) -> (Option<Stream>, usize) {
    let ctx = chain.host(PROGRAM).query(chain.env(PROGRAM));
    let stream = STREAMS.get(&ctx, &1).unwrap();
    let early = EARLY.scan(&ctx, EARLY.prefix_of(&1u64)).unwrap();
    (stream, early.len())
}

fn streaming(wait: Option<u64>) -> MockChain {
    let chain = chain();
    serve(&chain, ADA, &[HTTP]).unwrap();
    stream(&chain, http_request(HTTP, MESSAGES, Vec::new()), wait);
    chain.at(2, 0);
    chain
}

#[test]
fn events_that_land_early_wait_for_the_gap_and_reach_the_requester_once_in_order() {
    let chain = streaming(None);
    events(&chain, &[(2, b"c"), (3, b"d")]);
    events(&chain, &[(5, b"f")]);
    assert!(heard(&chain).is_empty());
    assert_eq!(kept(&chain).1, 3);
    events(&chain, &[(0, b"a"), (1, b"b")]);
    assert_eq!(seqs(&chain), [0, 1, 2, 3]);
    events(&chain, &[(3, b"d"), (1, b"b")]);
    events(&chain, &[(4, b"e")]);
    assert_eq!(seqs(&chain), [0, 1, 2, 3, 4, 5]);
    let data: Vec<Vec<u8>> = heard(&chain).into_iter().map(|event| event.data).collect();
    assert_eq!(data, [b"a", b"b", b"c", b"d", b"e", b"f"]);
    assert_eq!(kept(&chain).1, 0);
}

#[test]
fn an_end_that_lands_before_its_events_waits_for_every_one_it_counts() {
    let chain = streaming(None);
    events(&chain, &[(1, b"b")]);
    end(&chain, 2);
    assert!(answers(&chain).is_empty());
    assert!(request_row(&chain, 1).is_some());
    events(&chain, &[(0, b"a")]);
    assert_eq!(seqs(&chain), [0, 1]);
    let [answered] = answers(&chain).try_into().unwrap();
    let answered: http::Response = abi::decode(&answered.unwrap()).unwrap();
    assert_eq!(answered.status, 200);
    assert_eq!(request_row(&chain, 1), None);
    assert_eq!(kept(&chain), (None, 0));
    let late = submit(&chain, &ADA, &Op::Events(vec![event(2, b"c")])).unwrap_err();
    assert_eq!(late.code, code::NOT_FOUND);
}

#[test]
fn an_end_after_every_counted_event_closes_at_once() {
    let chain = streaming(None);
    events(&chain, &[(0, b"a"), (1, b"b")]);
    end(&chain, 2);
    assert_eq!(answers(&chain).len(), 1);
    assert_eq!(kept(&chain), (None, 0));
}

#[test]
fn a_seq_back_at_zero_restarts_the_stream_and_drops_what_waited_from_the_call_before() {
    let chain = streaming(None);
    events(&chain, &[(0, b"old 0"), (1, b"old 1")]);
    events(&chain, &[(3, b"old 3")]);
    events(&chain, &[(0, b"new 0")]);
    assert_eq!(kept(&chain).1, 0);
    events(&chain, &[(1, b"new 1"), (2, b"new 2"), (3, b"new 3")]);
    let data: Vec<Vec<u8>> = heard(&chain).into_iter().map(|event| event.data).collect();
    let expected: [&[u8]; 6] = [b"old 0", b"old 1", b"new 0", b"new 1", b"new 2", b"new 3"];
    assert_eq!(data, expected);
}

#[test]
fn an_expiry_while_events_wait_closes_the_request_and_clears_its_stream() {
    let chain = streaming(Some(3));
    events(&chain, &[(1, b"b"), (2, b"c")]);
    end(&chain, 3);
    assert_eq!(kept(&chain).1, 2);
    chain.at(2 + 3 + 1, 0);
    let [expired] = answers(&chain).try_into().unwrap();
    assert_eq!(expired.unwrap_err().code, code::WRONG_STATE);
    assert_eq!(request_row(&chain, 1), None);
    assert_eq!(kept(&chain), (None, 0));
}

#[test]
fn frames_landing_in_one_block_move_the_expiry_once() {
    let chain = streaming(Some(5));
    events(&chain, &[(0, b"a")]);
    let moved = request_row(&chain, 1).unwrap().expiry;
    events(&chain, &[(1, b"b")]);
    assert_eq!(request_row(&chain, 1).unwrap().expiry, moved);
    assert_eq!(entry(&chain, moved).unwrap().height, 2 + 5);
    chain.at(3, 0);
    events(&chain, &[(2, b"c")]);
    let later = request_row(&chain, 1).unwrap().expiry;
    assert_ne!(later, moved);
    assert_eq!(entry(&chain, later).unwrap().height, 3 + 5);
}
