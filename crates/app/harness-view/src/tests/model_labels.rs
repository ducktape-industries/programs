//! Each session row names its own kept model on desktop and phone.
use super::*;

fn kept(thread: u64, model: &str) -> harness::ConversationModel {
    harness::ConversationModel {
        thread,
        kept: Some(harness::ModelKept {
            model: model.into(),
            max_tokens: 4096,
            thinking: None,
            compact_at: None,
            provider: "openai".into(),
            service: None,
        }),
        window: Window::Unasked,
    }
}

fn contains(node: &view::wire::Node, expected: &str) -> bool {
    if let view::wire::Node::Text(text) = node {
        return text.content == expected;
    }
    node.children()
        .iter()
        .any(|child| contains(child, expected))
}

fn label(cx: &TestAppContext, thread: u64, expected: &str) {
    let id = format!("thread-{thread}-model");
    let node = cx.find(&id).unwrap_or_else(|| panic!("missing {id}"));
    assert!(contains(node, expected), "{id} does not show {expected}");
}

fn session_models(touch: bool) {
    let mut net = Net {
        kept: vec![kept(7, "gpt-6.1-sol"), kept(3, "opus-5-5")],
        ..Net::default()
    };
    net.threads.push(thread(11, SCOUT, EDDY, "chat/deploys"));
    net.threads[1].closed = false;
    let (mut cx, shared) = seated(net, EDDY);
    cx.simulate_touch(touch);
    let (width, height) = match touch {
        true => (390., 844.),
        false => (1440., 1000.),
    };
    cx.simulate_resize(width, height);
    cx.run_until_parked();
    let changes = cx.host().stream::<Changes<harness::Harness>>();
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    label(&cx, 7, "gpt-6.1-sol");
    label(&cx, 3, "opus-5-5");
    assert!(cx.find("thread-11-model").is_none());
    shared.borrow_mut().kept[0] = kept(7, "session-model-updated");
    changes.send(Some(Change {
        height: 10,
        keys: vec![Written::Thread(7).key()],
    }));
    cx.run_until_parked();
    label(&cx, 7, "session-model-updated");
    label(&cx, 3, "opus-5-5");
    assert!(cx.find("thread-11-model").is_none());
    cx.simulate_click("thread-7");
    cx.run_until_parked();
    assert!(cx.has_text("#general · 7"));
}

#[test]
fn desktop_session_rows_name_their_kept_models() {
    session_models(false);
}

#[test]
fn phone_session_rows_name_their_kept_models() {
    session_models(true);
}
