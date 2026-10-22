use super::*;

fn send(chain: &MockChain, text: &str) -> u64 {
    let send = Op::Send {
        to: THERE,
        port: PORT.into(),
        op: abi::encode(&Note::Post(text.into())),
        deadline: None,
    };
    abi::decode(&submit(chain, USER, &send).unwrap()).unwrap()
}

fn asked(chain: &MockChain) {
    let rows = assigned(chain, ADA);
    assert_eq!(
        rows.len(),
        1,
        "the pump asks egress once at a time: {rows:?}"
    );
}

fn owing(texts: &[&str]) -> (MockChain, Vec<Packet>) {
    let chain = chain();
    open(&chain, &[1, 2, 3, 4]);
    serve(&chain, ADA);
    for text in texts {
        send(&chain, text);
    }
    chain.at(3, 0);
    let owed = outbound(&chain, THERE, 0);
    (chain, owed)
}

fn acking(acks: Vec<Settled>) -> Epoch {
    Epoch {
        acks,
        ..Epoch::seated(&[1, 2, 3, 4], Vec::new())
    }
}

fn one(epoch: Epoch) -> Built {
    there(vec![epoch]).pop().expect("one epoch was built")
}

fn posted(packet: &Packet) -> Settled {
    settled_of(packet, Ok(b"posted".to_vec()))
}

fn owed_still(chain: &MockChain, seqs: &[u64]) {
    let owed: Vec<u64> = outbound(chain, THERE, 0).iter().map(|p| p.seq).collect();
    assert_eq!(owed, seqs);
    for seq in seqs {
        assert_eq!(status_of(chain, THERE, *seq), Some(Status::Pending));
    }
}

fn refused(chain: &MockChain) -> String {
    link_of(chain, THERE)
        .and_then(|link| link.refused)
        .expect("the link says why it took no ack")
}

fn low_rows(chain: &MockChain) -> Vec<u64> {
    (1..=8)
        .filter(|n| raw(chain, &low_key(&THERE, *n)).is_some())
        .collect()
}

#[test]
fn a_proven_ack_settles_its_packet_exactly_once() {
    let (chain, owed) = owing(&["one", "two"]);
    let built = one(acking(vec![posted(&owed[0])]));
    answer_next(&chain, acked(1, Some(&built.header), built.acks.clone())).unwrap();
    let done = Status::Done(Ok(b"posted".to_vec()));
    assert_eq!(status_of(&chain, THERE, 1), Some(done.clone()));
    owed_still(&chain, &[2]);
    assert_eq!(link_of(&chain, THERE).and_then(|link| link.header), Some(1));
    answer_next(&chain, acked(1, None, built.acks)).unwrap();
    assert_eq!(status_of(&chain, THERE, 1), Some(done));
    owed_still(&chain, &[2]);
    assert_eq!(link_of(&chain, THERE).and_then(|link| link.refused), None);
}

#[test]
fn an_ack_for_a_packet_never_delivered_is_refused_and_the_packet_stays_owed() {
    let (chain, owed) = owing(&["one"]);
    let built = one(acking(Vec::new()));
    let forged = ProvenAck {
        settled: posted(&owed[0]),
        proof: built.header.validators_proof.clone(),
    };
    answer_next(&chain, acked(1, Some(&built.header), vec![forged])).unwrap();
    owed_still(&chain, &[1]);
    assert!(refused(&chain).contains("does not hold"));
    asked(&chain);
}

#[test]
fn an_ack_whose_result_was_flipped_is_refused_and_the_genuine_one_lands() {
    let (chain, owed) = owing(&["one"]);
    let built = one(acking(vec![posted(&owed[0])]));
    let mut flipped = built.acks.clone();
    flipped[0].settled.result = Err(abi::Refusal::new(code::UNAUTHORIZED, "no port"));
    answer_next(&chain, acked(1, Some(&built.header), flipped)).unwrap();
    owed_still(&chain, &[1]);
    answer_next(&chain, acked(1, Some(&built.header), built.acks)).unwrap();
    let done = Status::Done(Ok(b"posted".to_vec()));
    assert_eq!(status_of(&chain, THERE, 1), Some(done));
    assert!(outbound(&chain, THERE, 0).is_empty());
}

