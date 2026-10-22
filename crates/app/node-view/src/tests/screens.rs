//! The node-less screen dumps: each state of the sheet, light and dark, for
//! the app's renderer (`ducktape-app --render-tree <json>`). A network like
//! the design's: five validators (in sync, one block short, 12 behind, two
//! short, stopped at 3,871) and three residents, at 4,295.
use super::*;
use view::Theme;

const HEIGHT: u64 = 4295;
const VALIDATORS: usize = 5;
const MEMBERS: usize = 8;

fn key(index: usize) -> Vec<u8> {
    (0..32u32)
        .map(|byte| (byte * 151 + index as u32 * 89 + 36) as u8)
        .collect()
}

fn sheet_status(this: usize) -> NodeStatus {
    NodeStatus {
        chain_id: "testkit".into(),
        time: 1_790_121_600_000,
        block_time_ms: 1000,
        epoch_length: 64,
        height: HEIGHT,
        tip: [0xb2; 32],
        root: [0x41; 32],
        epoch: 67,
        identity: key(this),
        contract: 1,
    }
}

fn leader(height: u64) -> Vec<u8> {
    let turn = match height > 4250 {
        true => height % 3,
        false => height % 4,
    };
    key(turn as usize)
}

fn sheet_blocks(page: BlockPage) -> BlockList {
    let below = page.before.unwrap_or(HEIGHT + 1);
    let blocks = (below.saturating_sub(page.limit as u64)..below)
        .rev()
        .map(|height| Block {
            height,
            proposer: Some(leader(height)),
            ..Block::default()
        })
        .collect();
    BlockList {
        blocks,
        listed_from: Some(0),
    }
}

fn sheet_valset(cx: &TestAppContext) {
    let members = (0..MEMBERS)
        .map(|index| {
            let (port, role) = match index < VALIDATORS {
                true => (44571 + index, valset::Role::Validator),
                false => (44581 + index - VALIDATORS, valset::Role::Resident),
            };
            membership(&key(index), &format!("127.0.0.1:{port}"), role)
        })
        .collect();
    serve_set(cx, (0..VALIDATORS).map(key).collect(), members);
}

const SYNCED: [Option<u64>; VALIDATORS] = [
    Some(HEIGHT),
    Some(HEIGHT - 1),
    Some(HEIGHT - 12),
    Some(HEIGHT - 2),
    Some(3871),
];
const BEHIND: [Option<u64>; VALIDATORS] = [
    Some(HEIGHT),
    Some(HEIGHT - 5),
    Some(HEIGHT - 19),
    Some(HEIGHT - 20),
    None,
];

fn sheet_network(signed: [Option<u64>; VALIDATORS]) -> NetworkStatus {
    NetworkStatus {
        height: HEIGHT,
        members: (0..MEMBERS)
            .map(|index| Peer {
                key: key(index),
                signed: signed.get(index).copied().flatten(),
            })
            .collect(),
    }
}

fn sheet(
    network: Option<NetworkStatus>,
    this: usize,
) -> (TestAppContext, StreamSender<ClockTicks>) {
    let mut cx = TestAppContext::new();
    let ticks = cx.host().stream::<ClockTicks>();
    cx.host()
        .handle::<ChainStatus>(move |()| Ok(sheet_status(this)));
    cx.host()
        .handle::<ChainBlocks>(|page| Ok(sheet_blocks(page)));
    match network {
        Some(network) => cx
            .host()
            .handle::<ChainNetwork>(move |()| Ok(network.clone())),
        None => cx
            .host()
            .refuse::<ChainNetwork>(methods::refusal::UNKNOWN_REQUEST, crate::words::NO_NETWORK),
    }
    sheet_valset(&cx);
    cx.open::<Nodes>();
    cx.run_until_parked();
    (cx, ticks)
}

fn silent((mut cx, ticks): (TestAppContext, StreamSender<ClockTicks>)) -> TestAppContext {
    cx.host().never::<ChainStatus>();
    cx.host().never::<ChainNetwork>();
    for _ in 0..=SILENT_TICKS {
        ticks.send(());
    }
    cx.run_until_parked();
    cx
}

