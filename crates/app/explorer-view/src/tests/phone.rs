//! The phone's screens (`ui/phone`): under a finger the view draws the
//! Expo app's Explorer. Each screen's states (reading, refused, empty,
//! full) and presses, by screen; then the sweep, every screen held to
//! `assert_fits` at 360, 390 and 430.
use super::access::{grants, reader, waiting};
use super::*;
use view::methods::Submit;
use view::testing::{self, StreamSender};

/// A chain a link can name (`reader`'s `test#1` names none).
const CHAIN: &str = "testkit#0a1b2c3d";

/// Under a finger, 390 wide: the phone tree.
fn phone(cx: &mut TestAppContext) {
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
}

/// The suite's node at block 12, on a phone.
fn on_phone() -> TestAppContext {
    let (mut cx, _) = ready();
    phone(&mut cx);
    cx
}

/// The suite's node at `tip`, on a phone, with the host's links to send.
fn linked(tip: u64) -> (TestAppContext, StreamSender<HostRoute>) {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(tip)));
    let routes = cx.host().stream::<HostRoute>();
    cx.open::<Explorer>();
    cx.run_until_parked();
    phone(&mut cx);
    (cx, routes)
}

/// The page a link names, opened on a phone.
fn opened(path: &str) -> TestAppContext {
    opened_over(path, |_| ())
}

/// The page a link names, opened on a phone over the suite's node, with
/// `prepare`'s answers in place of some of the node's.
fn opened_over(path: &str, prepare: impl FnOnce(&TestAppContext)) -> TestAppContext {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(12)));
    prepare(&cx);
    let routes = cx.host().stream::<HostRoute>();
    cx.open::<Explorer>();
    cx.run_until_parked();
    phone(&mut cx);
    routes.send(path.to_owned());
    cx.run_until_parked();
    cx
}

/// The reader's session on [`CHAIN`], where links are minted.
fn on_named_chain(cx: &TestAppContext, account: u64, signer: [u8; 32]) {
    cx.host().stream::<HostSession>().send(Session {
        connected: true,
        chain_id: CHAIN.into(),
        signer: abi::hex(&signer),
        account: Some(account),
        endpoint: String::new(),
    });
}

/// What the view writes to the clipboard, as the host takes it.
fn clipboard(cx: &TestAppContext) -> Rc<RefCell<Vec<String>>> {
    let copied = Rc::new(RefCell::new(Vec::new()));
    let written = copied.clone();
    cx.host().handle::<ClipboardWrite>(move |text| {
        written.borrow_mut().push(text);
        Ok(())
    });
    copied
}

fn click(cx: &mut TestAppContext, name: &str) {
    cx.simulate_click(name);
    cx.run_until_parked();
}

fn back(cx: &mut TestAppContext) {
    cx.simulate_back();
    cx.run_until_parked();
}

fn shows(cx: &TestAppContext, text: &str) {
    assert!(cx.has_text(text), "no {text:?} in {:?}", cx.texts());
}

// ---------- the stack ----------

#[test]
fn the_root_has_no_back_and_a_page_over_it_has_one() {
    let mut cx = on_phone();
    assert!(cx.back_handler().is_none(), "Back leaves the program");
    click(&mut cx, "explorer-block-11");
    assert!(cx.back_handler().is_some());
    back(&mut cx);
    assert!(cx.find("explorer-tabs").is_some() && cx.back_handler().is_none());
}

