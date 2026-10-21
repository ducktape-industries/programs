//! The reader's own order of the sidebar, chat's (`RoomOrder`): at each level what it leaves out
//! first, in chat's order, then what it names, in its order; Alt+↑/↓ and the room menu's Move up
//! and Move down write it whole. The rooms the reader wrote in (`PostedIn`) are theirs.
use super::context_menus::right_press;
use super::room::claims;
use super::*;
use crate::room_list::Spot;
use chat::Entry;

// Eddy's rooms, all his: General, Random, three jobs under jobs/, Design.
fn rooms() -> Vec<ChannelInfo> {
    vec![
        channel("general", "General", 1),
        channel("random", "Random", 1),
        channel("j-1", "jobs/j-1", 1),
        channel("j-2", "jobs/j-2", 1),
        channel("j-3", "jobs/j-3", 1),
        channel("design", "Design", 1),
    ]
}

// The view over `rooms`, chat holding `order` as eddy's until he writes another.
fn desk(order: Vec<Entry>) -> (TestAppContext, Entity<Chat>) {
    opened(order, false)
}

// The same on a phone, its rooms page in front.
fn phone(order: Vec<Entry>) -> (TestAppContext, Entity<Chat>) {
    opened(order, true)
}

fn opened(order: Vec<Entry>, touch: bool) -> (TestAppContext, Entity<Chat>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let held = std::rc::Rc::new(std::cell::RefCell::new(order));
    let kept = held.clone();
    cx.host()
        .handle::<view::methods::Submit<::chat::Chat>>(move |op| {
            if let chat::Op::SetRoomOrder { order } = &op {
                *kept.borrow_mut() = order.clone();
            }
            Ok(view::testing::applied(Vec::new()))
        });
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Channels { .. } => Reply::Channels(page(rooms())),
                Query::RoomOrder { .. } => Reply::RoomOrder(held.borrow().clone()),
                query => chat_reply(query),
            })
        }));
    let view = cx.open::<Chat>();
    if touch {
        cx.simulate_touch(true);
        cx.simulate_resize(390., 844.);
        cx.run_until_parked();
    }
    seat(&mut cx);
    cx.run_until_parked();
    (cx, view)
}

// The sidebar's channel rows as drawn: a room by its id, a folder by its path and a `/`.
fn drawn(view: &Entity<Chat>) -> Vec<String> {
    view.read(|chat| {
        let list = chat.room_list();
        let lines = chat.lines(&list.channels);
        lines
            .iter()
            .map(|line| match line.spot() {
                Spot::Room(room) => room,
                Spot::Folder { path, .. } => format!("{path}/"),
                Spot::More => "more".to_owned(),
                Spot::MoreFolder { path, .. } => format!("more/{path}/"),
            })
            .collect()
    })
}

fn orders(cx: &TestAppContext) -> Vec<Vec<Entry>> {
    let sent = cx.host().requests::<view::methods::Submit<::chat::Chat>>();
    sent.into_iter()
        .filter_map(|op| match op {
            chat::Op::SetRoomOrder { order } => Some(order),
            _ => None,
        })
        .collect()
}

fn room(id: &str) -> Entry {
    Entry::Room(id.into())
}

#[test]
fn with_no_order_each_level_stands_as_chat_lists_it() {
    let (_cx, view) = desk(Vec::new());
    assert_eq!(
        drawn(&view),
        ["general", "random", "jobs/", "j-1", "j-2", "j-3", "design"]
    );
}

#[test]
fn each_level_lists_what_the_order_leaves_out_first_then_what_it_names() {
    let order = vec![
        room("design"),
        Entry::Folder("jobs".into()),
        room("j-3"),
        room("j-1"),
    ];
    let (_cx, view) = desk(order);
    assert_eq!(
        drawn(&view),
        ["general", "random", "design", "jobs/", "j-2", "j-3", "j-1"],
        "a room the order never named, a new one, sits on top of its folder"
    );
}

