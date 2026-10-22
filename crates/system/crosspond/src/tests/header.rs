use super::*;

fn trusting(trusted: &[u64]) -> MockChain {
    let chain = chain();
    open(&chain, trusted);
    chain
}

fn epoch_of(seated: &[u64]) -> Built {
    there(vec![Epoch::seated(seated, Vec::new())])
        .pop()
        .expect("one epoch was built")
}

fn trust(chain: &MockChain) -> Option<(u64, Option<u64>)> {
    link_of(chain, THERE).map(|link| (link.trusted, link.header))
}

#[test]
fn a_real_header_is_taken_and_moves_the_trust_to_its_epoch() {
    let chain = trusting(&[1, 2, 3, 4]);
    let built = epoch_of(&[1, 2, 3, 4]);
    header(&chain, &built).unwrap();
    assert_eq!(trust(&chain), Some((1, Some(1))));
    assert_eq!(code_of(header(&chain, &built)), code::STALE);
}

#[test]
fn a_header_of_a_certificate_kind_this_network_does_not_name_is_refused_and_leaves_the_link() {
    let chain = trusting(&[1, 2, 3, 4]);
    let built = epoch_of(&[1, 2, 3, 4]);
    let link = [b"link/".as_slice(), &THERE].concat();
    let before = raw(&chain, &link);
    let op = Op::Header {
        from: THERE,
        header: built.header.clone(),
    };
    let mut bytes = abi::encode(&op);
    let kind = 1 + THERE.len();
    assert_eq!(bytes[kind], CertificateKind::Ed25519SimplexFinalize as u8);
    bytes[kind] = 1;
    let refused = chain.submit_raw(Origin::Signed(STRANGER.to_vec()), PROGRAM, &bytes);
    let refused = refused.expect_err("an unknown certificate kind is refused");
    assert_eq!(refused.code, code::INVALID_INPUT);
    assert!(
        refused.message.contains("did not decode"),
        "{}",
        refused.message
    );
    assert_eq!(raw(&chain, &link), before);
    assert_eq!(trust(&chain), Some((0, None)));
    header(&chain, &built).unwrap();
    assert_eq!(trust(&chain), Some((1, Some(1))));
}

#[test]
fn a_header_for_no_link_is_not_found() {
    let chain = chain();
    let built = epoch_of(&[1, 2, 3, 4]);
    assert_eq!(code_of(header(&chain, &built)), code::NOT_FOUND);
}

#[test]
fn a_certificate_over_another_block_is_refused() {
    let chain = trusting(&[1, 2, 3, 4]);
    let mut built = epoch_of(&[1, 2, 3, 4]);
    built.header.block.push(0);
    assert_eq!(code_of(header(&chain, &built)), code::INVALID_INPUT);
    assert_eq!(trust(&chain), Some((0, None)));
}

#[test]
fn a_block_without_its_settled_record_is_refused() {
    let chain = trusting(&[1, 2, 3, 4]);
    let unsettled = Epoch {
        settled: false,
        ..Epoch::seated(&[1, 2, 3, 4], Vec::new())
    };
    let built = there(vec![unsettled]).pop().unwrap();
    assert_eq!(code_of(header(&chain, &built)), code::INVALID_INPUT);
    assert_eq!(trust(&chain), Some((0, None)));
}

#[test]
fn roots_the_block_does_not_settle_are_refused() {
    let chain = trusting(&[1, 2, 3, 4]);
    let mut built = epoch_of(&[1, 2, 3, 4]);
    built.header.roots[1].1.0[0] ^= 1;
    assert_eq!(code_of(header(&chain, &built)), code::INVALID_INPUT);
    assert_eq!(trust(&chain), Some((0, None)));
}

#[test]
fn validators_the_proof_does_not_hold_are_refused() {
    let chain = trusting(&[1, 2, 3, 4]);
    let mut built = epoch_of(&[1, 2, 3, 4]);
    built.header.validators = seat_keys(&[4, 3, 2, 1]);
    assert_eq!(code_of(header(&chain, &built)), code::INVALID_INPUT);
    assert_eq!(trust(&chain), Some((0, None)));
}

#[test]
fn a_quorum_short_of_the_new_set_is_refused() {
    let chain = trusting(&[1, 2, 3, 4]);
    let short = Epoch {
        signers: vec![1, 2],
        ..Epoch::seated(&[1, 2, 3, 4], Vec::new())
    };
    let built = there(vec![short]).pop().unwrap();
    assert_eq!(code_of(header(&chain, &built)), code::INVALID_INPUT);
    assert_eq!(trust(&chain), Some((0, None)));
}

