use super::*;

#[test]
fn the_overview_shows_the_head_and_the_latest_blocks_and_transactions() {
    let (cx, _) = ready();
    let texts = cx.texts();
    assert!(
        cx.has_text("Height") && cx.has_text("Block every 1.0 s"),
        "{texts:?}"
    );
    assert!(cx.has_text("Next in 7 blocks"), "{texts:?}");
    assert!(cx.has_text("Validators") && cx.has_text("Accounts"));
    assert!(!cx.has_text("Transactions ") && !texts.iter().any(|t| t.contains("tx count")));
    assert!(cx.has_text("Latest activity") && cx.has_text("Latest transactions"));
    assert!(cx.has_text("chat") && cx.has_text("Ada") && cx.has_text("#3"));
    assert!(cx.has_text("mystery") && cx.has_text("02020202…0202"));
    assert!(
        cx.host().requests::<ChainTx>().is_empty(),
        "the latest transactions are the window's"
    );
    // the page tabs sit in a named tab list
    let Some(view::wire::Node::Container(bar)) = cx.find("explorer-tabs") else {
        panic!("a tab list");
    };
    assert_eq!(
        (
            bar.interactivity.as_ref().unwrap().role,
            bar.interactivity.as_ref().unwrap().aria.label.as_deref()
        ),
        (Some(view::Role::TabList), Some("Pages"))
    );
    assert!(
        cx.has_text("6f6f6f6f…6f6f"),
        "block 11's hash, shortened: {texts:?}"
    );
    // blocks 0–10 carry nothing: one quiet line, not eleven rows
    assert!(cx.has_text("0–10 · 11 empty blocks"), "{texts:?}");
    assert!(
        !cx.has_text("6c6c6c6c…6c6c"),
        "block 8 is folded: {texts:?}"
    );
    assert_eq!(
        cx.host().requests::<ChainBlocks>(),
        vec![BlockPage {
            before: None,
            limit: WINDOW as u32,
            most: 0,
        }],
        "13 blocks is the whole archive: one page"
    );
}

/// The pages are one tab list: one Tab stop, and → opens the next page
/// (automatic activation), wrapping from the last to the first.
#[test]
fn an_arrow_on_the_pages_opens_the_next_page() {
    let (mut cx, _) = ready();
    let tabs = cx.interactivity("explorer-tabs");
    assert_eq!(tabs.role, Some(view::Role::TabList));
    assert!(tabs.focusable && tabs.tab_stop == Some(true));
    assert!(!cx.interactivity("explorer-tab-blocks").focusable);
    cx.simulate_focus("explorer-tabs");
    cx.simulate_key_down("explorer-tabs", "right");
    cx.run_until_parked();
    assert!(cx.find("explorer-blocks").is_some(), "{:?}", cx.texts());
    let blocks = cx.interactivity("explorer-tab-blocks");
    assert_eq!(blocks.aria.selected, Some(true));
    assert!(blocks.aria.active_descendant);
    assert!(
        !cx.interactivity("explorer-tab-overview")
            .aria
            .active_descendant
    );
    cx.simulate_key_down("explorer-tabs", "left");
    cx.simulate_key_down("explorer-tabs", "left");
    cx.run_until_parked();
    assert!(cx.find("explorer-list").is_some(), "the Programs page");
    assert!(
        cx.interactivity("explorer-tab-programs")
            .aria
            .active_descendant
    );
}

/// → then ← on the pages comes back to the block the reader left, not to
/// the Blocks list: an arrow opens a tab's page as it was left (the door's
/// arrow probe in the census did → ← here and audited the list instead).
/// A click on a tab still opens its list.
#[test]
fn an_arrow_away_and_back_returns_to_the_block_it_left() {
    let (mut cx, _) = ready();
    cx.simulate_click("explorer-block-11");
    cx.run_until_parked();
    assert!(cx.find("explorer-block").is_some(), "{:?}", cx.texts());
    cx.simulate_focus("explorer-tabs");
    cx.simulate_key_down("explorer-tabs", "right");
    cx.run_until_parked();
    assert!(
        cx.find("explorer-transactions").is_some(),
        "{:?}",
        cx.texts()
    );
    cx.simulate_key_down("explorer-tabs", "left");
    cx.run_until_parked();
    assert!(
        cx.find("explorer-block").is_some() && cx.find("explorer-blocks").is_none(),
        "{:?}",
        cx.texts()
    );
    assert!(cx.has_text(&abi::hex(&[111; 32])), "block 11's hash");
    assert!(
        cx.interactivity("explorer-tab-blocks")
            .aria
            .active_descendant
    );
    assert!(
        cx.host().requests::<ChainBlock>().is_empty(),
        "block 11 is in the window"
    );
    cx.simulate_click("explorer-tab-blocks");
    cx.run_until_parked();
    assert!(cx.find("explorer-blocks").is_some(), "{:?}", cx.texts());
}

