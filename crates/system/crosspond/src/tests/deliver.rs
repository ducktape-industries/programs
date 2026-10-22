use super::*;

fn linked() -> MockChain {
    let chain = chain();
    open(&chain, &[1, 2, 3, 4]);
    open_port(&chain, PORT, vec![THERE]);
    chain
}

fn epoch_with(packets: Vec<Packet>) -> Built {
    there(vec![Epoch::seated(&[1, 2, 3, 4], packets)])
        .pop()
        .expect("one epoch was built")
}

fn headed(packets: Vec<Packet>) -> (MockChain, Vec<Proven>) {
    let chain = linked();
    let built = epoch_with(packets);
    header(&chain, &built).unwrap();
    (chain, built.proven)
}

fn lowered(packets: Vec<Packet>, n: u64) -> (MockChain, Built) {
    let chain = linked();
    let epoch = Epoch {
        lows: vec![(HERE, n)],
        ..Epoch::seated(&[1, 2, 3, 4], packets)
    };
    let built = there(vec![epoch]).pop().expect("one epoch was built");
    header(&chain, &built).unwrap();
    (chain, built)
}

fn next_in(chain: &MockChain) -> Option<u64> {
    link_of(chain, THERE).map(|link| link.next_in)
}

fn taken(seq: u64, result: Result<Vec<u8>, abi::Refusal>) -> Receipt {
    Receipt {
        seq,
        result: Some(result),
    }
}

fn refusal_code(receipt: &Receipt) -> Option<&str> {
    match &receipt.result {
        Some(Err(refusal)) => Some(refusal.reason.as_str()),
        _ => None,
    }
}

fn foreign_sender() -> Option<Principal> {
    let foreign = Foreign {
        network: THERE,
        account: SENDER,
    };
    Some(Principal::Foreign(foreign.into()))
}

fn named(chain: &MockChain, name: &str) -> Vec<AccountNumber> {
    let query = Query::Named {
        network: THERE,
        name: name.into(),
    };
    ask(chain, &query, |reply| match reply {
        Reply::Named(accounts) => Some(accounts),
        _ => None,
    })
}

fn name_of(chain: &MockChain) -> Option<String> {
    let foreign = Foreign {
        network: THERE,
        account: SENDER,
    };
    ask(chain, &Query::Name(foreign), |reply| match reply {
        Reply::Name(name) => Some(name),
        _ => None,
    })
}

#[test]
fn packets_delivered_in_order_run_as_their_foreign_senders() {
    let (chain, proven) = headed(vec![post(1), post(2)]);
    deliver(&chain, 1, proven).unwrap();
    let expected = vec![
        (foreign_sender(), "post 1".to_owned()),
        (foreign_sender(), "post 2".to_owned()),
    ];
    assert_eq!(posts(&chain), expected);
    let receipts = receipts(&chain, THERE, 0);
    assert_eq!(
        receipts,
        [
            taken(1, Ok(b"post 1".to_vec())),
            taken(2, Ok(b"post 2".to_vec()))
        ]
    );
    assert_eq!(next_in(&chain), Some(3));
    assert_eq!(name_of(&chain), Some("chief".into()));
    assert_eq!(named(&chain, "chief"), [SENDER]);
}

#[test]
fn the_latest_name_heard_is_the_one_kept() {
    let renamed = Packet {
        name: "boss".into(),
        ..post(2)
    };
    let (chain, proven) = headed(vec![post(1), renamed]);
    deliver(&chain, 1, proven).unwrap();
    assert_eq!(name_of(&chain), Some("boss".into()));
    assert!(named(&chain, "chief").is_empty());
    assert_eq!(named(&chain, "boss"), [SENDER]);
}

#[test]
fn a_packet_taken_already_runs_once_and_once_pruned_is_a_duplicate() {
    let (chain, built) = lowered(vec![post(1), post(2)], 3);
    let proven = built.proven;
    deliver(&chain, 1, proven.clone()).unwrap();
    deliver(&chain, 1, proven.clone()).unwrap();
    assert_eq!(posts(&chain).len(), 2);
    assert_eq!(receipts(&chain, THERE, 0).len(), 2);
    deliver_low(&chain, 1, built.lows.first().cloned(), Vec::new()).unwrap();
    let pruned = deliver_low(&chain, 1, None, proven[..1].to_vec());
    assert_eq!(code_of(pruned), code::DUPLICATE);
    assert_eq!(posts(&chain).len(), 2);
}

#[test]
fn a_packet_ahead_of_the_next_waits_for_the_ones_before() {
    let (chain, proven) = headed(vec![post(1), post(2)]);
    let ahead = deliver(&chain, 1, proven[1..].to_vec());
    assert_eq!(code_of(ahead), code::NOT_YET);
    assert_eq!(next_in(&chain), Some(1));
    assert!(posts(&chain).is_empty());
    deliver(&chain, 1, proven[..1].to_vec()).unwrap();
    deliver(&chain, 1, proven[1..].to_vec()).unwrap();
    assert_eq!(next_in(&chain), Some(3));
}

#[test]
fn packets_out_of_sequence_order_refuse_the_frame() {
    let (chain, proven) = headed(vec![post(1), post(2)]);
    let reversed = vec![proven[1].clone(), proven[0].clone()];
    assert_eq!(code_of(deliver(&chain, 1, reversed)), code::INVALID_INPUT);
    assert_eq!(next_in(&chain), Some(1));
}

