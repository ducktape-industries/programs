//! A sidebar room's menu at the pointer, over the screen (`design::menu`):
//! what a right press, the Menu key or Shift+F10 on a room offers
//! (`room_menu.rs`), and the row's part in opening it.
use view::accesskit::Action;
use view::design::Ladder;
use view::prelude::*;
use view::{Div, Stateful};

use crate::room_menu::{ROOM_MENU, RoomItem};
use crate::{Chat, RoomMenu, words};

pub fn over(
    chat: &Chat,
    screen: AnyElement,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let menu = chat
        .room_menu
        .as_ref()
        .and_then(|menu| popup(chat, menu, cx, theme, ladder));
    let close = cx.listener(|chat, _: &(), window, cx| chat.close_room_menu(window, cx));
    modal_overlay("chat-room-menu-overlay", words::ROOM_ACTIONS, screen, menu)
        .on_dismiss(close)
        .into_any_element()
}

fn popup(
    chat: &Chat,
    menu: &RoomMenu,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<design::Menu> {
    let at = menu.at?;
    let moved = cx.processor(|chat, index: usize, _, cx| chat.move_in_room_menu(index, cx));
    let popup = design::menu(ROOM_MENU, words::ROOM_ACTIONS, at, theme)
        .active(menu.active)
        .on_move(moved);
    let items = chat.room_items(&menu.room).into_iter();
    Some(items.fold(popup, |popup, item| {
        popup.item(room_item(&menu.room, item, cx, theme, ladder))
    }))
}

fn room_item(
    room: &str,
    item: RoomItem,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> design::MenuItem {
    let id = format!("{ROOM_MENU}/{}", item.slug());
    let (label, glyph) = (item.label(), item.glyph());
    let room = room.to_owned();
    let run = cx.listener(move |chat, _: &ClickEvent, window, cx| {
        chat.run_room_item(room.clone(), item.clone(), window, cx)
    });
    design::menu_item(id, label, ladder, theme, run).icon(glyph)
}

pub fn waits(chat: &Chat, room: &str) -> bool {
    chat.room_menu
        .as_ref()
        .is_some_and(|menu| menu.room == room && menu.at.is_none())
}

pub fn row(row: Stateful<Div>, room: &str, chat: &Chat, cx: &mut Context<Chat>) -> Stateful<Div> {
    let pressed = room.to_owned();
    let open = cx.listener(move |chat, event: &MouseDownEvent, window, cx| {
        chat.open_room_menu(pressed.clone(), Some(event.position), window, cx)
    });
    let asked = room.to_owned();
    let show = cx.listener(move |chat, _: &(), window, cx| {
        chat.open_room_menu(asked.clone(), None, window, cx)
    });
    let waits = waits(chat, room);
    let placed = cx.listener(|chat, bounds: &Bounds<Pixels>, window, cx| {
        chat.place_room_menu(*bounds, window, cx)
    });
    row.on_mouse_down(MouseButton::Right, open)
        .on_a11y_action(Action::ShowContextMenu, move |_, window, app| {
            show(&(), window, app)
        })
        .when(waits, |row| {
            row.relative().child(
                sensor("chat-room-menu-anchor", div().size_full())
                    .absolute()
                    .inset_0()
                    .on_bounds(placed),
            )
        })
}