/// Expo's `Stack`: a page goes over the one it was opened from, and Back
/// pops one.
#[test]
fn back_returns_to_the_page_a_page_was_opened_from() {
    let mut cx = on_phone();
    click(&mut cx, "explorer-tab-blocks");
    click(&mut cx, "explorer-block-12");
    click(&mut cx, "explorer-tx-12-0");
    back(&mut cx);
    assert!(
        cx.find("explorer-block-title").is_some() && cx.find("explorer-tx-title").is_none(),
        "{:?}",
        cx.texts()
    );
    back(&mut cx);
    assert!(
        cx.find("explorer-blocks-list").is_some(),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn a_link_that_names_nothing_is_a_page_back_leaves() {
    let mut cx = opened("nowhere/at/all");
    assert!(cx.find("explorer-unlinked").is_some(), "{:?}", cx.texts());
    shows(&cx, "Nothing to show");
    shows(
        &cx,
        "This link names nothing the Explorer shows: nowhere/at/all",
    );
    back(&mut cx);
    assert!(cx.find("explorer-unlinked").is_none() && cx.back_handler().is_none());
}

#[test]
fn a_block_link_whose_hash_nothing_has_is_a_page_saying_so() {
    let cx = opened(&format!("block/{}", abi::hex(&[0xee; 32])));
    assert!(
        cx.find("explorer-block-unfound").is_some(),
        "{:?}",
        cx.texts()
    );
    shows(&cx, "Not found");
    shows(&cx, "No block or transaction has this hash.");
}

#[test]
fn the_desktop_draws_its_own_pages_under_a_pointer() {
    let (mut cx, _) = ready();
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
    assert!(
        cx.find("explorer-search").is_some() && cx.find("explorer-search-open").is_none(),
        "the desktop's bar holds the field; the phone's title row opens it"
    );
}

/// Copy link on a page copies the page's own link; the line it leaves
/// goes at its Dismiss.
#[test]
fn copy_link_copies_the_pages_link_and_dismiss_clears_what_it_said() {
    let mut cx = reader(Some(3), ADA, false, grants(), waiting());
    on_named_chain(&cx, 3, ADA);
    let copied = clipboard(&cx);
    phone(&mut cx);
    click(&mut cx, "explorer-tab-overview");
    click(&mut cx, "explorer-block-11");
    click(&mut cx, "explorer-copy-link");
    assert_eq!(
        *copied.borrow(),
        [ducklink::mint(CHAIN, "explorer", &["block", "11"]).unwrap()]
    );
    shows(&cx, "Copied the link.");
    click(&mut cx, "explorer-note-dismiss");
    assert!(cx.find("explorer-note").is_none());
}

// ---------- the root (`explorer.rs`) ----------

#[test]
fn the_root_names_the_network_and_the_reader() {
    let mut cx = reader(Some(3), ADA, false, grants(), waiting());
    phone(&mut cx);
    shows(&cx, "Explorer");
    shows(&cx, "test · Ada");
    // the reader's avatar leads the root's title row (Expo `root`)
    assert!(cx.find("explorer-root-account").is_some());
    click(&mut cx, "explorer-tab-accounts");
    click(&mut cx, "explorer-account-5");
    assert!(cx.find("explorer-root-account").is_none(), "only the root");
}

#[test]
fn search_turns_the_title_row_into_the_field_and_close_puts_it_back() {
    let mut cx = on_phone();
    click(&mut cx, "explorer-search-open");
    assert!(cx.find("explorer-search").is_some() && !cx.has_text("Explorer"));
    click(&mut cx, "explorer-search-close");
    assert!(cx.find("explorer-search").is_none());
    shows(&cx, "Explorer");
}

#[test]
fn a_search_that_lands_opens_its_page_and_closes_the_field() {
    let mut cx = on_phone();
    click(&mut cx, "explorer-search-open");
    cx.simulate_input("explorer-search", "11");
    cx.simulate_submit("explorer-search");
    cx.run_until_parked();
    shows(&cx, "Block 11");
    back(&mut cx);
    assert!(cx.find("explorer-search").is_none(), "the field closed");
}

#[test]
fn a_search_that_finds_nothing_says_so_under_the_field() {
    let mut cx = on_phone();
    click(&mut cx, "explorer-search-open");
    cx.simulate_input("explorer-search", "nothing-here");
    let focused = |cx: &TestAppContext| cx.focused().and_then(|node| node.key()).map(str::to_owned);
    assert_eq!(focused(&cx).as_deref(), Some("explorer-search"));
    cx.simulate_submit("explorer-search");
    cx.run_until_parked();
    shows(&cx, "Nothing here is called “nothing-here”.");
    assert!(cx.find("explorer-search").is_some(), "the field stays open");
    // Return ended the edit, as Expo's field blurs: the keyboard left it
    assert_eq!(focused(&cx).as_deref(), Some("explorer-root"));
}

#[test]
fn the_strip_opens_each_page_as_the_root() {
    let mut cx = on_phone();
    for (tab, list) in [
        ("blocks", "explorer-blocks-list"),
        ("transactions", "explorer-transactions-list"),
        ("accounts", "explorer-accounts-list"),
        ("programs", "explorer-programs-list"),
        ("overview", "explorer-stats"),
    ] {
        click(&mut cx, &format!("explorer-tab-{tab}"));
        assert!(cx.find(list).is_some(), "{tab}: {:?}", cx.texts());
        assert!(cx.back_handler().is_none(), "{tab} is a root");
    }
}

// ---------- the pages under the strip (`pages.rs`) ----------

#[test]
fn a_page_of_the_window_says_reading_the_chain_until_it_lands() {
    let mut cx = TestAppContext::new();
    quiet_host(&cx);
    cx.open::<Explorer>();
    phone(&mut cx);
    shows(&cx, "Reading the chain…");
}

#[test]
fn a_refused_window_says_why_and_offers_retry() {
    let mut cx = TestAppContext::new();
    quiet_host(&cx);
    cx.host()
        .refuse::<ChainBlocks>("unavailable", "the node is catching up");
    cx.open::<Explorer>();
    phone(&mut cx);
    shows(&cx, "the node is catching up");
    assert!(cx.find("explorer-retry").is_some());
}

#[test]
fn the_overview_shows_the_figures_and_the_latest_blocks_and_transactions() {
    let cx = on_phone();
    for text in [
        // eyebrows cross as written; the host draws them in capitals
        "Height",
        "12",
        "Block every 1.0 s",
        "Epoch",
        "Validators",
        "Accounts",
    ] {
        shows(&cx, text);
    }
    shows(&cx, "Latest activity");
    shows(&cx, "0–10 · 11 empty blocks");
    shows(&cx, "Latest transactions");
    assert!(cx.find("explorer-tx-11-0").is_some() && cx.find("explorer-tx-12-0").is_some());
}

#[test]
fn a_window_without_transactions_says_so() {
    let (cx, _) = linked(10);
    shows(&cx, "No transactions in the last 11 blocks.");
}

#[test]
fn the_overviews_links_open_all_blocks_and_all_transactions() {
    let mut cx = on_phone();
    click(&mut cx, "explorer-all-blocks");
    assert!(cx.find("explorer-blocks-list").is_some());
    click(&mut cx, "explorer-tab-overview");
    click(&mut cx, "explorer-all-txs");
    assert!(cx.find("explorer-transactions-list").is_some());
}

#[test]
fn the_transactions_page_counts_the_window_and_lists_it_newest_first() {
    let mut cx = on_phone();
    click(&mut cx, "explorer-tab-transactions");
    shows(&cx, "3 transactions in the last 13 blocks");
    let rows: Vec<&str> = ["explorer-tx-12-0", "explorer-tx-11-0", "explorer-tx-11-1"]
        .into_iter()
        .filter(|row| cx.find(row).is_some())
        .collect();
    assert_eq!(rows.len(), 3, "{:?}", cx.texts());
}

#[test]
fn the_accounts_page_says_reading_until_identity_answers() {
    let mut cx = TestAppContext::new();
    quiet_host(&cx);
    cx.open::<Explorer>();
    phone(&mut cx);
    click(&mut cx, "explorer-tab-accounts");
    shows(&cx, "Reading accounts…");
}

#[test]
fn the_accounts_page_lists_each_account_and_what_it_is() {
    let mut cx = on_phone();
    click(&mut cx, "explorer-tab-accounts");
    shows(&cx, "3 · tx recorded");
    shows(&cx, "Ada");
    assert!(cx.find("explorer-account-5").is_some() && cx.find("explorer-account-6").is_some());
}

#[test]
fn the_blocks_page_says_where_the_nodes_archive_stops() {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(30)));
    cx.host().handle::<ChainBlocks>(|ask| {
        let mut listed = page(&chain(30), &ask);
        listed.blocks.retain(|block| block.height >= 20);
        listed.listed_from = Some(20);
        Ok(listed)
    });
    cx.open::<Explorer>();
    cx.run_until_parked();
    phone(&mut cx);
    click(&mut cx, "explorer-tab-blocks");
    shows(&cx, "History before block 20 is not indexed on this node.");
}