#[test]
fn an_ack_proven_under_another_root_is_refused() {
    let (chain, owed) = owing(&["one"]);
    let headed = one(acking(Vec::new()));
    let elsewhere = one(Epoch {
        packets: vec![post(9)],
        ..acking(vec![posted(&owed[0])])
    });
    assert_ne!(headed.header.roots, elsewhere.header.roots);
    answer_next(&chain, acked(1, Some(&headed.header), elsewhere.acks)).unwrap();
    owed_still(&chain, &[1]);
    assert!(refused(&chain).contains("does not hold"));
}

#[test]
fn an_ack_at_a_header_the_link_does_not_trust_is_refused() {
    let (chain, owed) = owing(&["one"]);
    let untrusted = one(Epoch {
        acks: vec![posted(&owed[0])],
        ..Epoch::seated(&[5, 6, 7, 8], Vec::new())
    });
    answer_next(
        &chain,
        acked(1, Some(&untrusted.header), untrusted.acks.clone()),
    )
    .unwrap();
    owed_still(&chain, &[1]);
    assert_eq!(link_of(&chain, THERE).and_then(|link| link.header), None);
    let mut forged = one(acking(vec![posted(&owed[0])]));
    let last = forged.header.certificate.len() - 1;
    forged.header.certificate[last] ^= 1;
    answer_next(&chain, acked(1, Some(&forged.header), forged.acks)).unwrap();
    owed_still(&chain, &[1]);
    answer_next(&chain, acked(1, None, untrusted.acks)).unwrap();
    owed_still(&chain, &[1]);
    assert!(refused(&chain).contains("holds no header"));
}

#[test]
fn a_genuine_settled_row_of_another_packet_or_link_is_refused_as_this_packets_ack() {
    let (chain, owed) = owing(&["one"]);
    let packet = &owed[0];
    let another_packet = Packet {
        op: abi::encode(&Note::Post("another".into())),
        ..packet.clone()
    };
    let another_link = Settled {
        destination: ELSEWHERE,
        ..posted(packet)
    };
    let another_source = Settled {
        source: ELSEWHERE,
        ..posted(packet)
    };
    let cases = [
        (posted(&another_packet), "another packet"),
        (another_link, "sent to another network"),
        (another_source, "another network sent"),
        (posted(packet), ""),
    ];
    let built = there(
        cases
            .iter()
            .map(|(settled, _)| acking(vec![settled.clone()]))
            .collect(),
    );
    for ((_, why), (built, epoch)) in cases.iter().zip(built.iter().zip(1u64..)) {
        let [genuine] = &built.acks[..] else {
            panic!("each epoch proves one row");
        };
        let proof = &genuine.proof;
        let key = settled_key(&genuine.settled.source, genuine.settled.seq);
        let root = built.header.roots[1].1.0;
        let value = abi::encode(&genuine.settled);
        assert!(state_proof::holds(
            &root,
            proof.location,
            &proof.proof,
            &key,
            &value
        ));
        let one_ack = acked(epoch, Some(&built.header), built.acks.clone());
        answer_next(&chain, one_ack).unwrap();
        if why.is_empty() {
            continue;
        }
        owed_still(&chain, &[1]);
        assert!(refused(&chain).contains(why), "{why}: {}", refused(&chain));
    }
    assert!(outbound(&chain, THERE, 0).is_empty());
}

#[test]
fn an_answer_that_is_not_acks_leaves_every_status_as_it_was() {
    let (chain, _) = owing(&["one"]);
    answer_next(&chain, Answer::Output(vec![1, 2, 3])).unwrap();
    owed_still(&chain, &[1]);
    assert!(refused(&chain).contains("no acks"));
    asked(&chain);
}

#[test]
fn the_low_water_row_rises_once_each_time_the_lowest_owed_packet_does() {
    let (chain, owed) = owing(&["one", "two", "three"]);
    let built = there(vec![
        acking(vec![posted(&owed[1])]),
        acking(vec![posted(&owed[0])]),
        acking(vec![posted(&owed[2])]),
    ]);
    let answers = built
        .iter()
        .zip(1u64..)
        .map(|(built, epoch)| acked(epoch, Some(&built.header), built.acks.clone()));
    let mut rows = Vec::new();
    for answer_of_epoch in answers {
        answer_next(&chain, answer_of_epoch).unwrap();
        rows.push(low_rows(&chain));
    }
    assert_eq!(rows, [vec![], vec![3], vec![4]]);
    assert!(outbound(&chain, THERE, 0).is_empty());
}
