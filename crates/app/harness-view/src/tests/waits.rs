//! What a conversation and its calls wait on at egress: the request, its node and its block on
//! the strip and in a relaying call's box, desktop and phone, and a turn no request is open for.
//! A result shows its first line until a press shows it whole.
use super::*;

const WAITING: &str = "Waiting on egress request 41 since block 9,001 at node abababab";
const LOST: &str = "No egress request is open for it · it last moved at block 9,003";

fn opened(net: Net) -> (TestAppContext, Shared) {
    let (mut cx, shared) = seated(net, EDDY);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    cx.simulate_click("thread-7");
    cx.run_until_parked();
    (cx, shared)
}

fn relay(id: u64, call: &str, made: u64) -> egress::RequestRow {
    let note = call.as_bytes().to_vec();
    let request = egress::Request::Relay { note };
    requested(id, request, egress::Delivery::Whole, made)
}

#[test]
fn a_turn_says_which_egress_request_it_waits_on() {
    let (cx, _) = opened(Net::default());
    assert!(cx.has_text(WAITING), "{:?}", cx.texts());
    assert!(!cx.has_text(LOST), "{:?}", cx.texts());
}

#[test]
fn another_threads_reply_is_not_this_ones_wait() {
    let net = Net {
        requests: vec![turn_request(41, 9, 9001)],
        ..Net::default()
    };
    let (cx, _) = opened(net);
    assert!(cx.has_text(LOST), "{:?}", cx.texts());
}

#[test]
fn a_turn_no_request_is_open_for_says_so_and_a_write_to_its_row_reads_the_waits_again() {
    let (mut cx, shared) = opened(Net::default());
    let changes = cx.host().stream::<Changes<harness::Harness>>();
    shared.borrow_mut().requests.clear();
    // egress's block alone moves no wait
    cx.host()
        .stream::<Changes<egress::Egress>>()
        .send(Some(Change {
            height: 10,
            keys: Vec::new(),
        }));
    cx.run_until_parked();
    assert!(cx.has_text(WAITING), "{:?}", cx.texts());
    changes.send(Some(Change {
        height: 11,
        keys: vec![Written::Thread(7).key()],
    }));
    cx.run_until_parked();
    assert!(cx.has_text(LOST), "{:?}", cx.texts());
    assert!(!cx.has_text(WAITING), "{:?}", cx.texts());
}

#[test]
fn learning_the_window_says_its_request() {
    let mut net = Net::default();
    net.threads[0].asking = Asking::Learning;
    let head = egress::http::Head {
        method: egress::http::Method::Get,
        target: "/v1/models/claude-sonnet-5-5".into(),
        headers: vec![("anthropic-version".into(), "2023-06-01".into())],
    };
    let request = egress::Request::Http {
        service: "anthropic".into(),
        head,
        body: Vec::new(),
    };
    net.requests = vec![requested(43, request, egress::Delivery::Whole, 9004)];
    let (cx, _) = opened(net);
    let words = "Waiting on egress request 43 since block 9,004 at node abababab";
    assert!(cx.has_text(words), "{:?}", cx.texts());
}

#[test]
fn a_relaying_call_says_its_relay_and_one_without_says_none_is_open() {
    let mut net = Net::default();
    net.calls[0].state = CallState::Relaying;
    net.requests.push(relay(42, "toolu_01", 9002));
    let (mut cx, shared) = opened(net);
    let changes = cx.host().stream::<Changes<harness::Harness>>();
    // the burst the loop is still in is open of itself
    cx.simulate_click("call-toolu_01");
    cx.run_until_parked();
    let relaying = "relaying on egress request 42 since block 9,002 at node abababab";
    assert!(cx.has_text(relaying), "{:?}", cx.texts());
    shared.borrow_mut().requests.retain(|row| row.id != 42);
    let call = Written::Call {
        thread: 7,
        call: "toolu_01".into(),
    };
    changes.send(Some(Change {
        height: 12,
        keys: vec![call.key()],
    }));
    cx.run_until_parked();
    let none = "no egress relay is open for it";
    assert!(cx.has_text(none), "{:?}", cx.texts());
}

#[test]
fn a_phone_says_what_the_turn_waits_on_and_when_nothing_is_open() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
    for key in ["agent-12", "thread-7"] {
        cx.simulate_click(key);
        cx.run_until_parked();
    }
    assert!(cx.has_text(WAITING), "{:?}", cx.texts());

    let net = Net {
        requests: Vec::new(),
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
    assert!(cx.has_text(LOST), "{:?}", cx.texts());
    assert!(cx.find("thread-wait").is_some());
}

#[test]
fn an_idle_conversation_reads_no_waits_and_says_none() {
    let mut net = Net::default();
    net.threads[0].asking = Asking::Idle;
    let (cx, _) = opened(net);
    assert!(!cx.has_text(WAITING), "{:?}", cx.texts());
    assert!(!cx.has_text(LOST), "{:?}", cx.texts());
    assert!(cx.find("thread-wait").is_none());
}

#[test]
fn a_result_shows_its_first_line_until_a_press_shows_it_whole() {
    let mut net = Net::default();
    let (_, result) = net
        .frames
        .iter_mut()
        .find(|(thread, frame)| *thread == 7 && frame.seq == 3)
        .unwrap();
    result.body = Body::Result {
        call: "toolu_01".into(),
        tool: "forge_log".into(),
        text: "c41a90e2 payments: timeout 3s -> 0.8s\n9b20d1aa payments: retry once".into(),
        failed: false,
    };
    let (mut cx, _) = opened(net);
    // the burst the loop is still in is open of itself
    cx.simulate_click("call-toolu_01");
    cx.run_until_parked();
    let first = "c41a90e2 payments: timeout 3s -> 0.8s";
    assert!(cx.has_text(first), "{:?}", cx.texts());
    assert!(!cx.has_text("Show less"), "{:?}", cx.texts());
    cx.simulate_focus("all-3");
    cx.simulate_click("all-3");
    cx.run_until_parked();
    let whole = "c41a90e2 payments: timeout 3s -> 0.8s\n9b20d1aa payments: retry once";
    assert!(cx.has_text(whole), "{:?}", cx.texts());
    cx.simulate_click("all-3-less");
    cx.run_until_parked();
    assert!(cx.has_text(first), "{:?}", cx.texts());
    assert!(!cx.has_text(whole), "{:?}", cx.texts());
}

#[test]
fn a_long_prompt_sentence_is_kept_whole_for_its_row_to_clip() {
    let mut net = Net::default();
    let long = "You are Scout, who reads every deploy log of the payments service before it answers anyone.";
    let Configured::Ready { config, .. } = &mut net.setups[0].configured else {
        panic!()
    };
    config.system = long.into();
    let (mut cx, _) = seated(net, EDDY);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    let words = format!("1 line · “{long}”");
    assert!(cx.has_text(&words), "{:?}", cx.texts());
}
