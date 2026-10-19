//! Who can start an agent's conversations and who can edit a model (`policies.rs`): the rows say
//! what ownership records, the manager claims an agent's conversations nobody holds, and what the
//! reader may do follows the item's policy once read, the rule from before policies without one.
use super::*;
use ownership::{Op as Owned, Writers};

// An item on record: harness's key, its owner's account and its policy.
type Held = (Vec<u8>, u64, ownership::Policy);

fn a_policy(id: u64, owner: u64, name: &str, write: Writers) -> ownership::Policy {
    ownership::Policy {
        id,
        owner: ownership::Party::Account(owner),
        name: name.into(),
        write,
        retired: false,
        switched_to: None,
    }
}

fn scouts() -> Vec<u8> {
    harness::tables::CONVERSATIONS.key(&SCOUT)
}

fn sonnet() -> Vec<u8> {
    harness::tables::MODELS.key(&SONNET.to_owned())
}

// What ownership answers: each of `records` on record and any other key with none, `policies` as
// every owner's; every ownership write lands.
fn owned(cx: &TestAppContext, records: Vec<Held>, policies: Vec<ownership::Policy>) {
    cx.host()
        .handle::<Query<ownership::Ownership>>(move |query| {
            let held = |asked: &Vec<u8>| records.iter().find(|(key, ..)| key == asked);
            Ok(match query {
                ownership::Query::Record { key, .. } => {
                    ownership::Reply::Record(held(&key).map(|(_, owner, policy)| {
                        ownership::Record {
                            owner: ownership::Party::Account(*owner),
                            policy: policy.id,
                            kind: None,
                        }
                    }))
                }
                ownership::Query::Effective { key, .. } => {
                    ownership::Reply::Policy(held(&key).map(|(.., policy)| policy.clone()))
                }
                ownership::Query::Policies { .. } => {
                    ownership::Reply::Policies(page(policies.clone()))
                }
                ownership::Query::Defaults { .. } => ownership::Reply::Defaults(page(Vec::new())),
                ownership::Query::Open { .. } => {
                    ownership::Reply::Open(policies.first().map(|policy| policy.id))
                }
                query => panic!("unexpected ownership query: {query:?}"),
            })
        });
    cx.host()
        .handle::<Submit<ownership::Ownership>>(|_| Ok(applied(Vec::new())));
}

fn harness_takes(cx: &TestAppContext) {
    cx.host()
        .handle::<Submit<harness::Harness>>(|_| Ok(applied(Vec::new())));
}

fn open(cx: &mut TestAppContext, key: &str) {
    cx.simulate_click(key);
    cx.run_until_parked();
}

#[test]
fn an_agents_page_says_who_starts_its_conversations_and_its_manager_claims_them() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    harness_takes(&cx);
    open(&mut cx, "agent-12");
    assert!(
        cx.has_text("Who can start its conversations"),
        "{:?}",
        cx.texts()
    );
    assert!(
        cx.has_text("Open (from before policies)"),
        "{:?}",
        cx.texts()
    );
    open(&mut cx, "agent-claim");
    assert_eq!(
        cx.host().requests::<Submit<harness::Harness>>(),
        [harness::Op::ClaimConversations { agent: SCOUT }]
    );
    let (mut cx, _) = seated(Net::default(), ADA);
    open(&mut cx, "agent-12");
    assert!(
        cx.find("agent-claim").is_none(),
        "only its manager claims them"
    );
}

