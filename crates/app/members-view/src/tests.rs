use super::*;
use ::activity::Activity;
use abi::Scheme;
use valset::Valset;
use view::methods::Query;
use view::methods::{
    Block, BlockList, BlockPage, ChainBlock, ChainBlocks, ChainTx, Change, Located,
    ProgramDescribe, Session, Tx, TxAsk, TxRef,
};
use view::testing::TestAppContext;
use view::wire::{ContainerNode, Node};
use view::{Hsla, StyleRefinement, Styled, Theme};

mod faces;
mod phone;

#[test]
fn the_root_tracks_the_shared_theme() {
    let (mut cx, _) = ready();
    let dark = Theme::dark();
    cx.set_global(dark);
    let style = cx.style("members");
    assert_eq!(
        style
            .background
            .as_ref()
            .and_then(|fill| fill.color())
            .and_then(|background| background.as_solid()),
        Some(dark.background)
    );
    assert_eq!(style.text.color, Some(dark.foreground));
}

fn account(number: u64, name: &str, control: identity::Control) -> identity::Account {
    identity::Account {
        number,
        card: identity::Card {
            name: name.into(),
            avatar: None,
            bio: Some(format!("{name}'s bio")),
            updated_at: 0,
        },
        control,
    }
}

fn key(key: &[u8], label: &str) -> identity::Key {
    identity::Key {
        scheme: Scheme::Ed25519,
        key: key.to_vec(),
        label: Some(label.into()),
        added_at: 0,
    }
}

fn person(number: u64, name: &str, keys: Vec<identity::Key>) -> identity::Account {
    account(number, name, identity::Control::User { keys })
}

fn program(number: u64, name: &str) -> identity::Account {
    account(
        number,
        name,
        identity::Control::System {
            program: name.into(),
        },
    )
}

fn agent(number: u64, name: &str, manager: u64, life: identity::Life) -> identity::Account {
    account(
        number,
        name,
        identity::Control::Program {
            script: None,
            manager,
            category: identity::Category::Agent,
            life,
        },
    )
}

fn page<T>(items: Vec<T>) -> programs::PageResponse<T> {
    programs::PageResponse {
        height: 1,
        items,
        next: None,
    }
}

const EDDY: &[u8] = b"\x01\x02";
const SCOUT: &[u8] = b"\x09\x09";

fn tx(signer: &[u8]) -> Tx {
    Tx {
        signer: signer.to_vec(),
        target: "chat".into(),
        size: 2,
        payload: Some(b"hi".to_vec()),
        ..Tx::default()
    }
}

fn block(height: u64, txs: Vec<Tx>) -> Block {
    Block {
        height,
        time: height * 60_000,
        txs,
        ..Block::default()
    }
}

fn account_of(signer: &[u8]) -> Option<u64> {
    match signer {
        EDDY => Some(7),
        SCOUT => Some(9),
        _ => None,
    }
}

/// `blocks`, oldest first, as the node serves them and the `activity`
/// program indexes them.
fn chain(cx: &mut TestAppContext, blocks: Vec<Block>) {
    let tip = blocks.last().cloned();
    let txs = blocks.clone();
    cx.host().handle::<ChainTx>(move |ask: TxAsk| {
        let TxRef::At { height, position } = ask.tx else {
            panic!("a frame's transaction is read by its place")
        };
        let block = txs.iter().find(|block| block.height == height);
        Ok(block.and_then(|block| {
            Some(Located {
                height,
                position,
                time: block.time,
                tx: block.txs.get(position as usize)?.clone(),
            })
        }))
    });
    cx.host().handle::<ChainBlocks>(move |page: BlockPage| {
        assert_eq!((page.before, page.limit), (None, 1), "only the tip is read");
        Ok(BlockList {
            blocks: tip.clone().into_iter().collect(),
            listed_from: Some(1),
        })
    });
    cx.host().handle::<Query<Activity>>(move |query| {
        let ::activity::Query::Acted { account, .. } = query else {
            panic!("{query:?}")
        };
        let frames = blocks
            .iter()
            .rev()
            .flat_map(|block| {
                let txs = block.txs.iter().enumerate().rev();
                txs.filter(move |(_, tx)| account_of(&tx.signer) == Some(account))
                    .map(move |(position, tx)| ::activity::Frame {
                        height: block.height,
                        position: position as u32,
                        time: block.time,
                        signer: tx.signer.clone(),
                        target: tx.target.clone(),
                        applied: true,
                    })
            })
            .collect();
        Ok(::activity::Reply::Frames(page(frames)))
    });
}

