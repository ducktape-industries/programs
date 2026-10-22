use super::*;

fn linked() -> MockChain {
    let chain = chain();
    open(&chain, &[1, 2, 3, 4]);
    open_port(&chain, PORT, vec![THERE]);
    open_port(&chain, PARK, vec![THERE]);
    serve(&chain, ADA);
    chain
}

fn lowering(packets: Vec<Packet>, lows: &[u64]) -> Epoch {
    Epoch {
        lows: lows.iter().map(|n| (HERE, *n)).collect(),
        ..Epoch::seated(&[1, 2, 3, 4], packets)
    }
}

fn kept(chain: &MockChain) -> Vec<u64> {
    receipts(chain, THERE, 0).iter().map(|r| r.seq).collect()
}

fn settled_seqs(chain: &MockChain) -> Vec<u64> {
    (1..=8)
        .filter(|seq| settled_row(chain, THERE, *seq).is_some())
        .collect()
}

fn mark(chain: &MockChain) -> Option<u64> {
    raw(chain, &[b"pruned/".as_slice(), &THERE].concat()).map(|bytes| abi::decode(&bytes).unwrap())
}

fn low_of(built: &Built, n: u64) -> ProvenLow {
    built
        .lows
        .iter()
        .find(|low| low.n == n)
        .cloned()
        .expect("the epoch proves that low-water row")
}

fn parked(seq: u64) -> Packet {
    Packet {
        port: PARK.into(),
        op: abi::encode(&()),
        ..post(seq)
    }
}

fn taken(packets: Vec<Packet>, lows: &[u64]) -> (MockChain, Built) {
    let chain = linked();
    let built = there(vec![lowering(packets, lows)])
        .pop()
        .expect("one epoch was built");
    header(&chain, &built).unwrap();
    deliver_low(&chain, 1, None, built.proven.clone()).unwrap();
    (chain, built)
}

#[test]
fn a_frame_at_retired_tag_8_does_not_decode_and_prunes_nothing() {
    let (chain, _) = taken(vec![post(1), post(2), post(3)], &[]);
    let retired = [
        [8u8].as_slice(),
        &THERE,
        &1u64.to_le_bytes(),
        &u64::MAX.to_le_bytes(),
        &0u32.to_le_bytes(),
    ]
    .concat();
    let refused = chain.submit_raw(Origin::Signed(STRANGER.to_vec()), PROGRAM, &retired);
    let refused = refused.expect_err("tag 8 is no op");
    assert_eq!(refused.code, code::INVALID_INPUT);
    assert!(
        refused.message.contains("did not decode"),
        "{}",
        refused.message
    );
    assert_eq!(kept(&chain), [1, 2, 3]);
    assert_eq!(settled_seqs(&chain), [1, 2, 3]);
    assert_eq!(mark(&chain), None);
}

#[test]
fn a_proven_low_water_mark_prunes_receipts_and_settled_rows_below_it_and_no_further() {
    let (chain, built) = taken(vec![post(1), post(2), post(3), post(4)], &[3]);
    deliver_low(&chain, 1, Some(low_of(&built, 3)), Vec::new()).unwrap();
    assert_eq!(kept(&chain), [3, 4]);
    assert_eq!(settled_seqs(&chain), [3, 4]);
    assert_eq!(mark(&chain), Some(3));
}

#[test]
fn a_stale_low_water_mark_prunes_only_below_it() {
    let (chain, built) = taken(vec![post(1), post(2), post(3), post(4)], &[2, 4]);
    deliver_low(&chain, 1, Some(low_of(&built, 2)), Vec::new()).unwrap();
    assert_eq!(kept(&chain), [2, 3, 4]);
    assert_eq!(settled_seqs(&chain), [2, 3, 4]);
    deliver_low(&chain, 1, Some(low_of(&built, 4)), Vec::new()).unwrap();
    assert_eq!(kept(&chain), [4]);
}

#[test]
fn the_pruned_mark_only_rises_and_an_older_proof_prunes_nothing_new() {
    let (chain, built) = taken(vec![post(1), parked(2), post(3), post(4)], &[2, 4]);
    deliver_low(&chain, 1, Some(low_of(&built, 4)), Vec::new()).unwrap();
    assert_eq!(kept(&chain), [2, 4]);
    assert_eq!(mark(&chain), Some(4));
    answer_next(&chain, Answer::Output(Vec::new())).unwrap();
    assert!(settled_row(&chain, THERE, 2).is_some());
    deliver_low(&chain, 1, Some(low_of(&built, 2)), Vec::new()).unwrap();
    deliver_low(&chain, 1, Some(low_of(&built, 4)), Vec::new()).unwrap();
    assert_eq!(mark(&chain), Some(4));
    assert_eq!(
        kept(&chain),
        [2, 4],
        "a mark it reached already prunes nothing new"
    );
    let later = there(vec![
        lowering(vec![post(1), parked(2), post(3), post(4)], &[2, 4]),
        lowering(Vec::new(), &[5]),
    ])
    .pop()
    .expect("two epochs were built");
    header(&chain, &later).unwrap();
    deliver_low(&chain, 2, Some(low_of(&later, 5)), Vec::new()).unwrap();
    assert_eq!(kept(&chain), Vec::<u64>::new());
    assert_eq!(mark(&chain), Some(5));
}

