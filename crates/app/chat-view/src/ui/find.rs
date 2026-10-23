//! The find bar over the open room: its field, where it stands among the matches, and the buttons
//! Enter, Shift+Enter and Escape stand for.
use view::Modifiers;
use view::design::Ladder;
use view::prelude::*;
use view::wire::{
    self,
    keyboard::{Key, Named},
};

use crate::Chat;
use crate::find::FIELD;

const FIND_IN_CHANNEL: &str = "Find in channel";

pub(crate) fn bar(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<impl IntoElement> {
    let find = chat.find.as_ref()?;
    let changed = cx.listener(|chat, _: &wire::TextChange, _window, cx| {
        chat.find_changed(cx);
        cx.notify();
    });
    let keys = cx.listener(|chat, event: &KeyDownEvent, window, cx| {
        match event.keystroke.key.as_str() {
            "enter" => chat.find_step(!event.keystroke.modifiers.shift, cx),
            "escape" => chat.find_close(window),
            _ => return,
        }
        cx.notify();
    });
    let claim = |key, shift| wire::KeyClaim {
        key: Key::Named(key),
        modifiers: Modifiers {
            shift,
            ..Modifiers::default()
        },
        command: false,
    };
    let field = Input::new(FIELD, &find.draft, FIND_IN_CHANNEL)
        .placeholder(FIND_IN_CHANNEL)
        .claims([
            claim(Named::Enter, false),
            claim(Named::Enter, true),
            claim(Named::Escape, false),
        ])
        .on_change(changed)
        .on_key(keys)
        .h(design::size::CONTROL)
        .flex_1()
        .px_2()
        .py_1()
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface);
    let older = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.find_step(true, cx);
        cx.notify();
    });
    let newer = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.find_step(false, cx);
        cx.notify();
    });
    let close = cx.listener(|chat, _: &ClickEvent, window, cx| {
        chat.find_close(window);
        cx.notify();
    });
    let said = chat.find_said().map(|said| {
        div()
            .id("chat-find-said")
            .text_size(design::text::SECONDARY)
            .text_color(theme.muted)
            .role(Role::Status)
            .aria_live(view::accesskit::Live::Polite)
            .child(said)
    });
    Some(
        div()
            .id("chat-find-bar")
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .role(Role::Group)
            .aria_label(FIND_IN_CHANNEL)
            .child(field)
            .children(said)
            .child(design::icon_button(
                "chat-find-older",
                "↑",
                "Older match (Enter)",
                ladder,
                theme,
                older,
            ))
            .child(design::icon_button(
                "chat-find-newer",
                "↓",
                "Newer match (Shift+Enter)",
                ladder,
                theme,
                newer,
            ))
            .child(design::icon_button(
                "chat-find-close",
                "✕",
                "Close find (Esc)",
                ladder,
                theme,
                close,
            )),
    )
}