/// Enter or Space on the pages opens the active tab's list, as a click on
/// the tab does: with an account open, an arrow away and back opens that
/// account again, and no link on any page leads to the Accounts list, so
/// without the press the keys never reach the list again.
#[test]
fn enter_on_the_accounts_tab_shows_the_accounts_list_again() {
    let (mut cx, _) = ready();
    cx.simulate_click("explorer-tab-accounts");
    cx.run_until_parked();
    cx.simulate_focus("explorer-accounts-list");
    cx.simulate_key_down("explorer-accounts-list", "down");
    cx.simulate_key_down("explorer-accounts-list", "enter");
    cx.run_until_parked();
    assert!(cx.find("explorer-account").is_some(), "{:?}", cx.texts());
    // the list left with its focus: Tab reaches the tabs
    cx.simulate_focus("explorer-tabs");
    cx.simulate_key_down("explorer-tabs", "right");
    cx.run_until_parked();
    assert!(cx.find("explorer-list").is_some(), "{:?}", cx.texts());
    cx.simulate_key_down("explorer-tabs", "left");
    cx.run_until_parked();
    assert!(
        cx.find("explorer-account").is_some() && cx.find("explorer-accounts").is_none(),
        "the arrow opens the account it left: {:?}",
        cx.texts()
    );
    cx.simulate_key_down("explorer-tabs", "enter");
    cx.run_until_parked();
    assert!(
        cx.find("explorer-accounts-list").is_some() && cx.find("explorer-account").is_none(),
        "Enter on the Accounts tab opens the list: {:?}",
        cx.texts()
    );
    assert!(
        cx.interactivity("explorer-tab-accounts")
            .aria
            .active_descendant
    );
    // the list is the tab's page now: an arrow away and back stays on it
    cx.simulate_key_down("explorer-tabs", "right");
    cx.simulate_key_down("explorer-tabs", "left");
    cx.run_until_parked();
    assert!(
        cx.find("explorer-accounts-list").is_some(),
        "{:?}",
        cx.texts()
    );
    // Space is the same press
    cx.simulate_focus("explorer-accounts-list");
    cx.simulate_key_down("explorer-accounts-list", "enter");
    cx.run_until_parked();
    assert!(cx.find("explorer-account").is_some(), "{:?}", cx.texts());
    cx.simulate_focus("explorer-tabs");
    cx.simulate_key_down("explorer-tabs", "space");
    cx.run_until_parked();
    assert!(
        cx.find("explorer-accounts-list").is_some() && cx.find("explorer-account").is_none(),
        "Space on the Accounts tab opens the list: {:?}",
        cx.texts()
    );
}

/// A restored view keeps each tab's last page along with the page shown:
/// → then ← after a restore still comes back to the block, not to the
/// Blocks list.
#[test]
fn after_a_restore_an_arrow_away_and_back_returns_to_the_block() {
    let (mut cx, _) = ready();
    cx.simulate_click("explorer-block-11");
    cx.run_until_parked();
    let bytes = cx.snapshot().unwrap();
    let mut restored = TestAppContext::new();
    node(&mut restored, Rc::new(RefCell::new(12)));
    restored.restore::<Explorer>(&bytes).unwrap();
    restored.run_until_parked();
    assert!(
        restored.find("explorer-block").is_some(),
        "{:?}",
        restored.texts()
    );
    restored.simulate_focus("explorer-tabs");
    restored.simulate_key_down("explorer-tabs", "right");
    restored.simulate_key_down("explorer-tabs", "left");
    assert!(
        restored.find("explorer-block").is_some() && restored.find("explorer-blocks").is_none(),
        "{:?}",
        restored.texts()
    );
    assert!(restored.has_text(&abi::hex(&[111; 32])), "block 11's hash");
}