// ---------- one block (`block.rs`) ----------

#[test]
fn a_block_page_shows_its_facts_and_its_transactions() {
    let mut cx = on_phone();
    click(&mut cx, "explorer-block-11");
    shows(&cx, "Block 11");
    shows(&cx, &abi::hex(&[111; 32]));
    shows(&cx, "validator 1 ");
    assert!(cx.find("explorer-parent").is_some());
    assert!(cx.find("explorer-tx-11-0").is_some() && cx.find("explorer-tx-11-1").is_some());
}

#[test]
fn the_steps_walk_the_blocks_and_stop_at_the_head() {
    let mut cx = on_phone();
    click(&mut cx, "explorer-block-12");
    assert!(
        cx.interactivity("explorer-next").on_click.is_none(),
        "no block past the head"
    );
    click(&mut cx, "explorer-previous");
    shows(&cx, "Block 11");
    click(&mut cx, "explorer-next");
    shows(&cx, "Block 12");
    back(&mut cx);
    assert!(
        cx.find("explorer-tabs").is_some(),
        "a step replaces the page"
    );
}

#[test]
fn a_block_with_no_transactions_says_so() {
    let cx = opened("block/5");
    shows(&cx, "No transactions in this block.");
}

#[test]
fn a_block_the_node_does_not_keep_says_so() {
    let cx = opened("block/99");
    shows(&cx, "No block 99");
    shows(&cx, "This node keeps no finalized block at this height.");
}

// ---------- one transaction (`tx.rs`) ----------

#[test]
fn a_transaction_page_shows_where_it_sits_how_it_ran_and_its_op() {
    let mut cx = on_phone();
    click(&mut cx, "explorer-tx-11-0");
    shows(&cx, "Transaction");
    shows(&cx, "In block 11");
    shows(&cx, "Accepted");
    assert!(cx.find("tx-hash").is_some(), "Expo's testID on the hash");
    shows(&cx, "Ada");
    shows(&cx, "Operation");
    shows(&cx, "Messages");
    shows(&cx, "the inbox is full");
}

