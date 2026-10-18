//! The open room: its rows, panes, lists and links.
use super::*;

#[test]
fn the_room_shows_its_rows_intro_and_actions() {
    let (mut cx, view) = opened();
    let texts = cx.texts();
    assert!(cx.has_text("hello"), "{texts:?}");
    assert!(cx.has_text("eddy") && cx.has_text("reviewer"));
    assert!(
        cx.has_text("Agent · managed by eddy"),
        "an agent wears its badge and its manager: {texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|t| t.starts_with("This is the very beginning of #General")),
        "{texts:?}"
    );
    // Each paragraph keeps a stable typed element id for interaction queries.
    for seq in [1, 2] {
        assert!(cx.find(&format!("chat-message-m{seq}-block-0")).is_some());
    }
    message::hover(&mut cx, &view, 1);
    cx.simulate_click("chat-message-m1-react");
    assert!(
        cx.find(&ui::menu::focus_key(Pane::Timeline, Mode::Reactions))
            .is_some(),
        "the host focus target must exist in the open menu"
    );
    assert!(
        cx.host()
            .requests::<view::methods::HostWidget>()
            .iter()
            .any(|command| {
                matches!(command, wire::WidgetCommand::Focus { target }
                if target.last() == Some(&wire::ElementIdWire::Name(
                    ui::menu::focus_key(Pane::Timeline, Mode::Reactions).into()
                )))
            })
    );
    view.read(|chat| {
        assert!(
            chat.menu
                .as_ref()
                .is_some_and(|m| m.mode == Mode::Reactions)
        )
    });
    cx.simulate_click("chat-reaction-🔥");
    cx.run_until_parked();
    assert!(
        cx.host()
            .requests::<Submit<::chat::Chat>>()
            .iter()
            .any(|op| matches!(op, Op::AddReaction { emoji, .. } if emoji == "🔥"))
    );
    view.read(|chat| assert!(chat.menu.is_none()));
    cx.simulate_click("chat-message-m1-more");
    assert!(
        cx.find(&ui::menu::focus_key(Pane::Timeline, Mode::More))
            .is_some()
    );
    assert!(cx.has_text("Reply in thread") && cx.has_text("Copy link"));
    cx.simulate_click("chat-message-m1-thread");
    cx.run_until_parked();
    view.read(|chat| {
        assert_eq!(
            chat.room.as_ref().unwrap().thread.as_ref().map(|t| t.root),
            Some(1)
        )
    });
    assert!(cx.has_text("Thread") && cx.has_text("Reply in thread"));
    cx.simulate_click("chat-thread-close");
    view.read(|chat| assert!(chat.room.as_ref().unwrap().thread.is_none()));
    cx.simulate_click("chat-room-details");
    assert!(cx.has_text("Archive channel"));
    cx.simulate_input("chat-details-name-input", "Lobby");
    cx.simulate_click("chat-details-rename-button");
    cx.run_until_parked();
    assert!(
        cx.host()
            .requests::<Submit<::chat::Chat>>()
            .iter()
            .any(|op| matches!(op, Op::RenameChannel { name, .. } if name == "Lobby"))
    );
}

