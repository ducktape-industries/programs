use super::*;

#[test]
fn the_window_follows_the_head_and_stops_where_the_archive_does() {
    let mut window = Chain::default();
    let blocks = chain(30);
    let ask = |before| BlockPage {
        before,
        limit: 20,
        most: 0,
    };
    window.land(None, page(&blocks, &ask(None)));
    assert_eq!((window.top(), window.blocks.len()), (Some(30), 20));
    assert!(!window.complete, "a full page may have more below it");
    window.land(Some(11), page(&blocks, &ask(Some(11))));
    assert_eq!(window.blocks.len(), 31);
    assert!(window.complete);
    let more = chain(32);
    window.land(None, page(&more, &ask(None)));
    assert_eq!((window.top(), window.blocks.len()), (Some(32), 33));
    assert!(
        window
            .blocks
            .windows(2)
            .all(|pair| pair[0].height == pair[1].height + 1)
    );
    assert_eq!(window.txs.len(), 3, "no transaction is folded in twice");
    assert_eq!(window.block(11).map(|block| block.txs), Some(2));
    // a head that no longer joins the window starts it again
    let far = chain(400);
    window.land(None, page(&far, &ask(None)));
    assert_eq!((window.top(), window.blocks.len()), (Some(400), 20));
    assert!(!window.complete && window.txs.is_empty());
}

#[test]
fn a_page_cut_short_reads_on_until_the_lowest_listed_block() {
    let mut window = Chain::default();
    let blocks = chain(30);
    let ask = |before| BlockPage {
        before,
        limit: 20,
        most: 0,
    };
    let mut short = page(&blocks, &ask(None));
    short.blocks.truncate(5);
    window.land(None, short);
    assert_eq!((window.top(), window.blocks.len()), (Some(30), 5));
    assert!(!window.complete, "a page cut by its bytes is not the end");
    let mut listed = page(&blocks, &ask(Some(26)));
    listed.blocks.retain(|block| block.height >= 20);
    listed.listed_from = Some(20);
    window.land(Some(26), listed);
    assert_eq!(window.blocks.last().map(|block| block.height), Some(20));
    assert!(window.complete);
    assert_eq!(window.unlisted_below(), Some(20));
}

#[test]
fn a_pushed_head_reads_only_the_new_blocks() {
    let mut cx = TestAppContext::new();
    let heads = cx.host().stream::<ChainHeads>();
    let tip = Rc::new(RefCell::new(12));
    node(&mut cx, tip.clone());
    cx.open::<Explorer>();
    cx.run_until_parked();
    let status_reads = cx.host().requests::<ChainStatus>().len();
    *tip.borrow_mut() = 14;
    heads.send(Head {
        height: 14,
        time: T0 + 14_000,
        id: [14; 32],
    });
    cx.run_until_parked();
    assert!(cx.has_text("13–14 · 2 empty blocks"), "{:?}", cx.texts());
    assert!(
        cx.has_text("14"),
        "the Height stat moved with the page: {:?}",
        cx.texts()
    );
    let explorer_asked = cx.host().requests::<ChainBlocks>();
    assert_eq!(explorer_asked.len(), 2, "{explorer_asked:?}");
    assert!(explorer_asked.iter().all(|ask| ask.before.is_none()));
    assert_eq!(
        cx.host().requests::<ChainStatus>().len(),
        status_reads,
        "a head moves the status without a read"
    );
}

