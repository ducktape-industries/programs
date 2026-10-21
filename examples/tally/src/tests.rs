use program::{Error, MockChain, Origin, code};

use crate::{Op, PROGRAM, Query, Reply, Tally};

const ADA: u64 = 1;
const BO: u64 = 2;

fn key_of(account: u64) -> Vec<u8> {
    vec![account as u8; 32]
}

fn chain() -> MockChain {
    let mut chain = MockChain::default();
    chain.seat::<Tally>(PROGRAM);
    chain.hold(key_of(ADA), ADA);
    chain.hold(key_of(BO), BO);
    chain
}

fn add(chain: &MockChain, account: u64, by: u64) -> Result<Vec<u8>, Error> {
    chain.submit(Origin::Signed(key_of(account)), PROGRAM, &Op::Add { by })
}

fn asked(chain: &MockChain, query: Query) -> Reply {
    chain.query(PROGRAM, &query).unwrap()
}

#[test]
fn each_add_counts_for_its_account_and_the_total_and_returns_the_total() {
    let chain = chain();
    assert_eq!(add(&chain, ADA, 2).unwrap(), program::abi::encode(&2u64));
    assert_eq!(add(&chain, BO, 3).unwrap(), program::abi::encode(&5u64));
    assert_eq!(asked(&chain, Query::Total), Reply::Total(5));
    assert_eq!(
        asked(&chain, Query::Added { account: ADA }),
        Reply::Added(2)
    );
    assert_eq!(asked(&chain, Query::Added { account: BO }), Reply::Added(3));
}

#[test]
fn an_add_that_would_overflow_is_refused_and_changes_nothing() {
    let chain = chain();
    add(&chain, ADA, u64::MAX).unwrap();
    let refused = add(&chain, BO, 1).unwrap_err();
    assert_eq!(refused.code, code::INVALID_INPUT);
    assert_eq!(asked(&chain, Query::Total), Reply::Total(u64::MAX));
    assert_eq!(asked(&chain, Query::Added { account: BO }), Reply::Added(0));
}

#[test]
fn a_key_that_holds_no_account_adds_nothing() {
    let chain = chain();
    let refused = chain
        .submit(Origin::Signed(vec![9; 32]), PROGRAM, &Op::Add { by: 1 })
        .unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
    assert_eq!(asked(&chain, Query::Total), Reply::Total(0));
}

#[test]
fn bytes_that_are_no_op_of_tally_are_refused() {
    let refused = chain()
        .submit_raw(Origin::Signed(key_of(ADA)), PROGRAM, [7])
        .unwrap_err();
    assert_eq!(refused.code, code::INVALID_INPUT);
}

#[test]
fn op_variants_only_append() {
    assert_eq!(describe::variants::<Op>(), ["Add"]);
}
