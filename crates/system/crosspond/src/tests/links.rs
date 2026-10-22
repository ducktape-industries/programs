use super::*;

#[test]
fn open_pins_the_trusted_set_and_refuses_a_known_id() {
    let chain = chain();
    open(&chain, &[1, 2, 3, 4]);
    let link = link_of(&chain, THERE).expect("the link is open");
    let expected = Link {
        name: THERE_NAME.into(),
        epoch_length: EPOCH_LENGTH,
        trusted: 0,
        header: None,
        next_in: 1,
        next_out: 1,
        refused: None,
        closed: false,
    };
    assert_eq!(link, expected);
    let again = Op::Open {
        network: THERE,
        name: "a copy".into(),
        epoch_length: EPOCH_LENGTH,
        epoch: 3,
        kind: CertificateKind::Ed25519SimplexFinalize,
        validators: seat_keys(&[9]),
    };
    assert_eq!(code_of(vote(&chain, &again)), code::ALREADY_EXISTS);
    assert_eq!(link_of(&chain, THERE), Some(expected));
}

#[test]
fn open_refuses_a_set_that_cannot_be_followed() {
    let chain = chain();
    let open = |epoch_length: u64, validators: Vec<Vec<u8>>| Op::Open {
        network: THERE,
        name: THERE_NAME.into(),
        epoch_length,
        epoch: 0,
        kind: CertificateKind::Ed25519SimplexFinalize,
        validators,
    };
    let doubled = [seat_keys(&[1]), seat_keys(&[1])].concat();
    for refused in [
        open(EPOCH_LENGTH, Vec::new()),
        open(EPOCH_LENGTH, doubled),
        open(0, seat_keys(&[1])),
    ] {
        assert_eq!(code_of(vote(&chain, &refused)), code::INVALID_INPUT);
    }
    assert_eq!(link_of(&chain, THERE), None);
}

#[test]
fn repin_and_close_name_a_linked_network() {
    let chain = chain();
    let repin = Op::Repin {
        network: THERE,
        epoch: 2,
        kind: CertificateKind::Ed25519SimplexFinalize,
        validators: seat_keys(&[5]),
    };
    assert_eq!(code_of(vote(&chain, &repin)), code::NOT_FOUND);
    let close = Op::Close { network: THERE };
    assert_eq!(code_of(vote(&chain, &close)), code::NOT_FOUND);
    open(&chain, &[1, 2, 3, 4]);
    vote(&chain, &repin).unwrap();
    assert_eq!(link_of(&chain, THERE).map(|link| link.trusted), Some(2));
}

#[test]
fn a_closed_link_takes_no_send_header_or_delivery() {
    let chain = chain();
    open(&chain, &[1, 2, 3, 4]);
    open_port(&chain, PORT, vec![THERE]);
    let built = there(vec![Epoch::seated(&[1, 2, 3, 4], vec![post(1)])]);
    vote(&chain, &Op::Close { network: THERE }).unwrap();
    assert_eq!(link_of(&chain, THERE).map(|link| link.closed), Some(true));
    let send = Op::Send {
        to: THERE,
        port: PORT.into(),
        op: Vec::new(),
        deadline: None,
    };
    assert_eq!(code_of(submit(&chain, USER, &send)), code::WRONG_STATE);
    assert_eq!(code_of(header(&chain, &built[0])), code::WRONG_STATE);
    let delivered = deliver(&chain, 1, built[0].proven.clone());
    assert_eq!(code_of(delivered), code::WRONG_STATE);
}

#[test]
fn a_port_lists_the_networks_it_takes_and_an_empty_list_closes_it() {
    let chain = chain();
    open_port(&chain, PORT, vec![THERE, ELSEWHERE, THERE]);
    let ports = |chain: &MockChain| {
        ask(chain, &Query::Ports, |reply| match reply {
            Reply::Ports(ports) => Some(ports),
            _ => None,
        })
    };
    let listed = BTreeMap::from([(PORT.to_owned(), vec![THERE, ELSEWHERE])]);
    assert_eq!(ports(&chain), listed);
    open_port(&chain, PORT, Vec::new());
    assert!(ports(&chain).is_empty());
}

fn linked(chain: &MockChain, name: &str) -> Vec<NetworkId> {
    let asked = Query::Linked { name: name.into() };
    ask(chain, &asked, |reply| match reply {
        Reply::Linked(networks) => Some(networks),
        _ => None,
    })
}

#[test]
fn home_is_this_networks_id_and_linked_names_every_link_opened_under_a_name() {
    let chain = chain();
    let home = ask(&chain, &Query::Home, |reply| match reply {
        Reply::Home(home) => Some(home),
        _ => None,
    });
    assert_eq!(home, HERE);
    assert!(linked(&chain, THERE_NAME).is_empty());
    open(&chain, &[1]);
    let namesake = Op::Open {
        network: ELSEWHERE,
        name: THERE_NAME.into(),
        epoch_length: EPOCH_LENGTH,
        epoch: 0,
        kind: CertificateKind::Ed25519SimplexFinalize,
        validators: seat_keys(&[2]),
    };
    vote(&chain, &namesake).unwrap();
    assert_eq!(linked(&chain, THERE_NAME), [THERE, ELSEWHERE]);
    assert!(linked(&chain, "ther").is_empty());
    vote(&chain, &Op::Close { network: THERE }).unwrap();
    assert_eq!(linked(&chain, THERE_NAME), [THERE, ELSEWHERE]);
}