#[test]
fn a_packet_past_its_deadline_is_stale_and_the_link_moves_on() {
    let late = Packet {
        deadline: Some(5),
        ..post(1)
    };
    let (chain, proven) = headed(vec![late, post(2)]);
    chain.at(2, 6);
    deliver(&chain, 1, proven).unwrap();
    let receipts = receipts(&chain, THERE, 0);
    assert_eq!(refusal_code(&receipts[0]), Some(code::STALE));
    assert_eq!(posts(&chain), [(foreign_sender(), "post 2".to_owned())]);
    assert_eq!(next_in(&chain), Some(3));
}

#[test]
fn a_port_that_does_not_take_the_network_refuses_its_packets() {
    let chain = chain();
    open(&chain, &[1, 2, 3, 4]);
    open_port(&chain, PORT, vec![ELSEWHERE]);
    let built = epoch_with(vec![post(1)]);
    header(&chain, &built).unwrap();
    deliver(&chain, 1, built.proven).unwrap();
    let receipts = receipts(&chain, THERE, 0);
    assert_eq!(refusal_code(&receipts[0]), Some(code::UNAUTHORIZED));
    assert!(posts(&chain).is_empty());
    assert_eq!(next_in(&chain), Some(2));
}

#[test]
fn a_port_whose_role_no_program_fills_is_unsupported() {
    let forge = Packet {
        port: "forge".into(),
        ..post(1)
    };
    let (chain, proven) = headed(vec![forge]);
    open_port(&chain, "forge", vec![THERE]);
    deliver(&chain, 1, proven).unwrap();
    let receipts = receipts(&chain, THERE, 0);
    assert_eq!(refusal_code(&receipts[0]), Some(code::UNSUPPORTED));
}

#[test]
fn a_bad_proof_refuses_the_whole_frame() {
    let (chain, mut proven) = headed(vec![post(1), post(2)]);
    proven[1].packet.op = abi::encode(&Note::Post("forged".into()));
    assert_eq!(
        code_of(deliver(&chain, 1, proven.clone())),
        code::INVALID_INPUT
    );
    proven[1].packet = post(2);
    proven[1].proof.location += 1;
    assert_eq!(code_of(deliver(&chain, 1, proven)), code::INVALID_INPUT);
    assert_eq!(next_in(&chain), Some(1));
    assert!(receipts(&chain, THERE, 0).is_empty());
    assert!(posts(&chain).is_empty());
}

#[test]
fn a_packet_for_another_network_is_refused() {
    let astray = Packet {
        destination: ELSEWHERE,
        ..post(1)
    };
    let (chain, proven) = headed(vec![astray]);
    assert_eq!(code_of(deliver(&chain, 1, proven)), code::INVALID_INPUT);
    assert_eq!(next_in(&chain), Some(1));
}

#[test]
fn an_op_the_target_cannot_decode_or_refuses_is_that_packets_receipt() {
    let garbled = Packet {
        op: vec![0xff],
        ..post(1)
    };
    let refused = packet(2, &Note::Refuse);
    let (chain, proven) = headed(vec![garbled, refused, post(3)]);
    deliver(&chain, 1, proven).unwrap();
    let receipts = receipts(&chain, THERE, 0);
    assert_eq!(refusal_code(&receipts[0]), Some(code::INVALID_INPUT));
    assert_eq!(refusal_code(&receipts[1]), Some(code::INVALID_INPUT));
    assert_eq!(receipts[2], taken(3, Ok(b"post 3".to_vec())));
    assert_eq!(posts(&chain).len(), 1);
    assert_eq!(next_in(&chain), Some(4));
}

#[test]
fn receipts_below_a_proven_low_water_mark_are_pruned() {
    let (chain, built) = lowered(vec![post(1), post(2), post(3)], 3);
    deliver(&chain, 1, built.proven).unwrap();
    assert_eq!(receipts(&chain, THERE, 0).len(), 3);
    deliver_low(&chain, 1, built.lows.first().cloned(), Vec::new()).unwrap();
    let kept: Vec<u64> = receipts(&chain, THERE, 0).iter().map(|r| r.seq).collect();
    assert_eq!(kept, [3]);
    let after: Vec<u64> = receipts(&chain, THERE, 2).iter().map(|r| r.seq).collect();
    assert_eq!(after, [3]);
}

#[test]
fn a_delivery_waits_for_a_header_and_proves_at_the_headers_epoch() {
    let chain = linked();
    let built = epoch_with(vec![post(1)]);
    let early = deliver(&chain, 1, built.proven.clone());
    assert_eq!(code_of(early), code::NOT_YET);
    header(&chain, &built).unwrap();
    let other = deliver(&chain, 2, built.proven.clone());
    assert_eq!(code_of(other), code::STALE);
    deliver(&chain, 1, built.proven).unwrap();
}

#[test]
fn a_delivery_for_no_link_is_not_found() {
    let chain = chain();
    let built = epoch_with(vec![post(1)]);
    assert_eq!(code_of(deliver(&chain, 1, built.proven)), code::NOT_FOUND);
}
