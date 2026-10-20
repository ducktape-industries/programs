//! Right-press menus: a message's and a sidebar room's, from the pointer,
//! the keys and assistive tech; and the cursors over a message.
use super::*;
use view::CursorStyle;
use view::methods::ClipboardWrite;

pub(super) fn right_press(cx: &mut TestAppContext, key: &str) {
    let event = view::MouseDownEvent {
        button: view::MouseButton::Right,
        position: view::Point {
            x: view::px(240.),
            y: view::px(180.),
        },
        modifiers: Default::default(),
        click_count: 1,
        first_mouse: false,
    };
    cx.simulate_mouse_down(key, event);
    cx.run_until_parked();
}

fn menu_of(view: &Entity<Chat>) -> Option<(u64, Mode, (f32, f32), bool)> {
    view.read(|chat| {
        let menu = chat.menu.as_ref()?;
        Some((menu.seq, menu.mode, menu.at, chat.menu_at_pointer))
    })
}

fn focused(cx: &TestAppContext) -> Option<&str> {
    cx.focused().and_then(wire::Node::key)
}

#[test]
fn a_right_press_on_a_message_opens_its_menu_at_the_pointer() {
    let (mut cx, view) = opened();
    right_press(&mut cx, "chat-message-m1-row");
    assert_eq!(menu_of(&view), Some((1, Mode::More, (240., 180.), true)));
    let menu = cx.interactivity("chat-room-message-action-focus");
    assert_eq!(menu.role, Some(view::Role::Menu));
    assert_eq!(focused(&cx), Some("chat-room-message-action-focus"));
    for id in [
        "chat-menu-reply",
        "chat-menu-copy-text",
        "chat-menu-copy-link",
    ] {
        assert_eq!(
            cx.interactivity(id).role,
            Some(view::Role::MenuItem),
            "{id}"
        );
        let cursor = cx.style(id).mouse_cursor;
        assert_eq!(cursor, Some(CursorStyle::PointingHand), "{id}");
    }
    cx.simulate_key_down("chat-room-message-action-focus", "escape");
    assert_eq!(menu_of(&view), None);
}

#[test]
fn copy_text_puts_the_messages_words_on_the_clipboard() {
    let (mut cx, view) = opened();
    right_press(&mut cx, "chat-message-m1-row");
    cx.simulate_click("chat-menu-copy-text");
    cx.run_until_parked();
    let body = view.read(|chat| {
        let messages = chat.messages(Pane::Timeline);
        let message = messages.iter().find(|message| message.seq == 1);
        message.map(|message| message.body.clone())
    });
    assert!(body.as_ref().is_some_and(|body| !body.is_empty()));
    let copied = cx.host().requests::<ClipboardWrite>();
    assert_eq!(copied.last(), body.as_ref());
    assert_eq!(menu_of(&view), None);
}

#[test]
fn the_menu_key_or_shift_f10_opens_the_active_messages_menu() {
    let (mut cx, view) = opened();
    let newest = view.read(|chat| {
        let messages = chat.messages(Pane::Timeline);
        messages.last().map(|message| message.seq)
    });
    let spot = view.read(|chat| chat.key_spot(Pane::Timeline));
    for key in ["shift-f10", "menu"] {
        cx.simulate_focus("chat-message-list");
        cx.simulate_key_down("chat-message-list", key);
        let opened = newest.map(|seq| (seq, Mode::More, spot, false));
        assert_eq!(menu_of(&view), opened, "{key}");
        cx.simulate_dismiss("chat-menu-overlay");
        assert_eq!(menu_of(&view), None);
    }
    cx.simulate_focus("chat-message-list");
    cx.simulate_key_down("chat-message-list", "f10");
    assert_eq!(menu_of(&view), None, "F10 alone is no menu key");
}

#[test]
fn assistive_tech_opens_a_messages_menu_where_the_keys_do() {
    let (mut cx, view) = opened();
    cx.simulate_a11y_action(
        "chat-message-m2-row",
        view::accesskit::Action::ShowContextMenu,
        None,
    );
    cx.run_until_parked();
    let spot = view.read(|chat| chat.key_spot(Pane::Timeline));
    assert_eq!(menu_of(&view), Some((2, Mode::More, spot, false)));
}

