use super::*;

#[test]
fn the_manager_starts_or_forks_a_conversation_from_the_agent_page() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    cx.host()
        .handle::<Submit<harness::Harness>>(|_| Ok(applied(view::methods::encode(&11u64))));
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(cx.find("thread-7-current").is_some(), "{:?}", cx.texts());
    assert!(cx.find("thread-3-current").is_none());
    cx.simulate_click("agent-new-thread");
    cx.run_until_parked();
    cx.simulate_click("agent-fork-thread");
    cx.run_until_parked();
    assert_eq!(
        cx.host().requests::<Submit<harness::Harness>>(),
        [
            harness::Op::NewThread { agent: SCOUT },
            harness::Op::ForkThread { agent: SCOUT }
        ]
    );
}

#[test]
fn only_the_manager_sees_new_and_fork_and_no_current_conversation_shows_no_current_badge() {
    let (mut cx, _) = seated(Net::default(), ADA);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(cx.find("agent-new-thread").is_none());
    assert!(cx.find("agent-fork-thread").is_none());

    let mut net = Net::default();
    net.setups[0].current = None;
    let (mut cx, _) = seated(net, EDDY);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(cx.find("agent-new-thread").is_some());
    assert!(cx.find("thread-7-current").is_none());
}

#[test]
fn closing_a_conversation_says_whether_it_is_the_current_one() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    cx.simulate_click("thread-3");
    cx.run_until_parked();
    assert!(
        cx.find("thread-close").is_none(),
        "a closed conversation closes no more"
    );

    let mut net = Net::default();
    net.threads[1].closed = false;
    let (mut cx, _) = seated(net, EDDY);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    cx.simulate_click("thread-3");
    cx.run_until_parked();
    cx.simulate_click("thread-close");
    cx.run_until_parked();
    assert!(
        cx.has_text(
            "Close this conversation? It is not the agent's current one, so nothing more reaches it."
        ),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn a_conversation_reads_by_its_number_and_its_room_by_the_name_chat_gives_it() {
    // a room's channel id says nothing to a person, and a conversation opened before an agent
    // kept one conversation carries that id in its title
    const ROOM: &str = "channel-18df5a416a71b5fa-1792250406982472";
    let mut net = Net::default();
    net.rooms.push((ROOM.to_owned(), "payments".to_owned()));
    net.threads[0].place = format!("chat/{ROOM}");
    net.threads[0].title = format!("chat channel {ROOM}");
    let (mut cx, _) = seated(net, EDDY);
    // the list's sentence names its room; the agent's rows, its heading and the window title
    // add its number
    assert!(
        cx.has_text("in #payments · waiting for the model"),
        "{:?}",
        cx.texts()
    );
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(cx.has_text("#payments · 7"), "{:?}", cx.texts());
    cx.simulate_click("thread-7");
    cx.run_until_parked();
    assert!(cx.find("thread-place").is_some());
    assert!(cx.has_text("#payments · 7"), "{:?}", cx.texts());
    assert!(
        cx.texts().iter().all(|text| !text.contains(ROOM)),
        "{:?}",
        cx.texts()
    );
    assert_eq!(
        cx.host().requests::<HostTitle>().last().map(String::as_str),
        Some("Scout · #payments · 7")
    );
}

#[test]
fn a_conversation_with_no_place_yet_reads_by_its_number_in_the_agents_sentence() {
    let mut net = Net::default();
    net.threads[0].place = String::new();
    net.threads[0].asking = Asking::Idle;
    let (cx, _) = seated(net, EDDY);
    assert!(cx.has_text("last in Conversation 7"), "{:?}", cx.texts());
}

#[test]
fn the_strip_says_what_the_conversation_did_last_or_that_it_closed() {
    let mut net = Net::default();
    net.threads[0].asking = Asking::Idle;
    net.threads[0].last_stop = Some(Stop::EndTurn);
    let (mut cx, _) = seated(net, EDDY);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    cx.simulate_click("thread-7");
    cx.run_until_parked();
    let last = format!(
        "3 frames · last at {}, {}",
        view::design::clock(LAST_TIME),
        view::design::day(LAST_TIME)
    );
    for text in ["Idle", "Answered", last.as_str()] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    cx.simulate_click("thread-back");
    cx.run_until_parked();
    cx.simulate_click("thread-3");
    cx.run_until_parked();
    for text in ["Closed", "0 frames · closed at block 9,003"] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
}

#[test]
fn where_an_entry_is_copies_on_a_press() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    cx.simulate_click("thread-7");
    cx.run_until_parked();
    cx.simulate_click("meta-1");
    cx.run_until_parked();
    cx.simulate_click("meta-2");
    cx.run_until_parked();
    assert_eq!(
        cx.host().requests::<ClipboardWrite>(),
        [
            "#1 · block 9,001",
            "#2–#3 · blocks 9,002–9,003 · snapshot abababababab"
        ]
    );
}

