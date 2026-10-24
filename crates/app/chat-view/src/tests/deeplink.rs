//! Exact message landing and its visible focus through the host route.
use super::*;

fn boot(touch: bool) -> (TestAppContext, Entity<Chat>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    threaded(&cx, 300, |seq| (11..=200).contains(&seq).then_some(10));
    let view = cx.open::<Chat>();
    cx.simulate_touch(touch);
    if touch {
        cx.simulate_resize(390., 844.);
    } else {
        cx.simulate_resize(1280., 800.);
    }
    seat(&mut cx);
    (cx, view)
}

fn route(cx: &mut TestAppContext, seq: u64) {
    cx.host()
        .stream::<view::methods::HostRoute>()
        .send(format!("general/{seq}"));
    cx.run_until_parked();
}

fn background(cx: &TestAppContext, touch: bool, seq: u64) -> Option<view::Hsla> {
    let id = match touch {
        true => format!("chat-message-m{seq}-row"),
        false => format!("chat-message-m{seq}-card"),
    };
    let Some(wire::Node::Container(node)) = cx.find(&id) else {
        return None;
    };
    cx.styles()[node.style]
        .background
        .as_ref()
        .and_then(|fill| fill.color())
        .and_then(|color| color.as_solid())
}

fn focused(cx: &TestAppContext, touch: bool, seq: u64) {
    assert_eq!(
        background(cx, touch, seq),
        Some(view::Theme::light().accent_soft),
        "exact target {seq} must be visibly focused"
    );
    let Some(wire::Node::Container(row)) = cx.find(&format!("chat-message-m{seq}-row")) else {
        panic!("linked message row must exist");
    };
    assert_eq!(
        row.interactivity
            .as_ref()
            .unwrap()
            .aria
            .description
            .as_deref(),
        Some("Linked message")
    );
}

#[test]
fn root_links_focus_the_exact_row_in_both_shells_and_replace_stale_threads() {
    for touch in [false, true] {
        let (mut cx, view) = boot(touch);
        route(&mut cx, 150);
        route(&mut cx, 250);
        assert!(view.read(|chat| chat.room.as_ref().unwrap().thread.is_none()));
        focused(&cx, touch, 250);
        route(&mut cx, 300);
        focused(&cx, touch, 300);
        route(&mut cx, 250);
        focused(&cx, touch, 250);
    }
}

#[test]
fn reply_links_focus_the_reply_even_when_its_root_is_outside_the_window() {
    for touch in [false, true] {
        let (mut cx, view) = boot(touch);
        route(&mut cx, 150);
        assert_eq!(
            view.read(|chat| chat.room.as_ref().unwrap().thread.as_ref().unwrap().root),
            10
        );
        focused(&cx, touch, 150);
        cx.host()
            .stream::<Changes<::chat::Chat>>()
            .send(block(4, posted("general")));
        cx.run_until_parked();
        focused(&cx, touch, 150);
        route(&mut cx, 160);
        focused(&cx, touch, 160);
    }
}

#[test]
fn a_same_room_link_leaves_the_tag_filter_and_old_actions() {
    for touch in [false, true] {
        let (mut cx, view) = boot(touch);
        route(&mut cx, 250);
        cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
            Ok(match query {
                Query::TagSearch { .. } => Reply::TagHits(page(vec![row(300, 7, "filtered")])),
                query => chat_reply(query),
            })
        }));
        cx.update(&view, |chat, _, cx| {
            chat.pick_filter(Some("tag".into()), cx);
            chat.show_menu(Pane::Timeline, 300, 0, Mode::More);
            cx.notify();
        });
        cx.run_until_parked();
        route(&mut cx, 250);
        view.read(|chat| {
            assert!(chat.filtered().is_none());
            assert!(chat.menu.is_none());
        });
        focused(&cx, touch, 250);
    }
}

#[test]
fn a_missing_target_never_selects_a_neighbour_and_a_refusal_stays_a_refusal() {
    for touch in [false, true] {
        let (mut cx, view) = boot(touch);
        cx.host()
            .handle::<Ask<::chat::Chat>>(rooms_alone(|query| match query {
                Query::MessagesAround { seq: 250, .. } => Ok(Reply::Messages(vec![
                    row(249, 7, "before"),
                    row(251, 7, "after"),
                ])),
                Query::MessagesAround { .. } => Err(wire::Error::new("refused", "private room")),
                query => Ok(chat_reply(query)),
            }));
        route(&mut cx, 250);
        view.read(|chat| assert!(chat.room.as_ref().unwrap().messages.failed().is_some()));
        assert!(background(&cx, touch, 251).is_none());
        route(&mut cx, 260);
        view.read(|chat| assert!(chat.room.as_ref().unwrap().messages.failed().is_some()));
    }
}

