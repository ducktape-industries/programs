//! What a room's details offer for its notices: one radio group, the same
//! on the desktop's pane and the phone's page. A choice is chat's, on chain
//! (`Chat::choose_notices`), so every device and server tells the same.
use chat::Notices;
use view::design::Ladder;
use view::prelude::*;
use view::{Div, Stateful};

use crate::Chat;
use crate::notifier::Choice;
use crate::words;

/// What a room offers, in order; a direct room is on or off.
fn offered(dm: bool) -> &'static [Choice] {
    const ROOM: &[Choice] = &[
        Choice::Every,
        Choice::Default,
        Choice::Cut(Notices::Mentions),
        Choice::Cut(Notices::Off),
    ];
    const DIRECT: &[Choice] = &[Choice::Default, Choice::Cut(Notices::Off)];
    if dm { DIRECT } else { ROOM }
}

/// `room`'s notices, a row per choice with the chosen one checked; none is
/// checked until the reader's choices are read, and none presses without
/// an account to write as. The arrows move among them and Enter or Space
/// chooses: each choice is a write, so a step is not one.
pub(crate) fn choices(
    chat: &Chat,
    room: &str,
    dm: bool,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let offered = offered(dm);
    // every message, or mentions, in a direct room is every message
    let chosen = chat.choice(room).map(|choice| match choice {
        Choice::Every | Choice::Cut(Notices::Mentions) if dm => Choice::Default,
        choice => choice,
    });
    let at = chat
        .details
        .as_ref()
        .and_then(|details| details.notices_at)
        .or_else(|| offered.iter().position(|choice| Some(*choice) == chosen))
        .unwrap_or_default()
        .min(offered.len() - 1);
    let writes = chat.holds_account() && chat.session.connected;
    let mut group = design::composite(
        "chat-details-notices",
        Role::RadioGroup,
        words::NOTIFICATIONS,
    )
    .active(at, offered.len())
    .wrap()
    .on_move(cx.processor(|chat, index: usize, _, cx| {
        if let Some(details) = &mut chat.details {
            details.notices_at = Some(index);
            cx.notify();
        }
    }))
    .on_press(cx.processor(move |chat, index: usize, _, cx| {
        if writes && let Some(choice) = offered.get(index) {
            chat.choose_notices(*choice, cx);
        }
    }))
    .build()
    .flex()
    .flex_col();
    for (index, &choice) in offered.iter().enumerate() {
        let on = chosen == Some(choice);
        let (label, about) = words::choice(choice, dm);
        let pick = cx.listener(move |chat, _: &ClickEvent, _, cx| chat.choose_notices(choice, cx));
        let row = design::row(
            format!("chat-details-notices-{}", slug(choice)),
            ladder,
            theme,
        )
        .min_h(ladder.row_tall)
        .child(
            design::row_words(design::row_title(label, ladder, theme))
                .child(design::row_caption(about, ladder, theme)),
        )
        .child(mark(on, theme))
        .aria_toggled(on.into())
        .map(|row| match writes {
            true => design::pressed(row.cursor_pointer().on_click(pick), ladder),
            false => row.aria_disabled(true),
        });
        group = group.child(design::item(row, Role::RadioButton, index == at));
    }
    group
}

/// What a choice's row is keyed by: `chat-details-notices-<slug>`.
fn slug(choice: Choice) -> &'static str {
    match choice {
        Choice::Every => "every",
        Choice::Default => "default",
        Choice::Cut(Notices::Mentions) => "mentions",
        Choice::Cut(Notices::Off) => "off",
    }
}

/// The radio's dot: filled when chosen.
fn mark(on: bool, theme: &Theme) -> Div {
    div()
        .size(px(14.))
        .flex_none()
        .rounded_full()
        .border_1()
        .border_color(if on {
            theme.foreground
        } else {
            theme.border_strong
        })
        .when(on, |dot| dot.bg(theme.foreground))
}