#[test]
fn a_refused_transaction_shows_the_refusal_and_its_reason() {
    let mut cx = on_phone();
    click(&mut cx, "explorer-tx-12-0");
    shows(&cx, "Rejected");
    shows(&cx, "not a member");
    shows(&cx, "unauthorized");
}

/// Ours mints `tx/<height>/<position>`; Expo's links name the hash
/// (`tx/<hash>`), found as search finds one. Both open the same page.
#[test]
fn a_transaction_link_opens_by_its_place_or_by_its_hash() {
    for path in [
        "tx/11/0".to_owned(),
        format!("tx/{}", abi::hex(&[0xa1; 32])),
    ] {
        let cx = opened(&path);
        assert!(cx.find("tx-hash").is_some(), "{path}: {:?}", cx.texts());
        shows(&cx, "In block 11");
        shows(&cx, &abi::hex(&[0xa1; 32]));
    }
}

#[test]
fn a_transaction_the_node_does_not_keep_says_so() {
    let cx = opened("tx/11/7");
    shows(&cx, "Transaction not found");
}

/// A block or a transaction the page reads past the window: reading, then
/// refused with Retry, which asks the node again.
#[test]
fn a_page_read_past_the_window_says_reading_then_refused_and_retry_asks_again() {
    for (path, reading, asked_again) in [
        ("block/99", "Reading the block…", "No block 99"),
        (
            "tx/11/7",
            "Reading the transaction…",
            "Transaction not found",
        ),
    ] {
        let cx = opened_over(path, |cx| {
            cx.host().never::<ChainBlock>();
            cx.host().never::<ChainTx>();
        });
        shows(&cx, reading);
        let mut cx = opened_over(path, |cx| {
            cx.host()
                .refuse::<ChainBlock>("unavailable", "the archive is offline");
            cx.host()
                .refuse::<ChainTx>("unavailable", "the archive is offline");
        });
        shows(&cx, "the archive is offline");
        serve(&cx, || chain(12));
        click(&mut cx, "explorer-retry");
        shows(&cx, asked_again);
    }
}

// ---------- one account (`account.rs`) ----------

#[test]
fn an_account_page_shows_its_devices_the_programs_it_used_and_its_activity() {
    let mut cx = on_phone();
    click(&mut cx, "explorer-tab-accounts");
    click(&mut cx, "explorer-account-3");
    shows(&cx, "account 3 · User");
    shows(&cx, "laptop");
    shows(&cx, "Programs used");
    shows(&cx, "Activity");
    click(&mut cx, "explorer-used-chat");
    shows(&cx, "Transactions · chat");
}

/// Open DM opens chat on the reader's DM room, as members-view does: for
/// another person or a program account, not for the reader nor for a
/// program's own account.
#[test]
fn open_dm_opens_the_readers_room_with_another_account() {
    let mut cx = reader(Some(3), ADA, false, grants(), waiting());
    on_named_chain(&cx, 3, ADA);
    phone(&mut cx);
    click(&mut cx, "explorer-tab-accounts");
    click(&mut cx, "explorer-account-5");
    click(&mut cx, "explorer-open-dm");
    assert_eq!(
        cx.host().requests::<view::methods::LinkOpen>(),
        [ducklink::mint(CHAIN, chat::PROGRAM, &["dm-3-5"]).unwrap()]
    );
    for own in ["3", "6"] {
        back(&mut cx);
        click(&mut cx, &format!("explorer-account-{own}"));
        assert!(cx.find("explorer-open-dm").is_none(), "account {own}");
    }
}

#[test]
fn an_account_with_no_activity_says_so() {
    let cx = opened("account/5");
    shows(&cx, "No activity recorded.");
}

#[test]
fn an_account_identity_does_not_hold_says_so() {
    let cx = opened("account/99");
    shows(&cx, "No account #99");
    shows(&cx, "Identity holds no account by this number.");
}

#[test]
fn an_account_page_says_reading_then_why_identity_refused() {
    let cx = opened_over("account/3", |cx| cx.host().never::<Query<Identity>>());
    shows(&cx, "Reading the account…");
    let cx = opened_over("account/3", |cx| {
        cx.host()
            .refuse::<Query<Identity>>("unavailable", "identity is upgrading")
    });
    shows(&cx, "identity is upgrading");
}

#[test]
fn an_accounts_activity_says_reading_then_why_it_was_refused() {
    let cx = opened_over("account/3", |cx| cx.host().never::<Query<Activity>>());
    shows(&cx, "Reading its activity…");
    // what a network with no activity program answers: the code and the
    // program's bare name, said in full as Expo's core says it
    let cx = opened_over("account/3", |cx| {
        cx.host()
            .refuse::<Query<Activity>>("unknown_program", "activity")
    });
    shows(&cx, "This network runs no program called activity.");
}