/// A list is one Tab stop: ↓ moves the active row, Enter opens it; the
/// rows are options, never focusable, and the first is active on entry.
#[test]
fn an_arrow_and_enter_on_the_accounts_opens_the_second_account() {
    let (mut cx, _) = ready();
    cx.simulate_click("explorer-tab-accounts");
    cx.run_until_parked();
    let list = cx.interactivity("explorer-accounts-list");
    assert_eq!(list.role, Some(view::Role::ListBox));
    assert_eq!(list.aria.label.as_deref(), Some("Accounts"));
    assert!(list.focusable && list.tab_stop == Some(true));
    let rows: Vec<String> = cx
        .find("explorer-accounts-list-rows")
        .expect("the list")
        .children()
        .iter()
        .filter_map(|row| row.key().map(str::to_owned))
        .collect();
    assert!(rows.len() >= 2, "{rows:?}");
    let first = cx.interactivity(&rows[0]);
    assert_eq!(first.role, Some(view::Role::ListBoxOption));
    assert!(!first.focusable && first.aria.active_descendant);
    assert_eq!(first.aria.selected, Some(false));
    cx.simulate_focus("explorer-accounts-list");
    cx.simulate_key_down("explorer-accounts-list", "down");
    assert!(!cx.interactivity(&rows[0]).aria.active_descendant);
    assert!(cx.interactivity(&rows[1]).aria.active_descendant);
    cx.simulate_key_down("explorer-accounts-list", "enter");
    cx.run_until_parked();
    let number = rows[1].trim_start_matches("explorer-account-");
    assert!(cx.find("explorer-account").is_some(), "{:?}", cx.texts());
    let header = format!("account {number} ");
    assert!(
        cx.texts().iter().any(|text| text.starts_with(&header)),
        "{:?}",
        cx.texts()
    );
}

/// "All blocks →" and "All transactions →" are at least 24 px each way:
/// the box's own floor, so the bounds the door's AX-017 reads cannot come
/// in under it, whatever the text inside.
#[test]
fn the_overview_links_are_at_least_24_px_each_way() {
    // a style's length in pixels, as gpui prints it ("24px")
    fn pixels(length: Option<impl std::fmt::Display>) -> Option<f32> {
        length?.to_string().strip_suffix("px")?.parse().ok()
    }
    let (cx, _) = ready();
    for key in ["explorer-all-blocks", "explorer-all-txs"] {
        let floor = &cx.style(key).min_size;
        let (width, height) = (pixels(floor.width), pixels(floor.height));
        assert!(
            width >= Some(24.) && height >= Some(24.),
            "{key}: {width:?} × {height:?}"
        );
    }
}

#[test]
fn a_block_opens_with_its_fields_its_proposer_and_its_transactions() {
    let (mut cx, _) = ready();
    cx.simulate_click("explorer-block-11");
    cx.run_until_parked();
    let texts = cx.texts();
    assert!(
        cx.has_text("11") && cx.has_text(&abi::hex(&[111; 32])),
        "{texts:?}"
    );
    assert!(cx.has_text("validator 1"), "{texts:?}");
    assert_eq!(
        name(&cx, "explorer-tx-11-0").as_deref(),
        Some("chat, Accepted, block 11 · 0")
    );
    assert_eq!(
        name(&cx, "explorer-tx-11-1").as_deref(),
        Some("chat, block 11 · 1")
    );
    assert!(cx.has_text("Ada") && cx.has_text("11 · 1"), "{texts:?}");
    assert!(!cx.has_text("Post in #design"), "{texts:?}");
    assert!(
        !texts.iter().any(|t| t.contains("Applied")),
        "no receipts: {texts:?}"
    );
    assert!(!texts.iter().any(|t| t.contains("State root")), "{texts:?}");
    cx.simulate_click("explorer-next");
    cx.run_until_parked();
    assert_eq!(
        name(&cx, "explorer-tx-12-0").as_deref(),
        Some("mystery, Rejected, block 12 · 0")
    );
    assert!(!cx.has_text("mystery · 4 bytes"), "{:?}", cx.texts());
    assert!(
        cx.host().requests::<ChainBlock>().is_empty(),
        "both were in the window"
    );
    assert!(cx.host().requests::<ChainTx>().is_empty());
    assert!(cx.host().requests::<ProgramDescribe>().is_empty());
}

