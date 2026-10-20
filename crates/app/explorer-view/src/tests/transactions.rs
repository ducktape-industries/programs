use super::*;

fn far_past_the_window() -> TestAppContext {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(WINDOW as u64 + 100)));
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_click("explorer-tab-transactions");
    cx.run_until_parked();
    cx
}

fn asks_for_frames(cx: &TestAppContext) -> bool {
    let asked = cx.host().requests::<Query<Activity>>();
    asked
        .iter()
        .any(|query| matches!(query, activity::Query::Frames { .. }))
}

#[test]
fn the_transactions_page_lists_the_windows_transactions_and_asks_activity_for_none() {
    let (mut cx, _) = ready();
    cx.simulate_click("explorer-tab-transactions");
    cx.run_until_parked();
    assert!(
        cx.has_text("3 transactions in the last 13 blocks"),
        "{:?}",
        cx.texts()
    );
    assert_eq!(
        name(&cx, "explorer-tx-12-0").as_deref(),
        Some("mystery, Rejected, block 12 · 0")
    );
    assert!(cx.find("explorer-tx-11-1").is_some() && cx.find("explorer-tx-11-0").is_some());
    assert!(
        !cx.has_text(crate::words::OLDER_TRANSACTIONS),
        "the window reaches the chain's start"
    );
    assert!(!asks_for_frames(&cx));
    assert!(
        cx.host().requests::<ChainTx>().is_empty(),
        "no transaction is read to list it"
    );
    assert!(cx.host().requests::<ChainBlock>().is_empty());
}

#[test]
fn a_transaction_older_than_the_window_is_not_listed_and_the_page_says_where_it_is() {
    let cx = far_past_the_window();
    assert!(
        cx.has_text("No transactions in the last 1,000 blocks."),
        "{:?}",
        cx.texts()
    );
    assert!(
        cx.has_text(crate::words::OLDER_TRANSACTIONS),
        "{:?}",
        cx.texts()
    );
    assert!(cx.find("explorer-tx-11-0").is_none());
    assert!(!asks_for_frames(&cx));
}

#[test]
fn a_transaction_past_the_window_opens_by_its_place_and_reads_its_payload_alone() {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(WINDOW as u64 + 100)));
    let routes = cx.host().stream::<HostRoute>();
    cx.open::<Explorer>();
    cx.run_until_parked();
    routes.send("tx/11/0".into());
    cx.run_until_parked();
    assert!(
        cx.has_text("In block 11") && cx.has_text("hello there"),
        "{:?}",
        cx.texts()
    );
    assert_eq!(
        cx.host().requests::<ChainTx>(),
        [TxAsk {
            tx: TxRef::At {
                height: 11,
                position: 0
            },
            most: u32::MAX
        }]
    );
    assert!(cx.host().requests::<ChainBlock>().is_empty());
}

#[test]
fn a_transaction_in_the_window_opens_from_the_window_and_reads_only_its_payload() {
    let (mut cx, _) = ready();
    cx.simulate_click("explorer-tab-transactions");
    cx.run_until_parked();
    cx.simulate_click("explorer-tx-11-1");
    cx.run_until_parked();
    assert!(cx.has_text("Direct message"), "{:?}", cx.texts());
    assert_eq!(
        cx.host().requests::<ChainTx>(),
        [TxAsk {
            tx: TxRef::At {
                height: 11,
                position: 1
            },
            most: u32::MAX
        }]
    );
    assert!(cx.host().requests::<ChainBlock>().is_empty());
}

