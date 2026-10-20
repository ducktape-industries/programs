//! Message menus, the reaction picker and the edit field.
use super::*;

#[test]
fn menus_and_dialogs_are_modal_overlays_with_dismiss_routes() {
    let (mut cx, view) = opened();
    message::hover(&mut cx, &view, 1);
    cx.simulate_click("chat-message-m1-more");
    assert!(matches!(
        cx.find("chat-menu-overlay"),
        Some(wire::Node::Overlay {
            label: Some(label),
            on_dismiss: Some(_),
            ..
        }) if label == "Message menu"
    ));

    cx.simulate_dismiss("chat-menu-overlay");
    view.read(|chat| assert!(chat.menu.is_none()));
    cx.simulate_click("chat-sidebar-new-channel");
    assert!(matches!(
        cx.find("chat-create-overlay"),
        Some(wire::Node::Overlay {
            label: Some(label),
            on_dismiss: Some(_),
            ..
        }) if label == "Create channel"
    ));
    cx.simulate_dismiss("chat-create-overlay");
    view.read(|chat| assert!(chat.create.is_none()));
}

#[test]
fn a_chosen_message_lets_go_on_escape_a_press_elsewhere_or_an_arrow() {
    let (mut cx, view) = opened();
    let chosen = |view: &Entity<Chat>| {
        view.read(|chat| chat.menu.as_ref().map(|menu| (menu.seq, menu.mode)))
    };
    cx.simulate_click("chat-message-m1");
    assert_eq!(chosen(&view), Some((1, Mode::Toolbar)));
    assert!(cx.find("chat-message-m1-actions").is_some());
    assert!(
        !matches!(
            cx.find("chat-menu-overlay"),
            Some(wire::Node::Overlay { .. })
        ),
        "a chosen message opens no dialog"
    );
    let root = cx.interactivity("chat-root");
    assert!(root.on_key_down.is_some() && root.consumes_keys.contains(&"escape".into()));
    cx.simulate_focus("chat-sidebar-search");
    cx.simulate_key_down("chat-sidebar-search", "escape");
    assert_eq!(chosen(&view), None);
    assert!(cx.find("chat-message-m1-actions").is_none());
    assert!(
        cx.interactivity("chat-root").consumes_keys.is_empty(),
        "with nothing to let go, Escape is the host's"
    );
    cx.simulate_click("chat-message-m1");
    assert_eq!(chosen(&view), Some((1, Mode::Toolbar)));
    let down = |shift| view::MouseDownEvent {
        button: view::MouseButton::Left,
        position: Default::default(),
        modifiers: view::Modifiers {
            shift,
            ..Default::default()
        },
        click_count: 1,
        first_mouse: false,
    };
    cx.simulate_mouse_down("chat-room-header", down(false));
    assert_eq!(chosen(&view), None);
    cx.simulate_click("chat-message-m1");
    cx.simulate_mouse_down("chat-message-m2", down(false));
    cx.simulate_click("chat-message-m2");
    assert_eq!(chosen(&view), Some((2, Mode::Toolbar)));
    cx.simulate_focus("chat-message-list");
    cx.simulate_key_down("chat-message-list", "up");
    assert_eq!(chosen(&view), None);
    cx.simulate_click("chat-message-m1");
    cx.simulate_mouse_down("chat-message-m2", down(true));
    assert_eq!(
        chosen(&view),
        Some((1, Mode::Toolbar)),
        "a shift press keeps it for the range"
    );
    let handler = cx.interactivity("chat-message-m2").on_click.unwrap();
    let button = |shift| wire::click::ButtonEvent {
        button: wire::click::MouseButton::Left,
        position: Default::default(),
        modifiers: view::Modifiers {
            shift,
            ..Default::default()
        },
        click_count: 1,
    };
    cx.simulate_event(wire::Event::Click {
        handler,
        event: wire::click::Click::Mouse {
            down: button(true),
            up: button(true),
            first_mouse: false,
        },
    });
    cx.run_until_parked();
    assert_eq!(chosen(&view), None);
    view.read(|chat| {
        let range = chat.copy.expect("the copy range");
        assert_eq!((range.anchor, range.head), (1, 2));
    });
    assert!(cx.has_text("2 messages selected"));
    cx.simulate_key_down("chat-message-list", "escape");
    view.read(|chat| assert!(chat.copy.is_none()));
    assert!(!cx.has_text("2 messages selected"));
}

