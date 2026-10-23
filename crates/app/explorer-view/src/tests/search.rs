use super::*;

#[test]
fn search_finds_heights_hashes_accounts_and_programs() {
    let (mut cx, _) = ready();
    let search = |cx: &mut TestAppContext, text: &str| {
        cx.simulate_input("explorer-search", text);
        cx.simulate_submit("explorer-search");
        cx.run_until_parked();
    };
    search(&mut cx, &abi::hex(&[0xb2; 32]));
    assert!(cx.has_text("In block 12"), "{:?}", cx.texts());
    search(&mut cx, "#3");
    assert!(cx.has_text("account 3   User   1 device"));
    search(&mut cx, "chat");
    assert!(
        cx.find("explorer-program").is_some() && cx.has_text("reads and writes the clipboard"),
        "a program name opens its page: {:?}",
        cx.texts()
    );
    assert!(!cx.has_text("mystery"), "{:?}", cx.texts());
    search(&mut cx, &abi::hex(&[105; 32]));
    assert!(cx.has_text(&abi::hex(&[105; 32])), "{:?}", cx.texts());
    search(&mut cx, &abi::hex(&[0xee; 32]));
    assert!(
        cx.has_text("No block or transaction has this hash."),
        "{:?}",
        cx.texts()
    );
    let by_hash: Vec<_> = cx
        .host()
        .requests::<ChainTx>()
        .into_iter()
        .filter(|ask| matches!(ask.tx, TxRef::Hash(_)))
        .collect();
    assert_eq!(
        by_hash,
        [TxAsk {
            tx: TxRef::Hash([0xee; 32]),
            most: 0
        }]
    );
    search(&mut cx, "1,000");
    assert!(cx.has_text("No block 1,000"), "{:?}", cx.texts());
    assert_eq!(
        cx.host().requests::<ChainBlock>(),
        [summary(BlockRef::Id([0xee; 32]))]
    );
    assert!(cx.host().requests::<ChainFacts>().contains(&FactsAsk {
        before: Some(1001),
        limit: 1,
    }));
    search(&mut cx, "nobody");
    assert!(cx.has_text("Nothing here is called “nobody”."));
}

#[test]
fn a_transaction_past_the_window_is_found_by_its_hash() {
    let mut cx = TestAppContext::new();
    let tip = WINDOW as u64 + 100;
    node(&mut cx, Rc::new(RefCell::new(tip)));
    serve(&cx, move || {
        let mut blocks = chain(tip);
        for block in &mut blocks {
            block.id[..8].copy_from_slice(&block.height.to_le_bytes());
        }
        blocks
    });
    let routes = cx.host().stream::<HostRoute>();
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_input("explorer-search", &abi::hex(&[0xa1; 32]));
    cx.simulate_submit("explorer-search");
    cx.run_until_parked();
    assert!(
        cx.has_text("In block 11") && cx.has_text("hello there"),
        "{:?}",
        cx.texts()
    );
    cx.simulate_click("explorer-tab-overview");
    cx.run_until_parked();
    routes.send(format!("tx/{}", abi::hex(&[0xb2; 32])));
    cx.run_until_parked();
    assert!(cx.has_text("In block 12"), "{:?}", cx.texts());
    assert_eq!(
        cx.host().requests::<ChainBlock>(),
        [BlockRef::Id([0xa1; 32]), BlockRef::Id([0xb2; 32])].map(summary)
    );
}

#[test]
fn a_node_that_lists_from_a_later_block_says_so() {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(30)));
    cx.host().handle::<ChainFacts>(|ask| {
        let mut listed = view::rig::facts_page(&chain(30), &ask);
        listed.blocks.retain(|block| block.height >= 20);
        listed.listed_from = Some(20);
        Ok(listed)
    });
    cx.host().handle::<ChainBlock>(|_| Ok(None));
    cx.host().handle::<ChainTx>(|_| Ok(None));
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_click("explorer-tab-blocks");
    cx.run_until_parked();
    let unindexed = "History before block 20 is not indexed on this node.";
    assert!(cx.has_text(unindexed), "{:?}", cx.texts());
    let search = |cx: &mut TestAppContext, text: &str| {
        cx.simulate_input("explorer-search", text);
        cx.simulate_submit("explorer-search");
        cx.run_until_parked();
    };
    search(&mut cx, "12");
    assert!(cx.has_text("No block 12") && cx.has_text(unindexed));
    search(&mut cx, &abi::hex(&[0xb2; 32]));
    assert!(
        cx.has_text(&format!(
            "No block or transaction since block 20 has this hash. {unindexed}"
        )),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn the_search_field_holds_only_what_is_being_typed() {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(12)));
    let explorer = cx.open::<Explorer>();
    cx.run_until_parked();
    let field = |_: &TestAppContext| explorer.read(|view| view.search.text());
    cx.simulate_input("explorer-search", "11");
    cx.simulate_submit("explorer-search");
    cx.run_until_parked();
    assert!(
        cx.find("explorer-block-txs-heading").is_some(),
        "block 11 opened"
    );
    assert_eq!(field(&cx), "", "a search that lands clears the field");
    cx.simulate_input("explorer-search", "half typed");
    cx.simulate_click("explorer-next");
    cx.run_until_parked();
    assert_eq!(field(&cx), "", "prev/next clears it");
    cx.simulate_input("explorer-search", "half typed");
    cx.simulate_click("explorer-tab-accounts");
    cx.run_until_parked();
    assert_eq!(field(&cx), "", "a tab clears it");
    cx.simulate_input("explorer-search", "nobody");
    cx.simulate_submit("explorer-search");
    cx.run_until_parked();
    assert_eq!(field(&cx), "nobody", "a search that finds nothing keeps it");
}

