use super::*;
use crate::state::{CONVERSATIONS, PROVIDER_ROUTES};

fn record(chain: &MockChain, provider: &str) -> Option<ownership::Record> {
    let query = ownership::Query::Record {
        program: PROGRAM.into(),
        key: PROVIDER_ROUTES.key(&provider.to_owned()),
    };
    let ownership::Reply::Record(record) = chain.query(ownership::PROGRAM, &query).unwrap() else {
        panic!("Record answers a record");
    };
    record
}

fn window(chain: &MockChain, agent: AccountNumber) -> Window {
    let asked = Query::Card { agent };
    let Reply::Agent(Some(setup)) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!("the agent has a card");
    };
    setup
        .configured
        .reached()
        .expect("the agent is routed")
        .window
}

#[test]
fn harness_claims_its_routes_and_agents_conversations() {
    assert_eq!(PROVIDER_ROUTES.kind(), Some("harness.route"));
    assert_eq!(CONVERSATIONS.kind(), Some("harness.agent-conversation"));
}

#[test]
fn a_routes_policy_decides_who_sets_it_and_any_turn_still_learns_its_window() {
    let mut world = world();
    world.chain.seat::<ownership::Ownership>(ownership::PROGRAM);
    let chain = &world.chain;
    let removed = Op::RemoveRoute {
        provider: PROVIDER.into(),
    };
    chain.submit(signed(ADA), PROGRAM, &removed).unwrap();
    let ccs = [(SERVICE, Wire::AnthropicMessages)];
    chain
        .submit(signed(ADA), PROGRAM, &set_route(&ccs))
        .unwrap();
    let owned = record(chain, PROVIDER).expect("the guard recorded the route");
    assert_eq!(owned.owner, ownership::Party::Account(world.ada));
    assert_eq!(owned.kind.as_deref(), Some("harness.route"));

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
        key: PROVIDER_ROUTES.key(&PROVIDER.to_owned()),
        policy,
    };
    chain
        .submit(signed(ADA), ownership::PROGRAM, &relink)
        .unwrap();
    let error = chain
        .submit(signed(BO), PROGRAM, &set_route(&ccs))
        .unwrap_err();
    assert_eq!(error.code, code::UNAUTHORIZED);
    chain
        .submit(signed(ADA), PROGRAM, &set_route(&ccs))
        .unwrap();

    world.opened("m1", "hello", Some(WINDOW));
    assert_eq!(window(chain, world.agent), Window::Tokens(WINDOW));
    assert_eq!(record(chain, PROVIDER).unwrap().policy, policy);
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
fn adopt_owners_gives_every_agent_its_managers_conversations_row_and_knows_no_model_kind() {
    let mut world = world();
    world.chain.seat::<ownership::Ownership>(ownership::PROGRAM);
    let chain = &world.chain;
    let mut after = None;
    loop {
        let op = Op::AdoptOwners {
            kind: "harness.agent-conversation".into(),
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
    let models = Op::AdoptOwners {
        kind: "harness.model".into(),
        after: None,
        limit: 2,
    };
    let refused = chain.submit(signed(BO), PROGRAM, &models).unwrap_err();
    assert_eq!(refused.code, code::INVALID_INPUT);
    let query = ownership::Query::Effective {
        program: PROGRAM.into(),
        key: CONVERSATIONS.key(&world.agent),
    };
    let ownership::Reply::Policy(Some(conversations)) =
        chain.query(ownership::PROGRAM, &query).unwrap()
    else {
        panic!("the row is recorded");
    };
    assert_eq!(
        (conversations.name.as_str(), &conversations.owner),
        (ownership::AS_BEFORE, &ownership::Party::Account(world.ada))
    );
    let new = Op::NewThread { agent: world.agent };
    let refused = chain.submit(signed(BO), PROGRAM, &new).unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
    chain.submit(signed(ADA), PROGRAM, &new).unwrap();
}
