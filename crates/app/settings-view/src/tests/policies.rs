//! The Policies pane, on both designs: the reader's policies in words, what new items in each
//! scope use, and each op its sheets send the ownership program.
use super::*;
use ownership::{Linked, Ownership, Party, Policy, Scope, ScopeDefault, Writers};

fn page<T>(items: Vec<T>) -> ownership::PageResponse<T> {
    ownership::PageResponse {
        height: 42,
        items,
        next: None,
    }
}

fn policy(id: u64, name: &str, write: Writers) -> Policy {
    Policy {
        id,
        owner: Party::Account(7),
        name: name.into(),
        write,
        retired: false,
        switched_to: None,
    }
}

pub(super) fn ada() -> identity::Profile {
    identity::Profile {
        number: 9,
        name: "Ada".into(),
        kind: identity::Kind::User,
    }
}

pub(super) fn owning(cx: &TestAppContext) {
    cx.host().handle::<Query<Ownership>>(|q| {
        Ok(match q {
            ownership::Query::Policies {
                owner: Party::Account(7),
                ..
            } => {
                let team = Writers::Listed(vec![Party::Account(9), Party::Program("board".into())]);
                ownership::Reply::Policies(page(vec![
                    policy(1, ownership::OPEN_POLICY, Writers::Anyone),
                    policy(2, "Team", team),
                    policy(3, ownership::ONLY_ME, Writers::Owner),
                ]))
            }
            ownership::Query::Open {
                owner: Party::Account(7),
            } => ownership::Reply::Open(Some(1)),
            ownership::Query::OnlyMe {
                owner: Party::Account(7),
            } => ownership::Reply::OnlyMe(Some(3)),
            ownership::Query::Defaults {
                owner: Party::Account(7),
                ..
            } => ownership::Reply::Defaults(page(vec![
                ScopeDefault {
                    scope: Scope::Kind("chat.message".into()),
                    policy: 2,
                },
                ScopeDefault {
                    scope: Scope::Program("files".into()),
                    policy: 2,
                },
            ])),
            ownership::Query::Linked { policy: 2, .. } => {
                let item = Linked {
                    program: "chat".into(),
                    key: vec![1],
                };
                ownership::Reply::Linked(page(vec![item; 3]))
            }
            q => panic!("unexpected query: {q:?}"),
        })
    });
}

fn policies() -> TestAppContext {
    let mut cx = fixture("ready", false);
    cx.host()
        .handle::<Submit<Ownership>>(|_| Ok(view::testing::applied(Vec::new())));
    cx.simulate_click("settings/nav/policies");
    cx.run_until_parked();
    cx
}

fn sent(cx: &TestAppContext) -> Vec<ownership::Op> {
    cx.host().requests::<Submit<Ownership>>()
}

fn click(cx: &mut TestAppContext, key: &str) {
    cx.simulate_click(key);
    cx.run_until_parked();
}

