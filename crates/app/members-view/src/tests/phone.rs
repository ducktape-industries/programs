//! The phone's screens (`ui/phone/`): the roster and the member page as
//! the Expo app draws them, under a finger. The fixtures are the desk's
//! (`super`): the same network, read the same way.
use super::*;

fn phone() -> TestAppContext {
    let (mut cx, _) = ready();
    cx.host()
        .handle::<ProgramDescribe>(|(target, payload): (String, Vec<u8>)| {
            assert_eq!((target.as_str(), payload.as_slice()), ("chat", &b"hi"[..]));
            Ok(Some(view::methods::Description {
                title: "Post in #general".into(),
                fields: Vec::new(),
            }))
        });
    cx.host().stream::<HostSession>().send(Session {
        account: Some(7),
        chain_id: "testnet#0a1b2c3d".into(),
        signer: "0102".into(),
        ..Session::default()
    });
    on_a_phone(&mut cx);
    cx
}

fn on_a_phone(cx: &mut TestAppContext) {
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
}

fn last_link(cx: &TestAppContext) -> Option<String> {
    cx.host()
        .requests::<view::methods::LinkOpen>()
        .last()
        .cloned()
}

#[test]
fn sweep() {
    let mut cx = phone();
    view::testing::sweep(&mut cx, |cx| {
        assert_eq!(cx.back_handler(), None);
        for chip in 0..4 {
            presses_44(cx, &format!("members-chip-{chip}"), 32.);
        }
        cx.simulate_click("members-filter-open");
        cx.simulate_input("members-filter", "sc");
        cx.simulate_click("members-filter-close");
        cx.simulate_click("members-chip-3");
        cx.simulate_click("members-chip-0");
        cx.simulate_click("members-row-7");
        cx.run_until_parked();
        cx.simulate_click("members-manages-9");
        cx.run_until_parked();
        cx.simulate_click("members-manager");
        cx.run_until_parked();
        for _ in 0..3 {
            cx.simulate_back();
        }
        cx.simulate_click("members-row-8");
        cx.run_until_parked();
        cx.simulate_back();
        assert_eq!(cx.back_handler(), None);
    });
}

#[test]
fn the_root_picks_the_phone_screens_by_the_pointer_alone() {
    let (mut cx, _) = ready();
    cx.simulate_resize(360., 780.);
    assert!(cx.find("members-list-pane").is_some());
    cx.simulate_touch(true);
    assert!(cx.find("members-list-pane").is_none());
    assert!(cx.find("members-roster").is_some());
    cx.simulate_resize(1000., 780.);
    assert!(cx.find("members-roster").is_some());
    assert!(cx.find("members-list-resize").is_none());
}

#[test]
fn the_phone_roster_groups_people_agents_and_programs_and_says_what_each_is() {
    let cx = phone();
    let texts = cx.texts();
    let at = |text: &str| texts.iter().rposition(|t| t == text).unwrap();
    assert!(at("Members") < at("All") && at("All") < at("People"));
    assert!(at("People") < at("eddy") && at("eddy") < at("ada"));
    assert!(at("ada") < at("Agents") && at("Agents") < at("scout"));
    assert!(at("relay") < at("Programs") && at("Programs") < at("chat"));
    for text in [
        "testnet · 5 accounts",
        "  you",
        "User",
        "Program",
        "managed by eddy",
        "active",
        "revoked",
        "  5",
    ] {
        assert!(cx.has_text(text), "{text}: {texts:?}");
    }
    for gone in ["5 accounts", "Choose a member", "Agent · managed by eddy"] {
        assert!(!texts.iter().any(|t| t == gone), "{gone}");
    }
    assert_eq!(cx.back_handler(), None);
}

