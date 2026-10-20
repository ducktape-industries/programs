use super::*;

fn compacting(agent: Option<u64>) -> Net {
    let mut net = Net::default();
    let Configured::Routed { config, .. } = &mut net.setups[0].configured else {
        unreachable!()
    };
    config.compact_at = agent;
    net
}

#[test]
fn the_agent_page_says_when_its_conversations_compact_and_where_that_comes_from() {
    let cases = [
        (
            Some(650_000),
            "650,000 tokens",
            Some("the agent's configuration"),
        ),
        (None, "when the window fills", None),
    ];
    for (agent, words, source) in cases {
        let (mut cx, _) = seated(compacting(agent), EDDY);
        files_take(&cx);
        cx.simulate_click("agent-12");
        cx.run_until_parked();
        assert!(cx.has_text("Compacts at"), "{:?}", cx.texts());
        assert!(cx.has_text(words), "{words}: {:?}", cx.texts());
        if let Some(source) = source {
            assert!(cx.has_text(source), "{source}: {:?}", cx.texts());
        }
    }
}

#[test]
fn an_edit_keeps_the_compact_at_the_file_names() {
    let (mut cx, _) = seated(compacting(Some(650_000)), EDDY);
    files_take(&cx);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    cx.simulate_click("agent-edit");
    cx.run_until_parked();
    cx.simulate_click("create-next");
    cx.run_until_parked();
    let kept = Config {
        compact_at: Some(650_000),
        ..config_of("Scout")
    };
    assert_eq!(
        commits(&cx).last(),
        Some(&edit_of(
            "/home/Eddy/agents/scout.md",
            &kept,
            Some(SNAPSHOT),
            "Configure agent Scout (account 12)".into()
        ))
    );
}
