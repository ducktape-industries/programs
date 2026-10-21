//! The phone's design (`ui/phone/`), as the Expo app draws it: each state
//! of the Account screen and of the Agents pane, and each action through
//! its sheet, under a finger at 390 wide. The sweep holds them all at 360,
//! 390 and 430.
use super::*;

fn phone(state: &str) -> TestAppContext {
    let mut cx = fixture(state, false);
    on_phone(&mut cx);
    cx
}

fn on_phone(cx: &mut TestAppContext) {
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
}

fn agents() -> TestAppContext {
    let mut cx = phone("ready");
    cx.simulate_click("settings/strip/agents");
    cx
}

fn identity_moves(cx: &mut TestAppContext, height: u64) {
    let heads = cx.host().stream::<Changes<Identity>>();
    heads.send(Some(Change {
        height,
        keys: Vec::new(),
    }));
    cx.run_until_parked();
}

fn focused(cx: &TestAppContext) -> Option<String> {
    cx.focused().and_then(wire::Node::key).map(str::to_owned)
}

fn has_all(cx: &TestAppContext, texts: &[&str]) {
    for text in texts {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
}

#[test]
fn sweep() {
    let mut cx = fixture("ready", false);
    view::testing::sweep(&mut cx, |cx| {
        cx.simulate_click("settings/strip/account");
        assert_eq!(cx.back_handler(), None);
        cx.simulate_click("settings/strip/agents");
        cx.simulate_click("settings/agents/12");
        cx.simulate_click("settings/agents/12/rename/open");
        cx.simulate_click("settings/agents/12/rename/cancel");
        cx.simulate_click("settings/agents/12/answers/open");
        cx.simulate_click("settings/agents/12/answers/specific");
        cx.simulate_click("settings/agents/12/answers/done");
        cx.simulate_dismiss("settings/agents/12/sheet");
        cx.simulate_click("settings/agents/create/open");
        cx.simulate_click("settings/agents/create/cancel");
        cx.simulate_click("settings/agents/12");
        cx.simulate_click("settings/agents/12/key/open");
        cx.simulate_back();
        cx.simulate_click("settings/strip/policies");
        cx.run_until_parked();
        cx.simulate_click("settings/policies/2");
        cx.simulate_click("settings/policies/2/edit");
        cx.simulate_click("settings/policies/edit/write/specific");
        cx.simulate_back();
        cx.simulate_click("settings/policies/2");
        cx.simulate_click("settings/policies/2/remove");
        cx.run_until_parked();
        cx.simulate_dismiss("settings/policies/sheet");
        cx.simulate_click("settings/policies/default/kind-chat.channel");
        cx.simulate_back();
        cx.simulate_click("settings/strip/invites");
        assert_eq!(cx.back_handler(), None);
    });
}

#[test]
fn the_pointer_picks_the_design_not_the_width() {
    let mut cx = fixture("ready", false);
    cx.simulate_touch(true);
    cx.simulate_resize(820., 800.);
    assert!(cx.find("settings/strip/account").is_some());
    assert!(cx.find("settings/nav").is_none());
    cx.simulate_touch(false);
    cx.simulate_resize(360., 800.);
    assert!(cx.find("settings/strip/account").is_none());
    assert!(cx.find("settings/nav").is_some());
}

#[test]
fn a_person_reads_who_they_are_and_each_key() {
    let cx = phone("ready");
    has_all(
        &cx,
        &[
            "Maya",
            "account 7 · Person · 1 key",
            "Keys",
            "Laptop key",
            "abcd · fingerprint 123D 4C7E",
            "Validator",
        ],
    );
    for pane in ["account", "agents", "invites"] {
        assert!(
            cx.find(&format!("settings/strip/{pane}")).is_some(),
            "{pane}"
        );
    }
    assert!(!cx.has_text("account 7 · User"));
}

#[test]
fn an_agents_account_has_the_agents_pane() {
    let mut cx = phone("scout");
    assert!(cx.find("settings/strip/agents").is_some());
    cx.simulate_click("settings/strip/agents");
    cx.run_until_parked();
    has_all(&cx, &["Relay"]);
    assert!(cx.find("settings/agents/create/open").is_some());
}

#[test]
fn the_account_says_it_is_being_read() {
    assert!(phone("loading").has_text("Reading your account…"));
}

#[test]
fn a_refused_read_says_why_and_reads_again() {
    let mut cx = phone("refused");
    assert!(cx.has_text("Account query refused."));
    respond(&cx);
    cx.simulate_click("settings/account-retry");
    cx.run_until_parked();
    assert!(cx.has_text("Maya"), "{:?}", cx.texts());
}

#[test]
fn a_phone_with_no_key_has_no_account() {
    let cx = phone("empty");
    has_all(
        &cx,
        &[
            "No account",
            "No host key is selected. Sign in with a key to create an account.",
        ],
    );
}

#[test]
fn an_account_identity_does_not_hold_says_so() {
    let mut cx = phone("refused");
    cx.host().handle::<Query<Identity>>(|q| match q {
        identity::Query::Get { number: 7 } => Ok(identity::Reply::Account(None)),
        q => panic!("unexpected query: {q:?}"),
    });
    cx.simulate_click("settings/account-retry");
    cx.run_until_parked();
    assert!(
        cx.has_text("Identity holds no account 7."),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn an_unregistered_key_creates_its_account() {
    let mut cx = phone("unregistered");
    has_all(
        &cx,
        &[
            "Unregistered key",
            "no account yet",
            "Phone",
            "abcd · fingerprint 123D 4C7E",
            "Create an account",
        ],
    );
    assert!(
        cx.find("settings/strip/agents").is_none(),
        "a bare key manages nothing"
    );
    cx.simulate_click("settings/account/create/submit");
    cx.run_until_parked();
    assert!(cx.has_text("Enter an account name."));
    assert!(sent(&cx).is_empty());

    cx.host()
        .handle::<Submit<Identity>>(|_| Ok(view::testing::applied(Vec::new())));
    cx.simulate_input("settings/account/create/name", "Maya");
    cx.simulate_click("settings/account/create/submit");
    cx.run_until_parked();
    assert!(matches!(
        sent(&cx)[..],
        [identity::Op::Create { ref name, scheme: abi::Scheme::Ed25519 }] if name == "Maya"
    ));
    assert!(cx.has_text("Creating…"), "{:?}", cx.texts());
}

#[test]
fn a_refused_account_keeps_its_name_and_says_why() {
    let mut cx = phone("unregistered");
    cx.host()
        .refuse::<Submit<Identity>>("invalid", "a name is not empty");
    cx.simulate_input("settings/account/create/name", "Maya");
    cx.simulate_click("settings/account/create/submit");
    cx.run_until_parked();
    assert!(cx.has_text("That didn’t go through: a name is not empty"));
    let Some(wire::Node::Field { value, .. }) = cx.find("settings/account/create/name") else {
        panic!("the name field: {:?}", cx.texts())
    };
    assert_eq!(value, "Maya");
}

#[test]
fn a_stopped_field_tells_assistive_tech_why() {
    let mut cx = phone("unregistered");
    cx.simulate_click("settings/account/create/submit");
    let Some(wire::Node::Field { options, .. }) = cx.find("settings/account/create/name") else {
        panic!("the name field: {:?}", cx.texts())
    };
    assert!(options.invalid.is_some(), "{options:?}");
    assert_eq!(
        options.description.as_deref(),
        Some("Enter an account name.")
    );
}

#[test]
fn a_stopped_agents_key_says_why_it_does_not_act() {
    let cx = phone("suspended");
    has_all(
        &cx,
        &[
            "Scout",
            "a key of an agent",
            "Phone",
            "This key belongs to Scout, suspended by its manager.",
        ],
    );
    assert!(cx.find("settings/account/create").is_none());
}

#[test]
fn a_phone_says_who_mints_invites() {
    let mut cx = phone("ready");
    cx.simulate_click("settings/strip/invites");
    assert!(cx.has_text("Only the machine running this node can mint its invites."));
    assert!(cx.find("settings/invite/mint").is_none());
}

#[test]
fn a_person_reads_each_agent_and_whom_it_answers() {
    let cx = agents();
    has_all(
        &cx,
        &[
            "Agents act as accounts you manage. You answer for what they do.",
            "Scout",
            "active",
            "account 12 · 0 keys · answers only me",
            "external",
            "Connect an external agent",
            "An agent another machine runs with its own key. To have this network run one, create it in Agents.",
        ],
    );
    assert!(!cx.has_text("Add a key to an agent"), "{:?}", cx.texts());
}

#[test]
fn an_agents_row_opens_its_actions_and_back_closes_them() {
    let mut cx = agents();
    cx.simulate_click("settings/agents/12");
    assert!(
        matches!(cx.node("settings/agents/12/sheet"), wire::Node::Sheet { label, .. }
            if label.as_deref() == Some("Scout"))
    );
    has_all(
        &cx,
        &[
            "account 12 · active",
            "Rename",
            "Who it answers",
            "Only me",
            "Suspend",
            "Revoke",
            "final",
        ],
    );
    cx.simulate_back();
    assert!(cx.find("settings/agents/12/sheet").is_none());
    assert_eq!(cx.back_handler(), None);
}

#[test]
fn a_rename_that_lands_goes_back_to_the_agents_actions() {
    let mut cx = agents();
    cx.simulate_click("settings/agents/12");
    cx.simulate_click("settings/agents/12/rename/open");
    let Some(wire::Node::Field { value, .. }) = cx.find("settings/agents/12/name") else {
        panic!("the rename field: {:?}", cx.texts())
    };
    assert_eq!(value, "Scout");
    cx.host()
        .handle::<Submit<Identity>>(|_| Ok(view::testing::applied(Vec::new())));
    cx.simulate_input("settings/agents/12/name", "Scout II");
    cx.simulate_click("settings/agents/12/rename/submit");
    cx.run_until_parked();
    assert_eq!(
        sent(&cx),
        [identity::Op::SetName {
            account: 12,
            name: "Scout II".into()
        }]
    );
    assert!(cx.find("settings/agents/12/name").is_none());
    assert!(cx.find("settings/agents/12/rename/open").is_some());
}

#[test]
fn cancel_takes_a_rename_back_to_the_agents_actions() {
    let mut cx = agents();
    cx.simulate_click("settings/agents/12");
    cx.simulate_click("settings/agents/12/rename/open");
    cx.simulate_click("settings/agents/12/rename/cancel");
    assert!(cx.find("settings/agents/12/name").is_none());
    assert!(cx.find("settings/agents/12/rename/open").is_some());
    assert!(sent(&cx).is_empty());
}

#[test]
fn closing_the_sheet_drops_a_rename_not_sent() {
    let mut cx = agents();
    cx.simulate_click("settings/agents/12");
    cx.simulate_click("settings/agents/12/rename/open");
    cx.simulate_input("settings/agents/12/name", "Half typed");
    cx.simulate_back();
    cx.simulate_click("settings/agents/12");
    assert!(
        cx.find("settings/agents/12/name").is_none(),
        "the sheet opens on its actions"
    );
    cx.simulate_click("settings/agents/12/rename/open");
    let Some(wire::Node::Field { value, .. }) = cx.find("settings/agents/12/name") else {
        panic!("the rename field: {:?}", cx.texts())
    };
    assert_eq!(value, "Scout");
}

#[test]
fn suspend_and_revoke_are_one_press_each() {
    let mut cx = agents();
    cx.host()
        .handle::<Submit<Identity>>(|_| Ok(view::testing::applied(Vec::new())));
    cx.simulate_click("settings/agents/12");
    cx.simulate_click("settings/agents/12/suspend");
    cx.run_until_parked();
    cx.simulate_click("settings/agents/12/revoke");
    cx.run_until_parked();
    assert_eq!(
        sent(&cx),
        [
            identity::Op::Suspend { account: 12 },
            identity::Op::Revoke { account: 12 },
        ]
    );
}

#[test]
fn a_refused_suspend_says_why_under_the_actions() {
    let mut cx = agents();
    cx.host()
        .refuse::<Submit<Identity>>("unauthorized", "account 7 does not manage account 12");
    cx.simulate_click("settings/agents/12");
    cx.simulate_click("settings/agents/12/suspend");
    cx.run_until_parked();
    assert!(cx.has_text("That didn’t go through: account 7 does not manage account 12"));
    assert!(cx.find("settings/agents/12/suspend").is_some());
}

#[test]
fn a_suspended_agent_resumes_and_a_revoked_one_only_says_who_it_was() {
    let mut cx = agents();
    scout_is(&cx, identity::Life::Suspended { keys: Vec::new() });
    identity_moves(&mut cx, 43);
    cx.simulate_click("settings/agents/12");
    assert!(cx.find("settings/agents/12/suspend").is_none());
    cx.host()
        .handle::<Submit<Identity>>(|_| Ok(view::testing::applied(Vec::new())));
    cx.simulate_click("settings/agents/12/resume");
    cx.run_until_parked();
    assert_eq!(sent(&cx), [identity::Op::Resume { account: 12 }]);

    scout_is(&cx, identity::Life::Revoked);
    identity_moves(&mut cx, 44);
    assert!(cx.has_text("account 12 · revoked"), "{:?}", cx.texts());
    for action in ["rename/open", "answers/open", "resume", "revoke"] {
        assert!(
            cx.find(&format!("settings/agents/12/{action}")).is_none(),
            "{action}"
        );
    }
}

#[test]
fn whom_an_agent_answers_is_picked_in_its_sheet() {
    let (mut cx, answers) = answering(identity::Ownership::Owner);
    on_phone(&mut cx);
    cx.simulate_click("settings/agents/12");
    cx.simulate_click("settings/agents/12/answers/open");
    assert_eq!(picked(&cx), ["manager"]);

    cx.simulate_click("settings/agents/12/answers/anyone");
    cx.run_until_parked();
    assert_eq!(*answers.borrow(), identity::Ownership::Anyone);
    assert!(cx.has_text(WARNING), "{:?}", cx.texts());

    cx.simulate_click("settings/agents/12/answers/specific");
    assert!(cx.has_text("Add people by name or account number, or a program by its name."));
    cx.simulate_input("settings/agents/12/answers/name", "#9");
    cx.simulate_click("settings/agents/12/answers/add");
    cx.run_until_parked();
    assert_eq!(*answers.borrow(), listed(&[9]));
    assert!(cx.has_text("Eve"), "{:?}", cx.texts());

    cx.simulate_click("settings/agents/12/answers/done");
    assert!(cx.find("settings/agents/12/answers/name").is_none());
    assert!(cx.has_text("Specific people"), "{:?}", cx.texts());
}

#[test]
fn a_person_an_agent_answers_is_removed_in_one_press() {
    let (mut cx, answers) = answering(listed(&[8, 9]));
    on_phone(&mut cx);
    cx.simulate_click("settings/agents/12");
    cx.simulate_click("settings/agents/12/answers/open");
    has_all(&cx, &["Bob", "account 8", "Eve", "account 9"]);
    cx.simulate_click("settings/agents/12/answers/8/remove");
    cx.run_until_parked();
    assert_eq!(*answers.borrow(), listed(&[9]));
    assert!(!cx.has_text("account 8"), "{:?}", cx.texts());
}

#[test]
fn a_refused_answer_says_why_in_the_sheet() {
    let (mut cx, answers) = answering(identity::Ownership::Owner);
    on_phone(&mut cx);
    cx.host()
        .refuse::<Submit<Identity>>("unauthorized", "account 7 does not manage account 12");
    cx.simulate_click("settings/agents/12");
    cx.simulate_click("settings/agents/12/answers/open");
    cx.simulate_click("settings/agents/12/answers/anyone");
    cx.run_until_parked();
    assert_eq!(*answers.borrow(), identity::Ownership::Owner);
    assert_eq!(picked(&cx), ["manager"]);
    assert!(!cx.has_text(WARNING));
    assert!(cx.has_text("That didn’t go through: account 7 does not manage account 12"));
}

#[test]
fn a_created_agent_closes_its_sheet() {
    let mut cx = agents();
    cx.simulate_click("settings/agents/create/open");
    cx.simulate_click("settings/agents/create/submit");
    assert!(cx.has_text("Enter an agent name."));
    cx.host()
        .handle::<Submit<Identity>>(|_| Ok(view::testing::applied(Vec::new())));
    cx.simulate_input("settings/agents/create/name", "Bot");
    cx.simulate_click("settings/agents/create/submit");
    cx.run_until_parked();
    assert_eq!(
        sent(&cx),
        [identity::Op::CreateAgent { name: "Bot".into() }]
    );
    assert!(cx.find("settings/agents/create/sheet").is_none());
}

#[test]
fn a_refused_create_keeps_its_sheet_and_says_why() {
    let mut cx = agents();
    cx.host()
        .refuse::<Submit<Identity>>("invalid", "a name is taken");
    cx.simulate_click("settings/agents/create/open");
    cx.simulate_input("settings/agents/create/name", "Bot");
    cx.simulate_click("settings/agents/create/submit");
    cx.run_until_parked();
    assert!(cx.find("settings/agents/create/sheet").is_some());
    assert!(cx.has_text("That didn’t go through: a name is taken"));
}

#[test]
fn an_added_key_closes_its_sheet() {
    let mut cx = agents();
    cx.simulate_click("settings/agents/12");
    cx.simulate_click("settings/agents/12/key/open");
    cx.simulate_input("settings/agents/key/request", "zz");
    cx.simulate_click("settings/agents/key/submit");
    assert!(cx.has_text("That isn’t a key request for one of your agents."));
    let add = identity::Op::AddKey {
        scheme: abi::Scheme::Ed25519,
        label: Some("sandbox".into()),
        consent: identity::Consent {
            key: vec![0x51; 32],
            account: 12,
            expires_at: 500,
            proof: vec![0x52; 64],
        },
    };
    cx.host()
        .handle::<Submit<Identity>>(|_| Ok(view::testing::applied(Vec::new())));
    cx.simulate_input("settings/agents/key/request", &abi::hex(&abi::encode(&add)));
    cx.simulate_click("settings/agents/key/submit");
    cx.run_until_parked();
    assert_eq!(sent(&cx), [add]);
    assert!(cx.find("settings/agents/key/sheet").is_none());
}

#[test]
fn a_landing_leaves_a_sheet_opened_meanwhile() {
    let mut cx = agents();
    cx.simulate_click("settings/agents/12");
    cx.host()
        .handle::<Submit<Identity>>(|_| Ok(view::testing::applied(Vec::new())));
    let suspend = press(&cx, "settings/agents/12/suspend");
    let rename = press(&cx, "settings/agents/12/rename/open");
    cx.tick(vec![suspend, rename]);
    assert!(cx.find("settings/agents/12/name").is_some());
    cx.run_until_parked();
    assert!(
        cx.find("settings/agents/12/name").is_some(),
        "the suspend's landing took the rename away: {:?}",
        cx.texts()
    );
}

#[test]
fn a_create_landing_leaves_an_agents_sheet_open() {
    let mut cx = agents();
    cx.simulate_click("settings/agents/create/open");
    cx.simulate_input("settings/agents/create/name", "Bot");
    cx.host()
        .handle::<Submit<Identity>>(|_| Ok(view::testing::applied(view::methods::encode(&31u64))));
    let create = press(&cx, "settings/agents/create/submit");
    let cancel = press(&cx, "settings/agents/create/cancel");
    let agent = press(&cx, "settings/agents/12");
    cx.tick(vec![create, cancel, agent]);
    assert!(cx.find("settings/agents/12/sheet").is_some());
    cx.run_until_parked();
    assert!(
        cx.find("settings/agents/12/sheet").is_some(),
        "the create's landing closed Scout's sheet: {:?}",
        cx.texts()
    );
}

#[test]
fn a_connected_agent_goes_on_to_its_key_sheet() {
    let mut cx = agents();
    cx.simulate_click("settings/agents/create/open");
    cx.simulate_input("settings/agents/create/name", "Pager");
    cx.host()
        .handle::<Submit<Identity>>(|_| Ok(view::testing::applied(view::methods::encode(&31u64))));
    cx.simulate_click("settings/agents/create/submit");
    cx.run_until_parked();
    assert!(cx.find("settings/agents/create/sheet").is_none());
    assert!(
        cx.find("settings/agents/key/sheet").is_some(),
        "{:?}",
        cx.texts()
    );
    let asks = "Created account 31. On the agent's machine, sign a key request for account 31, then paste it here.";
    assert!(cx.has_text(asks), "{:?}", cx.texts());
    assert!(cx.find("settings/agents/key/open").is_none());
}

#[test]
fn back_goes_with_the_sheet_a_failed_read_took() {
    let mut cx = agents();
    cx.simulate_click("settings/agents/12");
    assert!(cx.back_handler().is_some());
    cx.host()
        .refuse::<Query<Identity>>("unavailable", "Account query refused.");
    identity_moves(&mut cx, 50);
    assert!(cx.find("settings/agents/12/sheet").is_none());
    assert_eq!(cx.back_handler(), None);
}

#[test]
fn a_closed_sheet_forgets_what_was_not_sent() {
    let mut cx = agents();
    cx.simulate_click("settings/agents/create/open");
    cx.simulate_input("settings/agents/create/name", "Half");
    cx.simulate_back();
    cx.simulate_click("settings/agents/12");
    cx.simulate_click("settings/agents/12/key/open");
    cx.simulate_input("settings/agents/key/request", "zz");
    cx.simulate_click("settings/agents/key/submit");
    assert!(cx.has_text("That isn’t a key request for one of your agents."));
    cx.simulate_back();
    cx.simulate_click("settings/agents/create/open");
    let Some(wire::Node::Field { value, .. }) = cx.find("settings/agents/create/name") else {
        panic!("the name field: {:?}", cx.texts())
    };
    assert_eq!(value, "");
    cx.simulate_back();
    cx.simulate_click("settings/agents/12");
    cx.simulate_click("settings/agents/12/key/open");
    assert!(!cx.has_text("That isn’t a key request for one of your agents."));
}

#[test]
fn a_name_field_takes_the_keyboard_as_it_opens() {
    let mut cx = agents();
    cx.simulate_click("settings/agents/create/open");
    assert_eq!(focused(&cx).as_deref(), Some("settings/agents/create/name"));
    cx.simulate_back();
    cx.simulate_click("settings/agents/12");
    cx.simulate_click("settings/agents/12/rename/open");
    assert_eq!(focused(&cx).as_deref(), Some("settings/agents/12/name"));
}

#[test]
fn a_refused_choice_has_its_own_line() {
    let (mut cx, _) = answering(listed(&[8]));
    on_phone(&mut cx);
    cx.host()
        .refuse::<Submit<Identity>>("unauthorized", "account 7 does not manage account 12");
    cx.simulate_click("settings/agents/12");
    cx.simulate_click("settings/agents/12/answers/open");
    cx.simulate_click("settings/agents/12/answers/anyone");
    cx.run_until_parked();
    assert!(cx.has_text("That didn’t go through: account 7 does not manage account 12"));
    let Some(wire::Node::Field { options, .. }) = cx.find("settings/agents/12/answers/name") else {
        panic!("the person field: {:?}", cx.texts())
    };
    assert!(
        options.invalid.is_none(),
        "the refusal is the choice's, not the field's"
    );
}

#[test]
fn whom_it_answers_opens_tall_and_scrolls() {
    let mut cx = agents();
    cx.simulate_click("settings/agents/12");
    let Some(wire::Node::Sheet { detents, .. }) = cx.find("settings/agents/12/sheet") else {
        panic!("the agent's sheet")
    };
    assert!(detents.is_empty(), "the actions sheet fits: {detents:?}");
    cx.simulate_click("settings/agents/12/answers/open");
    let wire::Node::Sheet { detents, .. } = cx.node("settings/agents/12/sheet") else {
        panic!("the agent's sheet")
    };
    assert_eq!(detents, &[0.85]);
    assert!(cx.find("settings/agents/12/scroll").is_some());
}

#[test]
fn a_remove_waits_for_a_choice_that_is_out() {
    let (mut cx, answers) = answering(listed(&[8, 9]));
    on_phone(&mut cx);
    cx.simulate_click("settings/agents/12");
    cx.simulate_click("settings/agents/12/answers/open");
    let anyone = press(&cx, "settings/agents/12/answers/anyone");
    let remove = press(&cx, "settings/agents/12/answers/8/remove");
    cx.tick(vec![anyone, remove]);
    assert!(cx.has_text("Who it answers · Saving…"));
    assert!(
        cx.interactivity("settings/agents/12/answers/8/remove")
            .on_click
            .is_none()
    );
    cx.run_until_parked();
    assert_eq!(
        answers_sent(&cx).len(),
        1,
        "a Remove went out: {:?}",
        answers_sent(&cx)
    );
    assert_eq!(*answers.borrow(), identity::Ownership::Anyone);
}

#[test]
fn a_person_add_landing_leaves_a_sheet_opened_meanwhile() {
    let (mut cx, answers) = answering(identity::Ownership::Owner);
    on_phone(&mut cx);
    cx.simulate_click("settings/agents/12");
    cx.simulate_click("settings/agents/12/answers/open");
    cx.simulate_click("settings/agents/12/answers/specific");
    cx.simulate_input("settings/agents/12/answers/name", "#9");
    let add = press(&cx, "settings/agents/12/answers/add");
    let wire::Node::Sheet {
        on_dismiss: Some(dismiss),
        ..
    } = cx.node("settings/agents/12/sheet")
    else {
        panic!("the agent's sheet")
    };
    let dismiss = wire::Event::Message(*dismiss);
    let create = press(&cx, "settings/agents/create/open");
    cx.tick(vec![add, dismiss, create]);
    cx.run_until_parked();
    assert_eq!(*answers.borrow(), listed(&[9]));
    assert!(
        cx.find("settings/agents/create/sheet").is_some(),
        "the add's landing closed the Create sheet: {:?}",
        cx.texts()
    );
}

#[test]
fn whom_it_answers_waits_while_any_write_is_out() {
    let (mut cx, _) = answering(listed(&[8]));
    on_phone(&mut cx);
    cx.host().never::<Submit<Identity>>();
    cx.simulate_click("settings/agents/12");
    cx.simulate_click("settings/agents/12/suspend");
    cx.simulate_click("settings/agents/12/answers/open");
    has_all(&cx, &["Who it answers · Saving…", "Adding…"]);
    for off in [
        "settings/agents/12/answers/8/remove",
        "settings/agents/12/answers/add",
    ] {
        assert!(cx.interactivity(off).on_click.is_none(), "{off} is on");
    }
}

#[test]
fn return_sends_only_the_create_fields() {
    let submits = |cx: &TestAppContext, key: &str| {
        let Some(wire::Node::Field { on_submit, .. }) = cx.find(key) else {
            panic!("no field {key}");
        };
        on_submit.is_some()
    };
    assert!(submits(
        &phone("unregistered"),
        "settings/account/create/name"
    ));
    let mut cx = agents();
    cx.simulate_click("settings/agents/create/open");
    assert!(submits(&cx, "settings/agents/create/name"));
    cx.simulate_back();
    cx.simulate_click("settings/agents/12");
    cx.simulate_click("settings/agents/12/rename/open");
    assert!(!submits(&cx, "settings/agents/12/name"));
    cx.simulate_click("settings/agents/12/rename/cancel");
    cx.simulate_click("settings/agents/12/answers/open");
    cx.simulate_click("settings/agents/12/answers/specific");
    assert!(!submits(&cx, "settings/agents/12/answers/name"));
}

#[test]
fn the_phones_fields_carry_their_keyboard_hints() {
    let hints = |cx: &TestAppContext, key: &str| {
        let Some(wire::Node::Field { options, .. }) = cx.find(key) else {
            panic!("no field {key}");
        };
        options.text_input
    };
    let account = phone("unregistered");
    let name = hints(&account, "settings/account/create/name");
    assert_eq!(name.autocapitalize, wire::Autocapitalize::Sentences);

    let mut cx = agents();
    cx.simulate_click("settings/agents/12");
    cx.simulate_click("settings/agents/12/key/open");
    let request = hints(&cx, "settings/agents/key/request");
    assert!(!request.autocorrect && !request.suggestions);
    assert_eq!(request.autocapitalize, wire::Autocapitalize::None);
    cx.simulate_back();
    cx.simulate_click("settings/agents/create/open");
    let name = hints(&cx, "settings/agents/create/name");
    assert_eq!(name.autocapitalize, wire::Autocapitalize::Sentences);
    cx.simulate_back();
    cx.simulate_click("settings/agents/12");
    cx.simulate_click("settings/agents/12/rename/open");
    let rename = hints(&cx, "settings/agents/12/name");
    assert_eq!(rename.autocapitalize, wire::Autocapitalize::Sentences);
    cx.simulate_click("settings/agents/12/rename/cancel");
    cx.simulate_click("settings/agents/12/answers/open");
    cx.simulate_click("settings/agents/12/answers/specific");
    let person = hints(&cx, "settings/agents/12/answers/name");
    assert!(!person.autocorrect);
    assert_eq!(person.autocapitalize, wire::Autocapitalize::None);
}