#[test]
fn a_closed_link_opens_again_from_fresh_trust_and_keeps_its_sequence_numbers() {
    let chain = chain();
    open(&chain, &[1, 2, 3, 4]);
    open_port(&chain, PORT, vec![THERE]);
    let built = there(vec![
        Epoch::seated(&[1, 2, 3, 4], vec![post(1)]),
        Epoch::seated(&[5, 6, 7], vec![post(1), post(2)]),
    ]);
    header(&chain, &built[0]).unwrap();
    deliver(&chain, 1, built[0].proven.clone()).unwrap();
    let first = settled_row(&chain, THERE, 1).expect("packet 1 is settled");
    let send = Op::Send {
        to: THERE,
        port: PORT.into(),
        op: Vec::new(),
        deadline: None,
    };
    let sent =
        |chain: &MockChain| abi::decode::<u64>(&submit(chain, USER, &send).unwrap()).unwrap();
    assert_eq!(sent(&chain), 1);
    vote(&chain, &Op::Close { network: THERE }).unwrap();
    let reopen = Op::Open {
        network: THERE,
        name: THERE_NAME.into(),
        epoch_length: EPOCH_LENGTH,
        epoch: 1,
        kind: CertificateKind::Ed25519SimplexFinalize,
        validators: seat_keys(&[5, 6, 7]),
    };
    vote(&chain, &reopen).unwrap();
    let expected = Link {
        name: THERE_NAME.into(),
        epoch_length: EPOCH_LENGTH,
        trusted: 1,
        header: None,
        next_in: 2,
        next_out: 2,
        refused: None,
        closed: false,
    };
    assert_eq!(link_of(&chain, THERE), Some(expected));
    assert_eq!(code_of(vote(&chain, &reopen)), code::ALREADY_EXISTS);
    assert_eq!(
        outbound(&chain, THERE, 0).len(),
        1,
        "the packet owed stays owed"
    );
    assert_eq!(
        receipts(&chain, THERE, 0).len(),
        1,
        "the receipt taken stays"
    );
    header(&chain, &built[1]).unwrap();
    deliver(&chain, 2, built[1].proven.clone()).unwrap();
    assert_eq!(posts(&chain).len(), 2, "packet 1 runs once, packet 2 once");
    assert_eq!(
        settled_row(&chain, THERE, 1),
        Some(first),
        "packet 1 is settled once"
    );
    assert!(settled_row(&chain, THERE, 2).is_some());
    assert_eq!(link_of(&chain, THERE).map(|link| link.next_in), Some(3));
    assert_eq!(sent(&chain), 2, "no sequence number is used twice");
}

#[test]
fn a_packet_taken_before_a_close_is_never_run_again_after_the_link_opens_again() {
    let chain = chain();
    open(&chain, &[1, 2, 3, 4]);
    open_port(&chain, PORT, vec![THERE]);
    let acked_below_three = Epoch {
        lows: vec![(HERE, 3)],
        ..Epoch::seated(&[1, 2, 3, 4], vec![post(1), post(2)])
    };
    let built = there(vec![
        acked_below_three,
        Epoch::seated(&[1, 2, 3, 4], vec![post(1), post(2)]),
    ]);
    header(&chain, &built[0]).unwrap();
    deliver(&chain, 1, built[0].proven.clone()).unwrap();
    deliver_low(&chain, 1, built[0].lows.first().cloned(), Vec::new()).unwrap();
    assert!(
        receipts(&chain, THERE, 0).is_empty(),
        "both are acked and pruned"
    );
    assert_eq!(settled_row(&chain, THERE, 1), None);
    vote(&chain, &Op::Close { network: THERE }).unwrap();
    let reopen = Op::Open {
        network: THERE,
        name: THERE_NAME.into(),
        epoch_length: EPOCH_LENGTH,
        epoch: 1,
        kind: CertificateKind::Ed25519SimplexFinalize,
        validators: seat_keys(&[1, 2, 3, 4]),
    };
    vote(&chain, &reopen).unwrap();
    header(&chain, &built[1]).unwrap();
    let replayed = deliver(&chain, 2, built[1].proven.clone());
    assert_eq!(code_of(replayed), code::DUPLICATE);
    assert_eq!(
        settled_row(&chain, THERE, 1),
        None,
        "seq 1 is never settled again"
    );
    let old_frame = deliver(&chain, 1, built[0].proven.clone());
    assert_eq!(code_of(old_frame), code::STALE);
    assert_eq!(posts(&chain).len(), 2);
    assert_eq!(link_of(&chain, THERE).map(|link| link.next_in), Some(3));
}

#[test]
fn a_link_opened_again_under_a_new_name_is_linked_under_that_name_alone() {
    let chain = chain();
    open(&chain, &[1]);
    vote(&chain, &Op::Close { network: THERE }).unwrap();
    let renamed = Op::Open {
        network: THERE,
        name: "renamed".into(),
        epoch_length: EPOCH_LENGTH,
        epoch: 0,
        kind: CertificateKind::Ed25519SimplexFinalize,
        validators: seat_keys(&[1]),
    };
    vote(&chain, &renamed).unwrap();
    assert!(linked(&chain, THERE_NAME).is_empty());
    assert_eq!(linked(&chain, "renamed"), [THERE]);
}