/// eddy (#7, the reader, a validator) manages scout (#9, active) and relay
/// (#10, revoked); chat (#8) is a program; ada (#11) is another person.
fn respond(cx: &mut TestAppContext) {
    cx.host().handle::<Query<Identity>>(|query| {
        assert!(matches!(query, identity::Query::List { .. }));
        Ok(identity::Reply::Accounts(page(vec![
            program(8, "chat"),
            person(7, "eddy", vec![key(EDDY, "laptop")]),
            agent(
                9,
                "scout",
                7,
                identity::Life::Active {
                    keys: vec![key(SCOUT, "sandbox")],
                },
            ),
            agent(10, "relay", 7, identity::Life::Revoked),
            person(11, "ada", vec![key(b"\x0b", "phone")]),
        ])))
    });
    cx.host().handle::<Query<Valset>>(|query| {
        assert!(matches!(query, valset::Query::Memberships { .. }));
        Ok(valset::Reply::Memberships(page(vec![valset::Membership {
            key: EDDY.to_vec(),
            address: Some("10.0.0.1:4000".into()),
            halo: "10.0.0.1:9100".into(),
            role: valset::Role::Validator,
        }])))
    });
    // two blocks: scout posted at 12, eddy at 11 and 12
    chain(
        cx,
        vec![
            block(11, vec![tx(EDDY)]),
            block(12, vec![tx(EDDY), tx(SCOUT)]),
        ],
    );
}

/// The view open on [`respond`]'s host, seated as eddy (#7).
fn ready() -> (TestAppContext, Entity<Members>) {
    let mut cx = TestAppContext::new();
    respond(&mut cx);
    let view = cx.open::<Members>();
    cx.host().stream::<HostSession>().send(Session {
        account: Some(7),
        chain_id: "testnet#0a1b2c3d".into(),
        ..Session::default()
    });
    cx.run_until_parked();
    (cx, view)
}

/// The colour `text` is drawn in under `key`, inherited down the tree.
fn color_of(cx: &TestAppContext, key: &str, text: &str) -> Option<Hsla> {
    fn walk(
        cx: &TestAppContext,
        node: &Node,
        text: &str,
        color: Option<Hsla>,
    ) -> Option<Option<Hsla>> {
        let own = node.style().and_then(|style| cx.styles()[style].text.color);
        let color = own.or(color);
        if node.text() == Some(text) {
            return Some(color);
        }
        node.children()
            .iter()
            .find_map(|child| walk(cx, child, text, color))
    }
    walk(cx, cx.find(key)?, text, None).flatten()
}

#[test]
fn the_list_groups_people_then_agents_then_programs_and_chooses_no_one() {
    let (cx, _) = ready();
    let texts = cx.texts();
    // the last of each, past the chips of the same names
    let at = |text: &str| texts.iter().rposition(|t| t == text).unwrap();
    assert!(at("People") < at("eddy") && at("eddy") < at("ada"));
    assert!(at("ada") < at("Agents") && at("Agents") < at("scout"));
    assert!(at("relay") < at("Programs") && at("Programs") < at("chat"));
    for text in [
        "5 accounts",
        "you",
        "User",
        "Program",
        "Agent · managed by eddy",
        "Agent · managed by eddy · revoked",
        "Choose a member",
    ] {
        assert!(cx.has_text(text), "{text}: {texts:?}");
    }
    // nothing chosen, nothing read of the chain
    assert!(cx.host().requests::<Query<Activity>>().is_empty());
    assert_eq!(cx.host().requests::<Changes<Identity>>().len(), 1);
}