#[test]
fn the_copy_range_copies_from_its_bar() {
    let (mut cx, view) = opened();
    let copied = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
    let seen = copied.clone();
    cx.host()
        .handle::<view::methods::ClipboardWrite>(move |text| {
            *seen.borrow_mut() = text;
            Ok(())
        });
    cx.simulate_click("chat-message-m1");
    let shift = view::Modifiers {
        shift: true,
        ..Default::default()
    };
    let handler = cx.interactivity("chat-message-m2").on_click.unwrap();
    let button = wire::click::ButtonEvent {
        button: wire::click::MouseButton::Left,
        position: Default::default(),
        modifiers: shift,
        click_count: 1,
    };
    cx.simulate_event(wire::Event::Click {
        handler,
        event: wire::click::Click::Mouse {
            down: button.clone(),
            up: button,
            first_mouse: false,
        },
    });
    cx.run_until_parked();
    assert!(cx.has_text("2 messages selected"));
    cx.simulate_mouse_down(
        "chat-selection-copy",
        view::MouseDownEvent {
            button: view::MouseButton::Left,
            position: Default::default(),
            modifiers: Default::default(),
            click_count: 1,
            first_mouse: false,
        },
    );
    view.read(|chat| assert!(chat.copy.is_some(), "the press on Copy keeps the range"));
    cx.simulate_click("chat-selection-copy");
    cx.run_until_parked();
    assert_eq!(copied.borrow().lines().count(), 2, "{:?}", copied.borrow());
}

