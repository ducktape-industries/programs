//! What chat draws while its reads are out: each pane's static placeholders, which say they are
//! busy and draw no words, and "Not connected" only once a session says so.
use super::*;

fn busy(cx: &TestAppContext, key: &str, label: &str) {
    let region = cx.interactivity(key);
    assert!(region.aria.busy, "{key} says it is busy");
    assert_eq!(region.aria.label.as_deref(), Some(label));
    let node = cx.find(key).expect(key);
    assert!(!holds_words(node), "{key} draws no words: {node:?}");
}

fn holds_words(node: &wire::Node) -> bool {
    matches!(node, wire::Node::Text(_)) || node.children().iter().any(holds_words)
}

#[test]
fn rooms_read_before_the_session_wait_for_it_under_their_placeholders() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.open::<Chat>();
    cx.run_until_parked();
    assert!(!cx.has_text("Not connected"), "{:?}", cx.texts());
    busy(&cx, "chat-sidebar-loading", "Loading rooms…");
    busy(&cx, "chat-no-room-loading", "Loading channels…");
}

#[test]
fn before_the_host_hands_a_session_the_panes_draw_their_placeholders() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host().never::<Ask<::chat::Chat>>();
    cx.open::<Chat>();
    cx.run_until_parked();
    assert!(!cx.has_text("Not connected"), "{:?}", cx.texts());
    busy(&cx, "chat-sidebar-loading", "Loading rooms…");
    busy(&cx, "chat-no-room-loading", "Loading channels…");
}

#[test]
fn a_phone_before_its_session_draws_placeholders_and_none_of_its_rows() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.open::<Chat>();
    cx.simulate_touch(true);
    cx.run_until_parked();
    busy(&cx, "chat-rooms-loading", "Loading rooms…");
    let texts = cx.texts();
    assert!(
        !texts.iter().any(|text| text.contains("more channel")),
        "{texts:?}"
    );
    assert!(!cx.has_text("Not connected to a node."));
}

#[test]
fn a_session_without_a_node_says_not_connected() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.open::<Chat>();
    cx.host().stream::<HostSession>().send(Session::default());
    cx.run_until_parked();
    assert!(cx.has_text("Not connected"), "{:?}", cx.texts());
    assert!(cx.find("chat-sidebar-loading").is_none());
}

#[test]
fn the_rooms_draw_placeholders_until_they_land() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host().never::<Ask<::chat::Chat>>();
    cx.open::<Chat>();
    seat(&mut cx);
    busy(&cx, "chat-sidebar-loading", "Loading rooms…");
}

#[test]
fn a_rooms_messages_draw_placeholders_until_they_land() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let view = cx.open::<Chat>();
    seat(&mut cx);
    cx.host().never::<Ask<::chat::Chat>>();
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    busy(&cx, "chat-messages-loading", "Loading messages…");
    cx.update(&view, |chat, _, cx| {
        chat.room.as_mut().expect("a room is open").messages =
            Loadable::Ready(vec![row(1, 7, "hello")]);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.has_text("hello"));
    assert!(cx.find("chat-messages-loading").is_none());
}
