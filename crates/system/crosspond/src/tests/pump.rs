use abi::role::egress::DEFAULT_WAIT;

use super::*;

fn send(chain: &MockChain) -> u64 {
    let send = Op::Send {
        to: THERE,
        port: PORT.into(),
        op: abi::encode(&Note::Post("hello".into())),
        deadline: None,
    };
    abi::decode(&submit(chain, USER, &send).unwrap()).unwrap()
}

fn sending() -> MockChain {
    let chain = chain();
    open(&chain, &[1, 2, 3, 4]);
    chain
}

type Acks<'a> = &'a [(u64, Result<Vec<u8>, abi::Refusal>)];

fn proven(chain: &MockChain, epochs: &[Acks]) -> Vec<Answer> {
    let owed = outbound(chain, THERE, 0);
    let settled = |(seq, result): &(u64, Result<Vec<u8>, abi::Refusal>)| {
        let packet = owed.iter().find(|packet| packet.seq == *seq).unwrap();
        settled_of(packet, result.clone())
    };
    let epochs = epochs
        .iter()
        .map(|acks| Epoch {
            acks: acks.iter().map(settled).collect(),
            ..Epoch::seated(&[1, 2, 3, 4], Vec::new())
        })
        .collect();
    there(epochs)
        .into_iter()
        .zip(1u64..)
        .map(|(built, epoch)| acked(epoch, Some(&built.header), built.acks))
        .collect()
}

fn asked(chain: &MockChain) -> Vec<u64> {
    assigned(chain, ADA)
        .into_iter()
        .map(|row| {
            assert_eq!(row.request, egress::Request::Crosspond { link: THERE });
            row.id
        })
        .collect()
}

fn refused(chain: &MockChain) -> Option<String> {
    link_of(chain, THERE).and_then(|link| link.refused)
}

#[test]
fn a_scheduled_pump_parks_on_egress_and_resumes_with_the_acks() {
    let chain = sending();
    serve(&chain, ADA);
    assert_eq!(send(&chain), 1);
    chain.at(3, 0);
    let [id] = asked(&chain)[..] else {
        panic!("the pump asked egress once");
    };
    assert_eq!(chain.frames(), 1);
    assert_eq!(status_of(&chain, THERE, 1), Some(Status::Pending));
    let [acks] = &proven(&chain, &[&[(1, Ok(b"posted".to_vec()))]])[..] else {
        panic!("one epoch was built");
    };
    answer(&chain, ADA, id, acks.clone()).unwrap();
    assert_eq!(chain.frames(), 0);
    let done = Status::Done(Ok(b"posted".to_vec()));
    assert_eq!(status_of(&chain, THERE, 1), Some(done));
    assert!(outbound(&chain, THERE, 0).is_empty());
    assert!(asked(&chain).is_empty());
    send(&chain);
    chain.at(5, 0);
    assert_eq!(asked(&chain).len(), 1);
}

#[test]
fn the_pump_asks_again_while_packets_are_owed() {
    let chain = sending();
    serve(&chain, ADA);
    send(&chain);
    chain.at(3, 0);
    let [first] = asked(&chain)[..] else {
        panic!("the pump asked egress once");
    };
    send(&chain);
    let refusal = abi::Refusal::new(code::UNAUTHORIZED, "no port");
    let [one, two] = &proven(
        &chain,
        &[&[(1, Ok(Vec::new()))], &[(2, Err(refusal.clone()))]],
    )[..] else {
        panic!("two epochs were built");
    };
    answer(&chain, ADA, first, one.clone()).unwrap();
    let [again] = asked(&chain)[..] else {
        panic!("the pump asked again for packet 2");
    };
    assert_ne!(again, first);
    assert_eq!(chain.frames(), 1);
    chain.at(4, 0);
    answer(&chain, ADA, again, two.clone()).unwrap();
    assert_eq!(
        status_of(&chain, THERE, 2),
        Some(Status::Done(Err(refusal)))
    );
    assert!(asked(&chain).is_empty());
    assert_eq!(chain.frames(), 0);
}

#[test]
fn a_whole_relay_failure_is_recorded_as_refused_and_asked_again() {
    let chain = sending();
    serve(&chain, ADA);
    send(&chain);
    chain.at(3, 0);
    let [first] = asked(&chain)[..] else {
        panic!("the pump asked egress once");
    };
    let failed = Answer::Failed("crosspond: Op did not decode".into());
    answer(&chain, ADA, first, failed).unwrap();
    let Some(Status::Refused { reason }) = status_of(&chain, THERE, 1) else {
        panic!("the packet is refused");
    };
    assert!(reason.contains("crosspond: Op did not decode"));
    assert_eq!(refused(&chain), Some(reason));
    assert_eq!(outbound(&chain, THERE, 0).len(), 1);
    let [again] = asked(&chain)[..] else {
        panic!("the pump asked again");
    };
    chain.at(4, 0);
    let [acks] = &proven(&chain, &[&[(1, Ok(Vec::new()))]])[..] else {
        panic!("one epoch was built");
    };
    answer(&chain, ADA, again, acks.clone()).unwrap();
    assert_eq!(
        status_of(&chain, THERE, 1),
        Some(Status::Done(Ok(Vec::new())))
    );
    assert_eq!(refused(&chain), None);
}

#[test]
fn an_expired_request_records_nothing_and_is_asked_again() {
    let chain = sending();
    serve(&chain, ADA);
    send(&chain);
    chain.at(3, 0);
    let [first] = asked(&chain)[..] else {
        panic!("the pump asked egress once");
    };
    chain.at(2 + DEFAULT_WAIT + 1, 0);
    let [again] = asked(&chain)[..] else {
        panic!("the pump asked again after the expiry");
    };
    assert_ne!(again, first);
    assert_eq!(status_of(&chain, THERE, 1), Some(Status::Pending));
    assert_eq!(refused(&chain), None);
    assert_eq!(chain.frames(), 1);
}

#[test]
fn a_request_egress_refuses_at_once_is_recorded_and_the_next_send_asks_again() {
    let chain = sending();
    send(&chain);
    chain.at(3, 0);
    assert_eq!(chain.frames(), 0);
    let Some(Status::Refused { .. }) = status_of(&chain, THERE, 1) else {
        panic!("the packet is refused while no node serves crosspond");
    };
    assert!(refused(&chain).is_some());
    serve(&chain, ADA);
    send(&chain);
    chain.at(5, 0);
    assert_eq!(asked(&chain).len(), 1);
    let owed: Vec<u64> = outbound(&chain, THERE, 0).iter().map(|p| p.seq).collect();
    assert_eq!(owed, [1, 2]);
}

#[test]
fn a_pump_the_kernel_cancelled_is_replaced_once_its_request_could_have_expired() {
    let chain = sending();
    serve(&chain, ADA);
    send(&chain);
    chain.at(3, 0);
    chain.cancel_parked(PROGRAM);
    assert_eq!(chain.frames(), 0);
    send(&chain);
    assert_eq!(pumps_due(&chain, 4), 0);
    let expired = 2 + DEFAULT_WAIT + 1;
    chain.at(expired, 0);
    send(&chain);
    assert_eq!(pumps_due(&chain, expired + 1), 1);
}
