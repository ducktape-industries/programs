//! Who can edit a repository and a change (`policies.rs`): the rows and sheets say what ownership
//! records, its owner relinks from them, and what the reader may do follows the item's policy once
//! read, the rule from before policies for an item with no record.
use super::*;
use ownership::{Op as Owned, Writers};

fn custody() -> Vec<u8> {
    forge::tables::CUSTODY.key(&"project".to_owned())
}

fn settings_row_key() -> Vec<u8> {
    forge::tables::SETTINGS_CUSTODY.key(&"project".to_owned())
}

fn authorship() -> Vec<u8> {
    forge::tables::AUTHORSHIP.key(&("project".to_owned(), 1u64))
}

fn account(number: u64) -> ownership::Party {
    ownership::Party::Account(number)
}

fn a_policy(id: u64, owner: u64, name: &str, write: Writers) -> ownership::Policy {
    ownership::Policy {
        id,
        owner: account(owner),
        name: name.into(),
        write,
        retired: false,
        switched_to: None,
    }
}

type Held = (Vec<u8>, u64, ownership::Policy);

fn owned(cx: &TestAppContext, records: Vec<Held>, policies: Vec<ownership::Policy>) {
    cx.host()
        .handle::<ProgramQuery<ownership::Ownership>>(move |query| {
            let held = |asked: &Vec<u8>| records.iter().find(|(key, ..)| key == asked);
            Ok(match query {
                ownership::Query::Record { key, .. } => {
                    ownership::Reply::Record(held(&key).map(|(_, owner, policy)| {
                        ownership::Record {
                            owner: account(*owner),
                            policy: policy.id,
                            kind: None,
                        }
                    }))
                }
                ownership::Query::Effective { key, .. } => {
                    ownership::Reply::Policy(held(&key).map(|(.., policy)| policy.clone()))
                }
                ownership::Query::Policies { .. } => {
                    ownership::Reply::Policies(ownership::PageResponse {
                        height: 1,
                        items: policies.clone(),
                        next: None,
                    })
                }
                ownership::Query::Defaults { .. } => {
                    ownership::Reply::Defaults(ownership::PageResponse {
                        height: 1,
                        items: Vec::new(),
                        next: None,
                    })
                }
                ownership::Query::OnlyMe { .. } => ownership::Reply::OnlyMe(None),
                ownership::Query::Open { .. } => {
                    ownership::Reply::Open(policies.first().map(|policy| policy.id))
                }
                query => panic!("unexpected ownership query: {query:?}"),
            })
        });
    cx.host().handle::<ProgramQuery<Identity>>(|query| {
        Ok(match query {
            identity::Query::Profile { number } => {
                let name = match number {
                    1 => "Ada",
                    2 => "Rae",
                    4 => "Tal",
                    _ => "Wren",
                };
                identity::Reply::Profile(Some(identity::Profile {
                    number,
                    name: name.into(),
                    kind: identity::Kind::User,
                }))
            }
            identity::Query::List { .. } => no_accounts(),
            query => panic!("unexpected identity query: {query:?}"),
        })
    });
    cx.host()
        .handle::<Submit<ownership::Ownership>>(|_| Ok(view::testing::applied(Vec::new())));
}

fn opened_owned(
    account: u64,
    records: Vec<Held>,
    policies: Vec<ownership::Policy>,
) -> (TestAppContext, Entity<Forge>) {
    let (mut cx, view) = booted_as("default", account);
    owned(&cx, records, policies);
    cx.simulate_click("forge-repo-project-open");
    cx.run_until_parked();
    (cx, view)
}

fn change_owned(
    account: u64,
    records: Vec<Held>,
    policies: Vec<ownership::Policy>,
) -> (TestAppContext, Entity<Forge>) {
    let (mut cx, view) = opened_owned(account, records, policies);
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    cx.simulate_click("forge-change-1");
    cx.run_until_parked();
    (cx, view)
}

fn access_anyone(cx: &TestAppContext) {
    cx.host().handle::<ProgramQuery<access::Access>>(|query| {
        Ok(match query {
            access::Query::Writers { .. } => access::Reply::Writers(program::Writers::Anyone),
            access::Query::List { .. } => access::Reply::List(None),
            other => panic!("unexpected access query: {other:?}"),
        })
    });
}