#[test]
fn alt_down_moves_the_active_row_down_its_level_and_writes_the_order_whole() {
    let (mut cx, view) = desk(Vec::new());
    cx.simulate_focus("chat-sidebar-rooms-list");
    cx.simulate_key_down("chat-sidebar-rooms-list", "down");
    cx.simulate_key_down("chat-sidebar-rooms-list", "alt-down");
    cx.run_until_parked();
    assert_eq!(
        orders(&cx),
        [vec![
            room("general"),
            Entry::Folder("jobs".into()),
            room("random"),
            room("design"),
        ]]
    );
    assert_eq!(
        drawn(&view),
        ["general", "jobs/", "j-1", "j-2", "j-3", "random", "design"]
    );
    assert!(
        claims(&cx, "chat-sidebar-channel-random"),
        "the keys stay on the row they moved"
    );
    cx.simulate_key_down("chat-sidebar-rooms-list", "alt-up");
    cx.simulate_key_down("chat-sidebar-rooms-list", "alt-up");
    cx.simulate_key_down("chat-sidebar-rooms-list", "alt-up");
    cx.run_until_parked();
    assert_eq!(orders(&cx).len(), 3, "the top of its level goes no further");
    assert_eq!(drawn(&view)[0], "random");
}

#[test]
fn a_refused_order_puts_the_rows_back_and_says_why() {
    let (mut cx, view) = desk(Vec::new());
    cx.host()
        .handle::<view::methods::Submit<::chat::Chat>>(|_| {
            Ok(view::methods::Submitted::Refused {
                frame: [0; 32],
                until: 9,
                refusal: view::host::Error::new("unauthorized", "no account holds this key"),
            })
        });
    cx.update(&view, |chat, _, cx| chat.step(&room("general"), false, cx));
    cx.run_until_parked();
    assert_eq!(orders(&cx).len(), 1, "the move was sent");
    assert_eq!(
        drawn(&view),
        ["general", "random", "jobs/", "j-1", "j-2", "j-3", "design"],
        "put back as it was"
    );
    view.read(|chat| assert!(!chat.notice.is_empty(), "the banner says why"));
}

#[test]
fn two_refused_moves_in_a_row_leave_the_order_chat_holds() {
    let (mut cx, view) = desk(Vec::new());
    cx.host()
        .handle::<view::methods::Submit<::chat::Chat>>(|_| {
            Ok(view::methods::Submitted::Refused {
                frame: [0; 32],
                until: 9,
                refusal: view::host::Error::new("unauthorized", "no account holds this key"),
            })
        });
    cx.update(&view, |chat, _, cx| {
        chat.step(&room("general"), false, cx);
        chat.step(&room("general"), false, cx);
    });
    cx.run_until_parked();
    assert_eq!(orders(&cx).len(), 2, "both moves were sent");
    assert_eq!(
        drawn(&view),
        ["general", "random", "jobs/", "j-1", "j-2", "j-3", "design"],
        "chat took neither"
    );
}

#[test]
fn a_room_moves_only_inside_its_folder() {
    let (mut cx, view) = desk(Vec::new());
    cx.update(&view, |chat, _, cx| {
        chat.step(&room("j-1"), true, cx);
        chat.step(&room("j-3"), false, cx);
    });
    cx.run_until_parked();
    assert!(orders(&cx).is_empty(), "the ends of a folder hold");
    cx.update(&view, |chat, _, cx| chat.step(&room("j-3"), true, cx));
    cx.run_until_parked();
    assert_eq!(
        drawn(&view),
        ["general", "random", "jobs/", "j-1", "j-3", "j-2", "design"]
    );
    assert_eq!(orders(&cx), [vec![room("j-1"), room("j-3"), room("j-2")]]);
}

#[test]
fn the_room_menu_offers_a_move_where_its_level_has_room() {
    let (mut cx, view) = desk(Vec::new());
    let items = |view: &Entity<Chat>, room: &str| view.read(|chat| chat.room_items(room));
    let first = items(&view, "general");
    assert!(!first.contains(&crate::room_menu::RoomItem::MoveUp));
    assert!(first.contains(&crate::room_menu::RoomItem::MoveDown));
    let last = items(&view, "design");
    assert!(last.contains(&crate::room_menu::RoomItem::MoveUp));
    assert!(!last.contains(&crate::room_menu::RoomItem::MoveDown));
    right_press(&mut cx, "chat-sidebar-channel-general");
    assert_eq!(
        cx.interactivity("chat-room-menu/move-down").role,
        Some(view::Role::MenuItem)
    );
    cx.simulate_click("chat-room-menu/move-down");
    cx.run_until_parked();
    assert_eq!(
        drawn(&view),
        ["random", "general", "jobs/", "j-1", "j-2", "j-3", "design"]
    );
}

#[test]
fn a_room_the_reader_wrote_in_is_theirs() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
        Ok(match query {
            Query::Channels { .. } => {
                let mut theirs = channel("ops", "Ops", 1);
                theirs.channel.owner = Principal::Account(9);
                Reply::Channels(page(vec![channel("general", "General", 1), theirs]))
            }
            Query::PostedIn { .. } => Reply::PostedIn(page(vec!["ops".into()])),
            query => chat_reply(query),
        })
    }));
    let _view = cx.open::<Chat>();
    seat(&mut cx);
    cx.run_until_parked();
    assert!(cx.find("chat-sidebar-channel-ops").is_some(), "written in");
}

