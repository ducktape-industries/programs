//! The sheet as a view: what it asks the host, and what it draws from the
//! answers. The row words themselves are `tests/rows.rs`.
use super::*;
use view::methods::{
    Block, BlockList, BlockPage, ChainBlocks, ChainNetwork, ChainStatus, ClockTicks, NetworkStatus,
    NodeStatus, Peer, Query,
};
use view::testing::{StreamSender, TestAppContext};

mod network;
mod phone;
mod rows;
mod screens;
mod voting;

const THIS: [u8; 2] = [0xab, 0xcd];
const OTHER: [u8; 2] = [0x9c, 0x1e];
const UNLISTED: [u8; 2] = [0x5f, 0x0a];
const RESIDENT: [u8; 2] = [0x01, 0x02];

fn status() -> NodeStatus {
    NodeStatus {
        chain_id: "Workshop".into(),
        time: 100,
        block_time_ms: 1000,
        epoch_length: 100,
        height: 4200,
        tip: [0xab; 32],
        root: [0xcd; 32],
        epoch: 42,
        identity: THIS.to_vec(),
        contract: 7,
    }
}

fn blocks(page: BlockPage) -> BlockList {
    let below = page.before.unwrap_or(4201);
    let blocks = (below.saturating_sub(page.limit as u64)..below)
        .rev()
        .map(|height| Block {
            height,
            proposer: Some(if height % 2 == 0 { THIS } else { OTHER }.to_vec()),
            ..Block::default()
        })
        .collect();
    BlockList {
        blocks,
        listed_from: Some(0),
    }
}

fn seen(height: u64) -> NetworkStatus {
    let peer = |key: [u8; 2], signed| Peer {
        key: key.to_vec(),
        signed,
    };
    NetworkStatus {
        height,
        members: vec![
            peer(RESIDENT, None),
            peer(UNLISTED, Some(3871)),
            peer(OTHER, Some(height - 1)),
            peer(THIS, Some(height)),
        ],
    }
}

fn node(cx: &mut TestAppContext) {
    cx.simulate_resize(1200., 680.);
    cx.host().handle::<ChainStatus>(|()| Ok(status()));
    cx.host().handle::<ChainBlocks>(|page| Ok(blocks(page)));
    cx.host().handle::<ChainNetwork>(|()| Ok(seen(4200)));
    serve_identity(cx, Vec::new(), Vec::new());
}

fn serve_identity(
    cx: &TestAppContext,
    keys: Vec<(Vec<u8>, u64)>,
    profiles: Vec<identity::Profile>,
) {
    cx.host().handle::<Query<identity::Identity>>(move |query| {
        Ok(match query {
            identity::Query::OfKey { key } => {
                identity::Reply::Number(keys.iter().find(|(held, _)| *held == key).map(|(_, n)| *n))
            }
            identity::Query::Profile { number } => {
                identity::Reply::Profile(profiles.iter().find(|p| p.number == number).cloned())
            }
            other => panic!("unexpected query: {other:?}"),
        })
    });
}

fn membership(key: &[u8], address: &str, role: valset::Role) -> valset::Membership {
    valset::Membership {
        key: key.to_vec(),
        address: Some(address.into()),
        halo: "node.example.org:9100".into(),
        role,
    }
}

fn page<T>(items: Vec<T>) -> valset::PageResponse<T> {
    valset::PageResponse {
        height: 1,
        items,
        next: None,
    }
}

fn serve_set(cx: &TestAppContext, validators: Vec<Vec<u8>>, members: Vec<valset::Membership>) {
    cx.host().handle::<Query<Valset>>(move |query| {
        Ok(match query {
            valset::Query::Validators => valset::Reply::Validators(validators.clone()),
            valset::Query::Memberships { .. } => valset::Reply::Memberships(page(members.clone())),
            other => panic!("unexpected query: {other:?}"),
        })
    });
}

fn respond(cx: &mut TestAppContext) {
    serve_set(
        cx,
        vec![THIS.to_vec(), OTHER.to_vec(), UNLISTED.to_vec()],
        vec![
            membership(&RESIDENT, "10.0.0.9:4000", valset::Role::Resident),
            membership(&OTHER, "10.0.0.2:4000", valset::Role::Validator),
            membership(&THIS, "10.0.0.1:4000", valset::Role::Validator),
        ],
    );
}

