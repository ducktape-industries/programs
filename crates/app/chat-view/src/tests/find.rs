//! Find in the open room: Cmd/Ctrl+F opens a bar that marks the loaded messages holding every
//! word, steps through them, and asks chat's text search for the room's older matches.
use super::*;

fn marked(cx: &TestAppContext, key: &str) -> Vec<String> {
    let Some(wire::Node::RichText { text, runs, .. }) = cx.find(key) else {
        panic!("{key} is one rich text node");
    };
    let wire::RichTextRuns::Highlights(highlights) = runs else {
        return Vec::new();
    };
    highlights
        .iter()
        .filter(|(_, style)| style.background_color.is_some())
        .map(|(range, _)| text[range.clone()].to_owned())
        .collect()
}

fn on(cx: &TestAppContext, seq: u64) -> bool {
    cx.interactivity(&format!("chat-message-m{seq}"))
        .aria
        .active_descendant
}

fn searched(cx: &TestAppContext) -> Vec<::chat::PageRequest> {
    cx.host()
        .requests::<Ask<::chat::Chat>>()
        .into_iter()
        .filter_map(|query| match query {
            Query::TextSearch { page, .. } => Some(page),
            _ => None,
        })
        .collect()
}

fn around(cx: &TestAppContext) -> Vec<u64> {
    cx.host()
        .requests::<Ask<::chat::Chat>>()
        .into_iter()
        .filter_map(|query| match query {
            Query::MessagesAround { seq, .. } => Some(seq),
            _ => None,
        })
        .collect()
}

fn find_opened_in(cx: &mut TestAppContext, key: &str, keystroke: &str) {
    cx.simulate_focus(key);
    cx.simulate_key_down(key, keystroke);
    assert!(
        cx.find("chat-find").is_some(),
        "{keystroke} opens the find bar"
    );
}

#[test]
fn cmd_f_marks_the_loaded_matches_and_enter_steps_to_the_older_one() {
    let (mut cx, _) = opened();
    assert!(cx.find("chat-find").is_none());
    find_opened_in(&mut cx, "chat-message-list", "cmd-f");
    cx.simulate_focus("chat-find");
    cx.simulate_input("chat-find", "HE");
    assert_eq!(marked(&cx, "chat-message-m1-block-0"), ["he"]);
    assert_eq!(marked(&cx, "chat-message-m2-block-0"), ["he"]);
    assert!(cx.has_text("1 of 2+"), "{:?}", cx.texts());
    assert!(on(&cx, 2), "the newest match first");
    cx.simulate_field_key("chat-find", "enter");
    assert!(on(&cx, 1), "Enter goes to the older match");
    assert!(cx.has_text("2 of 2+"));
    cx.simulate_field_key("chat-find", "shift-enter");
    assert!(on(&cx, 2), "Shift+Enter goes back to the newer one");
    assert!(searched(&cx).is_empty(), "the loaded rows ask nothing");
    cx.simulate_field_key("chat-find", "escape");
    assert!(cx.find("chat-find").is_none(), "Escape closes it");
    assert!(marked(&cx, "chat-message-m1-block-0").is_empty());
}

#[test]
fn ctrl_f_in_the_composer_opens_it_too() {
    let (mut cx, _) = opened();
    find_opened_in(&mut cx, "draft-general/editor", "ctrl-f");
}

fn far_room(cx: &TestAppContext) {
    cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
        Ok(match query {
            Query::Channels { .. } => {
                Reply::Channels(page(vec![channel("general", "General", 51), review_room()]))
            }
            Query::Roots { channel_id, .. } if channel_id == "general" => {
                Reply::Roots(page(vec![row(50, 8, "bye"), row(51, 7, "hello now")]))
            }
            Query::TextSearch {
                text,
                channel_id,
                page: asked,
                ..
            } => {
                assert_eq!(text, "hello");
                assert_eq!(channel_id.as_deref(), Some("general"), "this room alone");
                assert!(asked.limit.is_some(), "a bounded page");
                match asked.after.as_deref() {
                    None => Reply::TextHits(::chat::PageResponse {
                        next: Some(b"older".to_vec()),
                        ..page(vec![row(51, 7, "hello now"), row(20, 7, "hello then")])
                    }),
                    Some(cursor) => {
                        assert_eq!(cursor, b"older", "the cursor the first page answered");
                        Reply::TextHits(page(vec![row(5, 7, "hello at first")]))
                    }
                }
            }
            Query::MessagesAround {
                channel_id, seq, ..
            } => Reply::Messages(vec![MsgRow {
                channel_id,
                ..row(seq, 7, &format!("hello {seq}"))
            }]),
            query => chat_reply(query),
        })
    }));
}

#[test]
fn enter_past_the_oldest_loaded_match_reads_the_rooms_search_a_page_at_a_time() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    far_room(&cx);
    let _view = cx.open::<Chat>();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    find_opened_in(&mut cx, "chat-message-list", "cmd-f");
    cx.simulate_focus("chat-find");
    cx.simulate_input("chat-find", "hello");
    assert!(cx.has_text("1 of 1+"), "{:?}", cx.texts());
    assert!(searched(&cx).is_empty());
    cx.simulate_field_key("chat-find", "enter");
    cx.run_until_parked();
    assert_eq!(searched(&cx).len(), 1);
    assert_eq!(searched(&cx)[0].after, None);
    assert_eq!(around(&cx), [20], "the hit lands with the rows around it");
    assert_eq!(marked(&cx, "chat-message-m20-block-0"), ["hello"]);
    assert!(cx.has_text("2 of 2+"), "{:?}", cx.texts());
    cx.simulate_field_key("chat-find", "enter");
    cx.run_until_parked();
    assert_eq!(searched(&cx).len(), 2, "the page after the cursor");
    assert_eq!(around(&cx), [20, 5]);
    assert_eq!(marked(&cx, "chat-message-m5-block-0"), ["hello"]);
    assert!(cx.has_text("3 of 3"), "{:?}", cx.texts());
    cx.simulate_field_key("chat-find", "enter");
    cx.run_until_parked();
    assert_eq!(searched(&cx).len(), 2, "no older page is left to ask");
    cx.simulate_field_key("chat-find", "shift-enter");
    cx.run_until_parked();
    assert_eq!(
        around(&cx),
        [20, 5, 20],
        "Shift+Enter lands on the newer hit"
    );
}

#[test]
fn another_room_opens_without_the_bar() {
    let (mut cx, view) = opened();
    find_opened_in(&mut cx, "chat-message-list", "cmd-f");
    cx.update(&view, |chat, _, cx| {
        cx.notify();
        chat.open_at("forge:web:3".into(), 0, cx);
    });
    cx.run_until_parked();
    assert!(cx.find("chat-find").is_none());
}
