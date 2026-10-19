use super::*;

fn head(chain: &MockChain) -> BlobId {
    let files::Reply::Head(Some(row)) = chain.query(files::PROGRAM, &files::Query::Head).unwrap()
    else {
        panic!()
    };
    row.id
}

fn turn_configs(chain: &MockChain, thread: u64) -> Vec<BlobId> {
    frames(chain, thread)
        .into_iter()
        .filter_map(|body| match body {
            Body::Turn { config, .. } => Some(config),
            _ => None,
        })
        .collect()
}

fn setup(chain: &MockChain, agent: AccountNumber) -> Option<AgentSetup> {
    let Reply::Agent(setup) = chain.query(PROGRAM, &Query::Agent { agent }).unwrap() else {
        panic!()
    };
    setup
}

#[test]
fn each_model_call_reads_the_configuration_at_the_head_and_its_turn_names_that_snapshot() {
    let world = world();
    let chain = &world.chain;
    let first = head(chain);
    let thread = world.opened("m1", "hello", Some(WINDOW));
    let (request, _) = open_request(chain);
    let sent = body(chain, &request);
    assert_eq!(sent["max_tokens"], 1024);
    assert_eq!(sent["system"][0]["text"], "Keep notes in /shared.");

    let french = Config {
        max_tokens: 2048,
        system: "Answer in French.".into(),
        ..scout()
    };
    let second = commit(chain, ADA, CONFIG, &french.write());
    chain.at(3, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Hi." }]),
        "end_turn",
    );
    assert_eq!(
        turn_configs(chain, thread),
        [first],
        "the turn names the snapshot its call read, not the edit made while it ran"
    );

    world.mention("m2", "again");
    let (request, _) = open_request(chain);
    let sent = body(chain, &request);
    assert_eq!(
        sent["max_tokens"], 2048,
        "the edit holds from the next call"
    );
    assert_eq!(sent["system"][0]["text"], "Answer in French.");
    chain.at(4, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Salut." }]),
        "end_turn",
    );
    assert_eq!(turn_configs(chain, thread), [first, second]);
}

#[test]
fn a_missing_or_malformed_configuration_or_an_unknown_model_is_noticed_once_and_the_agent_waits() {
    let world = world();
    let chain = &world.chain;
    let path = "/home/Ada/agents/later.md";
    scripted(chain, world.agent, path);
    let thread = world.mention("m1", "hello");
    let unread = notices(chain, thread);
    assert_eq!(unread.len(), 1);
    assert!(
        unread[0].contains(path) && unread[0].contains("could not be read"),
        "{}",
        unread[0]
    );
    assert!(assigned(chain, SERVICE).is_empty(), "no model is asked");
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);
    assert!(matches!(
        setup(chain, world.agent).unwrap().configured,
        Configured::Unreadable { .. }
    ));

    commit(chain, ADA, path, "+++\nmodel = 1\n+++\nhi\n");
    world.mention("m2", "hello again");
    let told = notices(chain, thread);
    assert_eq!(told.len(), 2, "one notice for each attempt");
    assert!(
        told[1].contains(path) && told[1].contains("is malformed"),
        "{}",
        told[1]
    );
    assert!(assigned(chain, SERVICE).is_empty());

    let elsewhere = Config {
        model: "nobody".into(),
        ..scout()
    };
    commit(chain, ADA, path, &elsewhere.write());
    world.mention("m3", "and again");
    let told = notices(chain, thread);
    assert_eq!(told.len(), 3);
    assert!(
        told[2].contains(path) && told[2].contains("names model nobody"),
        "{}",
        told[2]
    );
    assert!(assigned(chain, SERVICE).is_empty());
    assert!(matches!(
        setup(chain, world.agent).unwrap().configured,
        Configured::UnknownModel { config, .. } if config == elsewhere
    ));

    commit(chain, ADA, path, &scout().write());
    world.mention("m4", "now?");
    let (_, asked) = open_request(chain);
    assert_eq!(super::path(&asked), "/v1/models/claude-test");
    assert_eq!(notices(chain, thread).len(), 3);
}

#[test]
fn the_agent_query_resolves_what_harness_runs_an_account_with() {
    let world = world();
    let chain = &world.chain;
    let snapshot = head(chain);
    let Some(AgentSetup {
        agent,
        manager,
        standing,
        path,
        configured:
            Configured::Ready {
                snapshot: read,
                config,
                model,
            },
        current: None,
    }) = setup(chain, world.agent)
    else {
        panic!("{:?}", setup(chain, world.agent));
    };
    assert_eq!(
        (agent, manager, standing, path.as_str(), read),
        (world.agent, world.ada, Standing::Active, CONFIG, snapshot)
    );
    assert_eq!(config, scout());
    assert_eq!(
        (model.id.as_str(), model.wire, model.service.as_str()),
        (MODEL, Wire::AnthropicMessages, SERVICE)
    );
    assert_eq!(setup(chain, world.bo), None, "a user is no agent");
    let suspend = identity::Op::Suspend {
        account: world.agent,
    };
    identity_op(chain, ADA, suspend).unwrap();
    assert_eq!(
        setup(chain, world.agent).map(|setup| setup.standing),
        Some(Standing::Suspended)
    );
}

#[test]
fn harness_registers_with_attribution_at_its_init() {
    let world = world();
    let attribution::Reply::Registered(registered) = world
        .chain
        .query(attribution::PROGRAM, &attribution::Query::Registered)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(registered, [PROGRAM.to_owned()]);
}

// `Card` is `Agent` with the system prompt left out: a list of agents, read
// again on every block, carries no agent's prompt
#[test]
fn a_card_is_the_setup_without_its_system_prompt() {
    let world = world();
    let chain = &world.chain;
    let whole = setup(chain, world.agent).unwrap();
    let asked = Query::Card { agent: world.agent };
    let Reply::Agent(Some(card)) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!()
    };
    let mut expected = whole.clone();
    if let Configured::Ready { config, .. } = &mut expected.configured {
        assert_eq!(config.system, "Keep notes in /shared.");
        config.system = String::new();
    }
    assert_ne!(expected, whole, "the agent's setup has a prompt");
    assert_eq!(card, expected);
}