/// The panes lay out to the window the host says the view is in, in the
/// frame that shows it: a narrower window clamps the sidebar before it
/// is drawn, and the dividers keep their drag routes.
#[test]
fn the_panes_clamp_to_the_viewport_and_the_dividers_keep_their_behavior_routes() {
    let (mut cx, view) = opened();
    view.read(|chat| {
        assert_eq!(chat.layout.viewport, view::testing::VIEWPORT);
    });
    let full = StyleRefinement::default().size_full();
    let renders = cx.renders();
    cx.simulate_resize(640., 480.);
    assert_eq!(
        cx.renders(),
        renders + 1,
        "one frame, laid out to the new size"
    );
    view.read(|chat| {
        assert_eq!(chat.layout.viewport, (640., 480.));
        assert!(chat.layout.sidebar <= 320.);
    });
    assert!(matches!(
        cx.find("chat-sidebar-resize"),
        Some(wire::Node::ResizeHandle {
            on_drag: Some(_),
            ..
        })
    ));
    let sidebar = view.read(|chat| chat.layout.sidebar);
    cx.simulate_drag("chat-sidebar-resize", 18., 0.);
    view.read(|chat| assert_eq!(chat.layout.sidebar, sidebar + 18.));

    cx.simulate_resize(1280., 800.);
    cx.simulate_click("chat-room-details");
    assert!(matches!(
        cx.find("chat-details-resize"),
        Some(wire::Node::ResizeHandle {
            on_drag: Some(_),
            ..
        })
    ));
    assert!(cx.find("chat-side-over").is_none());

    // too narrow for the room beside it: the details cover the whole
    // screen, sidebar and room alike, with their close, and nothing to drag
    cx.simulate_resize(720., 480.);
    assert!(cx.find("chat-details-resize").is_none());
    let Some(wire::Node::Container(view::wire::ContainerNode { style, .. })) =
        cx.find("chat-side-over")
    else {
        panic!("the side pane floats")
    };
    let style = &cx.styles()[*style];
    assert_eq!(style.inset, StyleRefinement::default().inset_0().inset);
    let Some(wire::Node::Container(view::wire::ContainerNode { style, .. })) =
        cx.find("chat-details-pane")
    else {
        panic!("the details pane")
    };
    let style = &cx.styles()[*style];
    assert_eq!(style.size, full.size, "the pane's own width gives way");
    cx.simulate_click("chat-details-close");
    assert!(cx.find("chat-side-over").is_none());
}

#[test]
fn timeline_retains_virtual_tail_anchoring_and_scroll_feedback() {
    let (cx, _) = opened();
    let Some(wire::Node::Container(view::wire::ContainerNode { children, .. })) =
        cx.find("chat-message-list")
    else {
        panic!("message list keeps its authored container identity")
    };
    assert!(matches!(children.as_slice(), [wire::Node::List {
        item_count: 3,
        alignment: wire::ListAlignment::Bottom,
        following_tail: true,
        scroll_handler: Some(_),
        children,
        ..
    }] if children.len() == 3));
}