/// The chief's call A, as Expo reads them: each frame's transaction read
/// by its place and titled by what its program describes, its block after
/// its hash (`TxLine who={false}`).
#[test]
fn an_accounts_activity_rows_say_what_each_transaction_did() {
    let cx = opened("account/3");
    let op: chat::Op = borsh::from_slice(&post("design", "hello there")).unwrap();
    shows(&cx, &crate::decode::clip(&chat::describe(&op).title));
    shows(&cx, " · block 11");
    assert!(cx.find("explorer-tx-11-0").is_some() && cx.find("explorer-tx-11-1").is_some());
}

// ---------- the programs (`programs.rs`, `program.rs`, `txs.rs`) ----------

#[test]
fn the_programs_page_says_reading_until_the_role_answers() {
    let mut cx = TestAppContext::new();
    quiet_host(&cx);
    cx.open::<Explorer>();
    phone(&mut cx);
    click(&mut cx, "explorer-tab-programs");
    shows(&cx, "Reading the programs role…");
}

#[test]
fn the_programs_page_lists_what_is_scheduled_runs_and_is_listed() {
    let mut cx = on_phone();
    click(&mut cx, "explorer-tab-programs");
    shows(&cx, "Remove forge at block 120");
    shows(&cx, "by vote");
    shows(&cx, "sha256:abababab…abab · Validators' vote");
    shows(&cx, "view only · sha256:efefefef…efef · Validators' vote");
    shows(
        &cx,
        "No account maintains a program: only the validators' vote changes the programs role.",
    );
}

#[test]
fn a_running_program_opens_its_sheet_and_the_sheet_its_page() {
    let mut cx = on_phone();
    click(&mut cx, "explorer-tab-programs");
    click(&mut cx, "explorer-program-chat");
    assert!(cx.find("explorer-program-sheet").is_some());
    shows(&cx, "capabilities, code, scheduled");
    shows(&cx, "to chat");
    click(&mut cx, "explorer-program-about");
    assert!(
        cx.find("explorer-program-sheet").is_none(),
        "the sheet left"
    );
    shows(&cx, "reads and writes the clipboard");
    shows(&cx, "Maintained by");
}

#[test]
fn a_program_page_copies_its_code_id() {
    let mut cx = opened("program/chat");
    let copied = clipboard(&cx);
    click(&mut cx, "explorer-copy-code");
    shows(&cx, "Copied the code id.");
    assert_eq!(*copied.borrow(), [CHAT_CODE.to_owned()]);
}

#[test]
fn a_program_whose_view_asks_nothing_counts_none_and_says_so() {
    let (mut cx, routes) = linked(12);
    cx.host().handle::<BlobManifest>(|_| {
        Ok(Some(Manifested {
            size: 2_400_000,
            view: Some(CarriedView {
                size: 1_900_000,
                manifest: Some(view::wire::manifest::Manifest {
                    wire_id: "0123abcd".into(),
                    name: "Chat".into(),
                    description: String::new(),
                    capabilities: Vec::new(),
                    min_width: 480,
                    targets: Vec::new(),
                    icon: "icons/chat.svg".into(),
                    tint: Tint::Blue,
                }),
            }),
        }))
    });
    routes.send("program/chat".into());
    cx.run_until_parked();
    shows(&cx, "0 capabilities");
    shows(&cx, "Its view asks nothing of the host.");
}

#[test]
fn a_program_page_opens_the_programs_transactions() {
    let mut cx = opened("program/chat");
    shows(&cx, "2 transactions in the last 13 blocks →");
    click(&mut cx, "explorer-program-txs");
    shows(&cx, "Transactions · chat");
    assert!(
        cx.find("explorer-tx-12-0").is_none(),
        "mystery's is not chat's"
    );
}

#[test]
fn a_view_only_entry_has_no_way_to_transactions() {
    let cx = opened("program/explorer");
    shows(&cx, "view only");
    assert!(cx.find("explorer-program-txs").is_none());
}

#[test]
fn a_name_the_programs_role_does_not_hold_says_so() {
    let cx = opened("program/nothing");
    shows(&cx, "Nothing called nothing");
}

#[test]
fn add_offers_a_grant_to_a_reader_with_voices() {
    let mut cx = reader(Some(3), ADA, true, grants(), waiting());
    phone(&mut cx);
    click(&mut cx, "explorer-add-menu");
    shows(&cx, "Propose a grant");
    assert!(
        cx.find("explorer-add").is_none(),
        "Ada's grant names a running program: nothing to add"
    );
}

