//! Who can edit a repository and a change (`policies.rs`): the rows and sheets say what ownership
//! records, its owner relinks from them, and what the reader may do follows the item's policy once
//! read, the rule from before policies for an item with no record.
use super::*;
use ownership::{Op as Owned, Writers};

fn custody() -> Vec<u8> {
    forge::tables::CUSTODY.key(&"project".to_owned())
}

fn authorship() -> Vec<u8> {
    forge::tables::AUTHORSHIP.key(&("project".to_owned(), 1u64))
}

fn account(number: u64) -> ownership::Party {
    ownership::Party::Account(number)
}

// A policy of account `owner`'s.
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

// An item on record: forge's key, its owner's account and its policy.
type Held = (Vec<u8>, u64, ownership::Policy);

// What ownership answers: each of `records` on record and any other key with none, `policies` as
// the owner's; identity names the accounts as chat's roster does, and every ownership write lands.
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
            query => panic!("unexpected identity query: {query:?}"),
        })
    });
    cx.host()
        .handle::<Submit<ownership::Ownership>>(|_| Ok(view::testing::applied(Vec::new())));
}

// `project` open as `account`, ownership answering with `records` on record before it opens.
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

// Change 1 of `project`, on its Conversation tab, the details beside it.
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
    assert!(cx.has_text("You · Only me · Only you"), "{:?}", cx.texts());
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
fn the_repository_policy_decides_merge_and_settings_over_the_access_list() {
    let team = a_policy(5, 1, "Team", Writers::Listed(vec![account(2)]));
    let records = || vec![(custody(), 1, team.clone())];
    // Rae holds no write in access's list, but the repository's policy lists her
    let (cx, _) = change_owned(2, records(), vec![team.clone()]);
    assert!(!disabled(&cx, "forge-merge"), "the policy admits Rae");
    let (mut cx, _) = opened_owned(2, records(), vec![team.clone()]);
    settings(&mut cx);
    assert!(!disabled(&cx, "forge-settings-save"));
    assert!(
        disabled(&cx, "forge-settings-grant"),
        "access's list stays its owner's to set"
    );
    // access lists Wren, but the policy does not
    let (cx, _) = change_owned(9, records(), vec![team]);
    assert!(disabled(&cx, "forge-merge"), "the policy leaves Wren out");
}

#[test]
fn the_change_details_say_who_can_edit_it_and_its_policy_decides_edit() {
    let open = a_policy(3, 1, "Open", Writers::Anyone);
    let (cx, _) = change_owned(2, vec![(authorship(), 1, open.clone())], vec![open]);
    assert!(cx.find("forge-change-policy").is_some());
    assert!(cx.has_text("Ada · Open · Anyone"), "{:?}", cx.texts());
    assert!(
        !disabled(&cx, "forge-edit-change"),
        "the change's policy admits Rae"
    );
    assert!(
        disabled(&cx, "forge-merge"),
        "the repository has no record: as before policies, Rae writes nothing"
    );
}

#[test]
fn the_owner_on_record_decides_and_an_author_still_closes_their_own_change() {
    // Tal holds `project` on record, on a policy only Tal writes under: Ada, whom forge names its
    // owner and who wrote change 1, merges and configures nothing, and still closes her change
    let tals = a_policy(6, 4, "Only me", Writers::Owner);
    let records = || vec![(custody(), 4, tals.clone())];
    let (cx, _) = change_owned(1, records(), vec![tals.clone()]);
    assert!(disabled(&cx, "forge-merge"));
    assert!(!disabled(&cx, "forge-close-change"));
    let (mut cx, _) = opened_owned(1, records(), vec![tals]);
    settings(&mut cx);
    assert!(cx.has_text("Tal · Only me · Only Tal"), "{:?}", cx.texts());
    assert!(disabled(&cx, "forge-settings-save"));
    assert!(
        !disabled(&cx, "forge-settings-grant"),
        "access's list is still the one forge names its owner's"
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
    assert!(cx.has_text("You · Only me · Only you"), "{:?}", cx.texts());
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
