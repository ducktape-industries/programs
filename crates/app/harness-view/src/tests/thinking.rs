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
    cx.simulate_click("setup-edit");
    cx.run_until_parked();
    cx.simulate_click("setup-thinking-5");
    cx.simulate_click("setup-save");
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
fn a_visitor_reads_the_setup_and_may_open_its_editor() {
    let (mut cx, _) = seated(Net::default(), ADA);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(!cx.has_text("Model's default"));
    cx.simulate_click("setup");
    cx.run_until_parked();
    assert!(cx.has_text("Model's default"), "{:?}", cx.texts());
    assert!(cx.find("setup-edit").is_some(), "{:?}", cx.texts());
}

#[test]
fn an_edit_and_a_thinking_pick_keep_the_channels_it_listens_to_and_its_settle() {
    let mut net = Net::default();
    let Configured::Ready { config, .. } = &mut net.setups[0].configured else {
        unreachable!()
    };
    config.listen = vec!["general".into()];
    config.settle = Some(10_000);
    let hears = config.clone();
    let (mut cx, _) = seated(net, EDDY);
    files_take(&cx);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    cx.simulate_click("setup-edit");
    cx.run_until_parked();
    cx.simulate_click("setup-thinking-2");
    cx.simulate_click("setup-save");
    cx.run_until_parked();
    let thinks = Config {
        thinking: Some(Thinking::Low),
        ..hears.clone()
    };
    let message = "Configure agent Scout (account 12)";
    let path = "/home/Eddy/agents/scout.md";
    assert_eq!(
        cx.host().requests::<Submit<files::Files>>(),
        [edit_of(path, &thinks, Some(SNAPSHOT), message.into())]
    );
    cx.simulate_click("agent-edit");
    cx.run_until_parked();
    cx.simulate_click("create-next");
    cx.run_until_parked();
    assert_eq!(
        cx.host().requests::<Submit<files::Files>>().last(),
        Some(&edit_of(path, &hears, Some(SNAPSHOT), message.into())),
    );
}

#[test]
fn a_conversation_in_its_settle_window_says_since_which_block_it_collects() {
    let mut net = Net::default();
    net.threads[0].asking = Asking::Idle;
    net.settling = vec![(
        7,
        harness::SettleWindow {
            opened: 9_004,
            opened_time: LAST_TIME,
            until: LAST_TIME + 10_000,
            entry: 3,
        },
    )];
    let (mut cx, _) = seated(net, EDDY);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    cx.simulate_click("thread-7");
    cx.run_until_parked();
    let collecting = "Collecting messages since block 9,004";
    assert!(cx.has_text(collecting), "{:?}", cx.texts());
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
    assert!(cx.has_text(collecting), "{:?}", cx.texts());
}