#[test]
fn the_pane_says_each_policy_in_words_and_what_new_items_use() {
    let cx = policies();
    for text in [
        "Open (built in)",
        "Anyone · never changes",
        "Only me (built in)",
        "Team",
        "You, Ada and board",
        "Everything",
        "Everything in chat",
        "Everything in files",
        "Chat messages",
        "Egress services",
        "Agents' conversations",
        "Inherit · Only me",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    assert!(cx.interactivity("settings/policies/1").on_click.is_none());
    assert!(cx.interactivity("settings/policies/3").on_click.is_none());
    assert!(cx.interactivity("settings/policies/2").on_click.is_some());
    for scope in [
        "all",
        "program-egress",
        "program-files",
        "kind-egress.service",
    ] {
        let row = format!("settings/policies/default/{scope}");
        assert!(cx.find(&row).is_some(), "{row}");
    }
}

#[test]
fn a_new_policy_is_named_and_lists_whom_it_lets_write() {
    let mut cx = policies();
    click(&mut cx, "settings/policies/new");
    click(&mut cx, "settings/policies/edit/submit");
    assert!(cx.has_text("Name the policy."), "{:?}", cx.texts());
    assert!(sent(&cx).is_empty());
    cx.simulate_input("settings/policies/edit/name", "Reviewers");
    click(&mut cx, "settings/policies/edit/write/specific");
    cx.simulate_input("settings/policies/edit/person", "#9");
    click(&mut cx, "settings/policies/edit/people/add");
    assert!(cx.find("settings/policies/edit/people/9").is_some());
    click(&mut cx, "settings/policies/edit/submit");
    let write = Writers::Listed(vec![Party::Account(9)]);
    let created = ownership::Op::Create {
        name: "Reviewers".into(),
        write,
    };
    assert_eq!(sent(&cx), [created]);
    assert!(cx.find("settings/policies/sheet").is_none());
}

#[test]
fn an_edit_starts_from_the_policy_and_sends_what_changed() {
    let mut cx = policies();
    click(&mut cx, "settings/policies/2");
    click(&mut cx, "settings/policies/2/edit");
    assert!(cx.find("settings/policies/edit/people/9").is_some());
    assert!(cx.has_text("Ada"), "{:?}", cx.texts());
    click(
        &mut cx,
        "settings/policies/edit/people/program-board/remove",
    );
    click(&mut cx, "settings/policies/edit/submit");
    let edited = ownership::Op::Edit {
        policy: 2,
        name: "Team".into(),
        write: Writers::Listed(vec![Party::Account(9)]),
    };
    assert_eq!(sent(&cx), [edited]);
}

#[test]
fn remove_says_how_many_items_keep_the_policy_and_retires_it() {
    let mut cx = policies();
    click(&mut cx, "settings/policies/2");
    click(&mut cx, "settings/policies/2/remove");
    assert!(cx.has_text("On it now: 3 items."), "{:?}", cx.texts());
    click(&mut cx, "settings/policies/remove/submit");
    assert_eq!(sent(&cx), [ownership::Op::Retire { policy: 2 }]);
    assert!(cx.find("settings/policies/sheet").is_none());
}

#[test]
fn move_switches_everything_to_the_policy_picked() {
    let mut cx = policies();
    click(&mut cx, "settings/policies/2");
    click(&mut cx, "settings/policies/2/move");
    assert!(cx.has_text("On it now: 3 items."), "{:?}", cx.texts());
    assert!(cx.find("settings/policies/move/2").is_none());
    click(&mut cx, "settings/policies/move/1");
    assert_eq!(sent(&cx), [ownership::Op::Switch { from: 2, to: 1 }]);
}

#[test]
fn a_default_is_picked_and_put_back_to_inherit() {
    let mut cx = policies();
    click(&mut cx, "settings/policies/default/kind-chat.channel");
    click(&mut cx, "settings/policies/default/pick/2");
    click(&mut cx, "settings/policies/default/kind-chat.message");
    assert!(
        cx.has_text("In use · You, Ada and board"),
        "{:?}",
        cx.texts()
    );
    click(&mut cx, "settings/policies/default/inherit");
    let channel = Scope::Kind("chat.channel".into());
    let message = Scope::Kind("chat.message".into());
    let set = |scope, policy| ownership::Op::SetDefault { scope, policy };
    assert_eq!(sent(&cx), [set(channel, Some(2)), set(message, None)]);
    click(&mut cx, "settings/policies/default/kind-chat.message");
    click(&mut cx, "settings/policies/default/pick/2");
    assert_eq!(sent(&cx).len(), 2);
}

#[test]
fn a_refusal_stays_on_its_sheet() {
    let mut cx = policies();
    cx.host()
        .refuse::<Submit<Ownership>>("unauthorized", "policy Team (#2) is account 8's");
    click(&mut cx, "settings/policies/2");
    click(&mut cx, "settings/policies/2/remove");
    click(&mut cx, "settings/policies/remove/submit");
    assert!(
        cx.has_text("That didn’t go through: policy Team (#2) is account 8's"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.find("settings/policies/sheet").is_some());
}

#[test]
fn a_block_that_writes_the_readers_policies_reads_them_again() {
    let mut cx = policies();
    let read = |cx: &TestAppContext| cx.host().requests::<Query<Ownership>>().len();
    let before = read(&cx);
    let heads = cx.host().stream::<Changes<Ownership>>();
    heads.send(Some(Change {
        height: 50,
        keys: vec![b"record/x".to_vec()],
    }));
    cx.run_until_parked();
    assert_eq!(read(&cx), before);
    heads.send(Some(Change {
        height: 51,
        keys: vec![b"policy/x".to_vec()],
    }));
    cx.run_until_parked();
    assert!(read(&cx) > before);
}

#[test]
fn on_a_phone_the_strip_opens_policies_and_back_closes_a_sheet() {
    let mut cx = fixture("ready", false);
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    click(&mut cx, "settings/strip/policies");
    assert!(cx.has_text("You, Ada and board"), "{:?}", cx.texts());
    click(&mut cx, "settings/policies/2");
    assert!(cx.find("settings/policies/sheet").is_some());
    assert!(cx.back_handler().is_some());
    cx.simulate_back();
    cx.run_until_parked();
    assert!(cx.find("settings/policies/sheet").is_none());
    assert_eq!(cx.back_handler(), None);
}