#[test]
fn a_reader_the_conversations_policy_lists_starts_them_and_a_claimed_agent_offers_no_claim() {
    let team = a_policy(
        5,
        EDDY,
        "Team",
        Writers::Listed(vec![ownership::Party::Account(ADA)]),
    );
    let (mut cx, _) = seated(Net::default(), ADA);
    owned(
        &cx,
        vec![(scouts(), EDDY, team.clone())],
        vec![team.clone()],
    );
    open(&mut cx, "agent-12");
    assert!(
        cx.has_text("Eddy · Team · Eddy and Ada"),
        "{:?}",
        cx.texts()
    );
    assert!(
        cx.find("agent-new-thread").is_some(),
        "the policy lets Ada start one, though access lists only Eddy"
    );
    let (mut cx, _) = seated(Net::default(), EDDY);
    owned(&cx, vec![(scouts(), EDDY, team.clone())], vec![team]);
    open(&mut cx, "agent-12");
    assert!(cx.has_text("You · Team · You and Ada"), "{:?}", cx.texts());
    assert!(cx.find("agent-claim").is_none(), "Eddy holds them already");
}

#[test]
fn a_model_on_record_says_whose_it_is_in_its_row_and_its_form() {
    let open_to_all = a_policy(3, ADA, "Open", Writers::Anyone);
    let (mut cx, _) = seated(Net::default(), EDDY);
    owned(
        &cx,
        vec![(sonnet(), ADA, open_to_all.clone())],
        vec![open_to_all],
    );
    open(&mut cx, "agents-models");
    assert!(cx.has_text("Ada · Open · Anyone"), "{:?}", cx.texts());
    open(&mut cx, "model-sonnet");
    assert!(
        cx.find("model-id-kept").is_some(),
        "the form opens at the record"
    );
    assert!(cx.find("model-policy").is_some());
    assert!(cx.has_text("Who can edit"), "{:?}", cx.texts());
    assert!(
        cx.interactivity("model-policy").on_click.is_none(),
        "only its owner presses the row"
    );
}

#[test]
fn a_models_owner_relinks_it_from_its_form() {
    let open_to_all = a_policy(3, ADA, "Open", Writers::Anyone);
    let only = a_policy(4, ADA, "Only me", Writers::Owner);
    let (mut cx, _) = seated(Net::default(), ADA);
    owned(
        &cx,
        vec![(sonnet(), ADA, open_to_all.clone())],
        vec![open_to_all, only],
    );
    open(&mut cx, "agents-models");
    open(&mut cx, "model-sonnet");
    open(&mut cx, "model-policy");
    assert!(cx.find("model-policy/sheet").is_some());
    open(&mut cx, "model-policy/relink");
    assert!(cx.has_text("In use · Anyone"), "{:?}", cx.texts());
    open(&mut cx, "model-policy/relink/4");
    let sent = cx.host().requests::<Submit<ownership::Ownership>>();
    assert!(
        matches!(sent.as_slice(), [Owned::Relink { program, key, policy: 4 }] if program == harness::PROGRAM && *key == sonnet()),
        "{sent:?}"
    );
}

#[test]
fn on_a_phone_the_manager_claims_from_the_agents_sheet_and_back_closes_a_policy_sheet() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    harness_takes(&cx);
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
    open(&mut cx, "agent-12");
    assert!(
        cx.has_text("Who can start its conversations"),
        "{:?}",
        cx.texts()
    );
    open(&mut cx, "agent-actions");
    open(&mut cx, "agent-sheet-claim");
    assert_eq!(
        cx.host().requests::<Submit<harness::Harness>>(),
        [harness::Op::ClaimConversations { agent: SCOUT }]
    );
    let mine = a_policy(4, EDDY, "Only me", Writers::Owner);
    let (mut cx, _) = seated(Net::default(), EDDY);
    owned(&cx, vec![(scouts(), EDDY, mine.clone())], vec![mine]);
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
    open(&mut cx, "agent-12");
    open(&mut cx, "agent-conversations-policy");
    assert!(cx.find("agent-conversations-policy/sheet").is_some());
    cx.simulate_back();
    cx.run_until_parked();
    assert!(cx.find("agent-conversations-policy/sheet").is_none());
    assert!(
        cx.find("agent-page").is_some(),
        "Back closed the sheet, not the page"
    );
}