#[test]
fn message_menu_offers_only_what_the_reader_may_do_and_executes_it() {
    let (mut cx, view) = opened();
    let menu_on = |seq| Menu {
        pane: Pane::Timeline,
        seq,
        rev: 0,
        mode: Mode::More,
        at: (611., 455.),
    };
    cx.update(&view, |chat, _, cx| {
        chat.menu = Some(menu_on(1));
        chat.session.account = None;
        cx.notify();
    });
    cx.run_until_parked();
    for id in [
        "chat-menu-add-reaction",
        "chat-menu-edit",
        "chat-menu-delete",
    ] {
        assert!(cx.find(id).is_none(), "{id} offered to a reader");
    }
    assert!(cx.find("chat-menu-copy-link").is_some());

    cx.update(&view, |chat, _, cx| {
        chat.menu = Some(menu_on(2));
        chat.session.account = Some(7);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.find("chat-menu-edit").is_none(), "only the author edits");
    assert!(cx.find("chat-menu-delete").is_some(), "the owner deletes");

    cx.update(&view, |chat, _, cx| {
        chat.menu = Some(menu_on(1));
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.has_text("😀") && cx.has_text("✎") && cx.has_text("🗑"));
    cx.update(&view, |chat, _, cx| {
        chat.session.account = Some(7);
        cx.notify();
    });
    cx.run_until_parked();
    cx.simulate_click("chat-menu-delete");
    assert!(cx.has_text("Delete this message?"));
    let Some(wire::Node::Container(view::wire::ContainerNode {
        interactivity: Some(interactivity),
        ..
    })) = cx.find("chat-room-message-delete-frame")
    else {
        panic!("delete confirmation frame")
    };
    assert!(
        interactivity.occlude,
        "delete confirmation popup must occlude so its clicks don't also fire the row beneath"
    );
    cx.simulate_click("chat-menu-confirm-delete");
    cx.run_until_parked();
    assert!(
        cx.host()
            .requests::<Submit<::chat::Chat>>()
            .iter()
            .any(|op| {
                matches!(op, Op::DeleteMessage { channel_id, seq: 1 } if channel_id == "general")
            })
    );
}

#[test]
fn delete_confirmation_opens_with_the_keys_on_its_first_button() {
    let (mut cx, view) = opened();
    cx.update(&view, |chat, _, cx| {
        chat.menu = Some(Menu {
            pane: Pane::Timeline,
            seq: 1,
            rev: 0,
            mode: Mode::More,
            at: (611., 455.),
        });
        chat.session.account = Some(7);
        cx.notify();
    });
    cx.run_until_parked();
    cx.simulate_click("chat-menu-delete");
    assert!(
        cx.host()
            .requests::<view::methods::HostWidget>()
            .iter()
            .any(|command| matches!(
                command,
                wire::WidgetCommand::Focus { target }
                    if target.last() == Some(&wire::ElementIdWire::Name("chat-menu-cancel-delete".into()))
            )),
        "Cancel takes the keys on open"
    );
    let cancel = cx.interactivity("chat-menu-cancel-delete");
    assert!(
        cancel.focusable && cancel.tab_stop != Some(false),
        "Cancel is a stop"
    );
    let dialog = cx.interactivity("chat-room-message-delete-frame");
    assert_eq!(dialog.role, Some(view::Role::AlertDialog));
    assert_eq!(dialog.aria.label.as_deref(), Some("Delete this message?"));
    assert!(!dialog.focusable, "the frame offers no focus");
}

#[test]
fn reaction_picker_keeps_labels_and_its_stable_action_id() {
    let (mut cx, view) = opened();
    cx.update(&view, |chat, _, cx| {
        chat.menu = Some(Menu {
            pane: Pane::Timeline,
            seq: 1,
            rev: 0,
            mode: Mode::Reactions,
            at: (333., 222.),
        });
        cx.notify();
    });
    cx.run_until_parked();
    let Some(wire::Node::Container(view::wire::ContainerNode {
        interactivity: Some(interactivity),
        ..
    })) = cx.find("chat-reaction-🔥")
    else {
        panic!("reaction is a native cell");
    };
    assert_eq!(interactivity.aria.label.as_deref(), Some("React with 🔥"));
    assert_eq!(interactivity.aria.description, None);
    assert_eq!(
        cx.interactivity("chat-reaction-Smileys-😀")
            .aria
            .label
            .as_deref(),
        Some("React with 😀")
    );
    cx.simulate_click("chat-reaction-🔥");
    cx.run_until_parked();
    assert!(
        cx.host()
            .requests::<Submit<::chat::Chat>>()
            .iter()
            .any(|op| { matches!(op, Op::AddReaction { emoji, .. } if emoji == "🔥") })
    );
}

#[test]
fn an_archived_room_offers_the_author_delete_and_neither_a_reaction_nor_an_edit() {
    let (mut cx, view) = opened();
    cx.update(&view, |chat, _, cx| {
        let rooms = chat.channels.ready_mut().expect("the rooms are read");
        let general = rooms
            .iter_mut()
            .find(|info| info.channel.id == "general")
            .expect("general is listed");
        general.channel.archived = true;
        chat.menu = Some(Menu {
            pane: Pane::Timeline,
            seq: 1,
            rev: 0,
            mode: Mode::More,
            at: (611., 455.),
        });
        cx.notify();
    });
    cx.run_until_parked();
    assert!(
        cx.find("chat-menu-delete").is_some(),
        "chat lets the author delete in an archived room"
    );
    for id in ["chat-menu-add-reaction", "chat-menu-edit"] {
        assert!(cx.find(id).is_none(), "{id} offered where chat refuses it");
    }
}

#[test]
fn an_archived_room_draws_the_strip_and_the_chips_with_no_live_reaction() {
    let (mut cx, view) = opened();
    cx.update(&view, |chat, _, cx| {
        let rooms = chat.channels.ready_mut().expect("the rooms are read");
        let general = rooms
            .iter_mut()
            .find(|info| info.channel.id == "general")
            .expect("general is listed");
        general.channel.archived = true;
        let room = chat.room.as_mut().expect("a room is open");
        let rows = room.messages.ready_mut().expect("its rows are read");
        let first = rows.iter_mut().find(|row| row.seq == 1).expect("row 1");
        first.reactions = vec![::chat::Reaction {
            emoji: "👍".into(),
            count: 1,
            reacted_by_me: false,
        }];
        cx.notify();
    });
    cx.run_until_parked();
    message::hover(&mut cx, &view, 1);
    for key in ["thumbs-up", "react", "reaction-👍", "reaction-add"] {
        let id = format!("chat-message-m1-{key}");
        assert_eq!(cx.interactivity(&id).aria.disabled, Some(true), "{id}");
    }
}

#[test]
fn an_emoji_search_shows_every_match() {
    let (mut cx, view) = opened();
    let found = crate::emoji::search("c");
    assert!(
        found.len() > crate::emoji::PER_TAB,
        "a query that overflows a tab"
    );
    cx.update(&view, |chat, _, cx| {
        chat.menu = Some(Menu {
            pane: Pane::Timeline,
            seq: 1,
            rev: 0,
            mode: Mode::Reactions,
            at: (333., 222.),
        });
        chat.picker.query.reset("c");
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.has_text(&format!("{} MATCHES", found.len())));
    assert!(cx.find("chat-reaction-results-scroll").is_some());
    for emoji in found {
        assert!(
            cx.find(&format!("chat-reaction-{emoji}")).is_some(),
            "{emoji}"
        );
    }
}

#[test]
fn the_edit_field_saves() {
    let (mut cx, view) = opened();
    assert!(cx.has_text("Send") && !cx.has_text("Save"));
    edit(&mut cx, &view, 1);
    assert!(cx.find("chat-message-editing").is_some());
    assert!(cx.has_text("Save"));
    let key = crate::composer::Target::Edit {
        channel: view.read(|chat| chat.room_id()),
        seq: 1,
        base_rev: 0,
    }
    .key();
    let editor = format!("{key}/editor");
    assert!(
        cx.host()
            .requests::<view::methods::HostWidget>()
            .iter()
            .any(|command| matches!(
                command,
                wire::WidgetCommand::Focus { target }
                    if target.last() == Some(&wire::ElementIdWire::Name(editor.clone().into()))
            )),
        "the edit field takes the keys on open"
    );
    cx.simulate_click(&format!("{key}/cancel"));
    assert!(cx.find("chat-message-editing").is_none());
}

#[test]
fn the_message_menu_walks_its_items_and_enter_runs_one() {
    let (mut cx, view) = opened();
    message::hover(&mut cx, &view, 1);
    cx.simulate_click("chat-message-m1-more");
    let menu = cx.interactivity("chat-room-message-action-focus");
    assert_eq!(menu.role, Some(view::Role::Menu));
    assert!(menu.focusable && menu.tab_stop == Some(true));
    assert!(
        cx.host()
            .requests::<view::methods::HostWidget>()
            .iter()
            .any(|command| matches!(
                command,
                wire::WidgetCommand::Focus { target }
                    if target.last() == Some(&wire::ElementIdWire::Name("chat-room-message-action-focus".into()))
            )),
        "the menu takes the keys on open"
    );
    let reply = cx.interactivity("chat-menu-reply");
    assert_eq!(reply.role, Some(view::Role::MenuItem));
    assert!(!reply.focusable && reply.aria.active_descendant);
    cx.simulate_key_down("chat-room-message-action-focus", "down");
    assert!(
        cx.interactivity("chat-menu-add-reaction")
            .aria
            .active_descendant
    );
    assert!(!cx.interactivity("chat-menu-reply").aria.active_descendant);
    cx.simulate_key_down("chat-room-message-action-focus", "enter");
    view.read(|chat| {
        assert_eq!(
            chat.menu.as_ref().map(|menu| menu.mode),
            Some(Mode::Reactions)
        );
    });
    assert!(
        cx.host()
            .requests::<view::methods::HostWidget>()
            .iter()
            .any(|command| matches!(
                command,
                wire::WidgetCommand::Focus { target }
                    if target.last() == Some(&wire::ElementIdWire::Name("chat-room-message-reaction-focus".into()))
            )),
        "the search field takes the keys on open"
    );
    let frame = cx.interactivity("chat-room-message-reaction-frame");
    assert!(!frame.focusable, "the frame offers no focus");
    assert_eq!(frame.role, None, "the frame is no dialog of its own");
    assert!(matches!(
        cx.find("chat-menu-overlay"),
        Some(wire::Node::Overlay { label: Some(label), .. }) if label == "Add reaction"
    ));
}
