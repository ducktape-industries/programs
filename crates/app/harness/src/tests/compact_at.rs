use super::*;

fn configured(chain: &MockChain, compact_at: Option<u64>) {
    let config = Config {
        compact_at,
        ..scout()
    };
    commit(chain, ADA, CONFIG, &config.write());
}

fn model_compacts_at(chain: &MockChain, compact_at: Option<u64>) {
    let op = Op::SetCompactAt {
        id: MODEL.into(),
        compact_at,
    };
    chain.submit(signed(ADA), PROGRAM, &op).unwrap();
}

// the agent answers once with a context of 1,008 tokens, far inside the window, and hears again:
// whether harness then asks the model for a summary rather than a turn
fn summarizes_next(world: &World) -> bool {
    let chain = &world.chain;
    let thread = world.opened("m1", "remember the plan", Some(WINDOW));
    chain.at(3, 0);
    let said = json!([{ "type": "text", "text": "Noted." }]);
    model_says_after(chain, said, "end_turn", 1000);
    assert_eq!(thread_row(chain, thread).context, Context::Measured(1008));
    world.mention("m2", "and now?");
    let (request, _) = open_request(chain);
    body(chain, &request)["compaction"] == json!({ "type": "summarize" })
}

#[test]
fn a_thread_compacts_once_its_context_reaches_the_agents_compact_at() {
    let world = world();
    configured(&world.chain, Some(1000));
    assert!(summarizes_next(&world));
}

#[test]
fn an_agents_compact_at_wins_over_its_models() {
    let world = world();
    configured(&world.chain, Some(5000));
    model_compacts_at(&world.chain, Some(1000));
    assert!(!summarizes_next(&world));
}

#[test]
fn with_no_compact_at_of_its_own_the_agent_compacts_at_its_models() {
    let world = world();
    configured(&world.chain, None);
    model_compacts_at(&world.chain, Some(1000));
    assert!(summarizes_next(&world));
}

#[test]
fn with_no_compact_at_anywhere_a_thread_compacts_only_when_the_window_is_full() {
    let world = world();
    configured(&world.chain, None);
    assert!(!summarizes_next(&world));
}
