//! Native GPUI composition for Chat. State and program operations stay in the
//! root view; this module only builds the element tree and installs listeners.

pub(crate) mod answering;
pub(crate) mod attachment;
pub mod dialogs;
pub mod menu;
pub mod message;
pub(crate) mod notices;
pub(crate) mod phone;
pub mod room;
pub mod room_menu;
pub(crate) mod rules;
pub mod side;
pub mod sidebar;
pub(crate) mod timeline;
pub(crate) mod widget;

pub(crate) use view::design::{badge, button, empty_state, quiet};
use view::prelude::*;

use crate::Chat;
use crate::words;
use view::design::Ladder;

/// The desktop's chat: a window under a pointer (a phone draws
/// [`phone::render`] instead).
pub fn render(chat: &Chat, cx: &mut Context<Chat>) -> impl IntoElement {
    let theme = *cx.global::<Theme>();
    let ladder = &design::POINTER;
    let escape = cx.listener(|chat, event: &KeyDownEvent, _window, cx| {
        if event.keystroke.key == "escape" && !event.keystroke.modifiers.modified() && chat.holds()
        {
            chat.let_go();
            cx.notify();
        }
    });
    let press = cx.listener(|chat, event: &MouseDownEvent, _window, cx| {
        if !event.modifiers.shift && chat.holds() {
            chat.let_go();
            cx.notify();
        }
    });
    let placed = cx.listener(|chat, bounds: &Bounds<Pixels>, _window, cx| {
        chat.layout.pane = Some(bounds.into());
        cx.notify();
    });
    let screen = div()
        .id("chat-root")
        .relative()
        .flex()
        .size_full()
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_size(design::text::BODY)
        .role(Role::Group)
        .aria_label(words::CHAT)
        .on_key_down(escape)
        .when(chat.holds(), |screen| screen.consumes_keys(["escape"]))
        .on_any_mouse_down(press)
        .child(if chat.session.connected {
            connected(chat, cx, &theme, ladder).into_any_element()
        } else {
            empty_state(
                "chat-disconnected",
                "wifi-off",
                "Not connected",
                "Choose a network from the sidebar to reconnect.",
                ladder,
                &theme,
            )
            .into_any_element()
        })
        .child(
            sensor("chat-pane", div().size_full())
                .absolute()
                .inset_0()
                .on_bounds(placed),
        )
        .children(crate::policies::sheet(chat, cx, ladder, &theme))
        .into_any_element();
    let screen = with_menu(chat, screen, cx, &theme, ladder);
    let screen = attachment::over(chat, screen, cx, &theme, ladder);
    let screen = room_menu::over(chat, screen, cx, &theme, ladder);
    let screen = with_create(chat, screen, cx, &theme, ladder);
    with_dm_pick(chat, screen, cx, &theme, ladder)
}

fn with_menu(
    chat: &Chat,
    screen: AnyElement,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let dismiss = cx.listener(|chat, _: &(), _window, cx| {
        chat.close_menu();
        cx.notify();
    });
    let mode = chat.menu.as_ref().map(|menu| menu.mode);
    let label = match mode {
        Some(crate::Mode::Reactions) => words::ADD_REACTION,
        Some(crate::Mode::Profile) => "Agent profile",
        _ => "Message menu",
    };
    let overlay = modal_overlay(
        "chat-menu-overlay",
        label,
        screen,
        menu::floating(chat, cx, theme, ladder),
    )
    .on_dismiss(dismiss);
    let confirming = mode == Some(crate::Mode::Delete);
    match confirming {
        true => overlay.backdrop(hsla(0., 0., 0., 0.35)).into_any_element(),
        false => overlay.into_any_element(),
    }
}

fn with_create(
    chat: &Chat,
    screen: AnyElement,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let dismiss = cx.listener(|chat, _: &(), _window, cx| {
        chat.create = None;
        cx.notify();
    });
    let create = dialogs::channel_create(chat, cx, theme, ladder);
    let overlay = modal_overlay("chat-create-overlay", "Create channel", screen, create)
        .flex()
        .items_center()
        .justify_center()
        .p_6()
        .backdrop(hsla(0., 0., 0., 0.55));
    match chat.create.as_ref().is_some_and(|create| create.busy) {
        true => overlay.into_any_element(),
        false => overlay.on_dismiss(dismiss).into_any_element(),
    }
}

fn with_dm_pick(
    chat: &Chat,
    screen: AnyElement,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let dismiss = cx.listener(|chat, _: &(), _window, cx| {
        chat.dm_pick = None;
        cx.notify();
    });
    let pick = dialogs::dm_pick(chat, cx, theme, ladder);
    modal_overlay("chat-dm-pick-overlay", words::NEW_MESSAGE, screen, pick)
        .flex()
        .items_center()
        .justify_center()
        .p(dialogs::INSET)
        .backdrop(hsla(0., 0., 0., 0.55))
        .on_dismiss(dismiss)
        .into_any_element()
}

fn connected(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement {
    let panes = div()
        .id("chat-panes")
        .relative()
        .flex()
        .size_full()
        .child(sidebar::render(chat, cx, theme, ladder))
        .child(design::divider(
            "chat-sidebar-resize",
            "Resize the room list",
            theme,
            cx,
            |chat, dx| chat.layout.drag_sidebar(dx),
        ))
        .child(room::render(chat, cx, theme, ladder));
    let details = chat.details.is_some() && chat.room.is_some();
    let thread = chat.room.as_ref().is_some_and(|room| room.thread.is_some());
    let layout = chat.layout.clamped();
    let widget = chat.widget_side_shown();
    let (pane, width) = match (details, thread, widget) {
        (true, _, _) => (side::details(chat, cx, theme, ladder), layout.details),
        (false, true, _) => (side::thread(chat, cx, theme, ladder), layout.thread),
        (false, false, true) => (widget::side(chat, cx, theme, ladder), layout.details),
        (false, false, false) => return panes,
    };
    if !layout.docks(width) {
        return panes.child(design::over("chat-side-over", pane, theme));
    }
    let divider = match (details, thread) {
        (false, false) => design::divider(
            "chat-widget-resize",
            "Resize the widget",
            theme,
            cx,
            |chat, dx| chat.layout.drag_details(dx),
        ),
        (true, _) => design::divider(
            "chat-details-resize",
            "Resize channel details",
            theme,
            cx,
            |chat, dx| chat.layout.drag_details(dx),
        ),
        (false, true) => design::divider(
            "chat-thread-resize",
            "Resize the thread",
            theme,
            cx,
            |chat, dx| chat.layout.drag_thread(dx),
        ),
    };
    panes.child(divider).child(pane)
}