#[test]
fn an_unavailable_or_refused_reply_can_retry_the_same_target() {
    for touch in [false, true] {
        for refused in [false, true] {
            let (mut cx, view) = boot(touch);
            cx.host()
                .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| match query {
                    Query::MessagesAround { seq: 150, .. } => Ok(Reply::Messages(vec![MsgRow {
                        thread: Some(10),
                        ..row(150, 7, "linked reply")
                    }])),
                    Query::MessagesAround { seq: 10, .. } => {
                        Ok(Reply::Messages(vec![row(10, 7, "root")]))
                    }
                    Query::Thread { .. } if refused => {
                        Err(wire::Error::new("refused", "private thread"))
                    }
                    Query::Thread { .. } => Ok(Reply::Thread {
                        root: Some(row(10, 7, "root")),
                        replies: page(vec![MsgRow {
                            thread: Some(10),
                            ..row(151, 7, "neighbour")
                        }]),
                    }),
                    query => Ok(chat_reply(query)),
                }));
            route(&mut cx, 150);
            view.read(|chat| {
                let thread = chat.room.as_ref().unwrap().thread.as_ref().unwrap();
                assert!(thread.replies.failed().is_some());
                assert_eq!(thread.target, 150);
            });
            assert!(background(&cx, touch, 151).is_none());
            threaded(&cx, 300, |seq| (11..=200).contains(&seq).then_some(10));
            if touch {
                cx.simulate_click("chat-messages-retry");
            } else {
                cx.host()
                    .stream::<Changes<::chat::Chat>>()
                    .send(block(4, posted("general")));
            }
            cx.run_until_parked();
            focused(&cx, touch, 150);
        }
    }
}

#[test]
fn retrying_an_unreadable_root_window_keeps_the_original_linked_reply() {
    for touch in [false, true] {
        for refused in [false, true] {
            let (mut cx, view) = boot(touch);
            cx.host()
                .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| match query {
                    Query::MessagesAround { seq: 150, .. } => Ok(Reply::Messages(vec![MsgRow {
                        thread: Some(10),
                        ..row(150, 7, "linked reply")
                    }])),
                    Query::MessagesAround { seq: 10, .. } if refused => {
                        Err(wire::Error::new("refused", "private root"))
                    }
                    Query::MessagesAround { seq: 10, .. } => {
                        Ok(Reply::Messages(vec![row(9, 7, "neighbour")]))
                    }
                    Query::Thread { .. } => Ok(Reply::Thread {
                        root: Some(row(10, 7, "root")),
                        replies: page(vec![MsgRow {
                            thread: Some(10),
                            ..row(150, 7, "linked reply")
                        }]),
                    }),
                    query => Ok(chat_reply(query)),
                }));
            route(&mut cx, 150);
            view.read(|chat| {
                let room = chat.room.as_ref().unwrap();
                assert!(room.messages.failed().is_some());
                assert_eq!(room.thread.as_ref().unwrap().target, 150);
            });
            threaded(&cx, 300, |seq| (11..=200).contains(&seq).then_some(10));
            cx.host()
                .stream::<Changes<::chat::Chat>>()
                .send(block(4, posted("general")));
            cx.run_until_parked();
            view.read(|chat| {
                let room = chat.room.as_ref().unwrap();
                assert!(room.messages.ready().is_some());
                assert_eq!(room.thread.as_ref().unwrap().root, 10);
                assert_eq!(room.focused, Some((Pane::Thread, 150)));
            });
            focused(&cx, touch, 150);
        }
    }
}

#[test]
fn export_deeplink_screens() {
    let Some(out) = ::view::testing::screens("CHAT_SCREEN_EXPORT", "chat/fixtures") else {
        return;
    };
    let mut listed = Vec::new();
    for touch in [false, true] {
        for theme in [view::Theme::light(), view::Theme::dark()] {
            let (mut cx, _) = boot(touch);
            cx.set_global(theme);
            cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
                Ok(match query {
                    Query::MessagesAround { seq: 150, .. } => Reply::Messages(vec![MsgRow {
                        thread: Some(10),
                        ..row(150, 7, "This is the linked message.")
                    }]),
                    Query::MessagesAround { seq: 10, .. } => {
                        Reply::Messages(vec![row(10, 7, "The thread root.")])
                    }
                    Query::Thread { .. } => Reply::Thread {
                        root: Some(row(10, 7, "The thread root.")),
                        replies: page(vec![MsgRow {
                            thread: Some(10),
                            ..row(150, 7, "This is the linked message.")
                        }]),
                    },
                    query => chat_reply(query),
                })
            }));
            route(&mut cx, 150);
            assert_eq!(background(&cx, touch, 150), Some(theme.accent_soft));
            let shell = if touch { "phone" } else { "desktop" };
            let mode = if theme == view::Theme::light() {
                "light"
            } else {
                "dark"
            };
            let name = format!("deeplink-{shell}-{mode}");
            std::fs::write(
                out.join(format!("{name}.json")),
                serde_json::to_vec(&cx.whole_frame()).unwrap(),
            )
            .unwrap();
            let (width, height) = cx.viewport();
            listed.push(serde_json::json!({"name": name, "width": width as u32, "height": height as u32, "theme": mode}));
        }
    }
    std::fs::write(
        out.join("deeplink-manifest.json"),
        serde_json::to_vec(&listed).unwrap(),
    )
    .unwrap();
}