/// An archive far longer than the window is read as far as the window, in
/// one page: the node lists every block it keeps to a page that asks for
/// them all. (On garage, 56k blocks: that page was 14.6 MB, decoded in one
/// call to keep the newest 1,000, and the view ran out of fuel.)
#[test]
fn a_long_archive_is_read_only_as_far_as_the_window() {
    let mut cx = TestAppContext::new();
    let heads = cx.host().stream::<ChainHeads>();
    let tip = Rc::new(RefCell::new(52_000));
    node(&mut cx, tip.clone());
    cx.open::<Explorer>();
    cx.run_until_parked();
    let window = BlockPage {
        before: None,
        limit: WINDOW as u32,
        most: 0,
    };
    assert_eq!(
        cx.host().requests::<ChainBlocks>(),
        std::slice::from_ref(&window)
    );
    assert!(cx.has_text("52,000"), "the Height stat: {:?}", cx.texts());
    // back from a long sleep, the head is far past the window: still one
    // window's worth, not every block since
    *tip.borrow_mut() = 104_000;
    heads.send(Head {
        height: 104_000,
        time: T0 + 104_000_000,
        id: [4; 32],
    });
    cx.run_until_parked();
    assert_eq!(
        cx.host().requests::<ChainBlocks>(),
        [window.clone(), window]
    );
    assert!(cx.has_text("104,000"), "the Height stat: {:?}", cx.texts());
}

/// A head alone draws nothing: the screen moves once, when the page it
/// brings lands. (Drawn per head item, the census spent 74M fuel on a
/// redraw of the Transactions page that changed nothing, every block.)
#[test]
fn a_pushed_head_is_drawn_with_the_page_it_brings_not_before() {
    let mut cx = TestAppContext::new();
    let heads = cx.host().stream::<ChainHeads>();
    let tip = Rc::new(RefCell::new(12));
    node(&mut cx, tip.clone());
    cx.open::<Explorer>();
    cx.run_until_parked();
    assert!(cx.has_text("12"), "the Height stat: {:?}", cx.texts());
    // the page the head asks for never comes: nothing on screen may move
    cx.host().never::<ChainBlocks>();
    heads.send(Head {
        height: 40,
        time: T0 + 40_000,
        id: [40; 32],
    });
    cx.run_until_parked();
    assert_eq!(
        cx.host().requests::<ChainBlocks>().len(),
        2,
        "the head is pulled"
    );
    assert!(cx.has_text("12") && !cx.has_text("40"), "{:?}", cx.texts());
}

/// The node's tip runs on while a head's page is asked (a block a second
/// on garage), so the page read down from it stops short of the window.
/// The phone's Overview keeps its rows on every frame while the view reads
/// on to the window's top, and the window lands once, whole. (It was
/// started again from the one block: on the owner's iPhone the Latest
/// activity list fell to one row and filled again, every block.)
#[test]
fn a_tip_past_the_pushed_head_keeps_the_overviews_rows_on_every_frame() {
    let mut cx = TestAppContext::new();
    let heads = cx.host().stream::<ChainHeads>();
    let tip = Rc::new(RefCell::new(12));
    node(&mut cx, tip.clone());
    let explorer = cx.open::<Explorer>();
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
    let rows = [
        "explorer-block-12",
        "explorer-block-11",
        "explorer-tx-12-0",
        "explorer-tx-11-1",
        "explorer-tx-11-0",
    ];
    assert!(
        rows.iter().all(|row| cx.find(row).is_some()),
        "{:?}",
        cx.texts()
    );
    let asked = cx.host().requests::<ChainBlocks>().len();
    // the head says 13; the node is at 14 by the time its page is read
    *tip.borrow_mut() = 14;
    heads.send(Head {
        height: 13,
        time: T0 + 13_000,
        id: [113; 32],
    });
    for tick in 0..6 {
        cx.tick(Vec::new());
        for row in rows {
            assert!(
                cx.find(row).is_some(),
                "tick {tick}: {row} left the frame: {:?}",
                cx.texts()
            );
        }
        let held = cx.update(&explorer, |view, _, _| view.chain.blocks.len());
        assert!(held >= 13, "tick {tick}: the window fell to {held} blocks");
    }
    cx.run_until_parked();
    assert!(cx.has_text("13–14 · 2 empty blocks"), "{:?}", cx.texts());
    let heights = cx.update(&explorer, |view, _, _| {
        view.chain
            .blocks
            .iter()
            .map(|block| block.height)
            .collect::<Vec<_>>()
    });
    assert_eq!(heights, (0..=14).rev().collect::<Vec<_>>());
    let new: Vec<_> = cx.host().requests::<ChainBlocks>()[asked..]
        .iter()
        .map(|ask| (ask.before, ask.limit))
        .collect();
    assert_eq!(new, [(None, 1), (Some(14), 1)], "only the blocks it lacks");
}