// The rows of `desk` laid out 30 px tall from y 100, in the order drawn: the middle of row `row`.
fn laid_out(cx: &mut TestAppContext) -> impl Fn(usize) -> view::Point<view::Pixels> + use<> {
    let rows = [
        "general",
        "random",
        "folder-jobs",
        "j-1",
        "j-2",
        "j-3",
        "design",
    ];
    for (at, row) in rows.iter().enumerate() {
        let top = 100. + 30. * at as f32;
        cx.simulate_bounds(
            &format!("chat-sidebar-bounds-{row}"),
            (0., top),
            (200., 30.),
        );
    }
    |row| view::Point {
        x: view::px(100.),
        y: view::px(115. + 30. * row as f32),
    }
}

fn press(position: view::Point<view::Pixels>) -> view::MouseDownEvent {
    view::MouseDownEvent {
        button: view::MouseButton::Left,
        position,
        modifiers: Default::default(),
        click_count: 1,
        first_mouse: false,
    }
}

fn moved(position: view::Point<view::Pixels>) -> view::MouseMoveEvent {
    view::MouseMoveEvent {
        position,
        pressed_button: Some(view::MouseButton::Left),
        modifiers: Default::default(),
    }
}

fn release(position: view::Point<view::Pixels>) -> view::MouseUpEvent {
    view::MouseUpEvent {
        button: view::MouseButton::Left,
        position,
        modifiers: Default::default(),
        click_count: 1,
    }
}

#[test]
fn a_row_dragged_onto_another_of_its_level_takes_its_place() {
    let (mut cx, view) = desk(Vec::new());
    let at = laid_out(&mut cx);
    cx.simulate_mouse_down("chat-sidebar-channel-general", press(at(0)));
    cx.simulate_mouse_move("chat-sidebar-channel-j-1", moved(at(3)));
    let onto = |view: &Entity<Chat>| view.read(|chat| chat.drag.as_ref()?.onto.clone());
    assert_eq!(onto(&view), None, "a row of another level is no place");
    cx.simulate_mouse_move("chat-sidebar-folder-jobs", moved(at(2)));
    cx.run_until_parked();
    assert_eq!(onto(&view), Some(Entry::Folder("jobs".into())));
    let lands = view.read(|chat| chat.landing(&Entry::Folder("jobs".into())));
    assert_eq!(lands, Some(true), "from above, it lands below");
    cx.simulate_mouse_up("chat-sidebar-folder-jobs", release(at(2)));
    cx.run_until_parked();
    assert_eq!(
        drawn(&view),
        ["random", "jobs/", "j-1", "j-2", "j-3", "general", "design"]
    );
    assert_eq!(
        orders(&cx),
        [vec![
            room("random"),
            Entry::Folder("jobs".into()),
            room("general"),
            room("design"),
        ]]
    );
    assert!(view.read(|chat| chat.drag.is_none()));
}

#[test]
fn a_press_let_go_where_it_began_moves_nothing() {
    let (mut cx, view) = desk(Vec::new());
    let at = laid_out(&mut cx);
    cx.simulate_mouse_down("chat-sidebar-channel-random", press(at(1)));
    cx.simulate_mouse_move("chat-sidebar-channel-random", moved(at(1)));
    cx.simulate_mouse_up("chat-sidebar-channel-random", release(at(1)));
    cx.run_until_parked();
    assert!(orders(&cx).is_empty());
    assert_eq!(drawn(&view)[1], "random");
}

#[test]
fn a_room_dragged_inside_its_folder_lands_above_a_row_it_came_up_to() {
    let (mut cx, view) = desk(Vec::new());
    let at = laid_out(&mut cx);
    cx.simulate_mouse_down("chat-sidebar-channel-j-3", press(at(5)));
    cx.simulate_mouse_move("chat-sidebar-channel-j-1", moved(at(3)));
    cx.run_until_parked();
    assert_eq!(view.read(|chat| chat.landing(&room("j-1"))), Some(false));
    cx.simulate_mouse_up("chat-sidebar-channel-j-1", release(at(3)));
    cx.run_until_parked();
    assert_eq!(
        drawn(&view),
        ["general", "random", "jobs/", "j-3", "j-1", "j-2", "design"]
    );
}

