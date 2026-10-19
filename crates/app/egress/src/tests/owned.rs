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

fn assign(nodes: &[[u8; 32]]) -> Op {
    Op::Assign {
        service: SERVICE.into(),
        nodes: nodes.iter().map(|key| key.to_vec()).collect(),
    }
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