#[test]
fn a_transaction_shows_its_block_anchor_signer_and_operation() {
    let (mut cx, _) = ready();
    cx.simulate_click("explorer-tx-11-0");
    cx.run_until_parked();
    let texts = cx.texts();
    assert!(cx.has_text("In block 11"), "{texts:?}");
    assert!(cx.has_text("Block 10"), "{texts:?}");
    assert!(cx.has_text(&abi::hex(&[0xa1; 32])));
    assert!(
        cx.has_text("#3 laptop · ed25519 01010101…0101"),
        "{texts:?}"
    );
    assert!(cx.has_text("code abababab…abab"), "{texts:?}");
    assert!(cx.has_text("channel") && cx.has_text("#design"));
    assert!(cx.has_text("text") && cx.has_text("hello there"));
    assert!(cx.has_text("Accepted"), "{texts:?}");
    // the message it emitted, and that one's refused message under it
    assert!(cx.has_text("Messages") && cx.has_text("notify") && cx.has_text("mail"));
    assert!(
        cx.has_text("Rejected") && cx.has_text("the inbox is full") && cx.has_text("capacity"),
        "{texts:?}"
    );
    cx.simulate_click("explorer-from");
    cx.run_until_parked();
    assert!(cx.has_text("2 transactions recorded"), "{:?}", cx.texts());
}

/// The desktop has no Back: however many pages the reader goes through, it
/// keeps only the one shown over its list (the phone stacks them for
/// Back).
#[test]
fn the_desktop_keeps_only_the_page_shown_over_its_list() {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(12)));
    let explorer = cx.open::<Explorer>();
    cx.run_until_parked();
    let pages = || {
        explorer.read(|view| {
            let pages = &view.pages;
            (pages.depth(), pages.first().clone(), pages.top().clone())
        })
    };
    cx.simulate_click("explorer-tx-11-0");
    cx.run_until_parked();
    cx.simulate_click("explorer-from");
    cx.run_until_parked();
    assert!(cx.has_text("2 transactions recorded"), "{:?}", cx.texts());
    // one page over the root, not the transaction under the account
    assert_eq!(pages(), (1, Route::Overview, Route::Account(3)));
}

#[test]
fn a_rejected_transaction_says_so_and_why() {
    let (mut cx, _) = ready();
    assert!(
        cx.find("explorer-tx-mark-12-0").is_some(),
        "its row marks it"
    );
    cx.simulate_click("explorer-tx-12-0");
    cx.run_until_parked();
    let texts = cx.texts();
    assert!(
        cx.has_text("Rejected") && cx.has_text("not a member") && cx.has_text("unauthorized"),
        "{texts:?}"
    );
    assert!(
        !cx.has_text("Accepted") && !cx.has_text("Messages"),
        "{texts:?}"
    );
}

#[test]
fn a_transaction_without_a_receipt_shows_no_outcome() {
    let (mut cx, _) = ready();
    cx.simulate_click("explorer-block-11");
    cx.run_until_parked();
    assert!(cx.find("explorer-tx-mark-11-1").is_none());
    assert!(cx.find("explorer-tx-mark-11-0").is_some());
    cx.simulate_click("explorer-tx-11-1");
    cx.run_until_parked();
    let texts = cx.texts();
    assert!(cx.has_text("ping"), "{texts:?}");
    assert!(
        !texts
            .iter()
            .any(|t| t == "Status" || t == "Accepted" || t == "Rejected"),
        "{texts:?}"
    );
}

#[test]
fn two_like_transactions_are_named_apart_by_their_place() {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(12)));
    let push = |seed| {
        let op = forge::Op::Push {
            repo: "big-history".into(),
            request: vec![1],
            pack: None,
        };
        Tx {
            receipt: Some(receipt(forge::PROGRAM, None, Vec::new())),
            ..tx(seed, ADA, forge::PROGRAM, borsh::to_vec(&op).unwrap())
        }
    };
    let mut blocks = chain(12);
    blocks[12].txs = vec![push(0xd4), push(0xe5)];
    serve(&cx, move || blocks.clone());
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_click("explorer-block-12");
    cx.run_until_parked();
    assert_eq!(
        name(&cx, "explorer-tx-12-0").as_deref(),
        Some("forge, Accepted, block 12 · 0")
    );
    assert_eq!(
        name(&cx, "explorer-tx-12-1").as_deref(),
        Some("forge, Accepted, block 12 · 1")
    );
}

