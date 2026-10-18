use super::*;
use harness::Thinking;

#[test]
fn the_create_form_writes_the_thinking_level_it_picks() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    identity_takes(&cx);
    files_take(&cx);
    cx.simulate_click("agents-create");
    cx.run_until_parked();
    cx.simulate_input("create-name", "Rover");
    fill(&mut cx);
    cx.simulate_click("create-thinking-4");
    cx.simulate_click("create-next");
    cx.run_until_parked();
    let thinks = Config {
        thinking: Some(Thinking::High),
        ..rover()
    };
    assert_eq!(
        cx.host().requests::<Submit<files::Files>>(),
        [edit_of(
            "/home/Eddy/agents/rover.md",
            &thinks,
            None,
            "Configure agent Rover (account 31)".into()
        )]
    );
}

#[test]
fn the_agent_shows_its_thinking_and_its_manager_sets_it_there() {
    let mut net = Net::default();
    let Configured::Ready { config, .. } = &mut net.setups[0].configured else {
        unreachable!()
    };
    config.thinking = Some(Thinking::Low);
    let (mut cx, _) = seated(net, EDDY);
    files_take(&cx);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(cx.has_text("Thinking"), "{:?}", cx.texts());
    assert!(cx.has_text("Low"), "{:?}", cx.texts());
    cx.simulate_click("agent-thinking-level-5");
    cx.run_until_parked();
    let deeper = Config {
        thinking: Some(Thinking::Max),
        ..config_of("Scout")
    };
    assert_eq!(
        cx.host().requests::<Submit<files::Files>>(),
        [edit_of(
            "/home/Eddy/agents/scout.md",
            &deeper,
            Some(SNAPSHOT),
            "Configure agent Scout (account 12)".into()
        )]
    );
    assert!(cx.host().requests::<Submit<Identity>>().is_empty());

    cx.simulate_click("agent-edit");
    cx.run_until_parked();
    cx.simulate_click("create-next");
    cx.run_until_parked();
    let kept = Config {
        thinking: Some(Thinking::Low),
        ..config_of("Scout")
    };
    assert_eq!(
        cx.host().requests::<Submit<files::Files>>().last(),
        Some(&edit_of(
            "/home/Eddy/agents/scout.md",
            &kept,
            Some(SNAPSHOT),
            "Configure agent Scout (account 12)".into()
        )),
        "an edit keeps the level the file names"
    );
}

#[test]
fn only_the_manager_sees_the_thinking_picker() {
    let (mut cx, _) = seated(Net::default(), ADA);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    // a visitor's setup is folded until a press opens it
    assert!(!cx.has_text("Model's default"));
    cx.simulate_click("setup");
    cx.run_until_parked();
    assert!(cx.has_text("Model's default"), "{:?}", cx.texts());
    assert!(cx.find("agent-thinking-level").is_none());
}
