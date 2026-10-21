use super::*;

fn frames_asked(cx: &TestAppContext) -> Vec<harness::Query> {
    let asked = cx.host().requests::<Query<harness::Harness>>();
    asked
        .into_iter()
        .filter(|query| {
            matches!(
                query,
                harness::Query::Frames { .. } | harness::Query::FramesFrom { .. }
            )
        })
        .collect()
}

fn wrote(height: u64, written: &[Written]) -> Option<Change> {
    Some(Change {
        height,
        keys: written.iter().map(Written::key).collect(),
    })
}

// the list's reads so far: harness's cards, identity's accounts, the relay nodes, chat's names
fn roster_reads(cx: &TestAppContext) -> [usize; 4] {
    let harness = cx.host().requests::<Query<harness::Harness>>();
    let identity = cx.host().requests::<Query<Identity>>();
    let egress = cx.host().requests::<Query<egress::Egress>>();
    let chat = cx.host().requests::<Query<chat::Chat>>();
    let cards = harness
        .iter()
        .filter(|query| matches!(query, harness::Query::Cards { .. }));
    let accounts = identity
        .iter()
        .filter(|query| matches!(query, identity::Query::List { .. }));
    let relay = egress.iter().filter(
        |query| matches!(query, egress::Query::Serving { service } if service == egress::RELAY),
    );
    let names = chat
        .iter()
        .filter(|query| matches!(query, chat::Query::Names { .. }));
    [
        cards.count(),
        accounts.count(),
        relay.count(),
        names.count(),
    ]
}

fn open_thread(cx: &mut TestAppContext) {
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    cx.simulate_click("thread-7");
    cx.run_until_parked();
}

#[test]
fn the_agent_screen_adds_up_its_usage_from_tallies_and_reads_no_frames() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(
        cx.has_text("Usage · 39,926 tokens over 2 conversations"),
        "{:?}",
        cx.texts()
    );
    let frames = frames_asked(&cx);
    assert!(frames.is_empty(), "{frames:?}");
    let asked = cx.host().requests::<Query<harness::Harness>>();
    let listed = asked
        .iter()
        .filter(|query| matches!(query, harness::Query::Conversations { agent: SCOUT, .. }));
    assert!(listed.count() > 0, "{asked:?}");
    let each = asked.iter().filter(|query| {
        matches!(
            query,
            harness::Query::Threads { .. }
                | harness::Query::Tally { .. }
                | harness::Query::Topic { .. }
        )
    });
    assert_eq!(
        each.count(),
        0,
        "one read, never one per conversation: {asked:?}"
    );
}

#[test]
fn a_conversation_reads_the_frames_its_list_shows_and_the_calls_it_draws() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    open_thread(&mut cx);
    assert!(
        cx.has_text("Reading the deploy log first."),
        "{:?}",
        cx.texts()
    );
    let asked = cx.host().requests::<Query<harness::Harness>>();
    let whole = asked.iter().any(|query| {
        matches!(
            query,
            harness::Query::Frames { .. } | harness::Query::Calls { .. }
        )
    });
    assert!(!whole, "{asked:?}");
    let call = harness::Query::Call {
        thread: 7,
        call: "toolu_01".into(),
    };
    assert!(asked.contains(&call), "{asked:?}");
}

#[test]
fn a_new_frame_reads_only_the_frames_after_those_held() {
    let (mut cx, shared) = seated(Net::default(), EDDY);
    let changes = cx.host().stream::<Changes<harness::Harness>>();
    open_thread(&mut cx);
    {
        let mut net = shared.borrow_mut();
        net.threads[0].frames = 4;
        net.frames.push((7, post(4, "Is it fixed?")));
    }
    let new = [Written::Thread(7), Written::Frame { thread: 7, seq: 4 }];
    changes.send(wrote(10, &new));
    cx.run_until_parked();
    assert!(cx.has_text("Is it fixed?"), "{:?}", cx.texts());
    let after = harness::Query::FramesFrom {
        thread: 7,
        from: 4,
        page: harness::PageRequest {
            after: None,
            limit: None,
        },
    };
    assert_eq!(frames_asked(&cx).last(), Some(&after));
}

