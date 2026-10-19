//! Agents on a phone (`ui/phone/`): the root, an agent and its setup's pages, a conversation
//! and its usage page, the sheets each page opens from its title row and an entry's facts from
//! a long press, Back through all of it, and the sweep that holds every frame to the touch
//! ladder at 360, 390 and 430. The fixtures are the desktop's (`super`): the same network.
use super::*;
use view::wire;

/// The view as a phone draws it: a finger, 390 wide, seated as `me`.
fn phone(net: Net, me: u64) -> (TestAppContext, Shared) {
    let (mut cx, shared) = seated(net, me);
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
    (cx, shared)
}

/// `taps` pressed in turn, each settled.
fn tap(cx: &mut TestAppContext, taps: &[&str]) {
    for key in taps {
        cx.simulate_click(key);
        cx.run_until_parked();
    }
}

/// A finger on `key`, held (`long`) or lifted at once: the touch click its route hears.
fn touch(cx: &mut TestAppContext, key: &str, long: bool) {
    let handler = cx
        .interactivity(key)
        .on_click
        .unwrap_or_else(|| panic!("{key} has no click route"));
    cx.tick(vec![wire::Event::Click {
        handler,
        event: wire::click::Click::Touch {
            position: Default::default(),
            tap_count: 1,
            long_press: long,
        },
    }]);
    cx.run_until_parked();
}

fn long_press(cx: &mut TestAppContext, key: &str) {
    touch(cx, key, true);
}