/// The node lands the same frame again (the same bytes, so the same hash,
/// in a later block or twice in one): each row is still its own element,
/// so the host takes the frame instead of refusing it (a refused frame
/// ends the view: "duplicate typed element identity among siblings").
#[test]
fn a_frame_landed_again_is_a_row_of_its_own() {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(12)));
    let mut blocks = chain(12);
    let again = blocks[12].txs[0].clone();
    blocks[11].txs.push(again.clone());
    blocks[12].txs.push(again);
    let indexed = blocks.clone();
    index(&cx, move || indexed.clone());
    serve(&cx, move || blocks.clone());
    cx.open::<Explorer>();
    cx.run_until_parked();
    for id in ["explorer-tx-12-0", "explorer-tx-11-2", "explorer-tx-12-1"] {
        assert!(cx.find(id).is_some(), "{id}: {:?}", cx.texts());
    }
    cx.simulate_click("explorer-block-12");
    cx.run_until_parked();
    assert_eq!(
        name(&cx, "explorer-tx-12-0").as_deref(),
        Some("mystery, Rejected, block 12 · 0")
    );
    assert_eq!(
        name(&cx, "explorer-tx-12-1").as_deref(),
        Some("mystery, Rejected, block 12 · 1")
    );
    cx.simulate_click("explorer-previous");
    cx.run_until_parked();
    assert_eq!(
        name(&cx, "explorer-tx-11-2").as_deref(),
        Some("mystery, Rejected, block 11 · 2")
    );
}

#[test]
fn an_account_shows_its_devices_and_what_it_used() {
    let (mut cx, _) = ready();
    cx.simulate_input("explorer-search", "ada");
    cx.simulate_submit("explorer-search");
    cx.run_until_parked();
    let texts = cx.texts();
    assert!(cx.has_text("account 3   User   1 device"), "{texts:?}");
    assert!(
        cx.has_text("laptop") && cx.has_text("last used 1s ago"),
        "{texts:?}"
    );
    assert!(cx.has_text("Programs used") && cx.has_text("2 tx"));
    assert!(!cx.has_text("mystery"), "not Ada's");
}

#[test]
fn programs_lists_what_runs_and_what_is_scheduled() {
    let (mut cx, _) = ready();
    cx.simulate_click("explorer-tab-programs");
    cx.run_until_parked();
    assert!(cx.has_text("2 programs") && cx.has_text("identity"));
    assert!(cx.has_text("Remove") && cx.has_text("forge") && cx.has_text("at 120"));
    assert!(cx.texts().iter().any(|text| text == "abababab…abab"));
    assert!(
        cx.has_text("1 view") && cx.has_text("explorer") && cx.has_text("view only"),
        "a view-only entry is listed beside the programs"
    );
}

#[test]
fn the_scheduled_changes_survive_a_snapshot() {
    let (mut cx, _) = ready();
    cx.simulate_click("explorer-tab-programs");
    cx.run_until_parked();
    let bytes = cx.snapshot().unwrap();
    let mut restored = TestAppContext::new();
    quiet_host(&restored);
    restored.restore::<Explorer>(&bytes).unwrap();
    restored.run_until_parked();
    assert!(restored.has_text("Remove") && restored.has_text("at 120"));
}

