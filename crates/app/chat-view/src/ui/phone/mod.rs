//! The phone's chat: the Expo app's chat stack (mobile
//! `app/(tabs)/chat/_layout.tsx`), one screen at a time, chosen once at the
//! view's root by `window.touch()`. Reads the page stack ([`Chat::pages`]),
//! the open message menu (an actions or picker sheet, the delete
//! confirmation) and the session; writes nothing itself: Back pops the
//! stack through [`Chat::back`]. Nothing under here asks whether the device
//! is touch: every screen is the Expo design, on the ladder's
//! [`design::TOUCH`] values. Without a node only the rooms page shows, and
//! says so.
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
    // what is drawn, decided once: the page on top and what is over it
    let page = page(chat);
    let sheet = actions::sheet(chat, cx, &theme, ladder)
        .or_else(|| picker::sheet(chat, cx, &theme, ladder))
        .or_else(|| details::sheet(chat, cx, &theme, ladder))
        .or_else(|| crate::policies::sheet(chat, cx, ladder, &theme));
    let confirm = actions::confirm(chat, cx, &theme, ladder);
    // Back goes somewhere exactly while a page is over the rooms or a sheet
    // or the dialog is over the page (rule 13); the host leaves the
    // program else
    let back = (page != Page::Rooms || sheet.is_some() || confirm.is_some()).then(|| {
        cx.listener(|chat, _: &BackEvent, _window, cx| {
            chat.back();
            cx.notify();
        })
    });
    let placed = cx.listener(|chat, bounds: &Bounds<Pixels>, _window, cx| {
        chat.layout.pane = Some(bounds.into());
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

/// The page on top: the stack's, or the rooms alone without a node.
fn page(chat: &Chat) -> Page {
    match chat.session.connected {
        true => *chat.pages().top(),
        false => Page::Rooms,
    }
}

/// `page`, drawn.
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
    // the stack holds a room's pages only while a room is open
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
