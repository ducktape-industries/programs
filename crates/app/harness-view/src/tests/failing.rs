//! A conversation whose inputs end in final errors wears a quiet Failing badge, on its row and in
//! its header, on desktop and phone, from harness's `FinalErrors`; a closed one never does, and an
//! answered turn takes it off.
use super::*;

fn failing(thread: u64, count: u64) -> harness::FinalError {
    harness::FinalError {
        thread,
        text: "invalid_request_error: prompt is too long".into(),
        frame: 4,
        height: 9010,
        time: 0,
        since: 9001,
        count,
    }
}

#[test]
fn a_conversation_in_final_errors_wears_a_failing_badge_on_its_row_and_its_header() {
    let net = Net {
        failing: vec![failing(7, 2), failing(3, 1)],
        ..Net::default()
    };
    let (mut cx, shared) = seated(net, EDDY);
    let changes = cx.host().stream::<Changes<harness::Harness>>();
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    let badge = cx.interactivity("thread-7-failing");
    assert_eq!(badge.role, Some(view::Role::Status));
    assert!(
        cx.find("thread-3-failing").is_none(),
        "a closed conversation wears Closed alone"
    );
    cx.simulate_click("thread-7");
    cx.run_until_parked();
    assert!(cx.find("thread-failing").is_some(), "{:?}", cx.texts());

    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
    assert!(cx.find("thread-failing").is_some(), "{:?}", cx.texts());
    shared.borrow_mut().failing.clear();
    let answered = Written::FinalError {
        agent: SCOUT,
        thread: 7,
    };
    changes.send(Some(Change {
        height: 10,
        keys: vec![answered.key()],
    }));
    cx.run_until_parked();
    assert!(cx.find("thread-failing").is_none(), "{:?}", cx.texts());
    cx.simulate_back();
    cx.run_until_parked();
    assert!(cx.find("thread-7-failing").is_none(), "{:?}", cx.texts());
}

#[test]
fn the_failing_badge_comes_on_when_a_final_error_is_written() {
    let (mut cx, shared) = seated(Net::default(), EDDY);
    let changes = cx.host().stream::<Changes<harness::Harness>>();
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(cx.find("thread-7-failing").is_none(), "{:?}", cx.texts());
    shared.borrow_mut().failing.push(failing(7, 1));
    let failed = Written::FinalError {
        agent: SCOUT,
        thread: 7,
    };
    changes.send(Some(Change {
        height: 10,
        keys: vec![failed.key()],
    }));
    cx.run_until_parked();
    assert!(cx.find("thread-7-failing").is_some(), "{:?}", cx.texts());
}