#[test]
fn a_parked_receipt_is_never_pruned_before_its_result_is_written() {
    let (chain, built) = taken(vec![post(1), parked(2), post(3)], &[4]);
    assert_eq!(receipts(&chain, THERE, 1)[0].result, None);
    deliver_low(&chain, 1, Some(low_of(&built, 4)), Vec::new()).unwrap();
    assert_eq!(kept(&chain), [2]);
    assert_eq!(settled_seqs(&chain), Vec::<u64>::new());
    answer_next(&chain, Answer::Output(Vec::new())).unwrap();
    assert_eq!(receipts(&chain, THERE, 1)[0].result, Some(Ok(Vec::new())));
    let settled = settled_row(&chain, THERE, 2).expect("the result settles");
    assert_eq!(settled, settled_of(&parked(2), Ok(Vec::new())));
}

#[test]
fn a_low_water_proof_for_another_network_or_under_another_header_is_refused() {
    let chain = linked();
    let built = there(vec![
        Epoch {
            lows: vec![(ELSEWHERE, 3), (HERE, 3)],
            ..Epoch::seated(&[1, 2, 3, 4], vec![post(1), post(2), post(3)])
        },
        lowering(vec![post(4)], &[]),
    ]);
    header(&chain, &built[0]).unwrap();
    deliver_low(&chain, 1, None, built[0].proven.clone()).unwrap();
    let for_elsewhere = ProvenLow {
        n: 3,
        proof: built[0].lows[0].proof.clone(),
    };
    let refused = deliver_low(&chain, 1, Some(for_elsewhere), Vec::new());
    assert_eq!(code_of(refused), code::INVALID_INPUT);
    assert_eq!(kept(&chain), [1, 2, 3]);
    header(&chain, &built[1]).unwrap();
    let older_header = deliver_low(&chain, 2, Some(low_of(&built[0], 3)), Vec::new());
    assert_eq!(code_of(older_header), code::INVALID_INPUT);
    let older_epoch = deliver_low(&chain, 1, Some(low_of(&built[0], 3)), Vec::new());
    assert_eq!(code_of(older_epoch), code::STALE);
    let another_network = there(vec![lowering(vec![post(9)], &[3])]).pop().unwrap();
    let another_root = deliver_low(&chain, 2, Some(low_of(&another_network, 3)), Vec::new());
    assert_eq!(code_of(another_root), code::INVALID_INPUT);
    assert_eq!(kept(&chain), [1, 2, 3]);
    assert_eq!(mark(&chain), None);
}

#[test]
fn a_frame_proves_its_low_water_mark_at_the_header_its_packets_are_proven_under() {
    let chain = linked();
    let built = there(vec![lowering(vec![post(1), post(2), post(3)], &[2])])
        .pop()
        .unwrap();
    header(&chain, &built).unwrap();
    deliver_low(&chain, 1, None, built.proven[..2].to_vec()).unwrap();
    deliver_low(
        &chain,
        1,
        Some(low_of(&built, 2)),
        built.proven[2..].to_vec(),
    )
    .unwrap();
    assert_eq!(kept(&chain), [2, 3]);
    assert_eq!(posts(&chain).len(), 3);
}

#[test]
fn a_settled_row_is_written_once_and_a_second_write_is_refused() {
    let (chain, built) = taken(vec![post(1)], &[]);
    let first = settled_row(&chain, THERE, 1).expect("packet 1 is settled");
    assert_eq!(first, settled_of(&post(1), Ok(b"post 1".to_vec())));
    deliver_low(&chain, 1, None, built.proven.clone()).unwrap();
    assert_eq!(settled_row(&chain, THERE, 1), Some(first.clone()));
    let ctx = chain.host(PROGRAM).exec(chain.env(PROGRAM));
    let again = crate::rules::settle(
        &ctx,
        &(THERE, 1),
        &HERE,
        first.packet,
        Err(abi::Refusal::new(code::STALE, "late")),
    );
    assert_eq!(code_of(again.map(|()| Vec::new())), code::WRONG_STATE);
    assert_eq!(settled_row(&chain, THERE, 1), Some(first));
}