#[test]
fn add_offers_a_program_to_a_holder_of_every_program() {
    let mut cx = reader(Some(5), STRANGER, false, grants(), waiting());
    phone(&mut cx);
    click(&mut cx, "explorer-add-menu");
    assert!(
        matches!(cx.node("explorer-add-menu-sheet"),
            view::wire::Node::Sheet { label: Some(label), .. } if label == "Add"),
        "the sheet is named as its control"
    );
    shows(&cx, "Add a program");
    shows(&cx, "by your grant");
}

/// The host's capsule beside the title row draws `ellipsis` and is named
/// More: what the reader may add is a `plus` named Add.
#[test]
fn add_is_a_plus_not_the_capsules_more() {
    let mut cx = reader(Some(5), STRANGER, true, grants(), waiting());
    phone(&mut cx);
    let row = format!("{:?}", cx.node("explorer-root-title"));
    assert!(!row.contains("icons/ellipsis.svg"), "{row}");
    let add = format!("{:?}", cx.node("explorer-add-menu"));
    assert!(add.contains("icons/plus.svg"), "{add}");
    let name = &cx.interactivity("explorer-add-menu").aria.label;
    assert_eq!(name.as_deref(), Some("Add"));
}

#[test]
fn a_reader_with_no_grant_and_no_voice_has_no_add() {
    let mut cx = reader(None, STRANGER, false, grants(), waiting());
    phone(&mut cx);
    assert!(cx.find("explorer-add-menu").is_none());
}

#[test]
fn a_maintainer_cancels_a_change_their_grant_made() {
    let mut cx = reader(Some(3), ADA, false, grants(), waiting());
    phone(&mut cx);
    click(&mut cx, "explorer-cancel-700-chat");
    assert_eq!(cx.host().requests::<Submit<Programs>>().len(), 1);
}

#[test]
fn the_holder_steps_down_from_their_own_grant() {
    let mut cx = reader(Some(3), ADA, false, grants(), waiting());
    phone(&mut cx);
    shows(&cx, "Step down");
    click(&mut cx, "explorer-revoke-3-chat");
    assert_eq!(cx.host().requests::<Submit<Programs>>().len(), 1);
}

#[test]
fn a_network_whose_programs_role_runs_nothing_says_so() {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(12)));
    programs_with(&cx, Vec::new(), Vec::new(), Vec::new());
    cx.open::<Explorer>();
    cx.run_until_parked();
    phone(&mut cx);
    click(&mut cx, "explorer-tab-programs");
    shows(&cx, "The programs role of this network runs nothing yet.");
}

#[test]
fn a_program_page_says_reading_then_why_the_programs_role_refused() {
    let cx = opened_over("program/chat", |cx| cx.host().never::<Query<Programs>>());
    shows(&cx, "Reading the programs role…");
    let cx = opened_over("program/chat", |cx| {
        cx.host()
            .refuse::<Query<Programs>>("unavailable", "the programs role is busy")
    });
    shows(&cx, "the programs role is busy");
}

/// The Capabilities section, by what the code's manifest came to.
#[test]
fn a_program_page_says_why_it_lists_no_capability() {
    type Prepare = Box<dyn Fn(&TestAppContext)>;
    let cases: [(&str, Prepare, &str); 6] = [
        (
            "chat",
            Box::new(|cx: &TestAppContext| cx.host().never::<BlobManifest>()),
            "Reading the code…",
        ),
        (
            "chat",
            Box::new(|cx: &TestAppContext| {
                cx.host()
                    .refuse::<BlobManifest>("unavailable", "the blob store is down")
            }),
            "the blob store is down",
        ),
        (
            "chat",
            Box::new(|cx: &TestAppContext| cx.host().handle::<BlobManifest>(|_| Ok(None))),
            "The node does not hold this code yet.",
        ),
        (
            "chat",
            Box::new(|cx: &TestAppContext| {
                cx.host().handle::<BlobManifest>(|_| {
                    let view = CarriedView {
                        size: 1_000,
                        manifest: None,
                    };
                    Ok(Some(Manifested {
                        size: 1_000,
                        view: Some(view),
                    }))
                })
            }),
            "Its view carries a manifest this app cannot read: built for another wire, or asking a capability this app does not have.",
        ),
        (
            "identity",
            Box::new(|_: &TestAppContext| ()),
            "This program draws nothing: no view, so nothing is asked of the host.",
        ),
        (
            "explorer",
            Box::new(|cx: &TestAppContext| {
                cx.host().handle::<BlobManifest>(|_| {
                    Ok(Some(Manifested {
                        size: 1_000,
                        view: None,
                    }))
                })
            }),
            "This app cannot read this view's manifest.",
        ),
    ];
    for (name, prepare, says) in cases {
        let cx = opened_over(&format!("program/{name}"), |cx| prepare(cx));
        shows(&cx, says);
    }
}