#[test]
fn a_link_opens_the_page_it_names() {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(12)));
    let routes = cx.host().stream::<HostRoute>();
    cx.open::<Explorer>();
    cx.run_until_parked();
    let open = |cx: &mut TestAppContext, route: &str| {
        routes.send(route.to_string());
        cx.run_until_parked();
    };
    open(&mut cx, &format!("tx/{}", abi::hex(&[0xa1; 32])));
    assert!(cx.has_text("In block 11") && cx.has_text("hello there"));
    open(&mut cx, "block/12");
    assert!(cx.has_text(&abi::hex(&[112; 32])), "{:?}", cx.texts());
    open(&mut cx, &format!("block/{}", abi::hex(&[105; 32])));
    assert!(cx.has_text(&abi::hex(&[105; 32])), "a block by its hash");
    open(&mut cx, "account/3");
    assert!(cx.has_text("account 3   User   1 device"));
    open(&mut cx, "txs/chat");
    assert!(cx.has_text("Transactions · chat"));
    open(&mut cx, "program/chat");
    assert!(
        cx.find("explorer-program").is_some() && cx.has_text("reads and writes the clipboard"),
        "{:?}",
        cx.texts()
    );
    open(&mut cx, "tx/12/0");
    assert!(cx.has_text("In block 12") && cx.has_text("not a member"));
    open(&mut cx, &format!("tx/{}", abi::hex(&[0xee; 32])));
    assert!(cx.has_text("No block or transaction has this hash."));
    open(&mut cx, "nowhere/1");
    assert!(cx.has_text("This link names nothing the Explorer shows: nowhere/1"));
}

#[test]
fn a_page_copies_its_link_once_the_session_names_a_chain() {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(12)));
    let props = cx.host().stream::<HostSession>();
    let copied = Rc::new(RefCell::new(String::new()));
    let seen = copied.clone();
    cx.host().handle::<ClipboardWrite>(move |text| {
        *seen.borrow_mut() = text;
        Ok(())
    });
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_click("explorer-block-11");
    cx.run_until_parked();
    assert!(cx.find("explorer-copy-link").is_none(), "no chain, no link");
    props.send(Session {
        chain_id: "testkit#0a1b2c3d".into(),
        ..Session::default()
    });
    cx.run_until_parked();
    cx.simulate_click("explorer-copy-link");
    cx.run_until_parked();
    assert_eq!(
        *copied.borrow(),
        "duck://testkit-0a1b2c3d/explorer/block/11"
    );
    assert!(cx.has_text("Copied the link."));
}

#[test]
fn every_route_reads_back_from_its_path() {
    for route in [
        Route::Overview,
        Route::Blocks,
        Route::Block(0),
        Route::Block(6230),
        Route::Transactions(None),
        Route::Transactions(Some("chat".into())),
        Route::Tx {
            height: 11,
            position: 0,
        },
        Route::Tx {
            height: 0,
            position: u32::MAX,
        },
        Route::Accounts,
        Route::Account(3),
        Route::Programs,
        Route::Program("chat".into()),
    ] {
        assert_eq!(
            Route::from_path(&route.path()),
            Some(route.clone()),
            "{route:?}"
        );
    }
    for nothing in [
        "block/07",
        "block/x",
        "tx/zz",
        "tx/11",
        "tx/11/01",
        "tx/11/4294967296",
        "program/",
        "txs/",
        "account/-1",
        "blocks/1",
    ] {
        assert_eq!(Route::from_path(nothing), None, "{nothing}");
    }
}

fn summary(by: BlockRef) -> BlockAsk {
    BlockAsk { by, most: 0 }
}
