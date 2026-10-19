//! A room's rules where they show: what its details say of them, and the line over the composer
//! saying the room has some, whose press opens the details. Reads the open room's rules.
use view::design::Ladder;
use view::prelude::*;

use crate::rules::Said;
use crate::{Chat, words};

pub(crate) fn said(chat: &Chat, theme: &Theme, ladder: &Ladder) -> Option<AnyElement> {
    let (text, from) = match chat.rules_said()? {
        Said::Text(text) => (text.to_owned(), None),
        Said::Reading(path) => (words::RULES_READING.to_owned(), Some(path)),
        Said::File { path, text } => (text.to_owned(), Some(path)),
        Said::Failed { path, why } => (why.to_owned(), Some(path)),
    };
    let from = from.map(|path| {
        div()
            .text_type(ladder.caption)
            .text_color(theme.muted)
            .child(words::rules_from(path))
    });
    Some(
        div()
            .id("chat-rules")
            .flex()
            .flex_col()
            .gap(design::space::XXS)
            .child(div().text_type(ladder.body).child(text))
            .children(from)
            .into_any_element(),
    )
}

pub(crate) fn line(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<AnyElement> {
    chat.room_rules()?;
    if chat.write_gate().is_some() {
        return None;
    }
    let open = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.open_details(cx);
        cx.notify();
    });
    Some(
        div()
            .id("chat-room-rules")
            .pb(design::space::XXS)
            .text_type(ladder.caption)
            .text_color(theme.faint)
            .cursor_pointer()
            .role(Role::Button)
            .focusable()
            .on_click(open)
            .child(words::HAS_RULES)
            .into_any_element(),
    )
}