/// Narrower than the two panels' minimums together (a 960 px view, less the
/// scroll bar), the latest transactions go under the latest blocks: a
/// column, each panel the page's width and its own content's height, so no
/// stretched gap and no rule left hanging. From 960 they sit side by side as
/// before, the transactions never narrower than they are alone at the view's
/// narrowest; narrower, their titles went to nothing.
#[test]
fn the_latest_panels_stack_where_the_transactions_would_squeeze() {
    use view::{StyleRefinement, Styled as _, design, px};
    let (mut cx, _) = ready();
    let style = |cx: &TestAppContext, id: &str| -> StyleRefinement { cx.style(id).clone() };
    let column = StyleRefinement::default().flex_col().flex_direction;
    let alone = px(<Explorer as View>::MIN_WINDOW_WIDTH as f32) - design::size::SCROLLBAR;
    let breakpoint = f32::from(px(320.) + alone + design::size::SCROLLBAR);
    assert_eq!(breakpoint, 960.);
    for width in [640., 900., breakpoint - 1.] {
        cx.simulate_resize(width, 760.);
        cx.run_until_parked();
        assert_eq!(
            style(&cx, "explorer-latest").flex_direction,
            column,
            "{width}"
        );
        assert_eq!(style(&cx, "explorer-latest").flex_grow, None, "{width}");
        for panel in ["explorer-latest-blocks", "explorer-latest-txs"] {
            let style = style(&cx, panel);
            // the page's width: stretched across the column, nothing caps it
            assert_eq!(
                (style.min_size.width, style.max_size.width),
                (None, None),
                "{panel} at {width}"
            );
            // its own height: nothing grows it down the column
            assert_eq!(style.flex_grow, None, "{panel} at {width}");
            assert_eq!(style.border_widths.right, None, "{panel} at {width}");
        }
    }
    for width in [breakpoint, 1200.] {
        cx.simulate_resize(width, 760.);
        cx.run_until_parked();
        let row = style(&cx, "explorer-latest");
        assert_eq!(
            (row.flex_direction, row.flex_grow),
            (None, Some(1.)),
            "{width}"
        );
        let blocks = style(&cx, "explorer-latest-blocks");
        assert_eq!(
            (
                blocks.flex_grow,
                blocks.min_size.width,
                blocks.max_size.width,
                blocks.border_widths.right
            ),
            (
                Some(1.),
                Some(px(320.).into()),
                Some(px(420.).into()),
                Some(px(1.).into())
            ),
            "{width}"
        );
        let txs = style(&cx, "explorer-latest-txs");
        assert_eq!(
            (txs.flex_grow, txs.min_size.width),
            (Some(1.), Some(alone.into())),
            "{width}"
        );
    }
}

#[test]
fn the_root_tracks_the_shared_theme() {
    let (mut cx, _) = ready();
    let dark = view::Theme::dark();
    cx.set_global(dark);
    let style = cx.style("explorer");
    assert_eq!(
        style
            .background
            .as_ref()
            .and_then(|fill| fill.color())
            .and_then(|background| background.as_solid()),
        Some(dark.background)
    );
}