fn opened_open(
    account: u64,
    records: Vec<Held>,
    policies: Vec<ownership::Policy>,
) -> (TestAppContext, Entity<Forge>) {
    let (mut cx, view) = booted_as("default", account);
    owned(&cx, records, policies);
    access_anyone(&cx);
    cx.simulate_click("forge-repo-project-open");
    cx.run_until_parked();
    (cx, view)
}

fn change_open(
    account: u64,
    records: Vec<Held>,
    policies: Vec<ownership::Policy>,
) -> (TestAppContext, Entity<Forge>) {
    let (mut cx, view) = opened_open(account, records, policies);
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    cx.simulate_click("forge-change-1");
    cx.run_until_parked();
    (cx, view)
}

fn settings(cx: &mut TestAppContext) {
    cx.simulate_click("forge-tab-settings");
    cx.run_until_parked();
}

#[test]
fn the_settings_say_who_can_edit_the_repository_and_its_owner_relinks_it() {
    let mine = a_policy(4, 1, "Only me", Writers::Owner);
    let team = a_policy(5, 1, "Team", Writers::Listed(vec![account(2)]));
    let (mut cx, _) = opened_owned(1, vec![(custody(), 1, mine.clone())], vec![mine, team]);
    settings(&mut cx);
    assert!(cx.has_text("Only you"), "{:?}", cx.texts());
    cx.simulate_click("forge-repo-policy");
    cx.run_until_parked();
    assert!(cx.find("forge-repo-policy/sheet").is_some());
    cx.simulate_click("forge-repo-policy/relink");
    cx.run_until_parked();
    assert!(cx.has_text("In use · Only you"), "{:?}", cx.texts());
    assert!(cx.has_text("You and Rae"), "{:?}", cx.texts());
    cx.simulate_click("forge-repo-policy/relink/5");
    cx.run_until_parked();
    let sent = cx.host().requests::<Submit<ownership::Ownership>>();
    assert!(
        matches!(sent.as_slice(), [Owned::Relink { program, key, policy: 5 }] if program == forge::PROGRAM && *key == custody()),
        "{sent:?}"
    );
    assert!(
        cx.find("forge-repo-policy/sheet").is_none(),
        "the landed relink closes it"
    );
}

#[test]
fn the_repository_policy_decides_merge_and_settings_over_an_open_access_list() {
    let team = a_policy(5, 1, "Team", Writers::Listed(vec![account(2)]));
    let records = || vec![(custody(), 1, team.clone())];
    let (cx, _) = change_open(2, records(), vec![team.clone()]);
    assert!(!disabled(&cx, "forge-merge"), "the policy admits Rae");
    let (mut cx, _) = opened_open(2, records(), vec![team.clone()]);
    settings(&mut cx);
    assert!(!disabled(&cx, "forge-settings-save"));
    assert!(
        disabled(&cx, "forge-settings-grant"),
        "access's list stays its owner's to set"
    );
    let (cx, _) = change_open(9, records(), vec![team]);
    assert!(disabled(&cx, "forge-merge"), "the policy leaves Wren out");
}

#[test]
fn the_change_details_say_who_can_edit_it_and_its_policy_decides_edit() {
    let open = a_policy(3, 1, "Open", Writers::Anyone);
    let (cx, _) = change_open(2, vec![(authorship(), 1, open.clone())], vec![open]);
    assert!(cx.find("forge-change-policy").is_some());
    assert!(cx.has_text("Anyone"), "{:?}", cx.texts());
    assert!(
        !disabled(&cx, "forge-edit-change"),
        "the change's policy admits Rae"
    );
    assert!(
        !disabled(&cx, "forge-merge"),
        "the repository has no record, so no policy narrows it: the guard leaves it open"
    );
}

#[test]
fn the_owner_on_record_decides_and_an_author_still_closes_their_own_change() {
    let tals = a_policy(6, 4, "Only me", Writers::Owner);
    let records = || vec![(custody(), 4, tals.clone())];
    let (cx, _) = change_owned(1, records(), vec![tals.clone()]);
    assert!(disabled(&cx, "forge-merge"));
    assert!(!disabled(&cx, "forge-close-change"));
    let (mut cx, _) = opened_owned(1, records(), vec![tals]);
    settings(&mut cx);
    assert!(cx.has_text("Only Tal"), "{:?}", cx.texts());
    assert!(disabled(&cx, "forge-settings-save"));
    assert!(
        disabled(&cx, "forge-settings-grant"),
        "the owner on record sets who writes it"
    );
}