#[test]
fn a_place_that_holds_no_transaction_says_so() {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(12)));
    let routes = cx.host().stream::<HostRoute>();
    cx.open::<Explorer>();
    cx.run_until_parked();
    routes.send("tx/12/5".into());
    cx.run_until_parked();
    assert!(
        cx.has_text("Transaction not found")
            && cx.has_text("This node keeps no transaction at position 5 of block 12."),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn without_the_activity_program_the_transactions_still_list() {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(12)));
    cx.host()
        .refuse::<Query<Activity>>("not_found", "no program is called activity");
    cx.open::<Explorer>();
    cx.run_until_parked();
    assert!(
        cx.has_text("Latest transactions") && cx.find("explorer-tx-12-0").is_some(),
        "{:?}",
        cx.texts()
    );
    cx.simulate_click("explorer-tab-transactions");
    cx.run_until_parked();
    assert!(
        cx.has_text("3 transactions in the last 13 blocks"),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn a_programs_transactions_are_the_windows_sent_to_it() {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(12)));
    let routes = cx.host().stream::<HostRoute>();
    cx.open::<Explorer>();
    cx.run_until_parked();
    routes.send("txs/chat".into());
    cx.run_until_parked();
    assert!(
        cx.has_text("Transactions · chat") && cx.has_text("2 transactions in the last 13 blocks"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.find("explorer-tx-11-0").is_some() && cx.find("explorer-tx-12-0").is_none());
    assert!(!asks_for_frames(&cx));
}

fn below(cursor: Vec<u8>) -> u64 {
    u64::MAX - u64::from_be_bytes(cursor[..8].try_into().expect("a key"))
}

fn recorded(height: u64, to: &str) -> Block {
    Block {
        height,
        id: [0; 32],
        parent: [0; 32],
        time: T0 + height * 1000,
        epoch: 0,
        proposer: None,
        txs: vec![tx(height as u8, ADA, to, Vec::new())],
    }
}

fn long_activity(
    frames: u64,
    to: fn(u64) -> &'static str,
) -> (TestAppContext, Rc<RefCell<Vec<Block>>>) {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(12)));
    let blocks = (1..=frames).map(|height| recorded(height, to(height)));
    let blocks = Rc::new(RefCell::new(blocks.collect::<Vec<_>>()));
    let held = blocks.clone();
    index(&cx, move || held.borrow().clone());
    (cx, blocks)
}

fn writes(cx: &TestAppContext) -> impl Fn(&mut TestAppContext) + use<> {
    let writes = cx.host().stream::<Changes<Activity>>();
    move |cx: &mut TestAppContext| {
        writes.send(Some(view::methods::Change {
            height: 5_000,
            keys: Vec::new(),
        }));
        cx.run_until_parked();
    }
}

fn frame(height: u64) -> String {
    format!("explorer-tx-{height}-0")
}

/// An account's page asks activity for its newest page alone; Show older
/// reads the one past it, and a transaction of its landing above them
/// shows as newer until Show newest reads them anew.
#[test]
fn an_account_page_asks_for_its_newest_page() {
    let (mut cx, blocks) = long_activity(5_000, |_| "chat");
    let write = writes(&cx);
    let routes = cx.host().stream::<HostRoute>();
    cx.open::<Explorer>();
    cx.run_until_parked();
    routes.send("account/3".into());
    cx.run_until_parked();
    let acted = |cx: &TestAppContext| {
        cx.host()
            .requests::<Query<Activity>>()
            .into_iter()
            .filter_map(|query| match query {
                activity::Query::Acted { account, page } => {
                    assert_eq!((account, page.limit), (3, Some(crate::queries::PAGE)));
                    Some(page.after.map(below))
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(acted(&cx), [None]);
    assert!(
        cx.has_text("5,000 transactions recorded"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.find(&frame(4_951)).is_some() && cx.find(&frame(4_950)).is_none());
    cx.simulate_click("explorer-activity-older");
    cx.run_until_parked();
    assert_eq!(acted(&cx), [None, Some(4_951)]);
    assert!(cx.find(&frame(4_901)).is_some(), "{:?}", cx.texts());
    blocks.borrow_mut().push(recorded(5_001, "chat"));
    write(&mut cx);
    assert_eq!(acted(&cx), [None, Some(4_951), None]);
    assert!(cx.has_text("Newer transactions"), "{:?}", cx.texts());
    cx.simulate_click("explorer-show-newest");
    cx.run_until_parked();
    assert_eq!(acted(&cx)[3..], [None]);
    assert!(cx.find(&frame(5_001)).is_some() && !cx.has_text("Newer transactions"));
}