#[test]
fn a_forged_signature_is_refused() {
    let chain = trusting(&[1, 2, 3, 4]);
    let mut built = epoch_of(&[1, 2, 3, 4]);
    let last = built.header.certificate.len() - 1;
    built.header.certificate[last] ^= 1;
    assert_eq!(code_of(header(&chain, &built)), code::INVALID_INPUT);
    assert_eq!(trust(&chain), Some((0, None)));
}

#[test]
fn signers_holding_a_third_or_less_of_the_trusted_set_wait_for_the_epochs_between() {
    let under = trusting(&[1, 2, 3, 4]);
    assert_eq!(
        code_of(header(&under, &epoch_of(&[1, 5, 6, 7]))),
        code::NOT_YET
    );
    assert_eq!(trust(&under), Some((0, None)));
    let at = trusting(&[1, 2, 3]);
    assert_eq!(code_of(header(&at, &epoch_of(&[1, 5, 6]))), code::NOT_YET);
    assert_eq!(trust(&at), Some((0, None)));
    let over = trusting(&[1, 2, 3, 4]);
    header(&over, &epoch_of(&[1, 2, 5, 6])).unwrap();
    assert_eq!(trust(&over), Some((1, Some(1))));
}

#[test]
fn the_relay_skips_epochs_the_overlap_carries() {
    let built = there(vec![
        Epoch::seated(&[1, 2, 3, 5], Vec::new()),
        Epoch::seated(&[1, 2, 3, 6], Vec::new()),
    ]);
    let chain = trusting(&[1, 2, 3, 4]);
    header(&chain, &built[1]).unwrap();
    assert_eq!(trust(&chain), Some((2, Some(2))));
    assert_eq!(code_of(header(&chain, &built[0])), code::STALE);
}

#[test]
fn a_header_past_the_overlap_is_taken_after_the_epochs_between() {
    let built = there(vec![
        Epoch::seated(&[1, 2, 5, 6], Vec::new()),
        Epoch::seated(&[5, 6, 7, 8], Vec::new()),
    ]);
    let chain = trusting(&[1, 2, 3, 4]);
    assert_eq!(code_of(header(&chain, &built[1])), code::NOT_YET);
    header(&chain, &built[0]).unwrap();
    header(&chain, &built[1]).unwrap();
    assert_eq!(trust(&chain), Some((2, Some(2))));
}

#[test]
fn a_stalled_link_moves_again_once_the_vote_repins_it() {
    let chain = trusting(&[1, 2, 3, 4]);
    let rotated = epoch_of(&[5, 6, 7, 8]);
    assert_eq!(code_of(header(&chain, &rotated)), code::NOT_YET);
    let repin = Op::Repin {
        network: THERE,
        epoch: 0,
        kind: CertificateKind::Ed25519SimplexFinalize,
        validators: seat_keys(&[5, 6, 7, 8]),
    };
    vote(&chain, &repin).unwrap();
    header(&chain, &rotated).unwrap();
    assert_eq!(trust(&chain), Some((1, Some(1))));
}

#[test]
fn a_block_that_does_not_open_its_epoch_is_refused() {
    let chain = chain();
    let open = Op::Open {
        network: THERE,
        name: THERE_NAME.into(),
        epoch_length: EPOCH_LENGTH + 1,
        epoch: 0,
        kind: CertificateKind::Ed25519SimplexFinalize,
        validators: seat_keys(&[1, 2, 3, 4]),
    };
    vote(&chain, &open).unwrap();
    let built = epoch_of(&[1, 2, 3, 4]);
    assert_eq!(code_of(header(&chain, &built)), code::INVALID_INPUT);
    assert_eq!(trust(&chain), Some((0, None)));
}

#[test]
fn a_certificate_from_a_network_of_another_name_is_refused() {
    let chain = chain();
    let open = Op::Open {
        network: THERE,
        name: "elsewhere".into(),
        epoch_length: EPOCH_LENGTH,
        epoch: 0,
        kind: CertificateKind::Ed25519SimplexFinalize,
        validators: seat_keys(&[1, 2, 3, 4]),
    };
    vote(&chain, &open).unwrap();
    let built = epoch_of(&[1, 2, 3, 4]);
    assert_eq!(code_of(header(&chain, &built)), code::INVALID_INPUT);
}