#[test]
fn a_write_in_flight_says_what_it_does_until_its_block() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    cx.host().never::<Submit<Identity>>();
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    cx.simulate_click("agent-remove");
    cx.run_until_parked();
    cx.simulate_click("agent-remove-confirm");
    cx.run_until_parked();
    assert!(cx.has_text("Taking Scout back…"), "{:?}", cx.texts());
    assert!(
        cx.has_text("sent · waiting for the block"),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn the_agents_topic_shows_beside_its_conversation_and_never_in_place_of_its_name() {
    const TOPIC: &str = "Finding why payments is slow";
    let net = Net {
        topics: vec![(7, TOPIC.into())],
        ..Net::default()
    };
    let quoted = format!("“{TOPIC}”");
    let (mut cx, _) = seated(net, EDDY);
    let sentence = format!("in #general · waiting for the model · {quoted}");
    assert!(cx.has_text(&sentence), "{sentence}: {:?}", cx.texts());
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(cx.find("thread-7-topic").is_some(), "{:?}", cx.texts());
    assert!(cx.has_text(&quoted), "{:?}", cx.texts());
    assert!(
        cx.find("thread-3-topic").is_none(),
        "a thread with none shows none"
    );
    cx.simulate_click("thread-7");
    cx.run_until_parked();
    assert!(cx.has_text("#general · 7"), "{:?}", cx.texts());
    assert!(cx.find("thread-topic").is_some(), "{:?}", cx.texts());
    assert!(cx.has_text(&quoted), "{:?}", cx.texts());
    assert_eq!(
        cx.host().requests::<HostTitle>().last().map(String::as_str),
        Some("Scout · #general · 7")
    );

    // a phone draws it in the row's caption and under the conversation's title row
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
    assert!(cx.find("thread-topic").is_some(), "{:?}", cx.texts());
    assert!(cx.has_text("#general · 7"), "{:?}", cx.texts());
    cx.simulate_back();
    cx.run_until_parked();
    assert!(cx.has_text(&quoted), "{:?}", cx.texts());
}

#[test]
fn a_conversation_bound_to_a_place_wears_a_bound_badge_on_desktop_and_phone() {
    let mut net = Net::default();
    let mut bound = thread(11, SCOUT, EDDY, "chat/deploys");
    bound.title = "conversation 11, bound to chat channel deploys".into();
    net.threads.push(bound);
    net.bound = vec![11];
    let (mut cx, _) = seated(net, EDDY);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(cx.find("thread-11-bound").is_some(), "{:?}", cx.texts());
    assert!(cx.has_text("Bound"), "{:?}", cx.texts());
    assert!(cx.find("thread-11-current").is_none());
    assert!(cx.find("thread-7-current").is_some());
    assert!(cx.find("thread-7-bound").is_none());

    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
    assert!(cx.find("thread-11-bound").is_some(), "{:?}", cx.texts());
    assert!(cx.find("thread-7-bound").is_none());
}