fn screen(state: &str) -> TestAppContext {
    let quiet_unsigned = [SYNCED[0], SYNCED[1], SYNCED[2], SYNCED[3], None];
    let resident = || sheet(Some(sheet_network([None; VALIDATORS])), VALIDATORS);
    match state {
        "synced" | "synced-680" | "synced-narrow" => sheet(Some(sheet_network(SYNCED)), 0).0,
        "quiet-unsigned" => sheet(Some(sheet_network(quiet_unsigned)), 0).0,
        "unknown-request" => sheet(None, 0).0,
        "not-answering" => silent(sheet(Some(sheet_network(SYNCED)), 0)),
        "behind-quiet" => sheet(Some(sheet_network(BEHIND)), 0).0,
        "resident" => resident().0,
        "resident-not-answering" => silent(resident()),
        "promoted" => {
            let own = [None, SYNCED[1], SYNCED[2], SYNCED[3], SYNCED[4]];
            sheet(Some(sheet_network(own)), 0).0
        }
        "refused-after-answer" => {
            let (mut cx, ticks) = sheet(Some(sheet_network(SYNCED)), 0);
            cx.host()
                .refuse::<ChainNetwork>("unavailable", "The node could not be reached.");
            ticks.send(());
            cx.run_until_parked();
            cx
        }
        "loading" => {
            let mut cx = TestAppContext::new();
            cx.host().never::<ChainStatus>();
            cx.host().never::<ChainNetwork>();
            cx.host().never::<Query<Valset>>();
            cx.open::<Nodes>();
            cx.run_until_parked();
            cx
        }
        "status-refused" => {
            let mut cx = TestAppContext::new();
            cx.host()
                .refuse::<ChainStatus>("unavailable", "The node is unavailable. Try again.");
            cx.host().never::<ChainNetwork>();
            sheet_valset(&cx);
            cx.open::<Nodes>();
            cx.run_until_parked();
            cx
        }
        "members-refused" | "members-empty" => {
            let mut cx = TestAppContext::new();
            cx.host().handle::<ChainStatus>(|()| Ok(sheet_status(0)));
            cx.host()
                .handle::<ChainBlocks>(|page| Ok(sheet_blocks(page)));
            cx.host()
                .handle::<ChainNetwork>(|()| Ok(sheet_network(SYNCED)));
            match state {
                "members-refused" => cx
                    .host()
                    .refuse::<Query<Valset>>("unavailable", "valset is not running here"),
                _ => serve_set(&cx, Vec::new(), Vec::new()),
            }
            cx.open::<Nodes>();
            cx.run_until_parked();
            cx
        }
        other => panic!("no screen {other}"),
    }
}

const SCREENS: [(&str, u32, u32); 15] = [
    ("synced", 1100, 680),
    ("synced-680", 680, 620),
    ("synced-narrow", 320, 800),
    ("quiet-unsigned", 1100, 680),
    ("unknown-request", 1100, 680),
    ("not-answering", 1100, 680),
    ("loading", 1100, 680),
    ("status-refused", 1100, 680),
    ("members-refused", 1100, 680),
    ("members-empty", 1100, 680),
    ("behind-quiet", 1100, 680),
    ("resident", 1100, 680),
    ("resident-not-answering", 1100, 680),
    ("promoted", 1100, 680),
    ("refused-after-answer", 1100, 680),
];

const SWEEP: [&str; 4] = ["synced", "resident", "not-answering", "behind-quiet"];
const WIDTHS: [u32; 8] = [480, 560, 680, 768, 960, 1000, 1064, 1280];

const APP_MIN_WIDTH: f32 = 480.;

fn in_app_frame(mut frame: view::wire::Frame) -> view::wire::Frame {
    use view::wire::{ContainerNode, ElementIdWire, Node, Style, StyleId};
    use view::{StyleRefinement, Styled, px};
    let inner = StyleRefinement::default()
        .size_full()
        .min_w(px(APP_MIN_WIDTH));
    let mut outer = StyleRefinement::default().size_full().overflow_hidden();
    outer.overflow.x = serde_json::from_value(serde_json::json!("Scroll")).unwrap();
    let first = frame.styles.len() as u32;
    frame
        .styles
        .extend([Style::new(&inner), Style::new(&outer)]);
    let inner = ContainerNode {
        id: None,
        style: StyleId(first),
        interactivity: Default::default(),
        children: frame.root.take().into_iter().collect(),
    };
    frame.root = Some(Node::Container(ContainerNode {
        id: Some(ElementIdWire::Name("app-frame".into())),
        style: StyleId(first + 1),
        interactivity: Default::default(),
        children: vec![Node::Container(inner)],
    }));
    frame
}

#[test]
fn export_node_screens() {
    let Some(out) = ::view::testing::screens("NODE_SCREEN_EXPORT", "node-screens") else {
        return;
    };
    let screens = SCREENS
        .iter()
        .enumerate()
        .map(|(index, (state, width, height))| {
            (format!("{:02}-{state}", index + 1), *state, *width, *height)
        })
        .chain(SWEEP.iter().flat_map(|state| {
            WIDTHS
                .iter()
                .map(move |width| (format!("w{width}-{state}"), *state, *width, 800))
        }));
    let mut manifest = Vec::new();
    for (name, state, width, height) in screens {
        let framed = (width as f32) < APP_MIN_WIDTH;
        for dark in [false, true] {
            let mut cx = screen(state);
            let laid_out = (width as f32).max(APP_MIN_WIDTH);
            cx.simulate_resize(laid_out, height as f32);
            cx.run_until_parked();
            if dark {
                cx.set_global(Theme::dark());
            }
            let theme = if dark { "dark" } else { "light" };
            let name = format!("{name}-{theme}");
            let tree = match framed {
                true => in_app_frame(cx.whole_frame()),
                false => cx.whole_frame(),
            };
            std::fs::write(
                out.join(format!("{name}.json")),
                serde_json::to_vec(&tree).unwrap(),
            )
            .unwrap();
            manifest.push(serde_json::json!({
                "name": name,
                "theme": theme,
                "width": width,
                "height": height,
                "how": "TestAppContext + FakeHost over canned node answers",
            }));
        }
    }
    std::fs::write(
        out.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
}