#[test]
fn unread_rooms_carry_a_dot_and_the_open_room_a_divider() {
    let (mut cx, view) = opened();
    cx.update(&view, |chat, _, cx| {
        chat.open("other".into(), cx);
        chat.channels_arrived(
            vec![
                channel("general", "General", 9),
                channel("other", "Other", 0),
            ],
            cx,
        );
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.find("chat-sidebar-channel-general-unread").is_some());
    cx.update(&view, |chat, _, cx| {
        chat.reads.entering = true;
        chat.room = Some(Room {
            id: "general".into(),
            messages: Loadable::Ready(vec![row(3, 7, "old"), row(9, 8, "new")]),
            at_tail: true,
            reaches_head: true,
            ..Room::default()
        });
        chat.channels_arrived(vec![channel("general", "General", 9)], cx);
        assert_eq!(chat.reads.boundary, 3);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.has_text("New messages"));
}

/// A full room (one window of rows), each with markup, a reaction and a
/// thread, renders inside the host's frame budget and under a regression
/// guard on its bytes: the native proxy for a render's fuel.
#[test]
fn a_full_room_renders_inside_the_frame_budget() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let rows: Vec<MsgRow> = (1..=WINDOW as u64).map(marked_row).collect();
    cx.host().handle::<Ask<::chat::Chat>>(move |query| {
        Ok(match query {
            Query::Accounts { .. } => Reply::Accounts(page(Vec::new())),
            Query::Channels { .. } => {
                Reply::Channels(page(vec![channel("general", "General", WINDOW as u64)]))
            }
            Query::Roots { .. } => Reply::Roots(page(rows.clone())),
            Query::Members { .. } => Reply::Members(page(Vec::new())),
            Query::Reactors { seqs, .. } => reactors(seqs),
            query => panic!("unexpected chat query: {query:?}"),
        })
    });
    cx.open::<Chat>();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    assert!(
        cx.texts()
            .iter()
            .any(|text| text.contains(&format!("row {WINDOW}"))),
        "{:?}",
        cx.texts()
    );
    let bytes = cx.frame_bytes();
    // A regression guard, not a host limit: about 1.5x what a full room drew
    // when measured. The host's own limits are the sanitize check inside
    // `frame_bytes`. Tighten when the room slims, raise only on purpose.
    const REGRESSION_GUARD: usize = 47_000;
    assert!(
        bytes < REGRESSION_GUARD,
        "a {WINDOW}-row room drew {bytes} bytes, over its guard {REGRESSION_GUARD}"
    );
}

/// The timeline's list keeps its path when a message menu opens over the
/// room: the host keys the list's scroll by it, and a new path started the
/// room over at its latest message.
#[test]
fn a_menu_opening_leaves_the_timeline_where_it_was() {
    fn list_path(node: &wire::Node) -> Option<Vec<wire::ElementIdWire>> {
        if let wire::Node::List { path, .. } = node {
            return Some(path.clone());
        }
        node.children().iter().find_map(list_path)
    }
    let (mut cx, view) = opened();
    let closed = list_path(cx.root()).expect("the timeline is a list");
    cx.update(&view, |chat, window, cx| {
        cx.notify();
        chat.open_menu(Pane::Timeline, 2, 0, Mode::More, window, cx);
    });
    cx.run_until_parked();
    assert!(cx.find("chat-menu-copy-link").is_some(), "the menu is open");
    assert_eq!(list_path(cx.root()), Some(closed));
}

/// Every element of the room keeps its path while Create channel is open
/// and after it is cancelled: the host keys the timeline's scroll and the
/// composer's field by the ids above them, and a dialog that moved the
/// room under itself started the timeline over at its latest message.
#[test]
fn create_channel_leaves_the_room_where_it_was() {
    fn paths(
        node: &wire::Node,
        above: &mut Vec<wire::ElementIdWire>,
        out: &mut Vec<Vec<wire::ElementIdWire>>,
    ) {
        let id = node.identity().cloned();
        if let Some(id) = &id {
            above.push(id.clone());
            out.push(above.clone());
        }
        node.children()
            .iter()
            .for_each(|child| paths(child, above, out));
        if id.is_some() {
            above.pop();
        }
    }
    let room = |cx: &TestAppContext| {
        let mut out = Vec::new();
        paths(cx.root(), &mut Vec::new(), &mut out);
        let root = wire::ElementIdWire::Name("chat-root".into());
        out.retain(|path| path.contains(&root));
        out
    };
    let (mut cx, _view) = opened();
    let closed = room(&cx);
    assert!(
        closed.iter().any(|path| path.ends_with(&[wire::ElementIdWire::Name(
            "draft-general/editor".into()
        )])),
        "the composer is in the room"
    );
    cx.simulate_click("chat-sidebar-new-channel");
    assert!(cx.find("chat-create-name").is_some(), "the dialog is open");
    assert_eq!(room(&cx), closed);
    cx.simulate_click("chat-create-cancel");
    assert!(
        cx.find("chat-create-name").is_none(),
        "the dialog is closed"
    );
    assert_eq!(room(&cx), closed);
}

/// Every field is named for what it is, apart from the hint drawn in it:
/// the room's and a thread's composers, the emoji search, a new channel's
/// name.
#[test]
fn a_field_is_named_apart_from_its_hint() {
    let (mut cx, view) = opened();
    let named = |cx: &TestAppContext, key: &str| {
        let Some(wire::Node::Field {
            options,
            placeholder,
            multiline,
            ..
        }) = cx.find(key)
        else {
            panic!("no field {key}");
        };
        (options.label.clone(), placeholder.clone(), *multiline)
    };
    thread(&mut cx, &view, 1);
    for (key, name, hint) in [
        ("draft-general/editor", "New message", "Message #General"),
        ("draft-general-1/editor", "Reply", "Reply in thread"),
    ] {
        assert_eq!(named(&cx, key), (name.into(), hint.into(), true), "{key}");
    }
    message::hover(&mut cx, &view, 1);
    cx.simulate_click("chat-message-m1-react");
    let (name, hint, _) = named(&cx, &ui::menu::focus_key(Pane::Timeline, Mode::Reactions));
    assert_eq!(
        (name.as_str(), hint.as_str()),
        ("Find an emoji to react with", "Search emoji")
    );
    cx.simulate_click("chat-sidebar-new-channel");
    let (name, hint, _) = named(&cx, "chat-create-name");
    assert_eq!(
        (name.as_str(), hint.as_str()),
        ("Name the new channel", "Channel name")
    );
}

/// A thread with nothing under its root says so, and its reply field takes
/// the keys once the replies are read.
#[test]
fn an_empty_thread_says_so_and_its_field_takes_focus() {
    let (mut cx, view) = opened();
    thread(&mut cx, &view, 1);
    assert!(cx.has_text("No replies yet"));
    let field = wire::ElementIdWire::Name("draft-general-1/editor".into());
    assert!(
        cx.host()
            .requests::<view::methods::HostWidget>()
            .iter()
            .any(|command| matches!(command, wire::WidgetCommand::Focus { target } if target.last() == Some(&field)))
    );
}

/// A forge room's link spells its `:` as `%3A`; the app hands the view the
/// route decoded, and the view lands in that room.
#[test]
fn a_link_to_a_forge_room_lands_in_it() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let routes = cx.host().stream::<view::methods::HostRoute>();
    let view = cx.open::<Chat>();
    seat(&mut cx);
    let link = crate::links::channel_link("testnet#0a1b2c3d", "forge:web:3", None).unwrap();
    assert!(link.ends_with("/chat/forge%3Aweb%3A3"), "{link}");
    // what the app does with a chain link: the tail, decoded, joined
    routes.send(ducklink::Link::parse(&link).unwrap().tail.join("/"));
    cx.run_until_parked();
    view.read(|chat| assert_eq!(chat.room.as_ref().unwrap().id, "forge:web:3"));
    // forge's own line reads as its event with a way to where forge shows
    // the change, not as a code block
    cx.update(&view, |chat, _, cx| {
        let line = chat::MsgRow {
            channel_id: "forge:web:3".into(),
            seq: 1,
            message_id: "forge-line".into(),
            height: 2,
            blocks: vec![chat::Block::Code {
                lang: Some("forge".into()),
                text: "review 7".into(),
            }],
            // forge's own account (9)
            ..chat::MsgRow::by(Principal::Account(9))
        };
        let room = chat.room.as_mut().unwrap();
        room.messages = Loadable::Ready(vec![line]);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.has_text("review 7"), "{:?}", cx.texts());
    assert!(cx.find("chat-message-forge-line-block-0-code").is_none());
    // the link is a cell of the message's row, beside the message's own
    let cell = cx
        .find("chat-message-forge-line-program-open-cell")
        .expect("the link's cell");
    assert_eq!(
        cell.interactivity().and_then(|cell| cell.role),
        Some(view::Role::GridCell)
    );
    assert_eq!(
        cell.children()
            .iter()
            .map(wire::Node::key)
            .collect::<Vec<_>>(),
        [Some("chat-message-forge-line-program-open")]
    );
    cx.simulate_click("chat-message-forge-line-program-open");
    let opened = cx.host().requests::<view::methods::LinkOpen>();
    assert_eq!(
        opened.last().map(String::as_str),
        Some("duck://testnet-0a1b2c3d/forge/web/3")
    );
}

/// A dm's sidebar row is called by its peer, not the avatar's initial drawn
/// before the name; an agent's badge is its description.
#[test]
fn a_dm_row_is_named_by_its_peer_not_the_avatar() {
    let (cx, _) = opened();
    let Some(wire::Node::Container(view::wire::ContainerNode {
        interactivity: Some(interactivity),
        ..
    })) = cx.find("chat-sidebar-dm-8")
    else {
        panic!("the dm row")
    };
    assert_eq!(interactivity.aria.label.as_deref(), Some("reviewer"));
    assert_eq!(interactivity.aria.description.as_deref(), Some("Agent"));
}

/// A dm belongs to its two peers alike: its details list them, with no way
/// to add a third, remove either, rename or archive it, as the program
/// refuses each.
#[test]
fn a_dms_details_show_its_two_people_and_nothing_to_reshape() {
    let (mut cx, view) = opened();
    let seat = |number| chat::MemberRow {
        principal: Principal::Account(number),
        height: 1,
        time: 1,
    };
    let seat_both = |cx: &mut TestAppContext| {
        cx.update(&view, |chat, _, cx| {
            chat.room.as_mut().unwrap().members = Loadable::Ready(vec![seat(7), seat(8)]);
            cx.notify();
        });
        cx.run_until_parked();
    };
    cx.simulate_click("chat-room-details");
    seat_both(&mut cx);
    assert!(cx.find("chat-details-add-member").is_some());
    assert!(cx.has_text("Remove"));
    cx.simulate_click("chat-sidebar-dm-8");
    cx.run_until_parked();
    cx.simulate_click("chat-room-details");
    seat_both(&mut cx);
    assert!(cx.has_text("reviewer") && cx.has_text("eddy"));
    assert!(cx.has_text("Conversation details") && !cx.has_text("Channel details"));
    for id in [
        "chat-details-name-input",
        "chat-details-rename-button",
        "chat-details-archive",
    ] {
        assert!(cx.find(id).is_none(), "{id} offered in a dm");
    }
    assert!(cx.find("chat-details-add-member").is_none());
    assert!(cx.find("chat-details-member-input").is_none());
    assert!(!cx.has_text("Remove"));
}

/// A room of 300 members lists every one, in the order the program
/// answers: the read follows `next` past its first page of 256.
#[test]
fn a_room_lists_every_member_past_the_first_page() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let member = |number| chat::MemberRow {
        principal: Principal::Account(number),
        height: 1,
        time: 1,
    };
    cx.host().handle::<Ask<::chat::Chat>>(move |query| {
        Ok(match query {
            Query::Accounts { .. } => Reply::Accounts(page(Vec::new())),
            Query::Channels { .. } => Reply::Channels(page(vec![channel("general", "General", 0)])),
            Query::Roots { .. } => Reply::Roots(page(Vec::new())),
            Query::Members { page: asked, .. } => Reply::Members(match asked.after.as_deref() {
                None => ::chat::PageResponse {
                    next: Some(vec![1]),
                    ..page((1..=256).map(member).collect())
                },
                Some([1]) => page((257..=300).map(member).collect()),
                Some(after) => panic!("a cursor no page named: {after:?}"),
            }),
            query @ (Query::Listening { .. } | Query::Notices { .. }) => chat_reply(query),
            query => panic!("unexpected chat query: {query:?}"),
        })
    });
    cx.open::<Chat>();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    cx.simulate_click("chat-room-details");
    let listed: Vec<String> = cx
        .texts()
        .into_iter()
        .filter(|text| text.starts_with("account "))
        .collect();
    let numbers: Vec<String> = (1..=300).map(|n| format!("account {n}")).collect();
    assert_eq!(listed, numbers);
}

