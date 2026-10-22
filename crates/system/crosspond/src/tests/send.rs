use crate::state::OUTBOX;

use super::*;

fn send(chain: &MockChain, op: Vec<u8>) -> Result<Vec<u8>, Error> {
    let send = Op::Send {
        to: THERE,
        port: PORT.into(),
        op,
        deadline: Some(9_000),
    };
    submit(chain, USER, &send)
}

#[test]
fn send_commits_the_packet_at_its_commitment_key_and_returns_its_seq() {
    let chain = chain();
    open(&chain, &[1, 2, 3, 4]);
    chain.at(3, 2_000);
    let op = abi::encode(&Note::Post("hello".into()));
    let seq: u64 = abi::decode(&send(&chain, op.clone()).unwrap()).unwrap();
    assert_eq!(seq, 1);
    let expected = Packet {
        source: HERE,
        destination: THERE,
        seq: 1,
        from: ALICE,
        name: "alice".into(),
        via: None,
        port: PORT.into(),
        op,
        sent: 2_000,
        deadline: Some(9_000),
    };
    assert_eq!(OUTBOX.key(&(THERE, 1u64)), commitment_key(&THERE, 1));
    let committed = chain
        .host(PROGRAM)
        .query(chain.env(PROGRAM))
        .get(commitment_key(&THERE, 1));
    assert_eq!(committed, Some(abi::encode(&expected)));
    assert_eq!(outbound(&chain, THERE, 0), [expected]);
    assert_eq!(status_of(&chain, THERE, 1), Some(Status::Pending));
    let next: u64 = abi::decode(&send(&chain, Vec::new()).unwrap()).unwrap();
    assert_eq!(next, 2);
    assert_eq!(link_of(&chain, THERE).map(|link| link.next_out), Some(3));
    let after: Vec<u64> = outbound(&chain, THERE, 1).iter().map(|p| p.seq).collect();
    assert_eq!(after, [2]);
}

#[test]
fn sends_schedule_one_pump_while_one_is_pending() {
    let chain = chain();
    open(&chain, &[1, 2, 3, 4]);
    send(&chain, Vec::new()).unwrap();
    send(&chain, Vec::new()).unwrap();
    assert_eq!(pumps_due(&chain, 2), 1);
}

#[test]
fn a_packet_past_the_links_max_packet_is_refused_with_capacity() {
    let chain = chain();
    open(&chain, &[1, 2, 3, 4]);
    let empty = Packet {
        source: HERE,
        destination: THERE,
        seq: 1,
        from: ALICE,
        name: "alice".into(),
        via: None,
        port: PORT.into(),
        op: Vec::new(),
        sent: 0,
        deadline: Some(9_000),
    };
    let bound = max_packet(THERE_NAME);
    assert_eq!(bound, max_packet("") - THERE_NAME.len());
    let room = bound - abi::encode(&empty).len();
    assert_eq!(code_of(send(&chain, vec![7; room + 1])), code::CAPACITY);
    assert!(outbound(&chain, THERE, 0).is_empty());
    send(&chain, vec![7; room]).unwrap();
    assert_eq!(abi::encode(&outbound(&chain, THERE, 0)[0]).len(), bound);
}

#[test]
fn a_send_names_a_linked_network() {
    let chain = chain();
    assert_eq!(code_of(send(&chain, Vec::new())), code::NOT_FOUND);
}