#[test]
fn the_root_lists_the_agents_with_their_state_then_what_else_the_view_holds() {
    let (cx, _) = phone(Net::default(), EDDY);
    for text in [
        "Agents",
        "garage · 1 relay node",
        "My agents",
        "Scout",
        "Working",
        "in #general · waiting for the model",
        // how long ago, in a phone's few letters
        "5m",
        "Pager",
        "Not handed to harness",
        "Other agents",
        "Atlas",
        "More",
        "Create an agent",
        "Models",
        "To talk to an agent, mention it or send it a DM in Chat.",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    // the desktop's columns are not drawn: no manager, no counts
    assert!(!cx.has_text("1 open · 1 closed"));
    assert!(
        cx.find("agents-page-account").is_some(),
        "the root leads with the reader"
    );
    assert_eq!(
        cx.back_handler(),
        None,
        "at the root Back leaves the program"
    );
}

#[test]
fn an_empty_network_says_so_and_offers_create() {
    let net = Net {
        accounts: vec![person(EDDY, "Eddy")],
        setups: Vec::new(),
        threads: Vec::new(),
        ..Net::default()
    };
    let (mut cx, _) = phone(net, EDDY);
    assert!(cx.has_text("No agents yet"), "{:?}", cx.texts());
    tap(&mut cx, &["agents-empty-create"]);
    // the form is its own page over the root, Back to it
    assert!(cx.find("create-page-back").is_some(), "{:?}", cx.texts());
    assert!(cx.find("create-name").is_some());
}

#[test]
fn an_agent_page_says_what_it_does_and_opens_each_part_of_its_setup() {
    let (mut cx, _) = phone(Net::default(), EDDY);
    tap(&mut cx, &["agent-12"]);
    for text in [
        "account 12 · managed by Eddy",
        "In #general · waiting for the model",
        "sonnet · 2 tools",
        "Send a DM",
        "Conversations",
        "#general · 7",
        "Current",
        "waiting for the model · 3 frames",
        "#release · 3",
        "Setup",
        "Model, thinking, tools",
        "sonnet · model's default · 2 tools",
        "Configuration file",
        "/home/Eddy/agents/scout.md · only Eddy may change it",
        "System prompt",
        "1 line · “You are Scout.”",
        "Usage",
        "39,926 tokens over 2 conversations",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    assert!(cx.back_handler().is_some());
    tap(&mut cx, &["agent-part-model"]);
    for text in [
        "Model",
        "4,096 tokens",
        "Thinking",
        "Model's default",
        "Configured",
        "Ready",
        "Edit front matter",
        "forge_log",
        "writes as Scout",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    cx.simulate_back();
    cx.run_until_parked();
    tap(&mut cx, &["agent-part-prompt"]);
    assert!(cx.has_text("You are Scout."), "{:?}", cx.texts());
    cx.simulate_back();
    cx.run_until_parked();
    tap(&mut cx, &["agent-part-file"]);
    assert!(cx.has_text("Only Eddy may change it"), "{:?}", cx.texts());
    cx.simulate_back();
    cx.run_until_parked();
    assert!(cx.has_text("Conversations"), "{:?}", cx.texts());
    cx.simulate_back();
    cx.run_until_parked();
    assert!(cx.has_text("My agents"), "{:?}", cx.texts());
    assert_eq!(cx.back_handler(), None);
}

#[test]
fn the_managers_sheet_edits_starts_forks_and_takes_back_on_a_second_press() {
    let (mut cx, _) = phone(Net::default(), EDDY);
    cx.host()
        .handle::<Submit<harness::Harness>>(|_| Ok(applied(view::methods::encode(&11u64))));
    identity_takes(&cx);
    tap(&mut cx, &["agent-12", "agent-actions"]);
    for text in [
        "manager actions",
        "Edit configuration",
        "New conversation",
        "starts from nothing",
        "Fork",
        "keeps the current context",
        "Take back from harness",
        "second press confirms",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    // Back closes the sheet before the page
    cx.simulate_back();
    cx.run_until_parked();
    assert!(cx.find("agent-sheet").is_none());
    assert!(cx.has_text("Conversations"), "{:?}", cx.texts());

    tap(&mut cx, &["agent-actions", "agent-sheet-new"]);
    tap(&mut cx, &["agent-actions", "agent-sheet-fork"]);
    assert_eq!(
        cx.host().requests::<Submit<harness::Harness>>(),
        [
            harness::Op::NewThread { agent: SCOUT },
            harness::Op::ForkThread { agent: SCOUT }
        ]
    );
    tap(&mut cx, &["agent-actions", "agent-sheet-remove"]);
    assert!(
        cx.has_text(
            "Take Scout back from harness? Its script is cleared, so harness stops answering for it. Its conversations and its configuration file stay."
        ),
        "{:?}",
        cx.texts()
    );
    assert!(cx.host().requests::<Submit<Identity>>().is_empty());
    tap(&mut cx, &["agent-remove-confirm"]);
    assert_eq!(
        cx.host().requests::<Submit<Identity>>(),
        [identity::Op::SetScript {
            account: SCOUT,
            script: None
        }]
    );
    assert!(cx.has_text("My agents"), "{:?}", cx.texts());

    // a visitor has no writes on it
    let (mut cx, _) = phone(Net::default(), ADA);
    tap(&mut cx, &["agent-12"]);
    assert!(cx.find("agent-actions").is_none());
}

#[test]
fn a_conversation_hides_where_each_entry_is_and_a_long_press_opens_its_facts() {
    let (mut cx, _) = phone(Net::default(), EDDY);
    tap(&mut cx, &["agent-12", "thread-7"]);
    for text in [
        "#general · 7",
        "Scout · current conversation",
        "Working",
        "Waiting for the model",
        "36%",
        "Ada mentioned you in chat channel general:\nWhy is payments slow?",
        "Looked at forge",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    assert!(
        !cx.has_text("#1 · block 9,001"),
        "a finger reads it in the facts"
    );
    assert!(cx.find("meta-1").is_none());
    // a tap on an entry does nothing; a long press opens its facts
    touch(&mut cx, "entry-1", false);
    assert!(cx.find("frame-sheet").is_none());
    long_press(&mut cx, "entry-1");
    for text in ["#1 · block 9,001", "Frame", "#1", "Block", "9,001"] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    tap(&mut cx, &["fact-0"]);
    assert_eq!(cx.host().requests::<ClipboardWrite>(), ["#1"]);
    cx.simulate_back();
    cx.run_until_parked();
    // a burst's facts are its frames, its blocks and its calls' ids
    long_press(&mut cx, "entry-2");
    for text in ["#2–#3", "9,002–9,003", "forge_log", "toolu_01"] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
}

#[test]
fn the_conversation_sheet_copies_its_link_opens_its_usage_and_closes_on_a_second_press() {
    let (mut cx, _) = phone(Net::default(), EDDY);
    cx.host()
        .handle::<Submit<harness::Harness>>(|_| Ok(applied(Vec::new())));
    tap(&mut cx, &["agent-12", "thread-7", "thread-actions"]);
    for text in [
        "Open in chat",
        "Copy link",
        "Usage and context",
        "39,926 tokens",
        "Close conversation",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    tap(&mut cx, &["thread-sheet-copy"]);
    assert_eq!(
        cx.host().requests::<ClipboardWrite>(),
        ["duck://garage-0a1b2c3d/harness/12/chat/general"]
    );
    tap(&mut cx, &["thread-sheet-usage"]);
    let turn = format!("#2 · {}", view::design::clock(post(2, "").time));
    for text in [
        "Usage and context",
        "72,140 / 200,000 tokens · 36%",
        "input 1,204 · output 312 · cache read 38,410 · cache write 0 · all 39,926",
        "0 summaries",
        turn.as_str(),
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    cx.simulate_back();
    cx.run_until_parked();
    tap(&mut cx, &["thread-actions", "thread-sheet-close"]);
    assert!(
        cx.has_text("Close this conversation? It is the agent's current one, so the next message to the agent opens a new conversation with fresh context."),
        "{:?}",
        cx.texts()
    );
    assert!(cx.host().requests::<Submit<harness::Harness>>().is_empty());
    tap(&mut cx, &["thread-close-confirm"]);
    assert_eq!(
        cx.host().requests::<Submit<harness::Harness>>(),
        [harness::Op::Close { thread: 7 }]
    );
}

#[test]
fn a_stuck_conversation_says_nothing_recovers_it_and_what_waits() {
    let mut net = Net::default();
    net.threads[0].held = 2;
    net.held = vec![(
        7,
        Body::Post {
            author: Principal::Account(ADA),
            text: "Is the rollback done?\nmore".into(),
        },
    )];
    let (mut cx, _) = phone(net, EDDY);
    tap(&mut cx, &["agent-12", "thread-7"]);
    for text in [
        "Stuck",
        "Still waiting for the model",
        "2 waiting",
        "Nothing recovers a stuck conversation today. Its manager can close it.",
        "Waiting · follow when the turn ends",
        "Is the rollback done?",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
}

#[test]
fn no_node_and_a_refused_read_say_so_on_a_phone() {
    let net = Net {
        serving: vec![(egress::RELAY.into(), 1)],
        ..Net::default()
    };
    let (mut cx, _) = phone(net, EDDY);
    tap(&mut cx, &["agent-12"]);
    assert!(cx.has_text("No node"), "{:?}", cx.texts());
    assert!(
        cx.has_text("The model is not asked; the agent stays silent in chat. An operator assigns a node to the service and turns relay on."),
        "{:?}",
        cx.texts()
    );

    let (mut cx, _) = phone(Net::default(), EDDY);
    cx.host()
        .refuse::<Query<harness::Harness>>("unavailable", "query timed out");
    tap(&mut cx, &["agent-12"]);
    assert!(cx.find("agent-refused").is_some(), "{:?}", cx.texts());
}

#[test]
fn the_models_open_the_form_that_sets_one_as_its_own_page() {
    let (mut cx, _) = phone(Net::default(), EDDY);
    cx.host()
        .handle::<Submit<harness::Harness>>(|_| Ok(applied(Vec::new())));
    tap(&mut cx, &["agents-models"]);
    for text in [
        "Models",
        "sonnet",
        "anthropic · ",
        "1 node serving",
        " · 200K window",
        "Ada",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    tap(&mut cx, &["models-set"]);
    cx.simulate_input("model-id", "opus");
    tap(&mut cx, &["service-anthropic"]);
    cx.simulate_input("model-name", "claude-opus-5-5");
    tap(&mut cx, &["model-next"]);
    assert!(cx.has_text("Save opus"), "{:?}", cx.texts());
    tap(&mut cx, &["model-set"]);
    assert_eq!(
        cx.host().requests::<Submit<harness::Harness>>(),
        [harness::Op::SetModel {
            id: "opus".into(),
            wire: Wire::AnthropicMessages,
            service: "anthropic".into(),
            name: "claude-opus-5-5".into(),
        }]
    );
    // a set record goes back to the models it was set over
    assert!(cx.find("models-list").is_some(), "{:?}", cx.texts());
}

#[test]
fn a_record_opens_its_form_page_and_is_removed_after_a_yes() {
    let (mut cx, _) = phone(Net::default(), EDDY);
    cx.host()
        .handle::<Submit<harness::Harness>>(|_| Ok(applied(Vec::new())));
    tap(&mut cx, &["agents-models", "model-sonnet"]);
    assert!(cx.has_text("Edit sonnet"), "{:?}", cx.texts());
    tap(&mut cx, &["model-remove"]);
    let naming = "Scout and Atlas still name it in their configuration: each asks no model until it names another.";
    assert!(cx.has_text(naming), "{:?}", cx.texts());
    assert!(cx.host().requests::<Submit<harness::Harness>>().is_empty());
    tap(&mut cx, &["model-remove-yes"]);
    assert_eq!(
        cx.host().requests::<Submit<harness::Harness>>(),
        [harness::Op::RemoveModel { id: SONNET.into() }]
    );
    // a removed record goes back to the models
    assert!(cx.find("models-list").is_some(), "{:?}", cx.texts());
}

#[test]
fn the_create_form_is_one_page_and_says_how_far_a_create_got() {
    let (mut cx, _) = phone(Net::default(), EDDY);
    identity_takes(&cx);
    cx.host()
        .refuse::<Submit<files::Files>>("stale", "the file changed since the base snapshot");
    tap(&mut cx, &["agents-create"]);
    assert!(
        cx.has_text("account, file, harness · each op confirmed"),
        "{:?}",
        cx.texts()
    );
    cx.simulate_input("create-name", "Rover");
    fill(&mut cx);
    tap(&mut cx, &["create-next"]);
    assert!(
        cx.has_text("1 of 5 sent · next: write its configuration"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.has_text("Continue"), "{:?}", cx.texts());
}

/// The sweep: the root, an agent, its parts and sheet, a conversation, its sheet, facts and
/// usage page, the models and their form, the create form, at 360/390/430 under a finger,
/// every frame audited and fitted.
#[test]
fn sweep() {
    let (mut cx, _) = phone(Net::default(), EDDY);
    view::testing::sweep(&mut cx, |cx| {
        assert_eq!(cx.back_handler(), None);
        tap(cx, &["agent-12", "agent-actions"]);
        cx.simulate_back();
        tap(cx, &["agent-part-model", "setup-edit"]);
        cx.simulate_back();
        tap(cx, &["agent-part-usage"]);
        cx.simulate_back();
        tap(cx, &["thread-7", "call-toolu_01", "thread-actions"]);
        cx.simulate_back();
        long_press(cx, "entry-1");
        cx.simulate_back();
        tap(cx, &["thread-actions", "thread-sheet-usage"]);
        cx.simulate_back();
        cx.simulate_back();
        cx.simulate_back();
        cx.run_until_parked();
        tap(cx, &["agents-models", "models-set"]);
        cx.simulate_back();
        tap(cx, &["model-sonnet", "model-remove"]);
        cx.simulate_back();
        cx.simulate_back();
        cx.run_until_parked();
        tap(cx, &["agents-create"]);
        cx.simulate_back();
        cx.run_until_parked();
        assert_eq!(cx.back_handler(), None);
    });
}
