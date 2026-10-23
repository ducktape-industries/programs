//! One program's page: what its code declares, read through `blob.manifest`
//! as the app reads a manifest to seat a view, and the rest of what the
//! programs role and the chain say of it.
use super::*;

fn opened(row: &str) -> TestAppContext {
    let (mut cx, _) = ready();
    cx.simulate_click("explorer-tab-programs");
    cx.run_until_parked();
    cx.simulate_click(row);
    cx.run_until_parked();
    cx
}

#[test]
fn a_program_row_opens_its_page_with_its_capabilities_first() {
    let cx = opened("explorer-program-chat");
    let texts = cx.texts();
    assert!(cx.find("explorer-program").is_some(), "{texts:?}");
    assert!(cx.has_text("chat") && cx.has_text("program"), "{texts:?}");
    assert!(cx.has_text("6 capabilities"), "{texts:?}");
    for (capability, allows) in [
        (
            "chain",
            "reads the chain: blocks, transactions and the node's status",
        ),
        ("op", "sends operations to the chain in your name"),
        ("clipboard", "reads and writes the clipboard"),
        ("store", "keeps its own data on this device"),
    ] {
        assert!(
            cx.find(&format!("explorer-capability-{capability}"))
                .is_some()
                && cx.has_text(allows),
            "{capability}: {texts:?}"
        );
    }
    assert!(
        cx.find("explorer-capability-web").is_none(),
        "only what it declares"
    );
    let at = |text: &str| texts.iter().position(|t| t == text).expect(text);
    assert!(at("Capabilities") < at("About") && at("About") < at("Scheduled"));
    assert!(cx.has_text(CHAT_CODE), "the code id in full: {texts:?}");
    assert!(
        cx.has_text("2.3 MB of code, 1.8 MB of it the view"),
        "{texts:?}"
    );
    assert!(cx.has_text("Params") && cx.has_text("010203"), "{texts:?}");
    assert!(
        cx.has_text("Chat · Chat, as the suite plays it.") && cx.has_text("480 px"),
        "{texts:?}"
    );
    assert!(cx.has_text("any account"), "chat fills no role: {texts:?}");
    assert!(
        cx.has_text("Nothing is scheduled for it.") && !cx.has_text("forge"),
        "forge's removal is another program's: {texts:?}"
    );
    assert!(cx.has_text("Transactions →"), "{texts:?}");
}

#[test]
fn the_transactions_link_opens_the_programs_transactions() {
    let mut cx = opened("explorer-program-chat");
    cx.simulate_click("explorer-program-txs");
    cx.run_until_parked();
    assert!(
        cx.has_text("Transactions · chat") && cx.has_text("2 transactions in the last 13 blocks"),
        "{:?}",
        cx.texts()
    );
    assert!(!cx.has_text("mystery"), "{:?}", cx.texts());
}

#[test]
fn a_program_without_a_view_says_it_draws_nothing() {
    let cx = opened("explorer-program-identity");
    let texts = cx.texts();
    assert!(cx.find("explorer-no-view").is_some(), "{texts:?}");
    assert!(
        cx.has_text("This program draws nothing: no view, so nothing is asked of the host."),
        "{texts:?}"
    );
    assert!(cx.has_text("126 KB"), "the code's size alone: {texts:?}");
    assert!(
        cx.has_text("validators' vote"),
        "identity fills a role: {texts:?}"
    );
    assert!(cx.find("explorer-program-role").is_some());
}

#[test]
fn a_view_only_entry_opens_its_page() {
    let cx = opened("explorer-view-explorer");
    let texts = cx.texts();
    assert!(cx.find("explorer-program").is_some(), "{texts:?}");
    assert!(cx.has_text("view only"), "{texts:?}");
    assert!(cx.has_text("2 capabilities") && cx.has_text("wakes on a timer"));
    assert!(cx.has_text("1.1 MB"), "{texts:?}");
    assert!(
        cx.find("explorer-program-txs").is_none(),
        "a view sends nothing: {texts:?}"
    );
}

#[test]
fn unknown_code_and_an_unreadable_manifest_are_said_as_such() {
    let (mut cx, _) = ready();
    cx.host().handle::<BlobManifest>(|id| {
        Ok(match id.as_str() {
            CHAT_CODE => Some(Manifested {
                size: 2_000,
                view: Some(CarriedView {
                    size: 1_000,
                    manifest: None,
                }),
            }),
            _ => None,
        })
    });
    cx.simulate_click("explorer-tab-programs");
    cx.run_until_parked();
    cx.simulate_click("explorer-program-chat");
    cx.run_until_parked();
    assert!(
        cx.find("explorer-unread-view").is_some()
            && cx.has_text("Its view carries a manifest this app cannot read: built for another wire, or asking a capability this app does not have."),
        "{:?}",
        cx.texts()
    );
    cx.simulate_click("explorer-tab-programs");
    cx.run_until_parked();
    cx.simulate_click("explorer-program-identity");
    cx.run_until_parked();
    assert!(
        cx.find("explorer-no-code").is_some()
            && cx.has_text("The node does not hold this code yet."),
        "{:?}",
        cx.texts()
    );
    assert!(
        cx.host().requests::<BlobManifest>().len() == 2,
        "asked once per code: {:?}",
        cx.host().requests::<BlobManifest>()
    );

    let (mut cx, _) = ready();
    cx.host().handle::<BlobManifest>(|_| {
        Err(Error {
            code: "node_failed".into(),
            message: "the node did not answer".into(),
        })
    });
    cx.simulate_click("explorer-tab-programs");
    cx.run_until_parked();
    cx.simulate_click("explorer-program-chat");
    cx.run_until_parked();
    assert!(cx.has_text("the node did not answer"), "{:?}", cx.texts());
    assert!(cx.has_text(CHAT_CODE), "the rest of the page stands");
}

