use ::program::{Held, Program as _, Table};

use super::*;
use crate::state::MODELS;

fn record(chain: &MockChain, id: &str) -> Option<ownership::Record> {
    let query = ownership::Query::Record {
        program: PROGRAM.into(),
        key: MODELS.key(&id.to_owned()),
    };
    let ownership::Reply::Record(record) = chain.query(ownership::PROGRAM, &query).unwrap() else {
        panic!("Record answers a record");
    };
    record
}

fn model_row(chain: &MockChain, id: &str) -> ModelRow {
    let Reply::Model(Some(row)) = chain
        .query(PROGRAM, &Query::Model { id: id.into() })
        .unwrap()
    else {
        panic!("the model is set");
    };
    row
}

fn compact_at(tokens: u64) -> Op {
    Op::SetCompactAt {
        id: MODEL.into(),
        compact_at: Some(tokens),
    }
}

#[test]
fn harness_declares_its_models_owned_and_every_other_table_shared() {
    let owned: Vec<&Table> = Harness::TABLES
        .iter()
        .filter(|table| table.held != Held::Shared)
        .collect();
    assert_eq!(owned, [&MODELS.table()]);
    assert_eq!(owned[0].held, Held::Owned(Some("harness.model")));
}

#[test]
fn a_models_policy_decides_its_record_and_default_and_any_turn_still_learns_its_window() {
    let mut world = world();
    world.chain.seat::<ownership::Ownership>(ownership::PROGRAM);
    let chain = &world.chain;
    // a record made before the guard was seated has no owner on record: make it again
    let removed = Op::RemoveModel { id: MODEL.into() };
    chain.submit(signed(ADA), PROGRAM, &removed).unwrap();
    chain
        .submit(signed(ADA), PROGRAM, &set_model(MODEL, MODEL))
        .unwrap();
    let owned = record(chain, MODEL).expect("the guard recorded the model");
    assert_eq!(owned.owner, ownership::Party::Account(world.ada));
    assert_eq!(owned.kind.as_deref(), Some("harness.model"));

    // the access list lets anyone write it; the record's own policy is what narrows it now
    let anyone = access::Op::Set {
        program: PROGRAM.into(),
        key: crate::model_key(MODEL),
        list: Some(::program::Ownership::Anyone),
    };
    chain.submit(signed(ADA), "access", &anyone).unwrap();
    chain
        .submit(signed(BO), PROGRAM, &compact_at(1000))
        .unwrap();
    let strict = ownership::Op::Create {
        name: "mine".into(),
        write: ownership::Writers::Owner,
    };
    let made = chain
        .submit(signed(ADA), ownership::PROGRAM, &strict)
        .unwrap();
    let policy: ownership::PolicyId = abi::decode(&made).unwrap();
    let relink = ownership::Op::Relink {
        program: PROGRAM.into(),
        key: MODELS.key(&MODEL.to_owned()),
        policy,
    };
    chain
        .submit(signed(ADA), ownership::PROGRAM, &relink)
        .unwrap();
    for refused in [compact_at(2000), set_model(MODEL, "claude-bo")] {
        let error = chain.submit(signed(BO), PROGRAM, &refused).unwrap_err();
        assert_eq!(error.code, code::UNAUTHORIZED, "{refused:?}");
    }
    chain
        .submit(signed(ADA), PROGRAM, &compact_at(3000))
        .unwrap();
    assert_eq!(model_row(chain, MODEL).compact_at, Some(3000));

    // an agent's turn learns the window of a model it doesn't own
    world.opened("m1", "hello", Some(WINDOW));
    assert_eq!(model_row(chain, MODEL).window, Window::Tokens(WINDOW));
    assert_eq!(record(chain, MODEL).unwrap().policy, policy);
}