/// A room opened around a landing seq (a notification, a link) re-reads its
/// window on a chat write, so an edit or a reaction shows there too.
#[test]
fn a_landed_room_rereads_its_window() {
    let (mut cx, view) = opened();
    cx.host().handle::<Ask<::chat::Chat>>(|query| {
        Ok(match query {
            Query::MessagesAround {
                channel_id,
                seq,
                page,
                ..
            } => {
                assert_eq!((channel_id.as_str(), seq), ("general", 1), "its middle");
                assert_eq!(page.limit, Some(SPAN as u64), "as wide as a window grows");
                Reply::Messages(vec![row(1, 7, "hello"), row(2, 8, "edited since")])
            }
            Query::Channels { .. } => Reply::Channels(page(vec![channel("general", "General", 3)])),
            Query::Members { .. } => Reply::Members(page(Vec::new())),
            query => panic!("unexpected chat query: {query:?}"),
        })
    });
    cx.update(&view, |chat, _, cx| {
        chat.room.as_mut().unwrap().landed = true;
        cx.notify();
        chat.refresh(cx);
    });
    cx.run_until_parked();
    assert!(cx.has_text("edited since"), "{:?}", cx.texts());
}

/// Off the live tail, "Jump to latest" floats over the list's foot: it
/// takes no row of its own, so the list keeps its height, and it lets a
/// click beside the button through to the row under it.
#[test]
fn jump_to_latest_floats_over_the_list() {
    let (mut cx, view) = opened();
    cx.update(&view, |chat, _, cx| {
        chat.room = Some(Room {
            id: "general".into(),
            messages: Loadable::Ready(vec![row(1, 7, "old"), row(2, 8, "older")]),
            at_tail: false,
            ..Room::default()
        });
        cx.notify();
    });
    cx.run_until_parked();
    let Some(wire::Node::Container(view::wire::ContainerNode {
        style,
        interactivity,
        ..
    })) = cx.find("chat-jump-latest")
    else {
        panic!("jump to latest")
    };
    let style = &cx.styles()[*style];
    assert_eq!(
        style.position,
        view::StyleRefinement::default().absolute().position
    );
    assert!(interactivity.as_ref().is_none_or(|i| !i.occlude));
    assert!(cx.find("chat-jump-latest-button").is_some());
}