/// A node that answers the read below with the same page again moves
/// nothing: the read stops after that one ask, the window stays drawn, and
/// the next head reads on as ever.
#[test]
fn a_read_below_that_reaches_no_lower_stops_and_keeps_the_window() {
    let mut cx = TestAppContext::new();
    let heads = cx.host().stream::<ChainHeads>();
    let tip = Rc::new(RefCell::new(12));
    node(&mut cx, tip.clone());
    let explorer = cx.open::<Explorer>();
    cx.run_until_parked();
    let asked = cx.host().requests::<ChainBlocks>().len();
    // block 14, whatever is asked
    cx.host().handle::<ChainBlocks>(|_| {
        let top = BlockPage {
            before: None,
            limit: 1,
            most: 0,
        };
        Ok(page(&chain(14), &top))
    });
    let head = |height: u64| Head {
        height,
        time: T0 + height * 1000,
        id: [(height as u8).wrapping_add(100); 32],
    };
    heads.send(head(13));
    cx.run_until_parked();
    let new: Vec<_> = cx.host().requests::<ChainBlocks>()[asked..]
        .iter()
        .map(|ask| (ask.before, ask.limit))
        .collect();
    assert_eq!(new, [(None, 1), (Some(14), 1)]);
    let heights = |cx: &mut TestAppContext| {
        cx.update(&explorer, |view, _, _| {
            view.chain
                .blocks
                .iter()
                .map(|block| block.height)
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(heights(&mut cx), (0..=12).rev().collect::<Vec<_>>());
    assert!(cx.find("explorer-tx-11-0").is_some(), "{:?}", cx.texts());
    // the node answers as it should again: the next head joins
    *tip.borrow_mut() = 14;
    serve(&cx, move || chain(*tip.borrow()));
    heads.send(head(14));
    cx.run_until_parked();
    assert_eq!(heights(&mut cx), (0..=14).rev().collect::<Vec<_>>());
}

#[test]
fn a_refused_window_says_why_and_retry_reads_again() {
    let mut cx = TestAppContext::new();
    let tip = Rc::new(RefCell::new(12));
    node(&mut cx, tip);
    cx.host()
        .refuse::<ChainBlocks>("not_found", "this node serves no blocks");
    cx.open::<Explorer>();
    cx.run_until_parked();
    assert!(cx.has_text("this node serves no blocks"));
    let chain_ = chain(12);
    cx.host()
        .handle::<ChainBlocks>(move |ask| Ok(page(&chain_, &ask)));
    cx.simulate_click("explorer-retry");
    cx.run_until_parked();
    assert!(cx.has_text("Latest activity"));
}

#[test]
fn a_snapshot_restores_without_reading_the_window_again() {
    let (cx, _) = ready();
    let bytes = cx.snapshot().unwrap();
    let mut restored = TestAppContext::new();
    quiet_host(&restored);
    restored.restore::<Explorer>(&bytes).unwrap();
    restored.run_until_parked();
    assert!(restored.has_text("6f6f6f6f…6f6f"), "{:?}", restored.texts());
    assert!(restored.find("explorer-block-12").is_some());
    assert!(restored.host().requests::<ChainBlocks>().is_empty());
    // the latest transactions are the window's: restored with it
    assert!(
        restored.find("explorer-tx-12-0").is_some(),
        "{:?}",
        restored.texts()
    );
    assert!(!restored.has_text("Reading transactions…"));
}

/// Every page over a full window stays inside the host's frame budgets, and
/// under a per-page regression guard on its bytes: the native proxy for a
/// render's fuel.
/// (Time is no proxy here: a debug build encodes the whole view
/// (`wire::encode`, MessagePack) around every update to catch a missed
/// `notify`.)
#[test]
fn a_full_window_renders_inside_the_frame_budget() {
    let mut cx = TestAppContext::new();
    heavy(&mut cx);
    cx.open::<Explorer>();
    cx.run_until_parked();
    let mut sizes = vec![("overview", cx.frame_bytes())];
    for tab in ["blocks", "transactions", "accounts", "programs"] {
        cx.simulate_click(&format!("explorer-tab-{tab}"));
        cx.run_until_parked();
        sizes.push((tab, cx.frame_bytes()));
    }
    cx.simulate_click("explorer-tab-transactions");
    cx.run_until_parked();
    assert!(cx.has_text("forge"), "{:?}", cx.texts());
    cx.simulate_click(&format!("explorer-tx-{WINDOW}-0"));
    cx.run_until_parked();
    assert!(cx.has_text("1048576 bytes · 50505050…5050"));
    assert_eq!(
        cx.host().requests::<ChainTx>(),
        [TxAsk {
            tx: TxRef::At {
                height: WINDOW as u64,
                position: 0
            },
            most: u32::MAX
        }],
        "only the opened push reads its payload"
    );
    sizes.push(("a 1 MB push", cx.frame_bytes()));
    // A regression guard, not a host limit: about 1.5x what each page drew
    // when measured. The host's own limits are the sanitize check inside
    // `frame_bytes`. Tighten when a page slims, raise only on purpose.
    const REGRESSION_GUARD: [(&str, usize); 6] = [
        ("overview", 11_000),
        ("blocks", 12_000),
        ("transactions", 15_000),
        ("accounts", 2_900),
        ("programs", 3_700),
        ("a 1 MB push", 4_000),
    ];
    for ((page, bytes), (_, guard)) in sizes.into_iter().zip(REGRESSION_GUARD) {
        assert!(
            bytes < guard,
            "{page} drew {bytes} bytes, over its guard {guard}"
        );
    }
}

/// The snapshot keeps each transaction's decoded op, not its payload: a
/// 1 MB push in the window does not ride along, and a restored row still
/// reads as its op.
#[test]
fn a_snapshot_keeps_ops_not_payloads() {
    let mut cx = TestAppContext::new();
    heavy(&mut cx);
    cx.open::<Explorer>();
    cx.run_until_parked();
    let bytes = cx.snapshot().unwrap();
    assert!(bytes.len() < 1 << 20, "a snapshot of {} bytes", bytes.len());
    let mut restored = TestAppContext::new();
    heavy(&mut restored);
    restored.restore::<Explorer>(&bytes).unwrap();
    restored.run_until_parked();
    restored.simulate_click("explorer-tab-transactions");
    restored.run_until_parked();
    restored.simulate_click(&format!("explorer-tx-{WINDOW}-0"));
    restored.run_until_parked();
    assert!(restored.has_text("Push · app"), "{:?}", restored.texts());
}

#[test]
fn runs_of_empty_blocks_fold_into_one_line_and_the_list_reaches_back() {
    let block = |height: u64, txs: usize| BlockRow {
        height,
        txs,
        ..BlockRow::default()
    };
    // newest first: 20 empty, 19 busy, 18 empty, 17 busy, 16..=3 empty, 2 busy
    let mut blocks = vec![block(20, 0), block(19, 2), block(18, 0), block(17, 1)];
    blocks.extend((3..=16).rev().map(|height| block(height, 0)));
    blocks.push(block(2, 1));
    let lines = crate::chain::lines(&blocks, 6);
    let shape: Vec<String> = lines
        .iter()
        .map(|line| match line {
            crate::chain::Line::Block(block) => block.height.to_string(),
            crate::chain::Line::Empty { newest, oldest } => format!("{oldest}-{newest}"),
        })
        .collect();
    // a lone empty block stays a row; a run folds; the fold lets six lines
    // reach back to block 2
    assert_eq!(shape, ["20", "19", "18", "17", "3-16", "2"]);
    assert_eq!(crate::chain::lines(&blocks, 3).len(), 3);
}
