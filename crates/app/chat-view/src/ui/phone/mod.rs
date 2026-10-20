//! The phone's chat: the Expo app's chat stack, one screen at a time, chosen
//! at the view's root by `window.touch()`, on [`design::TOUCH`]. Writes
//! nothing itself: Back pops the stack through [`Chat::back`].
use view::prelude::*;

use crate::words;
use crate::{Chat, Page};
use view::design::Ladder;

mod actions;
mod composer;
mod details;
mod dm_pick;
mod message;
mod new;
mod parts;
mod picker;
mod rich;
mod room;
mod rooms;
mod search;
mod thread;
mod timeline;

pub fn render(chat: &Chat, cx: &mut Context<Chat>) -> AnyElement {
    let theme = *cx.global::<Theme>();
    let ladder = &design::TOUCH;
    let page = page(chat);
    let sheet = actions::sheet(chat, cx, &theme, ladder)
        .or_else(|| actions::reactors(chat, cx, &theme, ladder))
        .or_else(|| picker::sheet(chat, cx, &theme, ladder))
        .or_else(|| details::sheet(chat, cx, &theme, ladder))
        .or_else(|| crate::policies::sheet(chat, cx, ladder, &theme));
    let confirm = actions::confirm(chat, cx, &theme, ladder);
    let back = (page != Page::Rooms || sheet.is_some() || confirm.is_some()).then(|| {
        cx.listener(|chat, _: &BackEvent, _window, cx| {
            chat.back();
            cx.notify();
        })
    });
    let placed = cx.listener(|chat, bounds: &Bounds<Pixels>, _window, cx| {
        chat.layout.placed(bounds.into());
        cx.notify();
    });
    let screen = div()
        .id("chat-root")
        .relative()
        .size_full()
        .flex()
        .flex_col()
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_type(ladder.body)
        .role(Role::Group)
        .aria_label(words::CHAT)
        .child(screen(chat, page, cx, &theme, ladder))
        .children(sheet)
        .child(
            sensor("chat-pane", div().size_full())
                .absolute()
                .inset_0()
                .on_bounds(placed),
        )
        .into_any_element();
    let screen = actions::with_confirm(screen, confirm, cx, &theme);
    div()
        .id("chat")
        .size_full()
        .when_some(back, |root, back| root.on_back(back))
        .child(screen)
        .into_any_element()
}

fn page(chat: &Chat) -> Page {
    match chat.session.connected {
        true => *chat.pages().top(),
        false => Page::Rooms,
    }
}

fn screen(
    chat: &Chat,
    page: Page,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let back = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.back();
        cx.notify();
    });
    match (page, &chat.room) {
        (Page::Room, Some(room)) => room::screen(chat, room, back, cx, theme, ladder),
        (Page::Thread, Some(room)) => thread::screen(chat, room, back, cx, theme, ladder),
        (Page::Details, Some(room)) => details::screen(chat, room, back, cx, theme, ladder),
        (Page::New, _) => new::screen(chat, back, cx, theme, ladder),
        (Page::NewMessage, _) => dm_pick::screen(chat, back, cx, theme, ladder),
        (Page::Search, _) => search::screen(chat, back, cx, theme, ladder),
        _ => rooms::screen(chat, cx, theme, ladder),
    }
}