#[test]
fn accounts_say_what_each_is_as_members_does() {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(12)));
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_click("explorer-tab-accounts");
    cx.run_until_parked();
    for text in [
        "User",
        "Agent · managed by Ada · suspended",
        "Program · forge",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    cx.simulate_click("explorer-account-5");
    cx.run_until_parked();
    assert!(
        cx.has_text("account 5   Agent · managed by Ada · suspended   0 devices"),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn the_transactions_list_lowers_only_the_rows_the_host_shows() {
    use view::wire::Node;
    use view::wire::list::UniformListScrollStrategy as Strategy;
    let mut cx = TestAppContext::new();
    heavy(&mut cx);
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_click("explorer-tab-transactions");
    cx.run_until_parked();
    // row `index` is the transaction of block `WINDOW - index`
    let row = |index: usize| format!("explorer-tx-{}-0", WINDOW - index);
    let list = |cx: &TestAppContext| match cx.find("explorer-transactions-list-rows") {
        Some(Node::UniformList {
            count,
            children,
            scroll_request,
            ..
        }) => (*count, children.len(), *scroll_request),
        other => panic!("no list of rows: {other:?}"),
    };
    let (count, lowered, _) = list(&cx);
    assert_eq!(count, WINDOW, "every transaction the window holds");
    assert!(
        (14..count).contains(&lowered),
        "a screenful and a margin, not every row: {lowered}"
    );
    assert!(cx.find(&row(0)).is_some() && cx.find(&row(49)).is_none());
    let first = cx.interactivity(&row(0));
    assert_eq!(first.role, Some(view::Role::ListBoxOption));
    assert!(first.aria.active_descendant && !first.focusable);

    // the host scrolled to rows 30..44: they come with a margin, the rows
    // far above go, the measured row stays
    cx.simulate_range("explorer-transactions-list", 30..44);
    assert!(cx.find(&row(40)).is_some() && cx.find(&row(49)).is_some());
    assert!(cx.find(&row(5)).is_none(), "{:?}", list(&cx));
    assert!(cx.find(&row(0)).is_some(), "the row the host measures");

    // ↓ on the list: the second row is active, and the frame that claims
    // it carries the scroll to it (the request crosses once)
    cx.simulate_focus("explorer-transactions-list");
    cx.simulate_key_down("explorer-transactions-list", "down");
    let (_, _, request) = list(&cx);
    assert_eq!(
        request.map(|request| (request.index, request.strategy)),
        Some((1, Strategy::Nearest))
    );
    assert!(cx.interactivity(&row(1)).aria.active_descendant);
    // the host scrolled there: the rows it left go with the next frame
    cx.simulate_range("explorer-transactions-list", 1..15);
    cx.simulate_key_down("explorer-transactions-list", "down");
    assert!(cx.interactivity(&row(2)).aria.active_descendant);
    assert!(cx.find(&row(40)).is_none(), "{:?}", list(&cx));

    // another page, and back: the list opens at its top, the first row
    // active, the rows the scroll reached let go
    cx.simulate_click("explorer-tab-blocks");
    cx.run_until_parked();
    cx.simulate_click("explorer-tab-transactions");
    cx.run_until_parked();
    assert!(cx.interactivity(&row(0)).aria.active_descendant);
    assert!(cx.find(&row(40)).is_none(), "{:?}", list(&cx));
}

/// The Transactions list scrolls for its page, so its bar belongs where a
/// page's is, at the pane's edge beside the rows: the page is no scroller
/// there (no gutter of its own) and the list reaches the edge, where the
/// host keeps the bar's gutter beside it. The view writes no room for the
/// bar; the heading keeps the same inset, so it ends where the rows do.
#[test]
fn the_transactions_list_reaches_the_panes_edge_and_writes_no_gutter() {
    use view::wire::Node;
    let bar = view::design::size::SCROLLBAR;
    let mut cx = TestAppContext::new();
    heavy(&mut cx);
    cx.open::<Explorer>();
    cx.run_until_parked();
    let page = |cx: &TestAppContext| {
        let page = cx.style("explorer-page");
        (page.overflow.y.is_some(), page.padding.right)
    };
    assert_eq!(page(&cx), (true, Some(bar.into())), "a page scrolls");
    cx.simulate_click("explorer-tab-transactions");
    cx.run_until_parked();
    assert_eq!(page(&cx), (false, None), "the list scrolls, not the page");
    let rows = "explorer-transactions-list-rows";
    assert!(matches!(cx.find(rows), Some(Node::UniformList { .. })));
    let right = |key: &str| cx.style(key).padding.right;
    assert_eq!(right(rows), None, "the gutter is the host's");
    assert_eq!(
        cx.style("explorer-transactions-heading").margin.right,
        Some(bar.into())
    );
    assert_eq!(
        right("explorer-transactions-list"),
        None,
        "the box reaches the edge"
    );
}

/// An account's activity is its newest page, drawn whole.
#[test]
fn an_accounts_activity_and_a_blocks_transactions_are_drawn_whole() {
    let mut cx = TestAppContext::new();
    heavy(&mut cx);
    cx.open::<Explorer>();
    cx.run_until_parked();
    let block = format!("explorer-block-{WINDOW}");
    cx.simulate_click(&block);
    cx.run_until_parked();
    assert!(cx.find("explorer-block").is_some(), "{:?}", cx.texts());
    assert!(cx.find(&format!("explorer-tx-{WINDOW}-0")).is_some());
    assert!(
        cx.find("explorer-block-txs-rows").is_none(),
        "a block's transactions are no window"
    );
    cx.simulate_input("explorer-search", "ada");
    cx.simulate_submit("explorer-search");
    cx.run_until_parked();
    assert!(cx.find("explorer-activity").is_some(), "{:?}", cx.texts());
    assert!(
        cx.find("explorer-activity-list-rows").is_none(),
        "an account's activity is no window"
    );
    let row = |index: usize| format!("explorer-tx-{}-0", WINDOW - index);
    let page = crate::queries::PAGE as usize;
    assert!(cx.find(&row(0)).is_some() && cx.find(&row(page - 1)).is_some());
    assert!(
        cx.find(&row(page)).is_none(),
        "the page past it is not read"
    );
}