#[test]
fn the_phone_roster_says_it_is_reading_until_the_roster_lands() {
    let mut cx = TestAppContext::new();
    cx.host().never::<Query<Identity>>();
    cx.open::<Members>();
    on_a_phone(&mut cx);
    assert!(cx.has_text("Reading the roster…"));
    assert!(
        cx.find("members-chips").is_none(),
        "no chips with nobody listed"
    );
}

#[test]
fn a_refused_phone_roster_says_why_and_retry_asks_again() {
    let mut cx = TestAppContext::new();
    cx.host()
        .refuse::<Query<Identity>>("unavailable", "identity is not running here");
    cx.open::<Members>();
    on_a_phone(&mut cx);
    assert!(cx.has_text("identity is not running here"));
    respond(&mut cx);
    cx.simulate_click("members-retry");
    cx.run_until_parked();
    assert!(cx.find("members-row-7").is_some());
}

#[test]
fn a_phone_roster_with_nobody_in_it_says_so() {
    let mut cx = TestAppContext::new();
    cx.host()
        .handle::<Query<Identity>>(|_| Ok(identity::Reply::Accounts(page(vec![]))));
    cx.host()
        .handle::<Query<Valset>>(|_| Ok(valset::Reply::Memberships(page(vec![]))));
    cx.open::<Members>();
    on_a_phone(&mut cx);
    assert!(cx.has_text("No accounts"));
    assert!(cx.has_text("The identity program of this network holds no accounts yet."));
    assert!(cx.find("members-chips").is_none());
}

#[test]
fn the_phone_filter_takes_the_title_and_close_filter_empties_it() {
    let mut cx = phone();
    assert!(cx.find("members-filter").is_none());
    cx.simulate_click("members-filter-open");
    assert!(
        cx.find("members-roster-heading").is_none(),
        "the field in its place"
    );
    cx.simulate_input("members-filter", "sc");
    assert!(cx.find("members-row-9").is_some() && cx.find("members-row-7").is_none());
    cx.simulate_input("members-filter", "zz");
    assert!(cx.has_text("Nothing matches") && cx.has_text("No account reads like “zz”."));
    cx.simulate_click("members-filter-close");
    assert!(cx.find("members-filter").is_none());
    assert!(cx.find("members-row-7").is_some() && cx.find("members-row-9").is_some());
}

#[test]
fn the_filter_carries_its_keyboard_hints() {
    let mut cx = phone();
    cx.simulate_click("members-filter-open");
    let Some(Node::Field { options, .. }) = cx.find("members-filter") else {
        panic!("no filter field");
    };
    assert!(!options.text_input.autocorrect);
    assert_eq!(
        options.text_input.autocapitalize,
        view::wire::Autocapitalize::None
    );
}

#[test]
fn the_filter_edge_is_the_fields_own() {
    let mut cx = phone();
    cx.simulate_click("members-filter-open");
    let hairline = StyleRefinement::default().border_1();
    assert_eq!(
        style(&cx, "members-filter").border_widths.left,
        hairline.border_widths.left
    );
    let around = parent(cx.root(), "members-filter").expect("the field's box");
    let around = around.style().map(|style| &cx.styles()[style]);
    assert!(around.is_none_or(|style| style.border_widths.left.is_none()));
}

#[test]
fn a_phone_chip_narrows_the_roster_to_its_group() {
    let mut cx = phone();
    cx.simulate_click("members-chip-3");
    assert!(cx.find("members-row-8").is_some() && cx.find("members-row-7").is_none());
    cx.simulate_click("members-chip-0");
    assert!(cx.find("members-row-7").is_some());
}

#[test]
fn the_phone_arrows_move_a_cursor_and_open_nothing() {
    let mut cx = phone();
    cx.simulate_focus("members-list");
    cx.simulate_key_down("members-list", "down");
    assert_eq!(cx.back_handler(), None, "still the roster");
    assert_eq!(cx.interactivity("members-row-11").aria.selected, Some(true));
    cx.simulate_key_down("members-list", "enter");
    cx.run_until_parked();
    assert!(cx.has_text("account 11 · User · 1 device"));
}

