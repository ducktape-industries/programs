use super::*;
use crate::state::{CONVERSATIONS, MODELS};

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
fn harness_claims_its_models_and_agents_conversations() {
    assert_eq!(MODELS.kind(), Some("harness.model"));
    assert_eq!(CONVERSATIONS.kind(), Some("harness.agent-conversation"));
}

#[test]
fn a_models_policy_decides_its_record_and_default_and_any_turn_still_learns_its_window() {
    let mut world = world();
    world.chain.seat::<ownership::Ownership>(ownership::PROGRAM);
    let chain = &world.chain;
    let removed = Op::RemoveModel { id: MODEL.into() };
    chain.submit(signed(ADA), PROGRAM, &removed).unwrap();
    chain
        .submit(signed(ADA), PROGRAM, &set_model(MODEL, MODEL))
        .unwrap();
    let owned = record(chain, MODEL).expect("the guard recorded the model");
    assert_eq!(owned.owner, ownership::Party::Account(world.ada));
    assert_eq!(owned.kind.as_deref(), Some("harness.model"));

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

    world.opened("m1", "hello", Some(WINDOW));
    assert_eq!(model_row(chain, MODEL).window, Window::Tokens(WINDOW));
    assert_eq!(record(chain, MODEL).unwrap().policy, policy);
}

#[test]
fn an_agents_manager_claims_who_starts_its_conversations_and_its_policy_decides() {
    let mut world = world();
    world.chain.seat::<ownership::Ownership>(ownership::PROGRAM);
    let chain = &world.chain;
    let agent = world.agent;
    let claim = Op::ClaimConversations { agent };
    let refused = chain.submit(signed(BO), PROGRAM, &claim).unwrap_err();
    assert_eq!(
        refused.code,
        code::UNAUTHORIZED,
        "only its manager claims it"
    );
    chain.submit(signed(ADA), PROGRAM, &claim).unwrap();
    chain.submit(signed(ADA), PROGRAM, &claim).unwrap();
    let query = ownership::Query::Record {
        program: PROGRAM.into(),
        key: CONVERSATIONS.key(&agent),
    };
    let ownership::Reply::Record(Some(owned)) = chain.query(ownership::PROGRAM, &query).unwrap()
    else {
        panic!("the guard recorded the claim");
    };
    assert_eq!(owned.owner, ownership::Party::Account(world.ada));
    assert_eq!(owned.kind.as_deref(), Some("harness.agent-conversation"));
    let anyone = access::Op::Set {
        program: PROGRAM.into(),
        key: crate::conversation_key(agent),
        list: Some(::program::Ownership::Anyone),
    };
    chain.submit(signed(ADA), "access", &anyone).unwrap();
    let new = Op::NewThread { agent };
    chain.submit(signed(BO), PROGRAM, &new).unwrap();
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
        key: CONVERSATIONS.key(&agent),
        policy,
    };
    chain
        .submit(signed(ADA), ownership::PROGRAM, &relink)
        .unwrap();
    for op in [Op::NewThread { agent }, Op::ForkThread { agent }] {
        let error = chain.submit(signed(BO), PROGRAM, &op).unwrap_err();
        assert_eq!(error.code, code::UNAUTHORIZED, "{op:?}");
    }
    chain.submit(signed(ADA), PROGRAM, &new).unwrap();
}

#[test]
fn adopt_owners_records_models_and_gives_every_agent_its_managers_conversations_row() {
    let mut world = world();
    world.chain.seat::<ownership::Ownership>(ownership::PROGRAM);
    let chain = &world.chain;
    for kind in ["harness.model", "harness.agent-conversation"] {
        let mut after = None;
        loop {
            let op = Op::AdoptOwners {
                kind: kind.into(),
                after,
                limit: 2,
            };
            let next: Option<Vec<u8>> =
                abi::decode(&chain.submit(signed(BO), PROGRAM, &op).unwrap()).unwrap();
            if next.is_none() {
                break;
            }
            after = next;
        }
    }
    let effective = |key: Vec<u8>| {
        let query = ownership::Query::Effective {
            program: PROGRAM.into(),
            key,
        };
        let ownership::Reply::Policy(Some(policy)) =
            chain.query(ownership::PROGRAM, &query).unwrap()
        else {
            panic!("the row is recorded");
        };
        policy
    };
    let model = effective(MODELS.key(&MODEL.to_owned()));
    assert_eq!(
        (model.name.as_str(), &model.owner),
        (ownership::AS_BEFORE, &ownership::Party::Account(world.ada))
    );
    let conversations = effective(CONVERSATIONS.key(&world.agent));
    assert_eq!(
        (conversations.name.as_str(), &conversations.owner),
        (ownership::AS_BEFORE, &ownership::Party::Account(world.ada))
    );
    let new = Op::NewThread { agent: world.agent };
    let refused = chain.submit(signed(BO), PROGRAM, &new).unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
    chain.submit(signed(ADA), PROGRAM, &new).unwrap();
}
