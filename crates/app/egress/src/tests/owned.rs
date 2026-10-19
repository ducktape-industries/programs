//! Who owns a service's keys: the guard's record of it, the policy that narrows assigning it, and
//! the list its owner lends it with.
use super::*;
use crate::state::{SERVICES, Service};

fn owned_chain() -> MockChain {
    let mut chain = chain();
    chain.seat::<ownership::Ownership>(ownership::PROGRAM);
    chain
}

fn record(chain: &MockChain) -> Option<ownership::Record> {
    let query = ownership::Query::Record {
        program: PROGRAM.into(),
        key: SERVICES.key(&SERVICE.to_owned()),
    };
    let ownership::Reply::Record(record) = chain.query(ownership::PROGRAM, &query).unwrap() else {
        panic!("Record answers a record");
    };
    record
}

fn account(chain: &MockChain, key: &[u8]) -> u64 {
    let asked = identity::Query::OfKey { key: key.to_vec() };
    let identity::Reply::Number(Some(number)) = chain.query(identity::PROGRAM, &asked).unwrap()
    else {
        panic!("the key holds an account");
    };
    number
}

fn lent_to(chain: &MockChain, number: u64) -> bool {
    let asked = access::Query::Writers {
        program: PROGRAM.into(),
        key: lend_key(SERVICE),
    };
    let access::Reply::Writers(writers) = chain.query("access", &asked).unwrap() else {
        panic!("Writers answers who writes the key");
    };
    writers.allows(&[::program::Party::Account(number)])
}

fn lend(
    chain: &MockChain,
    key: &[u8],
    list: Option<::program::Ownership>,
) -> Result<Vec<u8>, Error> {
    let set = access::Op::Set {
        program: PROGRAM.into(),
        key: lend_key(SERVICE),
        list,
    };
    chain.submit(Origin::Signed(key.to_vec()), "access", &set)
}

fn assign(nodes: &[[u8; 32]]) -> Op {
    Op::Assign {
        service: SERVICE.into(),
        nodes: nodes.iter().map(|key| key.to_vec()).collect(),
    }
}

#[test]
fn a_service_is_lent_by_its_owner_to_the_accounts_its_lend_list_names() {
    let chain = chain();
    let register = Op::Register {
        service: SERVICE.into(),
    };
    submit(&chain, OWNER, &register).unwrap();
    let other = account(&chain, OTHER);
    assert!(
        !lent_to(&chain, other),
        "a service is lent to no one at first"
    );
    let refused = lend(&chain, OTHER, Some(::program::Ownership::Anyone)).unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
    let listed = ::program::Ownership::Listed(vec![::program::Party::Account(other)]);
    lend(&chain, OWNER, Some(listed)).unwrap();
    assert!(lent_to(&chain, other));
    lend(&chain, OWNER, None).unwrap();
    assert!(!lent_to(&chain, other), "clearing the list ends the loan");
}

#[test]
fn egress_claims_each_service_it_registers() {
    assert_eq!(SERVICES.kind(), Some(<Service as store::Kind>::KIND));
    assert_eq!(<Service as store::Kind>::KIND, "egress.service");
}

#[test]
fn a_service_is_its_registrants_and_its_policy_decides_who_assigns() {
    let chain = owned_chain();
    let register = Op::Register {
        service: SERVICE.into(),
    };
    submit(&chain, OWNER, &register).unwrap();
    let record = record(&chain).expect("the guard recorded the service");
    assert_eq!(record.kind.as_deref(), Some("egress.service"));
    let ownership::Party::Account(owner) = record.owner else {
        panic!("an account registered it");
    };
    // the access list lets anyone assign; the service's policy is what narrows it now
    let anyone = access::Op::Set {
        program: PROGRAM.into(),
        key: service_key(SERVICE),
        list: Some(::program::Ownership::Anyone),
    };
    chain
        .submit(Origin::Signed(OWNER.to_vec()), "access", &anyone)
        .unwrap();
    submit(&chain, OTHER, &assign(&[BO])).unwrap();
    let strict = ownership::Op::Create {
        name: "mine".into(),
        write: ownership::Writers::Owner,
    };
    let made = chain
        .submit(Origin::Signed(OWNER.to_vec()), ownership::PROGRAM, &strict)
        .unwrap();
    let policy: ownership::PolicyId = abi::decode(&made).unwrap();
    let relink = ownership::Op::Relink {
        program: PROGRAM.into(),
        key: SERVICES.key(&SERVICE.to_owned()),
        policy,
    };
    chain
        .submit(Origin::Signed(OWNER.to_vec()), ownership::PROGRAM, &relink)
        .unwrap();
    let refused = submit(&chain, OTHER, &assign(&[ADA])).unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
    submit(&chain, OWNER, &assign(&[ADA])).unwrap();
    let Reply::Service(Some(row)) = chain
        .query(
            PROGRAM,
            &Query::Service {
                name: SERVICE.into(),
            },
        )
        .unwrap()
    else {
        panic!("the service is registered");
    };
    assert_eq!((row.owner, row.nodes), (owner, vec![ADA.to_vec()]));
}