/// The rooms are one list box: ↓ reaches the direct message under the
/// channels, and Enter opens it here. No link goes to the host: the host
/// hands a link's route to a chat seat, so another chat window would
/// switch rooms with this one.
#[test]
fn an_arrow_and_enter_on_the_rooms_opens_the_next_room() {
    let (mut cx, view) = opened();
    let list = cx.interactivity("chat-sidebar-rooms-list");
    assert_eq!(list.role, Some(view::Role::ListBox));
    assert!(list.focusable && list.tab_stop == Some(true));
    let general = cx.interactivity("chat-sidebar-channel-general");
    assert!(!general.focusable && general.aria.active_descendant);
    assert!(
        cx.interactivity("chat-sidebar-new-channel").tab_stop == Some(true),
        "the header's button stays a stop of its own"
    );
    cx.simulate_key_down("chat-sidebar-rooms-list", "down");
    let dm = cx.interactivity("chat-sidebar-dm-8");
    assert_eq!(dm.role, Some(view::Role::ListBoxOption));
    assert!(dm.aria.active_descendant && dm.aria.selected == Some(false));
    view.read(|chat| assert_eq!(chat.room.as_ref().unwrap().id, "general"));
    cx.simulate_key_down("chat-sidebar-rooms-list", "enter");
    cx.run_until_parked();
    view.read(|chat| assert_eq!(chat.room.as_ref().unwrap().id, "dm-7-8"));
    assert_eq!(
        cx.interactivity("chat-sidebar-dm-8").aria.selected,
        Some(true)
    );
    assert_eq!(
        cx.host().requests::<view::methods::LinkOpen>(),
        Vec::<String>::new()
    );
}

