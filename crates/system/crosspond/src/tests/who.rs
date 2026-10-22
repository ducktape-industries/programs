use super::*;

fn votes() -> [Op; 4] {
    [
        Op::Open {
            network: THERE,
            name: THERE_NAME.into(),
            epoch_length: EPOCH_LENGTH,
            epoch: 0,
            kind: CertificateKind::Ed25519SimplexFinalize,
            validators: seat_keys(&[1, 2, 3, 4]),
        },
        Op::Repin {
            network: THERE,
            epoch: 0,
            kind: CertificateKind::Ed25519SimplexFinalize,
            validators: seat_keys(&[5, 6, 7, 8]),
        },
        Op::Close { network: THERE },
        Op::Port {
            role: PORT.into(),
            from: vec![THERE],
        },
    ]
}

fn send() -> Op {
    Op::Send {
        to: THERE,
        port: PORT.into(),
        op: abi::encode(&Note::Post("hi".into())),
        deadline: None,
    }
}

fn send_for(port: &str) -> Op {
    Op::SendFor {
        to: THERE,
        port: port.into(),
        op: abi::encode(&Note::Post("hi".into())),
        deadline: None,
        account: ALICE,
    }
}

#[test]
fn only_the_validators_vote_opens_repins_closes_and_sets_ports() {
    let chain = chain();
    for op in &votes() {
        assert_eq!(code_of(submit(&chain, USER, op)), code::UNAUTHORIZED);
        let by_board = chain.submit(Origin::Program(BOARD.into()), PROGRAM, op);
        assert_eq!(code_of(by_board), code::UNAUTHORIZED);
    }
    for op in &votes() {
        vote(&chain, op).unwrap();
    }
}

#[test]
fn send_takes_the_senders_account_and_refuses_a_keyless_or_foreign_sender() {
    let chain = chain();
    open(&chain, &[1, 2, 3, 4]);
    assert_eq!(
        code_of(submit(&chain, STRANGER, &send())),
        code::UNAUTHORIZED
    );
    let foreign = Foreign {
        network: THERE,
        account: SENDER,
    };
    let from_afar = chain.submit_foreign(foreign.into(), PROGRAM, &send());
    assert_eq!(code_of(from_afar), code::UNAUTHORIZED);
    submit(&chain, USER, &send()).unwrap();
    let [sent] = &outbound(&chain, THERE, 0)[..] else {
        panic!("one packet is owed");
    };
    assert_eq!((sent.from, sent.via.as_deref()), (ALICE, None));
}

#[test]
fn send_for_comes_only_from_the_program_bound_to_the_ports_role() {
    let chain = chain();
    open(&chain, &[1, 2, 3, 4]);
    let board = Origin::Program(BOARD.into());
    let unbound = chain.submit(board.clone(), PROGRAM, &send_for("forge"));
    assert_eq!(code_of(unbound), code::UNAUTHORIZED);
    let other = chain.submit(
        Origin::Program(egress::PROGRAM.into()),
        PROGRAM,
        &send_for(PORT),
    );
    assert_eq!(code_of(other), code::UNAUTHORIZED);
    assert_eq!(
        code_of(submit(&chain, USER, &send_for(PORT))),
        code::UNAUTHORIZED
    );
    chain.submit(board, PROGRAM, &send_for(PORT)).unwrap();
    let [sent] = &outbound(&chain, THERE, 0)[..] else {
        panic!("one packet is owed");
    };
    let fields = (sent.from, sent.via.as_deref(), sent.name.as_str());
    assert_eq!(fields, (ALICE, Some(PORT), "alice"));
}

#[test]
fn pump_runs_only_as_crossponds_own_scheduled_run() {
    let chain = chain();
    open(&chain, &[1, 2, 3, 4]);
    let pump = Op::Pump { to: THERE };
    assert_eq!(code_of(submit(&chain, USER, &pump)), code::UNAUTHORIZED);
    let by_itself = chain.submit(Origin::Program(PROGRAM.into()), PROGRAM, &pump);
    assert_eq!(code_of(by_itself), code::UNAUTHORIZED);
    assert_eq!(code_of(vote(&chain, &pump)), code::UNAUTHORIZED);
}

#[test]
fn anyone_submits_a_header_or_a_delivery_and_the_checks_decide() {
    let chain = chain();
    open(&chain, &[1, 2, 3, 4]);
    open_port(&chain, PORT, vec![THERE]);
    let built = there(vec![Epoch::seated(&[1, 2, 3, 4], vec![post(1)])]);
    header(&chain, &built[0]).unwrap();
    deliver(&chain, 1, built[0].proven.clone()).unwrap();
    assert_eq!(posts(&chain).len(), 1);
}