#[test]
fn choosing_a_person_shows_their_devices_the_agents_they_manage_and_their_activity() {
    let (mut cx, _) = ready();
    cx.simulate_click("members-row-7");
    cx.run_until_parked();
    for text in [
        "account 7",
        "1 device",
        "Validator",
        "eddy's bio",
        "Devices",
        "laptop",
        "0102",
        "Manages",
        "account 9 · Agent",
        "active",
        "revoked",
        "Recent activity",
        "newest first",
        "chat",
        "block 12 · 0",
        "block 11 · 0",
        "1m",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    // scout's post is scout's, not eddy's
    assert!(!cx.has_text("block 12 · 1"));
    assert!(cx.host().requests::<ChainBlock>().is_empty());
    // the reader's own account has no one to DM
    assert!(cx.find("members-open-dm").is_none());
    cx.simulate_click("members-explorer");
    assert_eq!(
        cx.host().requests::<view::methods::LinkOpen>(),
        vec!["duck://explorer/account/7".to_owned()]
    );
    cx.simulate_click("members-tx-0");
    assert_eq!(
        cx.host()
            .requests::<view::methods::LinkOpen>()
            .last()
            .map(String::as_str),
        Some("duck://explorer/tx/12/0")
    );
    // a managed agent is chosen in place
    cx.simulate_click("members-manages-9");
    cx.run_until_parked();
    assert!(cx.has_text("scout's bio"));
}

#[test]
fn an_agent_names_its_manager_its_standing_and_its_devices() {
    let (mut cx, _) = ready();
    cx.simulate_click("members-row-9");
    cx.run_until_parked();
    for text in [
        "account 9",
        "Agent · managed by",
        "eddy",
        "active",
        "1 device",
        "sandbox",
        "block 12 · 1",
        "Suspend, revoke, rename and keys live in Account → Agents, for the manager.",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    assert!(!cx.has_text("block 12 · 0") && !cx.has_text("block 11 · 0"));
    cx.simulate_click("members-open-dm");
    assert_eq!(
        cx.host().requests::<view::methods::LinkOpen>(),
        vec![ducklink::mint("testnet#0a1b2c3d", "chat", &["dm-7-9"]).unwrap()]
    );
    // the manager's name chooses the manager
    cx.simulate_click("members-manager");
    cx.run_until_parked();
    assert!(cx.has_text("eddy's bio"));
}

#[test]
fn a_program_has_no_devices_and_is_dmed_as_any_account() {
    let (mut cx, _) = ready();
    cx.simulate_click("members-row-8");
    cx.run_until_parked();
    assert!(cx.has_text("Program") && !cx.has_text("Program · chat"));
    assert!(cx.has_text("No devices"));
    assert!(!cx.has_text("Recent activity") && !cx.has_text("1 device"));
    cx.simulate_click("members-open-dm");
    assert_eq!(
        cx.host().requests::<view::methods::LinkOpen>(),
        vec![ducklink::mint("testnet#0a1b2c3d", "chat", &["dm-7-8"]).unwrap()]
    );
    assert!(cx.find("members-explorer").is_some());
    assert!(cx.host().requests::<Query<Activity>>().is_empty());
}

#[test]
fn a_revoked_agent_stays_listed_dimmed() {
    let (cx, _) = ready();
    let theme = Theme::light();
    assert_eq!(color_of(&cx, "members-row-10", "relay"), Some(theme.faint));
    assert_ne!(color_of(&cx, "members-row-9", "scout"), Some(theme.faint));
}

#[test]
fn the_filter_and_the_chips_narrow_together_and_keep_the_choice() {
    let (mut cx, _) = ready();
    cx.simulate_click("members-row-9");
    cx.run_until_parked();
    let reads = cx.host().requests::<Query<Identity>>().len();
    // Programs: only chat
    cx.simulate_click("members-chip-3");
    assert!(cx.find("members-row-8").is_some() && cx.find("members-row-7").is_none());
    // and a filter on top: nothing is both a program and "ed"
    cx.simulate_input("members-filter", "ed");
    assert!(cx.has_text("Nothing matches"));
    // People and "ed": eddy alone
    cx.simulate_click("members-chip-1");
    assert!(cx.find("members-row-7").is_some() && cx.find("members-row-11").is_none());
    // the chosen agent is hidden from the list, not from the detail
    assert!(cx.has_text("scout's bio"));
    cx.simulate_click("members-chip-0");
    cx.simulate_input("members-filter", "");
    assert!(cx.find("members-row-11").is_some());
    assert!(cx.has_text("scout's bio"));
    assert_eq!(cx.host().requests::<Query<Identity>>().len(), reads);
}

#[test]
fn loading_waits_for_the_host() {
    let mut cx = TestAppContext::new();
    cx.host().never::<Query<Identity>>();
    cx.open::<Members>();
    cx.run_until_parked();
    assert!(cx.has_text("Reading the roster…"));
}

#[test]
fn a_roster_with_nobody_in_it_says_so() {
    let mut cx = TestAppContext::new();
    cx.host()
        .handle::<Query<Identity>>(|_| Ok(identity::Reply::Accounts(page(vec![]))));
    cx.host()
        .handle::<Query<Valset>>(|_| Ok(valset::Reply::Memberships(page(vec![]))));
    cx.open::<Members>();
    cx.run_until_parked();
    assert!(cx.has_text("No accounts"));
}

#[test]
fn a_refusal_shows_its_sentence_and_retry_asks_again() {
    let mut cx = TestAppContext::new();
    cx.host()
        .refuse::<Query<Identity>>("unavailable", "identity is not running here");
    cx.open::<Members>();
    cx.run_until_parked();
    assert!(cx.has_text("identity is not running here"));
    respond(&mut cx);
    cx.simulate_click("members-retry");
    cx.run_until_parked();
    assert!(cx.has_text("eddy"));
    assert_eq!(cx.host().requests::<Query<Identity>>().len(), 2);
}

#[test]
fn a_live_bump_re_reads_and_a_snapshot_restores_the_choice() {
    let (mut cx, _) = ready();
    let feed = cx.host().stream::<Changes<Identity>>();
    cx.simulate_click("members-row-9");
    cx.run_until_parked();
    cx.host()
        .refuse::<Query<Identity>>("unavailable", "refresh temporarily unavailable");
    feed.send(None);
    cx.run_until_parked();
    assert!(cx.has_text("scout's bio"));
    assert_eq!(cx.host().requests::<Query<Identity>>().len(), 2);
    cx.host().handle::<Query<Identity>>(|_| {
        Ok(identity::Reply::Accounts(page(vec![
            person(7, "eddy", vec![key(EDDY, "laptop")]),
            agent(
                9,
                "scout",
                7,
                identity::Life::Suspended {
                    keys: vec![key(SCOUT, "sandbox")],
                },
            ),
        ])))
    });
    feed.send(None);
    cx.run_until_parked();
    assert!(cx.has_text("suspended") && !cx.has_text("ada"));
    cx.simulate_input("members-filter", "sc");
    cx.simulate_click("members-chip-2");
    let bytes = cx.snapshot().unwrap();
    let mut restored = TestAppContext::new();
    restored.host().never::<Query<Identity>>();
    restored.host().never::<ChainBlocks>();
    let view = restored.restore::<Members>(&bytes).unwrap();
    restored.run_until_parked();
    assert!(restored.has_text("scout's bio"));
    view.read(|view| {
        assert_eq!(view.filter.text(), "sc");
        assert_eq!(view.only, Some(Group::Agents));
        assert_eq!(view.selected(), Some(9));
    });
    assert_eq!(restored.host().requests::<Query<Identity>>().len(), 1);
    // the activity is read again for the restored choice
    assert_eq!(restored.host().requests::<ChainBlocks>().len(), 1);
}

/// A block that changed no account: identity's head re-reads the roster,
/// the same rows land, and the view draws nothing for either.
#[test]
fn a_live_bump_that_lands_the_same_roster_draws_nothing() {
    let (mut cx, _) = ready();
    let feed = cx.host().stream::<Changes<Identity>>();
    cx.simulate_click("members-row-9");
    cx.run_until_parked();
    let (asked, renders) = (cx.host().requests::<Query<Identity>>().len(), cx.renders());
    feed.send(Some(Change {
        height: 13,
        keys: Vec::new(),
    }));
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<Query<Identity>>().len(), asked + 1);
    assert_eq!(cx.renders(), renders, "the same roster drew nothing");
    assert!(cx.has_text("scout's bio"));
}

/// A key seated or unseated is valset's block, not identity's: the standing
/// in the list follows it with no identity bump.
#[test]
fn a_valset_block_reaches_the_standing_without_an_identity_bump() {
    let (mut cx, _) = ready();
    let standing = cx.host().stream::<Changes<Valset>>();
    cx.simulate_click("members-row-7");
    cx.run_until_parked();
    assert!(cx.has_text("Validator"), "{:?}", cx.texts());
    cx.host().handle::<Query<Valset>>(|_| {
        Ok(valset::Reply::Memberships(page(vec![valset::Membership {
            key: EDDY.to_vec(),
            address: Some("10.0.0.1:4000".into()),
            halo: "10.0.0.1:9100".into(),
            role: valset::Role::Resident,
        }])))
    });
    standing.send(Some(Change {
        height: 13,
        keys: Vec::new(),
    }));
    cx.run_until_parked();
    assert!(
        cx.has_text("Resident") && !cx.has_text("Validator"),
        "{:?}",
        cx.texts()
    );
}

/// The detail's activity read again lands what it shows: nothing draws.
#[test]
fn an_activity_read_anew_that_lands_the_same_draws_nothing() {
    let (mut cx, view) = ready();
    cx.simulate_click("members-row-7");
    cx.run_until_parked();
    assert!(cx.has_text("block 12 · 0"), "{:?}", cx.texts());
    let asked = cx.host().requests::<Query<Activity>>().len();
    let renders = cx.renders();
    cx.update(&view, |members, _, cx| members.read_activity(cx));
    cx.run_until_parked();
    assert!(
        cx.host().requests::<Query<Activity>>().len() > asked,
        "read anew"
    );
    assert_eq!(cx.renders(), renders, "the same activity drew nothing");
}

/// Nothing selected, the list still has an active row — its first shown —
/// so Tab lands on a row, not on the box; End selects the last shown row.
#[test]
fn the_first_row_is_active_before_a_choice_and_end_selects_the_last() {
    let (mut cx, _) = ready();
    let list = cx.interactivity("members-list");
    assert!(list.focusable && list.tab_stop == Some(true));
    let rows: Vec<String> = cx
        .find("members-list")
        .expect("the list")
        .children()
        .iter()
        .filter_map(|node| node.key())
        .filter(|key| key.starts_with("members-row-"))
        .map(str::to_owned)
        .collect();
    let first = cx.interactivity(&rows[0]);
    assert!(first.aria.active_descendant);
    assert_eq!(first.aria.selected, Some(false));
    assert!(
        rows[1..]
            .iter()
            .all(|row| !cx.interactivity(row).aria.active_descendant)
    );
    // Enter selects the default row, which Home could not reach (it is there)
    cx.simulate_focus("members-list");
    cx.simulate_key_down("members-list", "enter");
    cx.run_until_parked();
    assert_eq!(cx.interactivity(&rows[0]).aria.selected, Some(true));
    cx.simulate_key_down("members-list", "end");
    cx.run_until_parked();
    let last = cx.interactivity(rows.last().expect("rows"));
    assert!(last.aria.active_descendant);
    assert_eq!(last.aria.selected, Some(true));
    assert!(!cx.interactivity(&rows[0]).aria.active_descendant);
    cx.simulate_key_down("members-list", "home");
    cx.run_until_parked();
    assert_eq!(cx.interactivity(&rows[0]).aria.selected, Some(true));
}

#[test]
fn the_arrows_walk_the_list_and_the_chosen_row_is_its_active_one() {
    let (mut cx, _) = ready();
    let of = |cx: &TestAppContext, key: &str| match cx.find(key) {
        Some(Node::Container(ContainerNode {
            interactivity: Some(interactivity),
            ..
        })) => interactivity.clone(),
        _ => panic!("{key} is a container"),
    };
    let list = of(&cx, "members-list");
    assert_eq!(list.role, Some(view::Role::ListBox));
    assert!(list.focusable);
    // the first shown row is active before any choice, so ↓ selects the
    // second, and ↑ comes back
    cx.simulate_focus("members-list");
    for (key, number, other) in [("down", 11, 7), ("up", 7, 11)] {
        cx.simulate_key_down("members-list", key);
        let (row, other) = (
            of(&cx, &format!("members-row-{number}")),
            of(&cx, &format!("members-row-{other}")),
        );
        assert_eq!(row.role, Some(view::Role::ListBoxOption));
        // a focusable row would lose the flag to the host's sanitizer
        assert!(!row.focusable && row.aria.active_descendant);
        assert_eq!(row.aria.selected, Some(true));
        assert!(!other.aria.active_descendant);
        assert_eq!(other.aria.selected, Some(false));
    }
}

/// ↓ then ↑ (a reader trying the next member, the door's arrow probe)
/// comes back to an account whose block links are there at once, not to
/// "Reading…" for the length of a scan: what was read of it shows while it
/// is read anew, and the new read replaces it.
#[test]
fn an_account_chosen_again_shows_its_activity_at_once_and_reads_it_anew() {
    let (mut cx, _) = ready();
    cx.simulate_click("members-row-7");
    cx.run_until_parked();
    assert!(cx.has_text("block 12 · 0"), "{:?}", cx.texts());
    // eddy signs again at 13, and the index holds only that
    chain(&mut cx, vec![block(13, vec![tx(EDDY)])]);
    // ↓ ↑: read anew, the new read shows
    cx.simulate_key_down("members-list", "down");
    cx.simulate_key_down("members-list", "up");
    cx.run_until_parked();
    assert!(cx.has_text("block 13 · 0") && !cx.has_text("block 12 · 0"));
    // the chain stops answering: ↓ ↑ still shows eddy's links at once
    cx.host().never::<ChainBlocks>();
    cx.simulate_key_down("members-list", "down");
    cx.run_until_parked();
    assert!(cx.has_text("ada's bio"));
    cx.simulate_key_down("members-list", "up");
    cx.run_until_parked();
    assert!(cx.has_text("eddy's bio"));
    assert!(!cx.has_text("Reading its activity…"));
    assert!(
        cx.has_text("block 13 · 0") && cx.find("members-tx-0").is_some(),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn recent_activity_lists_frames_and_reads_no_transaction() {
    let (mut cx, _) = ready();
    let frame = |height, position, target: &str, applied| ::activity::Frame {
        height,
        position,
        time: height * 60_000,
        signer: EDDY.to_vec(),
        target: target.into(),
        applied,
    };
    let frames = vec![frame(12, 0, "chat", true), frame(11, 2, "forge", false)];
    cx.host()
        .handle::<Query<Activity>>(move |_| Ok(::activity::Reply::Frames(page(frames.clone()))));
    cx.simulate_click("members-row-7");
    cx.run_until_parked();
    let label = |key: &str| {
        cx.interactivity(key)
            .aria
            .label
            .as_deref()
            .map(String::from)
    };
    assert_eq!(
        label("members-tx-0").as_deref(),
        Some("Open chat, Accepted, block 12 · 0 in Explorer")
    );
    assert_eq!(
        label("members-tx-1").as_deref(),
        Some("Open forge, Rejected, block 11 · 2 in Explorer")
    );
    assert!(cx.has_text("forge") && cx.has_text("block 11 · 2"));
    assert!(
        cx.host().requests::<ChainTx>().is_empty(),
        "no transaction is read to list them"
    );
    assert!(cx.host().requests::<ProgramDescribe>().is_empty());
    assert_eq!(
        cx.host().requests::<ChainBlocks>().len(),
        1,
        "the tip, for its time"
    );
    cx.simulate_click("members-tx-1");
    assert_eq!(
        cx.host()
            .requests::<view::methods::LinkOpen>()
            .last()
            .map(String::as_str),
        Some("duck://explorer/tx/11/2")
    );
}

/// A row is called by the member's name, not the avatar's initial drawn
/// before it; what else the row says is its description.
#[test]
fn a_row_is_named_by_the_member_not_the_avatar() {
    let (cx, _) = ready();
    let aria = |key: &str| match cx.find(key) {
        Some(Node::Container(ContainerNode {
            interactivity: Some(interactivity),
            ..
        })) => interactivity.aria.clone(),
        _ => panic!("{key} is a container"),
    };
    for (number, name, description) in [
        (7, "eddy", "you · User"),
        (11, "ada", "User"),
        (10, "relay", "Agent · managed by eddy · revoked"),
        (8, "chat", "Program"),
    ] {
        let row = aria(&format!("members-row-{number}"));
        assert_eq!(row.label.as_deref(), Some(name));
        assert_eq!(row.description.as_deref(), Some(description));
    }
}

/// A Manages row is called by the agent, not the avatar's initial either;
/// its account and standing are its description.
#[test]
fn a_managed_agent_is_named_by_the_agent_not_the_avatar() {
    let (mut cx, _) = ready();
    cx.simulate_click("members-row-7");
    cx.run_until_parked();
    for (number, name, description) in [
        (9, "scout", "account 9 · Agent · active"),
        (10, "relay", "account 10 · Agent · revoked"),
    ] {
        let Some(Node::Container(ContainerNode {
            interactivity: Some(interactivity),
            ..
        })) = cx.find(&format!("members-manages-{number}"))
        else {
            panic!("members-manages-{number} is a container")
        };
        assert_eq!(interactivity.aria.label.as_deref(), Some(name));
        assert_eq!(interactivity.aria.description.as_deref(), Some(description));
    }
}

#[test]
fn a_kind_with_no_one_in_it_says_so_without_quoting_an_empty_filter() {
    let mut cx = TestAppContext::new();
    cx.host().handle::<Query<Identity>>(|_| {
        Ok(identity::Reply::Accounts(page(vec![person(
            7,
            "eddy",
            vec![key(EDDY, "laptop")],
        )])))
    });
    cx.host()
        .handle::<Query<Valset>>(|_| Ok(valset::Reply::Memberships(page(vec![]))));
    cx.open::<Members>();
    cx.run_until_parked();
    cx.simulate_click("members-chip-2");
    assert!(
        cx.has_text("No agents on this network yet."),
        "{:?}",
        cx.texts()
    );
    cx.simulate_input("members-filter", "zz");
    assert!(cx.has_text("No account reads like “zz”."));
}

#[test]
fn a_narrow_pane_covers_the_whole_screen_with_the_detail_and_its_close() {
    let (mut cx, _) = ready();
    cx.simulate_resize(1000., 640.);
    cx.simulate_click("members-row-7");
    assert!(cx.find("members-detail-over").is_none());
    assert!(cx.find("members-detail-close").is_none());

    cx.simulate_resize(720., 640.);
    assert!(cx.find("members-list-resize").is_none(), "nothing to drag");
    let full = StyleRefinement::default().inset_0();
    assert_eq!(style(&cx, "members-detail-over").inset, full.inset);
    let whole = StyleRefinement::default().size_full();
    assert_eq!(style(&cx, "members-detail").size, whole.size);
    cx.simulate_click("members-detail-close");
    assert!(cx.find("members-detail-over").is_none());
    assert!(cx.find("members-detail").is_none(), "the list alone");
    cx.simulate_click("members-row-7");
    assert!(cx.find("members-detail-over").is_some());
}

#[test]
fn the_docked_list_drags_within_its_bounds_and_leaves_the_detail_its_narrowest() {
    let (mut cx, _) = ready();
    cx.simulate_resize(1000., 640.);
    assert!(matches!(
        cx.find("members-list-resize"),
        Some(Node::ResizeHandle {
            on_drag: Some(_),
            ..
        })
    ));
    assert!(list_is(&cx, 400.));
    cx.simulate_drag("members-list-resize", 60., 0.);
    assert!(list_is(&cx, 460.));
    // never past the detail's 440 in 1000
    cx.simulate_drag("members-list-resize", 400., 0.);
    assert!(list_is(&cx, 560.));
    cx.simulate_drag("members-list-resize", -900., 0.);
    assert!(list_is(&cx, 320.));
    // the window shrinks: the list gives way before the detail floats
    cx.simulate_drag("members-list-resize", 200., 0.);
    cx.simulate_resize(800., 640.);
    assert!(cx.find("members-list-resize").is_some());
    assert!(list_is(&cx, 360.));
}

fn style(cx: &TestAppContext, key: &str) -> StyleRefinement {
    cx.style(key).clone()
}

/// Whether the docked list is `width` wide.
fn list_is(cx: &TestAppContext, width: f32) -> bool {
    style(cx, "members-list-pane").size.width
        == StyleRefinement::default().w(view::px(width)).size.width
}