/// By voices, a cancel and a revoke go to the vote as motions, never to
/// the programs role.
#[test]
fn a_reader_with_voices_proposes_a_cancel_and_a_revoke_to_the_vote() {
    let mut cx = reader(Some(6), STRANGER, true, grants(), waiting());
    phone(&mut cx);
    shows(&cx, "Propose cancel");
    shows(&cx, "Propose revoke");
    click(&mut cx, "explorer-cancel-120-forge");
    click(&mut cx, "explorer-revoke-5-all");
    let cancel = programs::Op::Cancel {
        height: 120,
        program: "forge".into(),
    };
    let revoke = programs::Op::Revoke(programs::Grant {
        scope: programs::Scope::All,
        account: 5,
    });
    let motions = [cancel, revoke].map(|op| {
        valset::Op::Vote(valset::Motion::Role {
            role: abi::role::PROGRAMS.into(),
            op: abi::encode(&op),
        })
    });
    assert_eq!(cx.host().requests::<Submit<Valset>>(), motions);
    assert!(cx.host().requests::<Submit<Programs>>().is_empty());
    shows(&cx, "Vote counted, 1 of 2: Revoke · every program.");
}

/// One write at a time: while one is out, every press that writes is off.
#[test]
fn every_press_that_writes_is_off_while_a_write_is_out() {
    let mut cx = reader(Some(3), ADA, false, grants(), waiting());
    cx.host().never::<Submit<Programs>>();
    phone(&mut cx);
    click(&mut cx, "explorer-cancel-700-chat");
    for press in ["explorer-cancel-700-chat", "explorer-revoke-3-chat"] {
        assert!(cx.interactivity(press).on_click.is_none(), "{press}");
    }
}

// ---------- the form (`form.rs`) ----------

/// Ada with voices, the grant form open from Add.
fn grant_form() -> TestAppContext {
    let mut cx = reader(Some(3), ADA, true, grants(), waiting());
    phone(&mut cx);
    click(&mut cx, "explorer-add-menu");
    click(&mut cx, "explorer-propose-grant");
    cx
}

#[test]
fn a_grant_form_names_the_account_typed() {
    let mut cx = grant_form();
    shows(&cx, "Type the account's number.");
    cx.simulate_input("explorer-form-account", "#3");
    cx.run_until_parked();
    shows(&cx, "#3 · Ada");
    cx.simulate_input("explorer-form-account", "9");
    cx.run_until_parked();
    shows(&cx, "No account 9 here.");
    shows(
        &cx,
        "This proposes a vote: the validators carry it in Nodes.",
    );
}

#[test]
fn a_form_that_builds_nothing_says_why_and_sends_nothing() {
    let mut cx = grant_form();
    click(&mut cx, "explorer-form-submit");
    shows(&cx, "An account is its number.");
    assert!(cx.host().requests::<Submit<Valset>>().is_empty());
}

/// Each of `ids` that tells assistive tech it is wrong, with why.
fn wrong<'a>(cx: &TestAppContext, ids: &[&'a str]) -> Vec<(&'a str, String)> {
    ids.iter()
        .filter_map(|&id| match cx.node(id) {
            view::wire::Node::Field { options, .. } => options
                .invalid
                .map(|_| (id, options.description.clone().unwrap_or_default())),
            other => panic!("{id} is no field: {other:?}"),
        })
        .collect()
}

#[test]
fn the_field_a_problem_is_about_tells_assistive_tech_it_is_wrong_and_why() {
    let mut cx = grant_form();
    click(&mut cx, "explorer-form-submit");
    assert_eq!(
        wrong(&cx, &["explorer-form-account", "explorer-form-scope"]),
        [(
            "explorer-form-account",
            "An account is its number.".to_owned()
        )]
    );
}

#[test]
fn a_problem_moves_to_the_field_it_is_about_and_a_refusal_marks_none() {
    let fields = ["explorer-form-code", "explorer-form-height"];
    let mut cx = reader(Some(3), ADA, false, grants(), waiting());
    cx.host()
        .refuse::<Submit<Programs>>("unauthorized", "no grant covers chat");
    phone(&mut cx);
    click(&mut cx, "explorer-program-chat");
    click(&mut cx, "explorer-upgrade-chat");
    click(&mut cx, "explorer-form-submit");
    assert_eq!(
        wrong(&cx, &fields),
        [(
            "explorer-form-code",
            "A code id is sha256: and 64 hex digits.".to_owned()
        )]
    );
    cx.simulate_input("explorer-form-code", CHAT_CODE);
    cx.simulate_input("explorer-form-height", "12");
    click(&mut cx, "explorer-form-submit");
    assert_eq!(
        wrong(&cx, &fields),
        [(
            "explorer-form-height",
            "Block 12 has passed: pick a later one.".to_owned()
        )]
    );
    cx.simulate_input("explorer-form-height", "5000");
    click(&mut cx, "explorer-form-submit");
    // the node's refusal, passed on by the host (`design::failure`)
    shows(&cx, "That didn’t go through: no grant covers chat");
    assert_eq!(wrong(&cx, &fields), [], "the refusal is the whole form's");
}

