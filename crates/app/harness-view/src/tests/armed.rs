//! What a conversation's agent waits for (`wait_for`), on the strip under what its turn waits on,
//! desktop and phone, read again when a block writes its armed waits.
use super::*;

const WAITS: &str = "Waits for pod duckpond-7 to end (since block 9,002)";

fn waiting(id: u64, made: u64, event: harness::WaitFor) -> harness::WaitRow {
    harness::WaitRow {
        id,
        agent: SCOUT,
        thread: 7,
        made,
        event,
        ended: None,
    }
}

fn pod() -> harness::WaitFor {
    harness::WaitFor::Pod {
        name: "duckpond-7".into(),
    }
}

#[test]
fn a_conversation_says_what_its_agent_waits_for_and_reads_it_again_when_it_changes() {
    let net = Net {
        armed: vec![waiting(1, 9002, pod())],
        ..Net::default()
    };
    let (mut cx, shared) = seated(net, EDDY);
    for key in ["agent-12", "thread-7"] {
        cx.simulate_click(key);
        cx.run_until_parked();
    }
    assert!(cx.has_text(WAITS), "{:?}", cx.texts());
    let height = harness::WaitFor::Height { height: 12_000 };
    shared.borrow_mut().armed.push(waiting(2, 9003, height));
    let changes = cx.host().stream::<Changes<harness::Harness>>();
    changes.send(Some(Change {
        height: 12,
        keys: vec![Written::Armed(7).key()],
    }));
    cx.run_until_parked();
    let both = format!("{WAITS} · block 12,000 (since block 9,003)");
    assert!(cx.has_text(&both), "{:?}", cx.texts());
    shared.borrow_mut().armed.clear();
    changes.send(Some(Change {
        height: 13,
        keys: vec![Written::Armed(7).key()],
    }));
    cx.run_until_parked();
    assert!(cx.find("thread-armed").is_none(), "{:?}", cx.texts());
}

#[test]
fn a_phone_says_what_its_agent_waits_for() {
    let net = Net {
        armed: vec![waiting(1, 9002, pod())],
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
    assert!(cx.has_text(WAITS), "{:?}", cx.texts());
    assert!(cx.find("thread-armed").is_some());
}
