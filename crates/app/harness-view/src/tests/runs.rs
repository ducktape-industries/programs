//! The posts a conversation's agent goes on hearing after a mention, on the strip under what it
//! waits for, desktop and phone, read again when a block writes its agent's runs.
use super::*;

const RUN: &str =
    "Hears the rest of a mention: account 15 in chat channel general since message 7 (last 9)";

fn run(last: u64) -> harness::RunRow {
    harness::RunRow {
        channel: "general".into(),
        author: 15,
        mentioned: 7,
        last,
    }
}

#[test]
fn a_conversation_says_whose_posts_it_goes_on_hearing_and_reads_them_again_when_they_change() {
    let net = Net {
        runs: vec![(7, run(9))],
        ..Net::default()
    };
    let (mut cx, shared) = seated(net, EDDY);
    for key in ["agent-12", "thread-7"] {
        cx.simulate_click(key);
        cx.run_until_parked();
    }
    assert!(cx.has_text(RUN), "{:?}", cx.texts());
    shared.borrow_mut().runs[0].1.last = 10;
    let changes = cx.host().stream::<Changes<harness::Harness>>();
    changes.send(Some(Change {
        height: 12,
        keys: vec![Written::Runs(SCOUT).key()],
    }));
    cx.run_until_parked();
    let moved = RUN.replace("(last 9)", "(last 10)");
    assert!(cx.has_text(&moved), "{:?}", cx.texts());
    shared.borrow_mut().runs.clear();
    changes.send(Some(Change {
        height: 13,
        keys: vec![Written::Runs(SCOUT).key()],
    }));
    cx.run_until_parked();
    assert!(cx.find("thread-runs").is_none(), "{:?}", cx.texts());
}

#[test]
fn a_phone_says_whose_posts_its_agent_goes_on_hearing() {
    let net = Net {
        runs: vec![(7, run(9))],
        ..Net::default()
    };
    let (mut cx, _) = seated(net, EDDY);
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
    for key in ["agent-12", "thread-7"] {
        cx.simulate_click(key);
        cx.run_until_parked();
    }
    assert!(cx.has_text(RUN), "{:?}", cx.texts());
    assert!(cx.find("thread-runs").is_some());
}