#[test]
fn a_write_the_program_refuses_says_it_did_not_go_through() {
    let mut cx = reader(Some(3), ADA, false, grants(), waiting());
    cx.host().handle::<Submit<Programs>>(|_| {
        Ok(view::methods::Submitted::Refused {
            frame: [0; 32],
            until: 0,
            refusal: Error {
                code: "unauthorized".into(),
                message: "no grant covers chat".into(),
            },
        })
    });
    phone(&mut cx);
    click(&mut cx, "explorer-program-chat");
    click(&mut cx, "explorer-remove-chat");
    click(&mut cx, "explorer-form-submit");
    shows(&cx, "That didn’t go through: no grant covers chat");
}

#[test]
fn back_closes_a_sheet_before_it_leaves_the_page() {
    let mut cx = grant_form();
    assert!(cx.find("explorer-form-sheet").is_some());
    back(&mut cx);
    assert!(cx.find("explorer-form-sheet").is_none());
    assert!(cx.back_handler().is_none(), "the root again");
}

/// Add, by a grant over every program: a program asks its params and a
/// view none; a nameless one says so on its name; one that lands closes
/// the form and says what was sent.
#[test]
fn the_add_form_asks_what_it_adds_and_lands_its_schedule() {
    let mut cx = reader(Some(5), STRANGER, false, grants(), waiting());
    phone(&mut cx);
    click(&mut cx, "explorer-add-menu");
    click(&mut cx, "explorer-add");
    assert!(
        cx.find("explorer-form-params").is_some(),
        "a program's params"
    );
    click(&mut cx, "explorer-form-view");
    assert!(cx.find("explorer-form-params").is_none(), "a view has none");
    click(&mut cx, "explorer-form-program");
    click(&mut cx, "explorer-form-submit");
    assert_eq!(
        wrong(&cx, &["explorer-form-name"]),
        [("explorer-form-name", "Name the program.".to_owned())]
    );
    cx.simulate_input("explorer-form-name", "trade");
    cx.simulate_input("explorer-form-code", CHAT_CODE);
    cx.simulate_input("explorer-form-params", "0x0102");
    click(&mut cx, "explorer-form-submit");
    let added = programs::Op::Schedule(programs::Scheduled {
        height: 12 + programs::NOTICE + 30,
        change: programs::Change::Set(programs::Entry {
            program: "trade".into(),
            code: BlobId::Sha256([0xab; 32]),
            params: vec![1, 2],
        }),
    });
    assert_eq!(cx.host().requests::<Submit<Programs>>(), [added]);
    assert!(cx.find("explorer-form-sheet").is_none(), "the form closes");
    shows(&cx, "Sent to the programs role: Schedule · trade.");
}

// ---------- the sweep ----------

#[test]
fn every_screen_fits_a_phone() {
    // voices and a grant over every program: Add offers a program and a grant
    let mut cx = reader(Some(5), STRANGER, true, grants(), waiting());
    testing::sweep(&mut cx, |cx| {
        // the strip is a finger's 44 (Expo `theme.ts:81` `size.strip`) and
        // scrolls sideways (Expo's `ScrollView horizontal`: Programs runs
        // past a 390 edge on both), and each tab presses its whole height
        let strip = Some(view::design::TOUCH.strip.into());
        let bar = cx.style("explorer-tabs");
        assert_eq!(bar.size.height, strip);
        assert_eq!(format!("{:?}", bar.overflow.x), "Some(Scroll)");
        for tab in ["overview", "blocks", "transactions", "accounts"] {
            let id = format!("explorer-tab-{tab}");
            assert_eq!(cx.style(&id).size.height, strip);
            click(cx, &id);
        }
        click(cx, "explorer-account-3");
        back(cx);
        click(cx, "explorer-tab-overview");
        click(cx, "explorer-block-11");
        click(cx, "explorer-tx-11-0");
        back(cx);
        back(cx);
        click(cx, "explorer-tab-programs");
        click(cx, "explorer-add-menu");
        click(cx, "explorer-add");
        back(cx);
        click(cx, "explorer-add-menu");
        click(cx, "explorer-propose-grant");
        back(cx);
        click(cx, "explorer-program-chat");
        click(cx, "explorer-program-about");
        click(cx, "explorer-program-txs");
        back(cx);
        back(cx);
        // a missing page, then a search's line under the field
        let search = |cx: &mut TestAppContext, text: &str| {
            click(cx, "explorer-search-open");
            cx.simulate_input("explorer-search", text);
            cx.simulate_submit("explorer-search");
            cx.run_until_parked();
        };
        search(cx, "99");
        back(cx);
        search(cx, "nothing-here");
        click(cx, "explorer-search-close");
    });
}