#[test]
fn a_press_on_a_room_or_folder_row_is_the_rows_and_selects_no_text() {
    let (cx, _view) = desk(Vec::new());
    for row in [
        "chat-sidebar-channel-general",
        "chat-sidebar-channel-j-1",
        "chat-sidebar-folder-jobs",
    ] {
        assert!(
            cx.interactivity(row).consumes_click,
            "{row} consumes its click, so the host starts no selection under the drag"
        );
    }
}

#[test]
fn a_row_clicked_and_left_is_put_down_by_the_next_move() {
    let (mut cx, view) = desk(Vec::new());
    let at = laid_out(&mut cx);
    cx.simulate_mouse_down("chat-sidebar-channel-general", press(at(0)));
    cx.simulate_click("chat-sidebar-channel-general");
    let hovered = view::MouseMoveEvent {
        position: at(1),
        pressed_button: None,
        modifiers: Default::default(),
    };
    cx.simulate_mouse_move("chat-sidebar-channel-random", hovered);
    cx.run_until_parked();
    assert!(
        view.read(|chat| chat.drag.is_none()),
        "no row follows the pointer"
    );
    assert_eq!(view.read(|chat| chat.landing(&room("random"))), None);
    assert!(orders(&cx).is_empty());
}

// A finger held at `position` on the phone's rooms list until it long-presses.
fn long_press(cx: &mut TestAppContext, position: view::Point<view::Pixels>) {
    let handler = cx
        .interactivity("chat-rooms")
        .on_aux_click
        .expect("the rooms list hears a long press");
    cx.tick(vec![wire::Event::AuxClick {
        handler,
        event: wire::click::Click::Touch {
            position,
            tap_count: 1,
            long_press: true,
        },
    }]);
}

#[test]
fn a_long_press_lifts_a_phone_row_and_the_finger_moves_it_in_its_level() {
    let (mut cx, view) = phone(Vec::new());
    let at = laid_out(&mut cx);
    long_press(&mut cx, at(0));
    cx.run_until_parked();
    let held = view.read(|chat| chat.drag.clone());
    assert_eq!(
        held,
        Some(crate::room_list::Drag {
            entry: room("general"),
            onto: None,
        })
    );
    cx.simulate_mouse_move("chat-rooms-channel-j-1", moved(at(3)));
    let onto = |view: &Entity<Chat>| view.read(|chat| chat.drag.as_ref()?.onto.clone());
    assert_eq!(onto(&view), None, "a row of another level is no place");
    cx.simulate_mouse_move("chat-rooms-folder-jobs", moved(at(2)));
    cx.run_until_parked();
    let lands = view.read(|chat| chat.landing(&Entry::Folder("jobs".into())));
    assert_eq!(lands, Some(true), "from above, it lands below");
    cx.simulate_mouse_up("chat-rooms-folder-jobs", release(at(2)));
    cx.run_until_parked();
    assert_eq!(
        drawn(&view),
        ["random", "jobs/", "j-1", "j-2", "j-3", "general", "design"]
    );
    assert_eq!(orders(&cx).len(), 1);
    assert!(view.read(|chat| chat.drag.is_none()));
}

#[test]
fn a_phone_row_lifted_and_let_go_where_it_was_moves_nothing() {
    let (mut cx, view) = phone(Vec::new());
    let at = laid_out(&mut cx);
    long_press(&mut cx, at(1));
    cx.run_until_parked();
    assert!(view.read(|chat| chat.drag.is_some()));
    cx.simulate_mouse_up("chat-rooms-channel-random", release(at(1)));
    cx.run_until_parked();
    assert!(orders(&cx).is_empty());
    assert!(view.read(|chat| chat.drag.is_none()), "the row is put down");
    assert_eq!(drawn(&view)[1], "random");
}

#[test]
fn a_screen_reader_moves_a_phone_row_with_its_own_actions() {
    let (mut cx, view) = phone(Vec::new());
    cx.simulate_a11y_action(
        "chat-rooms-channel-general",
        wire::Action::CustomAction,
        Some(wire::ActionData::CustomAction(2)),
    );
    cx.run_until_parked();
    assert_eq!(
        drawn(&view),
        ["random", "general", "jobs/", "j-1", "j-2", "j-3", "design"]
    );
    cx.simulate_a11y_action(
        "chat-rooms-folder-jobs",
        wire::Action::CustomAction,
        Some(wire::ActionData::CustomAction(1)),
    );
    cx.run_until_parked();
    assert_eq!(
        drawn(&view),
        ["random", "jobs/", "j-1", "j-2", "j-3", "general", "design"]
    );
    assert_eq!(orders(&cx).len(), 2);
}