/// Whether the node `key` names claims the active descendant; a row the
/// list no longer draws claims nothing.
pub(super) fn claims(cx: &TestAppContext, key: &str) -> bool {
    cx.find(key)
        .and_then(wire::Node::interactivity)
        .is_some_and(|node| node.aria.active_descendant)
}

/// The thread pane is a grid of its own: its arrows move in the thread,
/// and the room's active message stays.
#[test]
fn an_arrow_in_the_thread_moves_in_the_thread_not_the_room() {
    let (mut cx, view) = opened();
    cx.update(&view, |chat, _, cx| {
        let room = chat.room.as_mut().unwrap();
        room.messages.ready_mut().unwrap()[0].reply_count = 1;
        room.thread = Some(Thread {
            root: 1,
            replies: Loadable::Ready(vec![MsgRow {
                thread: Some(1),
                ..row(3, 8, "a reply")
            }]),
            ..Thread::default()
        });
        cx.notify();
    });
    cx.run_until_parked();
    let thread = cx.interactivity("chat-thread-list");
    assert_eq!(thread.role, Some(view::Role::Grid));
    assert!(thread.focusable && thread.tab_stop == Some(true));
    // the thread's newest is its reply; the room's is still m2
    assert!(cx.interactivity("chat-message-m3").aria.active_descendant);
    assert!(cx.interactivity("chat-message-m2").aria.active_descendant);
    cx.simulate_focus("chat-thread-list");
    cx.simulate_key_down("chat-thread-list", "up");
    view.read(|chat| {
        assert_eq!(chat.thread_cursor.id.as_deref(), Some("m1"));
        assert_eq!(chat.timeline_cursor.id, None);
    });
    // the list draws the revealed row until the host asks for more
    assert!(!claims(&cx, "chat-message-m3"));
    assert!(cx.interactivity("chat-message-m2").aria.active_descendant);
    cx.simulate_key_down("chat-thread-list", "enter");
    view.read(|chat| {
        let menu = chat
            .menu
            .as_ref()
            .expect("the root is chosen in the thread");
        assert_eq!((menu.pane, menu.seq), (Pane::Thread, 1));
    });
}