fn ready() -> (TestAppContext, StreamSender<ClockTicks>) {
    let mut cx = TestAppContext::new();
    node(&mut cx);
    let ticks = cx.host().stream::<ClockTicks>();
    respond(&mut cx);
    cx.open::<Nodes>();
    cx.run_until_parked();
    (cx, ticks)
}

fn texts_of(cx: &TestAppContext, key: &str) -> Vec<String> {
    let node = cx.find(key).unwrap_or_else(|| panic!("no {key}"));
    let mut texts = Vec::new();
    collect(node, &mut texts);
    texts
}

fn collect(node: &view::wire::Node, texts: &mut Vec<String>) {
    use view::wire::Node;
    match node {
        Node::Text(text) => texts.push(text.content.to_string()),
        Node::Container(container) => {
            for child in &container.children {
                collect(child, texts);
            }
        }
        _ => {}
    }
}

#[test]
fn the_root_tracks_the_shared_theme() {
    let (mut cx, _) = ready();
    let dark = view::Theme::dark();
    cx.set_global(dark);
    let style = cx.style("nodes");
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

#[test]
fn the_head_names_the_network_and_this_node() {
    let mut cx = TestAppContext::new();
    node(&mut cx);
    let ticks = cx.host().stream::<ClockTicks>();
    cx.host()
        .refuse::<ChainStatus>("unavailable", "The node is unavailable. Try again.");
    respond(&mut cx);
    cx.open::<Nodes>();
    cx.run_until_parked();
    assert!(cx.has_text("The node is unavailable. Try again."));
    assert!(cx.find("nodes-table").is_none());
    cx.host().handle::<ChainStatus>(|()| Ok(status()));
    cx.simulate_click("nodes-status-retry");
    cx.run_until_parked();
    for text in [
        "Workshop",
        "In sync",
        "· this node",
        "abcd",
        "Height",
        "4,200",
        "Last block",
        "just now",
        "Block time",
        "1.0 s",
        "Epoch 42 · 1 / 100",
        "Next epoch in 99 blocks",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    cx.host().handle::<ChainStatus>(|()| {
        Ok(NodeStatus {
            height: 4201,
            ..status()
        })
    });
    ticks.send(());
    cx.run_until_parked();
    assert!(cx.has_text("4,201"));
}

#[test]
fn every_key_is_one_row_validators_first() {
    let (cx, _) = ready();
    assert!(cx.has_text("Validators · 3") && cx.has_text("Residents · 1"));
    let rows: Vec<_> = (0..4)
        .map(|index| texts_of(&cx, &format!("nodes-row-{index}"))[0].clone())
        .collect();
    assert_eq!(rows, ["abcd", "9c1e", "5f0a", "0102"]);
    let texts = texts_of(&cx, "nodes-table");
    for key in ["abcd", "9c1e", "5f0a", "0102"] {
        assert_eq!(texts.iter().filter(|text| *text == key).count(), 1, "{key}");
    }
    assert_eq!(
        texts_of(&cx, "nodes-row-2")[1],
        "—",
        "no membership, no address"
    );
    let Some(view::wire::Node::Container(view::wire::ContainerNode {
        interactivity: Some(interactivity),
        ..
    })) = cx.find("nodes-validators")
    else {
        panic!("group heading is a native container");
    };
    assert_eq!(interactivity.role, Some(view::Role::Heading));
}

#[test]
fn the_strip_reads_the_last_64() {
    let (cx, _) = ready();
    assert!(cx.has_text("Proposed · last 64 blocks · 4,137 → 4,200"));
    let pages = cx.host().requests::<ChainBlocks>();
    assert_eq!(pages.len(), 1, "{pages:?}");
    assert_eq!(pages[0].before, Some(4201));
    assert_eq!(
        texts_of(&cx, "nodes-row-3"),
        ["0102", "10.0.0.9:4000", "Doesn't propose", "—", "—", "—"]
    );
}

#[test]
fn a_new_head_reads_one_block() {
    let (mut cx, ticks) = ready();
    cx.host().handle::<ChainStatus>(|()| {
        Ok(NodeStatus {
            height: 4201,
            ..status()
        })
    });
    ticks.send(());
    cx.run_until_parked();
    let pages = cx.host().requests::<ChainBlocks>();
    assert_eq!(
        pages.last(),
        Some(&BlockPage {
            before: Some(4202),
            limit: 1,
            most: 0,
        })
    );
    assert!(cx.has_text("Proposed · last 64 blocks · 4,138 → 4,201"));
}

#[test]
fn loading_waits_for_the_host() {
    let mut cx = TestAppContext::new();
    node(&mut cx);
    cx.host().never::<Query<Valset>>();
    cx.open::<Nodes>();
    cx.run_until_parked();
    assert!(cx.has_text("Reading the members…"));
}

#[test]
fn an_empty_set_says_so() {
    let mut cx = TestAppContext::new();
    node(&mut cx);
    serve_set(&cx, Vec::new(), Vec::new());
    cx.open::<Nodes>();
    cx.run_until_parked();
    assert!(cx.has_text("No members"));
}

#[test]
fn a_refusal_shows_its_sentence_and_retry_asks_again() {
    let mut cx = TestAppContext::new();
    node(&mut cx);
    cx.host()
        .refuse::<Query<Valset>>("unavailable", "valset is not running here");
    cx.open::<Nodes>();
    cx.run_until_parked();
    assert!(cx.has_text("valset is not running here"));
    respond(&mut cx);
    cx.simulate_click("nodes-retry");
    cx.run_until_parked();
    assert!(cx.has_text("10.0.0.1:4000"), "{:?}", cx.texts());
}

#[test]
fn a_live_bump_re_reads_and_a_snapshot_restores_the_screen() {
    let mut cx = TestAppContext::new();
    node(&mut cx);
    let feed = cx.host().stream::<Changes<Valset>>();
    respond(&mut cx);
    cx.open::<Nodes>();
    cx.run_until_parked();
    let moved = membership(&THIS, "10.9.9.9:4000", valset::Role::Validator);
    serve_set(&cx, vec![THIS.to_vec()], vec![moved]);
    feed.send(None);
    cx.run_until_parked();
    assert!(cx.has_text("10.9.9.9:4000") && !cx.has_text("10.0.0.1:4000"));

    let bytes = cx.snapshot().unwrap();
    let mut restored = TestAppContext::new();
    restored.host().never::<ChainStatus>();
    restored.host().never::<ChainNetwork>();
    restored.host().never::<Query<Valset>>();
    restored.restore::<Nodes>(&bytes).unwrap();
    restored.run_until_parked();
    assert!(restored.has_text("10.9.9.9:4000") && restored.has_text("Workshop"));
    assert_eq!(restored.host().requests::<Query<Valset>>().len(), 1);
}

#[test]
fn a_refused_live_head_is_logged_and_the_set_stays() {
    let mut cx = TestAppContext::new();
    node(&mut cx);
    cx.host()
        .refuse::<Changes<Valset>>("unavailable", "no live heads here");
    respond(&mut cx);
    cx.open::<Nodes>();
    cx.run_until_parked();
    assert!(cx.has_text("10.0.0.1:4000"));
    assert!(
        cx.host()
            .requests::<view::methods::HostLog>()
            .iter()
            .any(|line| line.contains("valset's live heads") && line.contains("no live heads here")),
        "{:?}",
        cx.host().requests::<view::methods::HostLog>()
    );
}

#[test]
fn a_page_that_fills_nothing_is_not_asked_again() {
    let (mut cx, ticks) = ready();
    cx.host().handle::<ChainBlocks>(|page| {
        let mut listed = blocks(page);
        listed.blocks.retain(|block| block.height > 4201);
        listed.listed_from = Some(4202);
        Ok(listed)
    });
    let asked = cx.host().requests::<ChainBlocks>().len();
    for height in [4201, 4201, 4202] {
        cx.host()
            .handle::<ChainStatus>(move |()| Ok(NodeStatus { height, ..status() }));
        ticks.send(());
        cx.run_until_parked();
    }
    let pages = cx.host().requests::<ChainBlocks>();
    assert_eq!(
        pages[asked..],
        [
            BlockPage {
                before: Some(4202),
                limit: 1,
                most: 0,
            },
            BlockPage {
                before: Some(4203),
                limit: 1,
                most: 0,
            }
        ]
    );
}