fn roles(cx: &TestAppContext, access: bool) {
    cx.host()
        .handle::<ProgramQuery<programs::Programs>>(move |query| {
            Ok(match query {
                programs::Query::Roles => {
                    let bound = access.then_some((abi::role::ACCESS, "access"));
                    programs::Reply::Roles(abi::Roles::from_iter(bound))
                }
                query => panic!("unexpected programs query: {query:?}"),
            })
        });
}

fn makes_policy_seven(cx: &TestAppContext) {
    cx.host().handle::<Submit<ownership::Ownership>>(|op| {
        let output = match op {
            Owned::Create { .. } => abi::encode(&7u64),
            _ => Vec::new(),
        };
        Ok(view::testing::applied(output))
    });
}

fn grant_rae(cx: &mut TestAppContext) {
    settings(cx);
    cx.simulate_input("forge-settings-grant-input", "2");
    cx.simulate_click("forge-settings-grant");
    cx.run_until_parked();
}

#[test]
fn a_grant_relinks_the_repository_to_its_owners_policy_with_those_writers() {
    let mine = a_policy(4, 1, "Only me", Writers::Owner);
    let team = a_policy(5, 1, "Team", Writers::Listed(vec![account(2)]));
    let records = vec![(custody(), 1, mine.clone())];
    let (mut cx, _) = opened_owned(1, records, vec![mine, team]);
    grant_rae(&mut cx);
    assert_eq!(
        set_lists(&cx),
        [listed(&[2])],
        "access's list too, while access gates"
    );
    let sent = cx.host().requests::<Submit<ownership::Ownership>>();
    assert!(
        matches!(sent.as_slice(), [Owned::Relink { key, policy: 5, .. }] if *key == custody()),
        "{sent:?}"
    );
}

#[test]
fn with_access_unbound_a_grant_sends_no_access_list_and_makes_the_policy_it_needs() {
    let mine = a_policy(4, 1, "Only me", Writers::Owner);
    let (mut cx, _) = opened_owned(1, vec![(custody(), 1, mine.clone())], vec![mine]);
    roles(&cx, false);
    makes_policy_seven(&cx);
    grant_rae(&mut cx);
    assert!(
        cx.host().requests::<Submit<access::Access>>().is_empty(),
        "access is unbound: nothing is sent to it"
    );
    let sent = cx.host().requests::<Submit<ownership::Ownership>>();
    let rae = Writers::Listed(vec![account(2)]);
    assert!(
        matches!(
            sent.as_slice(),
            [
                Owned::Create { name, write },
                Owned::Relink { key, policy: 7, .. },
            ] if name == "project writers" && *write == rae && *key == custody()
        ),
        "{sent:?}"
    );
}

#[test]
fn on_a_phone_the_settings_and_the_details_carry_the_rows_and_back_closes_the_sheet() {
    let mine = a_policy(4, 1, "Only me", Writers::Owner);
    let records = vec![
        (custody(), 1, mine.clone()),
        (authorship(), 1, mine.clone()),
    ];
    let mut cx = TestAppContext::new();
    configure(&mut cx, "default");
    owned(&cx, records, vec![mine]);
    let props = cx.host().stream::<HostSession>();
    let _view = cx.open::<Forge>();
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
    props.send(seat(b"owner", Some(1)));
    cx.run_until_parked();
    let press = |cx: &mut TestAppContext, id: &str| {
        cx.simulate_click(id);
        cx.run_until_parked();
    };
    press(&mut cx, "forge-repo-project");
    press(&mut cx, "forge-tab-settings");
    assert!(cx.has_text("Only you"), "{:?}", cx.texts());
    press(&mut cx, "forge-repo-policy");
    assert!(cx.find("forge-repo-policy/sheet").is_some());
    cx.simulate_back();
    cx.run_until_parked();
    assert!(cx.find("forge-repo-policy/sheet").is_none());
    assert!(
        cx.find("forge-page-repo").is_some(),
        "Back closed the sheet, not the page"
    );
    press(&mut cx, "forge-tab-changes");
    press(&mut cx, "forge-change-project-1");
    press(&mut cx, "forge-change-tab-details");
    assert!(cx.find("forge-change-policy").is_some());
    press(&mut cx, "forge-change-policy");
    assert!(cx.find("forge-change-policy/sheet").is_some());
}