#[test]
fn a_right_press_on_a_room_offers_it_its_link_and_its_details() {
    let (mut cx, view) = opened();
    right_press(&mut cx, "chat-sidebar-channel-general");
    assert_eq!(
        cx.interactivity("chat-room-menu").role,
        Some(view::Role::Menu)
    );
    assert_eq!(focused(&cx), Some("chat-room-menu"));
    for slug in ["open", "copy-link", "details"] {
        let id = format!("chat-room-menu/{slug}");
        assert_eq!(
            cx.interactivity(&id).role,
            Some(view::Role::MenuItem),
            "{id}"
        );
    }
    cx.simulate_click("chat-room-menu/copy-link");
    cx.run_until_parked();
    let chain = view.read(|chat| chat.session.chain_id.clone());
    let link = crate::links::channel_link(&chain, "general", None);
    assert!(link.is_some());
    assert_eq!(cx.host().requests::<ClipboardWrite>().last(), link.as_ref());
    view.read(|chat| assert!(chat.room_menu.is_none()));
    assert_eq!(focused(&cx), Some("chat-sidebar-rooms-list"));

    right_press(&mut cx, "chat-sidebar-channel-general");
    cx.simulate_click("chat-room-menu/details");
    cx.run_until_parked();
    view.read(|chat| {
        assert_eq!(
            chat.room.as_ref().map(|room| room.id.as_str()),
            Some("general")
        );
        assert!(chat.details.is_some(), "the details open beside the room");
    });

    right_press(&mut cx, "chat-sidebar-channel-general");
    cx.simulate_click("chat-room-menu/open");
    cx.run_until_parked();
    view.read(|chat| {
        assert_eq!(
            chat.room.as_ref().map(|room| room.id.as_str()),
            Some("general")
        );
        assert!(chat.room_menu.is_none());
    });
}

#[test]
fn the_keys_open_the_active_rooms_menu_under_its_row() {
    let (mut cx, view) = opened();
    cx.simulate_focus("chat-sidebar-rooms-list");
    cx.simulate_key_down("chat-sidebar-rooms-list", "shift-f10");
    let waiting = |view: &Entity<Chat>| {
        view.read(|chat| {
            chat.room_menu
                .as_ref()
                .map(|menu| (menu.room.clone(), menu.at))
        })
    };
    assert_eq!(waiting(&view), Some(("general".to_owned(), None)));
    assert!(
        cx.find("chat-room-menu").is_none(),
        "not before the row says where"
    );
    cx.simulate_bounds("chat-room-menu-anchor", (8., 120.), (220., 32.));
    let under = view::Point {
        x: view::px(8.),
        y: view::px(152.),
    };
    assert_eq!(waiting(&view), Some(("general".to_owned(), Some(under))));
    assert_eq!(focused(&cx), Some("chat-room-menu"));
    cx.simulate_dismiss("chat-room-menu-overlay");
    assert_eq!(waiting(&view), None);
    assert_eq!(focused(&cx), Some("chat-sidebar-rooms-list"));

    cx.simulate_a11y_action(
        "chat-sidebar-channel-general",
        view::accesskit::Action::ShowContextMenu,
        None,
    );
    cx.run_until_parked();
    assert_eq!(waiting(&view), Some(("general".to_owned(), None)));
}

#[test]
fn a_messages_words_take_the_text_cursor_and_its_controls_the_pointer() {
    let (mut cx, view) = opened();
    message::hover(&mut cx, &view, 1);
    let cursor = |cx: &TestAppContext, id: &str| cx.style(id).mouse_cursor;
    assert_eq!(
        cursor(&cx, "chat-message-m1-block-0"),
        Some(CursorStyle::IBeam)
    );
    assert_eq!(
        cursor(&cx, "chat-message-m1-card"),
        None,
        "the ground keeps the arrow"
    );
    assert_eq!(
        cursor(&cx, "chat-message-m1-more"),
        Some(CursorStyle::PointingHand)
    );
    for id in [
        "chat-sidebar-channel-general",
        "chat-sidebar-new-channel",
        "chat-sidebar-new-dm",
    ] {
        assert_eq!(cursor(&cx, id), Some(CursorStyle::PointingHand), "{id}");
    }
}
