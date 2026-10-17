//! The menu at the pointer, over the page (`design::menu`): what a right
//! press on a row or on a folder's empty room offers (`crate::menu`), each
//! item a press, ↑ ↓ and Enter from the keys, Escape or a press outside to
//! close. A row opens it from the keys too, and says where it is while the
//! menu waits to open under it. Reads the open menu; writes the menu and
//! what its items run.
use view::accesskit::Action;

use super::*;
use crate::menu::{Item, MENU, Menu, On, asks_for_menu};
use crate::state::Sheet;
use crate::write::Write;

/// `screen` under the menu while one is open, at its place.
pub(super) fn over(view: &Files, screen: impl IntoElement, cx: Cx, k: &Kit) -> AnyElement {
    let menu = match view.sheet_shown() {
        Some(Sheet::Menu(menu)) => popup(view, menu, cx, k),
        _ => None,
    };
    let close = cx.listener(|view: &mut Files, _: &(), window, cx| view.close_menu(window, cx));
    modal_overlay("files-menu-overlay", words::ACTIONS, screen, menu)
        .on_dismiss(close)
        .into_any_element()
}

/// The menu at its place; none while its row has not said where it is.
fn popup(view: &Files, menu: &Menu, cx: Cx, k: &Kit) -> Option<design::Menu> {
    let at = menu.at?;
    let popup = design::menu(MENU, words::ACTIONS, at, &k.theme)
        .active(menu.active)
        .on_move(
            cx.processor(|view: &mut Files, index: usize, _, cx| view.move_in_menu(index, cx)),
        );
    Some(
        view.items(&menu.on)
            .into_iter()
            .fold(popup, |popup, item| popup.item(menu_item(item, cx, k))),
    )
}

fn menu_item(item: Item, cx: Cx, k: &Kit) -> design::MenuItem {
    let id = format!("files/menu/{}", item.slug());
    let (label, glyph) = (item.label(), item.glyph());
    let removes = matches!(item, Item::Write(Write::Remove { .. }));
    let run = cx.listener(move |view: &mut Files, _: &ClickEvent, window, cx| {
        view.run(item.clone(), window, cx)
    });
    design::menu_item(id, label, &k.ladder, &k.theme, run)
        .icon(glyph)
        .when(removes, |item| item.danger())
}

/// `element` opens the menu of `on` under a right press, at the pointer.
pub(super) fn pointed(element: Stateful<Div>, on: On, cx: Cx) -> Stateful<Div> {
    let open = cx.listener(
        move |view: &mut Files, event: &MouseDownEvent, window, cx| {
            view.open_menu(on.clone(), Some(event.position), window, cx)
        },
    );
    element.on_mouse_down(MouseButton::Right, open)
}

/// The row of the entry at `path` opens its menu: under a right press at
/// the pointer, and from the Menu key, Shift+F10 or assistive tech's
/// "show menu" under the row, which says where it is while the menu waits
/// to open there. The row keeps the pointer from the list around it, its
/// scroll aside, so a press on a row is never the list's empty room.
pub(super) fn row(row: Stateful<Div>, path: &str, view: &Files, cx: Cx) -> Stateful<Div> {
    let on = On::Entry(path.to_owned());
    let keyed = on.clone();
    let keys = cx.listener(move |view: &mut Files, event: &KeyDownEvent, window, cx| {
        if asks_for_menu(&event.keystroke) {
            view.open_menu(keyed.clone(), None, window, cx)
        }
    });
    let asked = on.clone();
    let show = cx.listener(move |view: &mut Files, _: &(), window, cx| {
        view.open_menu(asked.clone(), None, window, cx)
    });
    let waits = matches!(
        &view.sheet,
        Some(Sheet::Menu(Menu { on: waiting, at: None, .. })) if *waiting == on
    );
    let placed = cx.listener(|view: &mut Files, bounds: &Bounds<Pixels>, window, cx| {
        view.place_menu(*bounds, window, cx)
    });
    pointed(row, on, cx)
        .block_mouse_except_scroll()
        .on_key_down(keys)
        .on_a11y_action(Action::ShowContextMenu, move |_, window, app| {
            show(&(), window, app)
        })
        .when(waits, |row| {
            row.relative().child(
                sensor("files-menu-anchor", div().size_full())
                    .absolute()
                    .inset_0()
                    .on_bounds(placed),
            )
        })
}
