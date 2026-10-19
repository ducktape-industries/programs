use super::*;

// what the view asked harness for of a timeline: all of it, or a span from a seq
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

// a block of harness that wrote `written`
fn wrote(height: u64, written: &[Written]) -> Option<Change> {
    Some(Change {
        height,
        keys: written.iter().map(Written::key).collect(),
    })
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
    for thread in [7, 3] {
        let tally = harness::Query::Tally { thread };
        assert!(asked.contains(&tally), "{asked:?}");
    }
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

// The list reads each agent's card again on a block of another program, and
// never the setup with its prompt (`Agent`), which only the agent's own
// screen reads: a phone's list of agents read every agent's whole prompt on
// every block.
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
    let card = |query: &harness::Query| matches!(query, harness::Query::Card { .. });
    let prompt = |query: &harness::Query| matches!(query, harness::Query::Agent { .. });
    assert!(asked.iter().any(card), "{asked:?}");
    assert!(!asked.iter().any(prompt), "{asked:?}");
}