fn access_closed_to_its_owner(cx: &TestAppContext) {
    cx.host().handle::<ProgramQuery<access::Access>>(|query| {
        Ok(match query {
            access::Query::Writers { key, .. } => {
                access::Reply::Writers(program::Writers::Listed {
                    at: key,
                    owner: Some(Party::Account(1)),
                    list: Ownership::Owner,
                })
            }
            access::Query::List { .. } => access::Reply::List(Some(Ownership::Owner)),
            other => panic!("unexpected access query: {other:?}"),
        })
    });
}

#[test]
fn an_access_list_narrower_than_the_policy_is_what_the_writers_row_and_merge_follow() {
    let open = a_policy(3, 1, "Open", Writers::Anyone);
    let (mut cx, _) = booted_as("default", 2);
    owned(&cx, vec![(custody(), 1, open.clone())], vec![open]);
    access_closed_to_its_owner(&cx);
    cx.simulate_click("forge-repo-project-open");
    cx.run_until_parked();
    settings(&mut cx);
    assert!(
        cx.find("forge-writer-everyone").is_none(),
        "access lets only the owner write: {:?}",
        cx.texts()
    );
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    cx.simulate_click("forge-change-1");
    cx.run_until_parked();
    assert!(disabled(&cx, "forge-merge"), "access refuses Rae's merge");
}

#[test]
fn the_writers_row_lists_only_whom_both_the_policy_and_access_list() {
    let both = a_policy(
        6,
        1,
        "Rae and Wren",
        Writers::Listed(vec![account(2), account(9)]),
    );
    let (mut cx, _) = opened_owned(1, vec![(custody(), 1, both.clone())], vec![both]);
    settings(&mut cx);
    assert!(
        cx.find("forge-writer-acct-9").is_some(),
        "both list Wren: {:?}",
        cx.texts()
    );
    assert!(
        cx.find("forge-writer-acct-2").is_none(),
        "access leaves Rae out: {:?}",
        cx.texts()
    );
}

#[test]
fn settings_with_a_record_of_their_own_follow_it_and_their_owner_relinks_them() {
    let team = a_policy(5, 1, "Team", Writers::Listed(vec![account(2)]));
    let mine = a_policy(4, 1, "Only me", Writers::Owner);
    let records = || {
        vec![
            (custody(), 1, team.clone()),
            (settings_row_key(), 1, mine.clone()),
        ]
    };
    let policies = || vec![mine.clone(), team.clone()];
    let (cx, _) = change_open(2, records(), policies());
    assert!(
        !disabled(&cx, "forge-merge"),
        "the repository's policy admits Rae"
    );
    let (mut cx, view) = opened_open(2, records(), policies());
    settings(&mut cx);
    assert!(
        disabled(&cx, "forge-settings-save"),
        "the settings' own policy leaves Rae out"
    );
    assert!(
        cx.find("forge-settings-policy").is_some(),
        "{:?}",
        cx.texts()
    );
    cx.update(&view, |forge, _, cx| {
        forge.settings_policy.standing = Default::default();
        cx.notify();
    });
    cx.run_until_parked();
    assert!(
        disabled(&cx, "forge-settings-save"),
        "until the settings are read, the owner rule holds, not the repository's policy"
    );
    let (mut cx, _) = opened_open(1, records(), policies());
    settings(&mut cx);
    assert!(cx.has_text("Who can change settings"), "{:?}", cx.texts());
    assert!(!disabled(&cx, "forge-settings-save"));
    cx.simulate_click("forge-settings-policy");
    cx.run_until_parked();
    cx.simulate_click("forge-settings-policy/relink");
    cx.run_until_parked();
    cx.simulate_click("forge-settings-policy/relink/5");
    cx.run_until_parked();
    let sent = cx.host().requests::<Submit<ownership::Ownership>>();
    assert!(
        matches!(sent.as_slice(), [Owned::Relink { key, policy: 5, .. }] if *key == settings_row_key()),
        "{sent:?}"
    );
}

#[test]
fn settings_with_no_record_of_their_own_follow_the_repository_and_show_no_row() {
    let team = a_policy(5, 1, "Team", Writers::Listed(vec![account(2)]));
    let (mut cx, _) = opened_open(2, vec![(custody(), 1, team.clone())], vec![team]);
    settings(&mut cx);
    assert!(!disabled(&cx, "forge-settings-save"));
    assert!(
        cx.find("forge-settings-policy").is_none(),
        "{:?}",
        cx.texts()
    );
}