/// Older history landing above leaves a marked row its routes: the row
/// under a day marker is wrapped, and the wrapper carries the message's key,
/// so the row is filed by its message wherever it moves.
#[test]
fn older_history_leaves_a_marked_rows_routes_alone() {
    let (mut cx, view) = opened();
    cx.update(&view, |chat, _, cx| {
        let messages = chat.room.as_mut().unwrap().messages.ready_mut().unwrap();
        let mut dated = row(3, 7, "dated");
        dated.time = 1_790_264_527_000;
        messages.push(dated);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.find("chat-day-m3").is_some(), "m3 opens its day");
    let press = cx.interactivity("chat-message-m3").on_click;
    assert!(press.is_some());
    cx.update(&view, |chat, _, cx| {
        let messages = chat.room.as_mut().unwrap().messages.ready_mut().unwrap();
        messages.insert(0, row(10, 8, "older"));
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.find("chat-message-m10").is_some(), "the older row drew");
    assert!(cx.find("chat-day-m3").is_some(), "m3 still opens its day");
    assert_eq!(
        cx.interactivity("chat-message-m3").on_click,
        press,
        "the prepend renumbered m3"
    );
}

/// Every message row says its place in its pane's set, the one under a
/// day marker too: the host positions only a list item's own node, and a
/// marked message is wrapped, so the thread's root (always the first of
/// its day) said nothing (census AX-112).
#[test]
fn every_message_row_says_its_place_in_its_set() {
    let (mut cx, view) = opened();
    cx.update(&view, |chat, _, cx| {
        let room = chat.room.as_mut().unwrap();
        let rows = room.messages.ready_mut().unwrap();
        rows[0].reply_count = 1;
        for (row, time) in rows
            .iter_mut()
            .zip([1_000_000_000_000_u64, 1_000_000_000_001])
        {
            row.time = time;
        }
        room.thread = Some(Thread {
            root: 1,
            replies: Loadable::Ready(vec![MsgRow {
                thread: Some(1),
                time: 1_000_000_000_002,
                ..row(3, 8, "a reply")
            }]),
            ..Thread::default()
        });
        cx.notify();
    });
    cx.run_until_parked();
    // the root opens its day in both panes: it is wrapped with the marker
    assert!(cx.find("chat-day-m1").is_some(), "the day marker shows");
    fn rows<'a>(node: &'a wire::Node, key: &str, out: &mut Vec<&'a wire::Node>) {
        if node.key() == Some(key) {
            out.push(node);
        }
        for child in node.children() {
            rows(child, key, out);
        }
    }
    let places = |key: &str| -> Vec<(Option<usize>, Option<usize>)> {
        let mut found = Vec::new();
        rows(cx.root(), key, &mut found);
        found
            .iter()
            .map(|node| {
                let aria = &node.interactivity().expect("a row").aria;
                (aria.position_in_set, aria.size_of_set)
            })
            .collect()
    };
    // m1 heads the room (of m1, m2) and the thread (of m1, m3)
    assert_eq!(
        places("chat-message-m1-row"),
        vec![(Some(1), Some(2)), (Some(1), Some(2))]
    );
    assert_eq!(places("chat-message-m2-row"), vec![(Some(2), Some(2))]);
    assert_eq!(places("chat-message-m3-row"), vec![(Some(2), Some(2))]);
}

/// A search hit in the room already open lands the room on other rows:
/// the rows and the roster shown go, and the room reads as loading until
/// the window around the hit lands. (`open_at` blanks both slots on
/// purpose; `load` alone would keep what is shown.)
#[test]
fn a_jump_to_a_hit_in_the_open_room_shows_the_room_loading() {
    let (mut cx, view) = opened();
    assert!(cx.has_text("hello"), "{:?}", cx.texts());
    cx.simulate_input("chat-sidebar-search", "hello");
    cx.simulate_submit("chat-sidebar-search");
    cx.run_until_parked();
    // the window around the hit is asked and not answered yet
    cx.host().never::<Ask<::chat::Chat>>();
    cx.simulate_click("chat-search-hit-general-1");
    view.read(|chat| {
        let room = chat.room.as_ref().expect("the room stays open");
        assert_eq!(room.id, "general");
        assert!(room.messages.is_loading(), "{:?}", room.messages);
        assert!(room.members.is_loading(), "{:?}", room.members);
    });
    assert!(cx.has_text("Loading messages…"), "{:?}", cx.texts());
    assert!(
        cx.find("chat-message-m2-block-0").is_none(),
        "the rows of the window before are gone"
    );
}

/// The link to the node comes back: the rooms are read afresh, and the
/// sidebar says so instead of showing the list from before the drop.
#[test]
fn a_reconnect_shows_the_rooms_loading() {
    let (mut cx, view) = opened();
    let props = cx.host().stream::<HostSession>();
    let session = view.read(|chat| chat.session.clone());
    props.send(Session {
        connected: false,
        ..session.clone()
    });
    cx.run_until_parked();
    cx.host().never::<Ask<::chat::Chat>>();
    props.send(session);
    cx.run_until_parked();
    view.read(|chat| assert!(chat.channels.is_loading(), "{:?}", chat.channels));
    assert!(cx.has_text("Loading rooms…"), "{:?}", cx.texts());
    assert!(
        cx.find("chat-sidebar-channel-general").is_none(),
        "the list from before the drop is not shown as the node's"
    );
}

/// The window is named for the open room as its header names it: `#name`
/// for a channel, the peer for a direct room; nothing while the name is
/// still out, never the room's id. Each name goes to the host once.
#[test]
fn the_window_is_named_for_the_open_room() {
    use view::methods::HostTitle;
    let (mut cx, view) = opened();
    // no room at boot clears it; the room's name follows, once however
    // often the room is drawn
    assert_eq!(cx.host().requests::<HostTitle>(), ["", "#General"]);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<HostTitle>(), ["", "#General"]);
    // a direct room before the names land: no name yet
    cx.update(&view, |chat, _, cx| {
        cx.notify();
        chat.names = Loadable::Idle;
        chat.choose("dm-7-8".into(), cx);
    });
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<HostTitle>(), ["", "#General", ""]);
    cx.update(&view, |chat, _, cx| {
        cx.notify();
        chat.load_names(cx);
    });
    cx.run_until_parked();
    assert_eq!(
        cx.host().requests::<HostTitle>(),
        ["", "#General", "", "reviewer"]
    );
}