#[test]
fn a_view_only_entrys_unread_view_is_never_a_program_that_draws_nothing() {
    let answer = |view: Option<CarriedView>| {
        move |id: String| {
            Ok((id == EXPLORER_CODE).then(|| Manifested {
                size: 1_200_000,
                view: view.clone(),
            }))
        }
    };
    let (mut cx, _) = ready();
    cx.host().handle::<BlobManifest>(answer(None));
    cx.simulate_click("explorer-tab-programs");
    cx.run_until_parked();
    cx.simulate_click("explorer-view-explorer");
    cx.run_until_parked();
    let texts = cx.texts();
    assert!(
        cx.find("explorer-unread-view").is_some()
            && cx.has_text("This app cannot read this view's manifest."),
        "{texts:?}"
    );
    assert!(cx.find("explorer-no-view").is_none(), "{texts:?}");

    let (mut cx, _) = ready();
    cx.host().handle::<BlobManifest>(answer(Some(CarriedView {
        size: 1_100_000,
        manifest: None,
    })));
    cx.simulate_click("explorer-tab-programs");
    cx.run_until_parked();
    cx.simulate_click("explorer-view-explorer");
    cx.run_until_parked();
    let texts = cx.texts();
    assert!(
        cx.find("explorer-unread-view").is_some()
            && cx.has_text("Its view carries a manifest this app cannot read: built for another wire, or asking a capability this app does not have."),
        "{texts:?}"
    );
    assert!(
        cx.has_text("1.1 MB"),
        "the rest of the page stands: {texts:?}"
    );
}

#[test]
fn enter_on_the_programs_list_opens_the_program() {
    let (mut cx, _) = ready();
    cx.simulate_click("explorer-tab-programs");
    cx.run_until_parked();
    let list = cx.interactivity("explorer-programs-list");
    assert_eq!(list.role, Some(view::Role::ListBox));
    cx.simulate_focus("explorer-programs-list");
    cx.simulate_key_down("explorer-programs-list", "down");
    cx.simulate_key_down("explorer-programs-list", "enter");
    cx.run_until_parked();
    assert!(
        cx.find("explorer-no-view").is_some(),
        "the second row, identity: {:?}",
        cx.texts()
    );
    cx.simulate_focus("explorer-tabs");
    cx.simulate_key_down("explorer-tabs", "left");
    cx.run_until_parked();
    assert!(cx.find("explorer-program").is_none());
    cx.simulate_key_down("explorer-tabs", "right");
    cx.run_until_parked();
    assert!(
        cx.find("explorer-no-view").is_some(),
        "back to the page it left: {:?}",
        cx.texts()
    );
    cx.simulate_click("explorer-tab-programs");
    cx.run_until_parked();
    cx.simulate_focus("explorer-views-list");
    cx.simulate_key_down("explorer-views-list", "enter");
    cx.run_until_parked();
    assert!(cx.has_text("wakes on a timer"), "{:?}", cx.texts());
}

#[test]
fn copy_puts_the_code_id_on_the_clipboard() {
    let mut cx = opened("explorer-program-chat");
    let copied = Rc::new(RefCell::new(String::new()));
    let held = copied.clone();
    cx.host().handle::<ClipboardWrite>(move |text| {
        *held.borrow_mut() = text;
        Ok(())
    });
    cx.simulate_click("explorer-copy-code");
    cx.run_until_parked();
    assert_eq!(*copied.borrow(), CHAT_CODE);
    assert!(cx.has_text("Copied the code id."), "{:?}", cx.texts());
}

#[test]
fn a_page_opened_before_the_roster_asks_when_it_lands() {
    let mut cx = TestAppContext::new();
    let routes = cx.host().stream::<HostRoute>();
    let heads = cx.host().stream::<Changes<Programs>>();
    node(&mut cx, Rc::new(RefCell::new(12)));
    cx.host().never::<Query<Programs>>();
    cx.open::<Explorer>();
    cx.run_until_parked();
    routes.send("program/chat".into());
    cx.run_until_parked();
    assert!(
        cx.has_text("Reading the programs role…"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.host().requests::<BlobManifest>().is_empty());
    respond(&mut cx);
    cx.host().answer_held::<Query<Programs>>();
    heads.send(Some(view::methods::Change {
        height: 13,
        keys: Vec::new(),
    }));
    cx.run_until_parked();
    assert!(
        cx.has_text("reads and writes the clipboard"),
        "{:?}",
        cx.texts()
    );
    assert_eq!(
        cx.host().requests::<BlobManifest>(),
        vec![CHAT_CODE.to_owned()]
    );
}

#[test]
fn a_program_says_the_commit_its_code_was_built_from() {
    let cx = opened("explorer-program-chat");
    let texts = cx.texts();
    assert!(cx.has_text("Built from"), "{texts:?}");
    assert!(cx.has_text("programs@4cd6980"), "{texts:?}");
    let cx = opened("explorer-program-identity");
    assert!(cx.has_text("its code names no commit"), "{:?}", cx.texts());
    let (mut cx, _) = ready();
    cx.simulate_click("explorer-tab-programs");
    cx.run_until_parked();
    assert!(cx.find("explorer-built-chat").is_some(), "{:?}", cx.texts());
    let unstamped = cx.find("explorer-built-identity");
    assert!(unstamped.is_none(), "a row names only a commit");
}