#[test]
fn a_phone_row_opens_its_member_and_back_returns() {
    let mut cx = phone();
    cx.simulate_click("members-row-7");
    cx.run_until_parked();
    assert!(cx.back_handler().is_some());
    assert!(cx.find("members-list").is_none(), "one page at a time");
    for text in [
        "eddy",
        "account 7",
        "account 7 · User · 1 device",
        "Validator",
        "eddy's bio",
        "Explorer",
        "Devices",
        "laptop",
        "added 1 Jan 1970",
        "Manages",
        "account 9 · Agent",
        "Recent activity",
        "Post in #general",
        "block 12",
        " · 0s",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    assert!(
        !cx.has_text("newest first"),
        "Expo's head has no count here"
    );
    assert!(cx.find("members-open-dm").is_none());
    cx.simulate_back();
    assert!(cx.find("members-list").is_some());
    assert_eq!(cx.back_handler(), None);
}

#[test]
fn a_member_page_marks_this_phone_and_links_its_blocks_explorer_and_a_dm() {
    let mut cx = phone();
    cx.simulate_click("members-row-7");
    cx.run_until_parked();
    let device = format!("{:?}", cx.node("members-device-0"));
    assert!(device.contains("icons/smartphone.svg"), "{device}");
    cx.simulate_click("members-block-0");
    assert_eq!(last_link(&cx).as_deref(), Some("duck://explorer/block/12"));
    cx.simulate_click("members-explorer-open");
    assert_eq!(last_link(&cx).as_deref(), Some("duck://explorer/account/7"));
    cx.simulate_click("members-explorer");
    assert_eq!(last_link(&cx).as_deref(), Some("duck://explorer/account/7"));
    cx.simulate_back();
    cx.simulate_click("members-row-11");
    cx.run_until_parked();
    let device = format!("{:?}", cx.node("members-device-0"));
    assert!(device.contains("icons/key-round.svg"), "{device}");
    cx.simulate_click("members-open-dm");
    assert!(last_link(&cx).is_some_and(|link| link.contains("/chat/")));
}

#[test]
fn an_agent_opens_over_its_manager_and_its_manager_over_that() {
    let mut cx = phone();
    cx.simulate_click("members-row-7");
    cx.run_until_parked();
    cx.simulate_click("members-manages-9");
    cx.run_until_parked();
    for text in [
        "account 9 · Agent · managed by ",
        "eddy",
        " · 1 device",
        "active",
        "Suspend, revoke, rename and keys live in Account → Agents, for the manager.",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    cx.simulate_click("members-manager");
    cx.run_until_parked();
    assert!(cx.has_text("account 7 · User · 1 device"));
    cx.simulate_back();
    assert!(cx.has_text("account 9 · Agent · managed by "));
    cx.simulate_back();
    assert!(cx.has_text("account 7 · User · 1 device"));
}

#[test]
fn a_program_page_has_no_devices_and_opens_a_dm() {
    let mut cx = phone();
    cx.simulate_click("members-row-8");
    cx.run_until_parked();
    assert!(cx.has_text("account 8 · Program") && cx.has_text("No devices"));
    assert!(!cx.has_text("Recent activity"));
    assert!(cx.find("members-open-dm").is_some());
}

#[test]
fn a_refused_activity_read_is_said_quietly() {
    let mut cx = phone();
    cx.host()
        .refuse::<Query<Activity>>("unknown_program", "activity");
    cx.simulate_click("members-row-11");
    cx.run_until_parked();
    assert!(cx.has_text("This network runs no program called activity."));
    assert!(cx.find("members-retry").is_none(), "nothing to retry");
}

#[test]
fn a_member_page_with_no_such_account_says_so() {
    let mut cx = phone();
    let feed = cx.host().stream::<Changes<Identity>>();
    cx.simulate_click("members-row-11");
    cx.run_until_parked();
    cx.host().handle::<Query<Identity>>(|_| {
        Ok(identity::Reply::Accounts(page(vec![person(
            7,
            "eddy",
            vec![key(EDDY, "laptop")],
        )])))
    });
    feed.send(None);
    cx.run_until_parked();
    for text in [
        "Account 11",
        "No account #11",
        "Identity holds no account by this number.",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    cx.simulate_back();
    assert!(cx.find("members-row-7").is_some());
}

fn ada_before_the_roster(answer: impl FnOnce(&mut TestAppContext)) -> TestAppContext {
    let mut cx = TestAppContext::new();
    answer(&mut cx);
    let view = cx.open::<Members>();
    on_a_phone(&mut cx);
    cx.update(&view, |members, _, cx| members.open(11, cx));
    cx.run_until_parked();
    cx
}

#[test]
fn a_member_page_reads_the_roster_until_it_lands() {
    let cx = ada_before_the_roster(|cx| cx.host().never::<Query<Identity>>());
    assert!(cx.has_text("Account 11") && cx.has_text("Reading the roster…"));
    assert!(cx.back_handler().is_some(), "Back leaves the page it draws");
}

#[test]
fn a_member_page_says_why_the_roster_was_refused_and_retry_reads_it_again() {
    let mut cx = ada_before_the_roster(|cx| {
        cx.host()
            .refuse::<Query<Identity>>("unavailable", "identity is not running here");
    });
    assert!(cx.has_text("Account 11") && cx.has_text("identity is not running here"));
    respond(&mut cx);
    cx.simulate_click("members-retry");
    cx.run_until_parked();
    assert!(cx.has_text("account 11 · User · 1 device"));
}

#[test]
fn a_phone_titles_activity_by_what_each_transaction_did() {
    let mut cx = phone();
    cx.simulate_click("members-row-7");
    cx.run_until_parked();
    let row = format!("{:?}", cx.node("members-signed-0"));
    assert!(row.contains("\"Post in #general\""), "{row}");
    let places: Vec<_> = cx
        .host()
        .requests::<ChainTx>()
        .into_iter()
        .map(|ask| (ask.tx, ask.most))
        .collect();
    assert_eq!(
        places,
        [
            (
                TxRef::At {
                    height: 12,
                    position: 0
                },
                4096
            ),
            (
                TxRef::At {
                    height: 11,
                    position: 0
                },
                4096
            ),
        ]
    );
    cx.host().handle::<ProgramDescribe>(|_| Ok(None));
    cx.simulate_back();
    cx.simulate_click("members-row-9");
    cx.run_until_parked();
    assert!(cx.has_text("chat · 2 bytes"), "{:?}", cx.texts());
}

#[test]
fn a_frame_whose_transaction_the_chain_lacks_is_left_out() {
    let mut cx = phone();
    cx.host().handle::<ChainTx>(|_| Ok(None));
    cx.simulate_click("members-row-7");
    cx.run_until_parked();
    assert!(cx.has_text("No activity recorded."), "{:?}", cx.texts());
}

#[test]
fn a_refused_description_leaves_the_size_and_is_logged() {
    let mut cx = phone();
    cx.host()
        .refuse::<ProgramDescribe>("internal", "describe trapped");
    cx.simulate_click("members-row-9");
    cx.run_until_parked();
    assert!(cx.has_text("chat · 2 bytes"), "{:?}", cx.texts());
    let logged = cx.host().requests::<view::methods::HostLog>();
    assert!(
        logged
            .iter()
            .any(|line| line == "Members: a description refused: internal: describe trapped"),
        "{logged:?}"
    );
}

#[test]
fn a_restored_phone_titles_its_activity() {
    let mut cx = phone();
    cx.simulate_click("members-row-7");
    cx.run_until_parked();
    let bytes = cx.snapshot().unwrap();
    cx.restore::<Members>(&bytes).unwrap();
    cx.run_until_parked();
    assert!(cx.has_text("Post in #general"), "{:?}", cx.texts());
}

#[test]
fn a_member_page_says_it_is_reading_its_activity() {
    let mut cx = phone();
    cx.host().never::<ChainBlocks>();
    cx.simulate_click("members-row-11");
    cx.run_until_parked();
    assert!(cx.has_text("Reading its activity…"));
}

#[test]
fn a_member_page_with_nothing_signed_says_no_activity_was_recorded() {
    let mut cx = phone();
    cx.simulate_click("members-row-11");
    cx.run_until_parked();
    assert!(cx.has_text("No activity recorded."));
}

#[test]
fn a_program_named_apart_from_its_program_says_which_it_runs() {
    let mut cx = phone();
    cx.host().handle::<Query<Identity>>(|_| {
        Ok(identity::Reply::Accounts(page(vec![
            person(7, "eddy", vec![key(EDDY, "laptop")]),
            account(
                12,
                "relay desk",
                identity::Control::System {
                    program: "relay".into(),
                },
            ),
        ])))
    });
    cx.host().stream::<Changes<Identity>>().send(None);
    cx.run_until_parked();
    assert!(cx.has_text("Program") && !cx.has_text("Program · relay"));
    cx.simulate_click("members-row-12");
    cx.run_until_parked();
    assert!(cx.has_text("account 12 · Program · relay"));
}

fn parent<'a>(node: &'a Node, key: &str) -> Option<&'a Node> {
    node.children()
        .iter()
        .find_map(|child| match child.key() == Some(key) {
            true => Some(node),
            false => parent(child, key),
        })
}

fn presses_44(cx: &TestAppContext, key: &str, drawn: f32) {
    let frame = parent(cx.root(), key).unwrap_or_else(|| panic!("no {key}"));
    let frame = &cx.styles()[frame.style().expect("a styled frame")];
    let declared = StyleRefinement::default().h(view::px(drawn)).border_1();
    assert_eq!(frame.size.height, declared.size.height, "{key}'s frame");
    assert_eq!(
        frame.border_widths.top, declared.border_widths.top,
        "{key}'s edge"
    );
    let reach = (drawn - 44.) / 2. - 1.;
    let pinned = StyleRefinement::default()
        .top(view::px(reach))
        .bottom(view::px(reach));
    let press = style(cx, key);
    assert_eq!(
        (press.inset.top, press.inset.bottom),
        (pinned.inset.top, pinned.inset.bottom),
        "{key}"
    );
}

#[test]
fn every_chip_and_small_button_presses_44_tall_within_its_row() {
    let mut cx = phone();
    for chip in 0..4 {
        presses_44(&cx, &format!("members-chip-{chip}"), 32.);
    }
    let row = style(&cx, "members-chips");
    let padded = StyleRefinement::default().py(view::px(8.));
    assert_eq!(
        (row.padding.top, row.padding.bottom),
        (padded.padding.top, padded.padding.bottom)
    );
    cx.simulate_click("members-row-11");
    cx.run_until_parked();
    small_presses_44(&cx, "members-open-dm");
    small_presses_44(&cx, "members-explorer");
}

fn small_presses_44(cx: &TestAppContext, key: &str) {
    let press = style(cx, key);
    let kit = StyleRefinement::default()
        .h(view::px(44.))
        .my(view::px(-4.));
    assert_eq!(press.size.height, kit.size.height, "{key}");
    assert_eq!(
        (press.margin.top, press.margin.bottom),
        (kit.margin.top, kit.margin.bottom),
        "{key}"
    );
    let face = &cx.node(key).children()[0];
    let face = &cx.styles()[face.style().expect("a styled face")];
    let drawn = StyleRefinement::default().h(view::px(36.));
    assert_eq!(face.size.height, drawn.size.height, "{key}'s face");
}