#[test]
fn a_block_that_writes_another_conversation_reads_nothing() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    let changes = cx.host().stream::<Changes<harness::Harness>>();
    open_thread(&mut cx);
    let before = cx.host().requests::<Query<harness::Harness>>().len();
    let elsewhere = [
        Written::Frame { thread: 9, seq: 1 },
        Written::Partial(9),
        Written::Call {
            thread: 9,
            call: "toolu_09".into(),
        },
    ];
    changes.send(wrote(10, &elsewhere));
    cx.run_until_parked();
    let asked = cx.host().requests::<Query<harness::Harness>>();
    assert_eq!(asked.len(), before, "{:?}", &asked[before..]);
}

#[test]
fn a_block_of_another_program_reads_the_setup_and_no_frames() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    let changes = cx.host().stream::<Changes<files::Files>>();
    open_thread(&mut cx);
    let frames = frames_asked(&cx).len();
    let setups = |cx: &TestAppContext| {
        let asked = cx.host().requests::<Query<harness::Harness>>();
        let setup = harness::Query::Card { agent: SCOUT };
        asked.iter().filter(|query| **query == setup).count()
    };
    let read = setups(&cx);
    changes.send(Some(Change {
        height: 10,
        keys: Vec::new(),
    }));
    cx.run_until_parked();
    assert_eq!(frames_asked(&cx).len(), frames);
    assert!(setups(&cx) > read, "the configuration file may have moved");
}

#[test]
fn the_list_reads_cards_and_no_prompts() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    let changes = cx.host().stream::<Changes<files::Files>>();
    changes.send(Some(Change {
        height: 10,
        keys: Vec::new(),
    }));
    cx.run_until_parked();
    let asked = cx.host().requests::<Query<harness::Harness>>();
    let card = |query: &harness::Query| matches!(query, harness::Query::Cards { .. });
    let prompt = |query: &harness::Query| matches!(query, harness::Query::Agent { .. });
    assert!(asked.iter().any(card), "{asked:?}");
    assert!(!asked.iter().any(prompt), "{asked:?}");
}

// The list is four reads: harness's cards, the accounts, the relay nodes, and the names of the
// rooms its cards name, in one ask. A write that moves a conversation reads the first three
// again; a room once named stays named, and nothing is read per agent or per conversation. (The
// read the view starts before the session names its reader is dropped for the reader's own, so
// an open may ask chat twice.)
#[test]
fn the_list_reads_four_things_and_a_write_three() {
    let (mut cx, shared) = seated(Net::default(), EDDY);
    let changes = cx.host().stream::<Changes<harness::Harness>>();
    cx.run_until_parked();
    assert!(cx.has_text("1 open · 1 closed"), "{:?}", cx.texts());
    let names = chat::Query::Names {
        ids: vec!["deploys".into(), "general".into()],
    };
    let asked = cx.host().requests::<Query<chat::Chat>>();
    assert_eq!(asked.last(), Some(&names), "{asked:?}");
    let cold = roster_reads(&cx);
    shared.borrow_mut().threads[0].closed = true;
    changes.send(wrote(10, &[Written::Thread(7)]));
    cx.run_until_parked();
    assert!(cx.has_text("0 open · 2 closed"), "{:?}", cx.texts());
    let warm = roster_reads(&cx);
    let moved: Vec<usize> = warm
        .iter()
        .zip(cold)
        .map(|(warm, cold)| warm - cold)
        .collect();
    assert_eq!(moved, [1, 1, 1, 0]);
    let asked = cx.host().requests::<Query<harness::Harness>>();
    let each = asked.iter().filter(|query| {
        matches!(
            query,
            harness::Query::Card { .. }
                | harness::Query::Threads { .. }
                | harness::Query::Thread { .. }
                | harness::Query::Topic { .. }
        )
    });
    assert_eq!(each.count(), 0, "{asked:?}");
}

// duckpond writes harness in every block: a roster read that a write overtakes lands what it
// read, and the write reads once more after it. Thrown away for each write instead, the list
// stayed on its first read forever.
#[test]
fn a_roster_read_lands_though_a_write_comes_in_every_block() {
    let (mut cx, shared) = seated(Net::default(), EDDY);
    let changes = cx.host().stream::<Changes<harness::Harness>>();
    cx.run_until_parked();
    changes.send(wrote(10, &[Written::Agent(SCOUT)]));
    cx.run_until_parked();
    let one_read = cx.reports().len();
    shared
        .borrow_mut()
        .threads
        .push(thread(11, SCOUT, EDDY, "chat/ops"));
    for height in 11..11 + 2 * one_read as u64 {
        changes.send(wrote(height, &[Written::Agent(SCOUT)]));
        cx.tick(Vec::new());
    }
    assert!(cx.has_text("2 open · 1 closed"), "{:?}", cx.texts());
}
