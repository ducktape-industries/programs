//! Nodes under a finger: the Expo app's two screens (`ui/phone/`), each in
//! the states Expo draws (full, still being read, refused, empty) and
//! through each of its presses; the width sweep at 360, 390 and 430
//! (SPEC §8.1d-1); and a vote's outcome in the Expo app's words.
use super::voting::{operators, sheet, sheet_view, submitted};
use super::*;
use crate::voting::cancel;
use valset::{Motion, Op, Pending, Voted};
use view::methods::{HostSession, Session, Submit, Submitted};
use view::testing;

/// The view as a phone 390 wide shows it.
fn phone(cx: &mut TestAppContext) {
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
}

/// Every text in `expected` is on screen.
fn shows(cx: &TestAppContext, expected: &[&str]) {
    for text in expected {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
}

fn click(cx: &mut TestAppContext, id: &str) {
    cx.simulate_click(id);
    cx.run_until_parked();
}

/// A change to valset's code the programs role holds for height 4,500,
/// made by a vote.
fn valset_upgrade() -> programs::Pending {
    programs::Pending {
        height: 4500,
        change: programs::Change::Set(programs::Entry {
            program: "valset".into(),
            code: abi::BlobId::Sha256([0x7e; 32]),
            params: b"params".to_vec(),
        }),
        by: None,
    }
}

/// A reader who votes (one voice of the two a motion needs), with a
/// promotion of the resident waiting on OTHER's vote, so the reader's own
/// vote enacts it, and [`valset_upgrade`] scheduled.
fn voter() -> TestAppContext {
    voter_view().0
}

/// [`voter`], and the view, for a test that sets what a read left.
fn voter_view() -> (TestAppContext, Entity<Nodes>) {
    let waiting = Pending {
        motion: Motion::Promote {
            key: RESIDENT.to_vec(),
        },
        voters: vec![OTHER.to_vec()],
    };
    let (mut cx, nodes) = sheet_view(&THIS, 2, vec![waiting], vec![valset_upgrade()]);
    phone(&mut cx);
    (cx, nodes)
}

/// Changes the view as a read would have left it, and draws it again.
fn set(cx: &mut TestAppContext, nodes: &Entity<Nodes>, change: impl FnOnce(&mut Nodes)) {
    cx.update(nodes, |view, _, cx| {
        change(view);
        cx.notify();
    });
    cx.run_until_parked();
}

/// What the node says of the network: [`seen`] at 4,200, with `change`.
fn network_where(cx: &TestAppContext, change: fn(&mut NetworkStatus)) {
    cx.host().handle::<ChainNetwork>(move |()| {
        let mut network = seen(4200);
        change(&mut network);
        Ok(network)
    });
}

/// The view on a phone over a node that answers every ask, but the one
/// `serve` answers its own way.
fn phone_where(serve: impl FnOnce(&TestAppContext)) -> TestAppContext {
    let mut cx = TestAppContext::new();
    node(&mut cx);
    respond(&mut cx);
    serve(&cx);
    cx.open::<Nodes>();
    phone(&mut cx);
    cx
}

// The list (Expo `app/(tabs)/nodes/index.tsx`)

#[test]
fn the_list_reads_the_network_its_numbers_and_every_member() {
    let (mut cx, _) = ready();
    phone(&mut cx);
    shows(
        &cx,
        &[
            "Workshop",
            "● In sync · this node abcd",
            "Height",
            "4,200",
            "last block just now",
            "Block time",
            "1,000 ms",
            "Epoch 42 · 1 / 100",
            "Next epoch in 99 blocks",
            "Validators",
            "3",
            "Residents",
            "1",
            "Height: the newest block the validator voted to finalize. Quiet: none for 20 blocks.",
        ],
    );
    assert!(cx.find("nodes-table").is_none(), "the desktop's table");
    assert_eq!(
        texts_of(&cx, "nodes-member-abcd"),
        ["abcd", "10.0.0.1:4000 · this node", "In sync"]
    );
    assert_eq!(
        texts_of(&cx, "nodes-member-9c1e"),
        ["9c1e", "10.0.0.2:4000", "In sync"]
    );
    assert_eq!(texts_of(&cx, "nodes-member-5f0a"), ["5f0a", "—", "Quiet"]);
    assert_eq!(
        texts_of(&cx, "nodes-member-0102"),
        ["0102", "10.0.0.9:4000"],
        "a resident's state is not reported"
    );
    assert_eq!(cx.back_handler(), None, "nothing to go back to");
}

#[test]
fn a_reader_who_does_not_vote_has_no_door_and_no_vote() {
    let (mut cx, _) = ready();
    phone(&mut cx);
    assert!(cx.find("nodes-door").is_none());
    assert!(!cx.has_text("Motions") && !cx.has_text("Scheduled changes"));
}

#[test]
fn the_numbers_wait_for_the_node_and_name_no_network_yet() {
    let cx = phone_where(|cx| cx.host().never::<ChainStatus>());
    shows(&cx, &["Nodes", "Reading node status…"]);
    assert!(
        cx.find("nodes-member-abcd").is_none(),
        "the rows read the node's height"
    );
}

#[test]
fn a_refused_status_says_why_and_retry_reads_it_again() {
    let mut cx = phone_where(|cx| {
        cx.host()
            .refuse::<ChainStatus>("unavailable", "The node is unavailable. Try again.")
    });
    shows(&cx, &["The node is unavailable. Try again.", "Retry"]);
    cx.host().handle::<ChainStatus>(|()| Ok(status()));
    click(&mut cx, "nodes-status-retry");
    assert!(cx.has_text("4,200") && cx.find("nodes-status-refused").is_none());
}

#[test]
fn the_members_wait_for_valset() {
    let cx = phone_where(|cx| cx.host().never::<Query<Valset>>());
    shows(&cx, &["Reading the members…"]);
}

#[test]
fn a_refused_member_read_says_why_and_retry_reads_it_again() {
    let mut cx = phone_where(|cx| {
        cx.host()
            .refuse::<Query<Valset>>("unavailable", "valset is not running here")
    });
    shows(&cx, &["valset is not running here", "Retry"]);
    respond(&mut cx);
    click(&mut cx, "nodes-retry");
    assert!(cx.find("nodes-member-abcd").is_some(), "{:?}", cx.texts());
}

#[test]
fn a_network_with_no_members_says_so() {
    let cx = phone_where(|cx| serve_set(cx, Vec::new(), Vec::new()));
    shows(
        &cx,
        &["No members", "The validator set of this network is empty."],
    );
}

#[test]
fn the_title_names_the_sessions_network_before_the_node_answers() {
    let mut cx = TestAppContext::new();
    node(&mut cx);
    respond(&mut cx);
    cx.host().never::<ChainStatus>();
    let session = cx.host().stream::<HostSession>();
    cx.open::<Nodes>();
    session.send(Session {
        connected: true,
        chain_id: "Harbour#2".into(),
        signer: String::new(),
        account: None,
        endpoint: String::new(),
    });
    phone(&mut cx);
    shows(&cx, &["Harbour", "Reading node status…"]);
}

#[test]
fn a_node_that_hears_no_votes_says_why_its_cells_are_empty() {
    let unsigned = phone_where(|cx| network_where(cx, |network| network.members[3].signed = None));
    assert_eq!(
        texts_of(&unsigned, "nodes-unheard"),
        ["This node isn't voting right now, so it can't see the validators' votes."]
    );
    let refused = phone_where(|cx| {
        cx.host()
            .refuse::<ChainNetwork>("unknown_request", "no such method")
    });
    assert_eq!(
        texts_of(&refused, "nodes-unheard"),
        ["This node doesn't report its validators' votes. Update the node."]
    );
}

#[test]
fn a_validator_behind_says_by_how_much_on_its_row_and_its_page() {
    let mut cx =
        phone_where(|cx| network_where(cx, |network| network.members[2].signed = Some(4195)));
    assert!(texts_of(&cx, "nodes-member-9c1e").contains(&"5 behind".to_owned()));
    click(&mut cx, "nodes-member-9c1e");
    shows(&cx, &["4,195 voted", "5", "5 behind"]);
}

#[test]
fn a_member_on_an_agent_says_who_operates_it_on_its_row_and_its_page() {
    let mut cx =
        phone_where(|cx| serve_identity(cx, vec![(OTHER.to_vec(), 11)], operators().to_vec()));
    assert!(
        texts_of(&cx, "nodes-member-9c1e")
            .contains(&" · operated by ada (validator vps)".to_owned())
    );
    click(&mut cx, "nodes-member-9c1e");
    shows(&cx, &["Operator", "operated by ada (validator vps)"]);
}

#[test]
fn a_silent_node_says_so_and_when_it_last_answered() {
    let (mut cx, ticks) = ready();
    phone(&mut cx);
    cx.host().never::<ChainStatus>();
    for _ in 0..4 {
        ticks.send(());
    }
    cx.run_until_parked();
    shows(
        &cx,
        &["● Not answering · this node abcd", "Last answer 4s ago"],
    );
    click(&mut cx, "nodes-member-abcd");
    shows(&cx, &["Last answer 4s ago"]);
}

#[test]
fn a_member_row_opens_its_page_and_back_returns_to_the_list() {
    let (mut cx, _) = ready();
    phone(&mut cx);
    click(&mut cx, "nodes-member-abcd");
    assert!(cx.find("nodes-page").is_none(), "the list gave way");
    assert!(cx.back_handler().is_some(), "Back has somewhere to go");
    cx.simulate_back();
    cx.run_until_parked();
    assert!(cx.find("nodes-node").is_none() && cx.find("nodes-page").is_some());
    assert_eq!(cx.back_handler(), None);
    click(&mut cx, "nodes-member-0102");
    click(&mut cx, "nodes-node-back");
    assert!(cx.find("nodes-page").is_some(), "the page's own Back");
}

#[test]
fn a_voter_reads_the_motions_that_wait_and_the_changes_scheduled() {
    let cx = voter();
    shows(
        &cx,
        &[
            "Motions",
            "Promote 0102 to validator",
            "1 of 2 · yours has not voted",
            "Vote",
            "Scheduled changes",
            "by vote · in 300 blocks",
            "Propose cancel",
            "Change a program in Explorer, Programs.",
        ],
    );
}

#[test]
fn a_voter_with_nothing_waiting_reads_no_motion_and_no_change() {
    let mut cx = sheet(&THIS, 2, Vec::new(), Vec::new());
    phone(&mut cx);
    shows(
        &cx,
        &["No motion waits for votes.", "No change is scheduled."],
    );
}

#[test]
fn a_refused_motions_read_says_why_and_retry_reads_it_again() {
    let (mut cx, nodes) = voter_view();
    set(&mut cx, &nodes, |view| {
        view.voting.motions = Loadable::Failed(Error::new("unavailable", "valset is restarting"));
    });
    shows(&cx, &["valset is restarting", "Retry"]);
    assert!(cx.find("nodes-motion-0").is_none());
    click(&mut cx, "nodes-motions-retry");
    assert!(cx.find("nodes-motions-refused").is_none());
    shows(&cx, &["Promote 0102 to validator"]);
}

#[test]
fn a_refused_schedule_read_says_why_and_retry_reads_it_again() {
    let (mut cx, nodes) = voter_view();
    set(&mut cx, &nodes, |view| {
        view.voting.scheduled =
            Loadable::Failed(Error::new("unavailable", "programs is restarting"));
    });
    shows(&cx, &["programs is restarting", "Retry"]);
    assert!(cx.find("nodes-scheduled-0").is_none());
    click(&mut cx, "nodes-scheduled-retry");
    assert!(cx.find("nodes-scheduled-refused").is_none());
    shows(&cx, &["Propose cancel"]);
}

#[test]
fn the_motions_and_the_schedule_wait_for_their_reads() {
    let (mut cx, nodes) = voter_view();
    set(&mut cx, &nodes, |view| {
        view.voting.motions = Loadable::Idle;
        view.voting.scheduled = Loadable::Idle;
    });
    shows(&cx, &["Reading the motions…", "Reading the schedule…"]);
}

#[test]
fn a_motion_the_reader_voted_on_reads_voted_and_has_no_vote() {
    let mine = Pending {
        motion: Motion::Promote {
            key: RESIDENT.to_vec(),
        },
        voters: vec![THIS.to_vec()],
    };
    let mut cx = sheet(&THIS, 3, vec![mine], Vec::new());
    phone(&mut cx);
    assert_eq!(texts_of(&cx, "nodes-motion-voted-0"), ["voted"]);
    assert!(cx.find("nodes-motion-vote-0").is_none());
}

#[test]
fn a_vote_in_flight_greys_every_vote() {
    let mut cx = voter();
    cx.host().never::<Submit<Valset>>();
    click(&mut cx, "nodes-door");
    click(&mut cx, "nodes-door-open");
    assert_eq!(submitted(&cx).len(), 1);
    for id in ["nodes-motion-vote-0", "nodes-scheduled-cancel-0"] {
        let press = cx.interactivity(id);
        assert!(
            press.aria.disabled == Some(true) && press.on_click.is_none(),
            "{id}"
        );
    }
}

/// The host's capsule beside the title row draws `ellipsis`: the door's
/// own control is a door, named The door.
#[test]
fn the_door_is_drawn_as_a_door_not_the_capsules_ellipsis() {
    let cx = voter();
    let row = format!("{:?}", cx.node("nodes-page-title"));
    assert!(!row.contains("icons/ellipsis.svg"), "{row}");
    let door = format!("{:?}", cx.node("nodes-door"));
    assert!(door.contains("icons/door-open.svg"), "{door}");
    let name = &cx.interactivity("nodes-door").aria.label;
    assert_eq!(name.as_deref(), Some("The door"));
}

#[test]
fn the_door_opens_as_a_sheet_and_back_closes_it_before_the_list() {
    let mut cx = voter();
    click(&mut cx, "nodes-door");
    shows(
        &cx,
        &[
            "Open the door",
            "anyone enrolls without an invite",
            "Close the door",
            "enrolling takes an invite",
        ],
    );
    assert!(cx.back_handler().is_some());
    cx.simulate_back();
    cx.run_until_parked();
    assert!(cx.find("nodes-door-sheet").is_none() && cx.find("nodes-page").is_some());
    assert_eq!(cx.back_handler(), None);
}

#[test]
fn a_door_row_closes_the_sheet_and_votes_its_motion() {
    let mut cx = voter();
    click(&mut cx, "nodes-door");
    click(&mut cx, "nodes-door-open");
    assert!(cx.find("nodes-door-sheet").is_none());
    assert_eq!(submitted(&cx), [Op::Vote(Motion::Door { open: true })]);
    shows(&cx, &["Vote counted: 1 of 2"]);
}

#[test]
fn a_vote_that_enacts_asks_first_and_not_now_sends_nothing() {
    let mut cx = voter();
    click(&mut cx, "nodes-motion-vote-0");
    shows(
        &cx,
        &[
            "Your vote reaches the quorum: this happens now, and a vote can't be taken back.",
            "Not now",
            "Vote and enact",
            "This phone's key signs it. The node runs the program's rules; a refusal comes back in its words.",
        ],
    );
    click(&mut cx, "nodes-confirm/cancel");
    assert!(cx.find("nodes-confirm-sheet").is_none() && submitted(&cx).is_empty());
    click(&mut cx, "nodes-motion-vote-0");
    cx.simulate_dismiss("nodes-confirm-sheet");
    cx.run_until_parked();
    assert!(cx.find("nodes-confirm-sheet").is_none() && submitted(&cx).is_empty());
}

#[test]
fn a_vote_that_enacts_goes_on_yes_and_its_outcome_stays_until_dismissed() {
    let mut cx = voter();
    cx.host()
        .handle::<Submit<Valset>>(|_| Ok(testing::applied(abi::encode(&Voted::Enacted))));
    click(&mut cx, "nodes-motion-vote-0");
    click(&mut cx, "nodes-confirm/submit");
    let promote = Motion::Promote {
        key: RESIDENT.to_vec(),
    };
    assert_eq!(submitted(&cx), [Op::Vote(promote)]);
    assert!(cx.has_text("Enacted") && cx.find("nodes-outcome").is_some());
    click(&mut cx, "nodes-outcome-dismiss");
    assert!(cx.find("nodes-outcome").is_none());
}

#[test]
fn a_landed_vote_says_so_on_the_screen_it_was_pressed_on_only() {
    let mut cx = voter();
    cx.host()
        .handle::<Submit<Valset>>(|_| Ok(testing::applied(abi::encode(&Voted::Enacted))));
    click(&mut cx, "nodes-door");
    click(&mut cx, "nodes-door-open");
    click(&mut cx, "nodes-member-9c1e");
    assert!(
        cx.find("nodes-outcome").is_none(),
        "the list's, not the page's"
    );
    click(&mut cx, "nodes-node-back");
    assert!(cx.has_text("Enacted"), "the list kept its message");
    click(&mut cx, "nodes-outcome-dismiss");
    click(&mut cx, "nodes-member-9c1e");
    click(&mut cx, "nodes-node-demote");
    assert!(cx.has_text("Enacted"));
    cx.simulate_back();
    cx.run_until_parked();
    assert!(cx.find("nodes-outcome").is_none(), "gone with its page");
    click(&mut cx, "nodes-member-9c1e");
    assert!(
        cx.find("nodes-outcome").is_none(),
        "and not back on a new one"
    );
}

#[test]
fn a_refused_vote_says_why_on_the_outcome_strip() {
    let mut cx = voter();
    cx.host().handle::<Submit<Valset>>(|_| {
        Ok(Submitted::Refused {
            frame: [0; 32],
            until: 0,
            refusal: Error::new("unauthorized", "only a validator votes"),
        })
    });
    click(&mut cx, "nodes-door");
    click(&mut cx, "nodes-door-close");
    shows(
        &cx,
        &["That didn’t go through: only a validator votes", "Dismiss"],
    );
}

#[test]
fn propose_cancel_votes_to_cancel_that_change() {
    let mut cx = voter();
    click(&mut cx, "nodes-scheduled-cancel-0");
    assert_eq!(submitted(&cx), [Op::Vote(cancel(&valset_upgrade()))]);
}

// A member's page (Expo `app/(tabs)/nodes/[key].tsx`)

#[test]
fn a_validator_page_reads_its_key_address_vote_and_the_blocks_it_led() {
    let (mut cx, _) = ready();
    phone(&mut cx);
    click(&mut cx, "nodes-member-abcd");
    shows(
        &cx,
        &[
            "abcd",
            "validator 1",
            "Key",
            "Address",
            "10.0.0.1:4000 · this node",
            "Height",
            "4,200 voted",
            "Behind",
            "0",
            "Status",
            "In sync",
            "Proposed",
            "last 64 blocks · 4,137 → 4,200",
        ],
    );
    assert!(cx.find("nodes-node-strip").is_some());
}

#[test]
fn a_resident_page_reads_no_vote_and_says_it_does_not_propose() {
    let (mut cx, _) = ready();
    phone(&mut cx);
    click(&mut cx, "nodes-member-0102");
    shows(&cx, &["resident", "Doesn't propose"]);
    assert!(!cx.has_text("Behind") && cx.find("nodes-node-strip").is_none());
}

#[test]
fn a_page_waits_for_the_members_it_reads() {
    let mut cx = TestAppContext::new();
    node(&mut cx);
    cx.host().never::<Query<Valset>>();
    let nodes = cx.open::<Nodes>();
    phone(&mut cx);
    cx.update(&nodes, |view, _, cx| {
        view.open = Some(THIS.to_vec());
        cx.notify();
    });
    cx.run_until_parked();
    shows(&cx, &["Node", "Reading the members…"]);
}

#[test]
fn a_page_on_a_key_that_left_reads_not_a_member() {
    let mut cx = TestAppContext::new();
    node(&mut cx);
    respond(&mut cx);
    let nodes = cx.open::<Nodes>();
    phone(&mut cx);
    click(&mut cx, "nodes-member-0102");
    serve_set(
        &cx,
        vec![THIS.to_vec()],
        vec![membership(&THIS, "10.0.0.1:4000", valset::Role::Validator)],
    );
    cx.update(&nodes, |view, _, cx| view.read(cx));
    cx.run_until_parked();
    shows(
        &cx,
        &[
            "Node",
            "Not a member",
            "This key holds no seat on the network.",
        ],
    );
}

#[test]
fn a_voter_demotes_or_removes_a_validator_from_its_page() {
    let mut cx = voter();
    click(&mut cx, "nodes-member-9c1e");
    assert!(cx.find("nodes-node-promote").is_none());
    click(&mut cx, "nodes-node-demote");
    click(&mut cx, "nodes-node-remove");
    let key = OTHER.to_vec();
    assert_eq!(
        submitted(&cx),
        [
            Op::Vote(Motion::Demote { key: key.clone() }),
            Op::Vote(Motion::Remove { key })
        ]
    );
}

#[test]
fn a_voter_promotes_a_resident_from_its_page() {
    let mut cx = voter();
    click(&mut cx, "nodes-member-0102");
    assert!(cx.find("nodes-node-demote").is_none());
    click(&mut cx, "nodes-node-promote");
    // OTHER voted for it already: the reader's vote enacts, so it asks
    click(&mut cx, "nodes-confirm/submit");
    let promote = Motion::Promote {
        key: RESIDENT.to_vec(),
    };
    assert_eq!(submitted(&cx), [Op::Vote(promote)]);
}

// Both screens

/// Under a finger at 360, 390 and 430 every frame fits and every press is
/// at least 44 each way, through the list, the door, the confirm sheet and
/// both kinds of member page; Back is set exactly while there is somewhere
/// to go back to.
#[test]
fn every_screen_fits_a_phone_360_390_and_430_wide() {
    let (mut cx, nodes) = voter_view();
    testing::sweep(&mut cx, |cx| {
        assert_eq!(cx.back_handler(), None, "nothing to go back to");
        assert!(cx.find("nodes-member-abcd").is_some(), "the members");
        assert!(cx.find("nodes-motion-vote-0").is_some(), "Vote");
        assert!(
            cx.find("nodes-scheduled-cancel-0").is_some(),
            "Propose cancel"
        );
        set(cx, &nodes, |view| {
            let refusal = Error::new("unavailable", "valset is restarting");
            view.voting.motions = Loadable::Failed(refusal);
            let said = "That didn’t go through: only a validator votes";
            view.voting.outcome = Some(Err(said.into()));
        });
        assert!(cx.find("nodes-motions-refused").is_some(), "a refused read");
        assert!(cx.find("nodes-outcome").is_some(), "a vote's outcome");
        click(cx, "nodes-motions-retry");
        click(cx, "nodes-outcome-dismiss");
        click(cx, "nodes-door");
        assert!(cx.find("nodes-door-sheet").is_some(), "the door sheet");
        cx.simulate_back();
        cx.run_until_parked();
        click(cx, "nodes-motion-vote-0");
        assert!(
            cx.find("nodes-confirm-sheet").is_some(),
            "the confirm sheet"
        );
        cx.simulate_back();
        cx.run_until_parked();
        assert!(cx.find("nodes-confirm-sheet").is_none());
        click(cx, "nodes-member-abcd");
        assert!(cx.find("nodes-node-remove").is_some(), "a validator's page");
        cx.simulate_back();
        cx.run_until_parked();
        click(cx, "nodes-member-0102");
        assert!(cx.find("nodes-node-promote").is_some(), "a resident's page");
        cx.simulate_back();
        cx.run_until_parked();
        assert!(cx.find("nodes-page").is_some(), "back on the list");
    });
}
