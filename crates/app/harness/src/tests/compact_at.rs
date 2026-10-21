use super::*;

fn configured(chain: &MockChain, compact_at: Option<u64>) {
    let config = Config {
        compact_at,
        ..scout()
    };
    commit(chain, ADA, CONFIG, &md(&config));
}

fn summarizes_next(world: &World) -> bool {
    let chain = &world.chain;
    let thread = world.opened("m1", "remember the plan", Some(WINDOW));
    chain.at(3, 0);
    let said = json!([{ "type": "text", "text": "Noted." }]);
    model_says_after(chain, said, "end_turn", 1000);
    assert_eq!(thread_row(chain, thread).context, Context::Measured(1008));
    world.mention("m2", "and now?");
    let (request, _) = open_request(chain);
    provider_body(chain, &request)["compaction"] == json!({ "type": "summarize" })
}

#[test]
fn a_thread_compacts_once_its_context_reaches_the_agents_compact_at() {
    let world = world();
    configured(&world.chain, Some(1000));
    assert!(summarizes_next(&world));
}

#[test]
fn with_no_compact_at_anywhere_a_thread_compacts_only_when_the_window_is_full() {
    let world = world();
    configured(&world.chain, None);
    assert!(!summarizes_next(&world));
}
