//! Native GPUI message lists. The room and thread use native variable-height lists;
//! callbacks still call the root view's existing message operations.

use std::rc::Rc;

use view::list as gpui_list;
use view::prelude::*;

use crate::listing::{Listing, listing};
use crate::message::{ChatMessage, new_day};
use crate::ui::room::selection_bar;
use crate::ui::{message, quiet};
use crate::words;
use crate::{Chat, Mode, Pane};
use view::design::Ladder;
use view::{Div, Stateful};

/// A pane's messages: the room's timeline or the open thread. Around the
/// list: paging older, the copy range's bar, the way back to the latest
/// message, and the edit field.
pub fn list(
    chat: &Chat,
    pane: Pane,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement {
    let messages = chat.messages(pane);
    let content = div()
        .id(match pane {
            Pane::Timeline => "chat-timeline",
            Pane::Thread => "chat-thread-messages",
        })
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col();
    if messages.is_empty()
        && let Some(nothing) = nothing_yet(chat, pane, theme)
    {
        return content.child(nothing);
    }
    let timeline = pane == Pane::Timeline;
    content
        .when_some(
            older(chat, cx, theme, ladder).filter(|_| timeline),
            |el, older| el.child(older),
        )
        .when(!messages.is_empty(), |el| {
            // the way back floats over the list's foot, taking no row of its own
            el.child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.))
                    .child(rows(chat, pane, messages.clone(), cx, theme, ladder))
                    .when(timeline, |el| {
                        el.when_some(jump_to_latest(chat, cx, theme, ladder), |el, jump| {
                            el.child(jump)
                        })
                    }),
            )
        })
        .when(
            timeline && chat.copy.is_some_and(|copy| copy.pane == pane),
            |el| el.child(selection_bar(chat, cx, theme, ladder)),
        )
        .when_some(super::menu::editing(chat, pane, cx), |el, editing| {
            el.child(editing)
        })
}

/// What an empty pane says: loading, refused, no replies yet, or the
/// room's beginning. None while the room itself is gone.
fn nothing_yet(chat: &Chat, pane: Pane, theme: &Theme) -> Option<AnyElement> {
    let room = chat.room.as_ref()?;
    let rows = match pane {
        Pane::Timeline => Some(&room.messages),
        Pane::Thread => room.thread.as_ref().map(|thread| &thread.replies),
    };
    if rows.is_some_and(|rows| matches!(rows, Loadable::Loading(_))) {
        return Some(
            quiet(words::LOADING_MESSAGES, theme)
                .p_4()
                .into_any_element(),
        );
    }
    if let Some(refusal) = rows.and_then(Loadable::failed) {
        return Some(
            quiet(design::sentence(refusal), theme)
                .p_4()
                .into_any_element(),
        );
    }
    Some(match pane {
        Pane::Thread => no_replies(theme).into_any_element(),
        Pane::Timeline => intro(&chat.intro()?, theme).into_any_element(),
    })
}

/// The way to older history, while there is some.
fn older(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<AnyElement> {
    let room = chat.room.as_ref().filter(|room| room.has_older)?;
    let control = if room.older_loading {
        div()
            .id("chat-load-older-button")
            .text_color(theme.muted)
            .child(words::LOADING_OLDER)
            .into_any_element()
    } else {
        let older = cx.listener(|chat, _: &ClickEvent, _window, cx| {
            cx.notify();
            chat.load_older(cx)
        });
        super::button(
            "chat-load-older-button",
            "Load older messages",
            ladder,
            theme,
            older,
        )
        .into_any_element()
    };
    let row = div()
        .id("chat-load-older")
        .flex()
        .justify_center()
        .p_2()
        .child(control);
    Some(row.into_any_element())
}

/// The grid the desktop's arrows walk, and where its cursor is: the active
/// message's id and cell.
fn grid(
    chat: &Chat,
    pane: Pane,
    messages: &Rc<[ChatMessage]>,
    lead: bool,
    state: &ListState,
    cx: &mut Context<Chat>,
) -> (Stateful<Div>, Option<String>, usize) {
    // One Tab stop, a grid: ↑ ↓ walk the messages (the list scrolls the
    // next one into view before it claims), ← → a message's cells (the
    // message, then its controls), Enter presses the active cell — on the
    // message, its click. Active by message id: the newest until the
    // arrows move.
    let cursor = chat.cursor(pane);
    let at = cursor
        .id
        .as_ref()
        .and_then(|id| messages.iter().position(|message| &message.id == id))
        .unwrap_or(messages.len().saturating_sub(1));
    let active_id = messages.get(at).map(|message| message.id.clone());
    let cell = cursor.cell;
    let reveal = state.clone();
    let moved_to = messages.clone();
    let pressed = messages.clone();
    let cell_row = active_id.clone();
    let grid = design::composite(
        match pane {
            Pane::Timeline => "chat-message-list",
            Pane::Thread => "chat-thread-list",
        },
        Role::Grid,
        match pane {
            Pane::Timeline => "Messages",
            Pane::Thread => "Replies",
        },
    )
    .active(at, messages.len())
    // the cells are counted as the active row is drawn (`Cursor::controls`),
    // so the bound is the recorded count at the key, not at the build
    .cells(cell, usize::MAX)
    .on_move(cx.processor(move |chat, index: usize, _, cx| {
        // the arrows leave the chosen message behind
        chat.let_go();
        let cursor = chat.cursor_mut(pane);
        cursor.id = Some(moved_to[index].id.clone());
        cursor.cell = 0;
        reveal.scroll_to_reveal_item(index + usize::from(lead));
        cx.notify();
    }))
    .on_move_cell(cx.processor(move |chat, cell: usize, _, cx| {
        let cursor = chat.cursor_mut(pane);
        let last = cursor
            .id
            .clone()
            .or_else(|| cell_row.clone())
            .map_or(0, |id| cursor.controls_of(&id).len());
        cursor.cell = cell.min(last);
        cx.notify();
    }))
    .on_press(cx.processor(move |chat, index: usize, window, cx| {
        let message = &pressed[index];
        let (id, seq, rev) = (message.id.clone(), message.seq, message.rev);
        let cursor = chat.cursor(pane);
        // the controls recorded are this message's, else the row was not
        // drawn since the cursor moved and no control is pressed
        let control = match cursor.cell {
            0 => None,
            cell => Some(cursor.controls_of(&id).get(cell - 1).cloned()),
        };
        chat.layout.press = chat.key_spot(pane);
        cx.notify();
        match control {
            None => chat.press_message(pane, seq, false),
            Some(Some(control)) => chat.act(pane, seq, rev, control, window, cx),
            Some(None) => {}
        }
    }))
    .build();
    // the Menu key or Shift+F10 opens the active message's menu where
    // Enter on its ⋯ does
    let asked = messages
        .get(at)
        .filter(|message| !message.pending && !message.deleted)
        .map(|message| (message.seq, message.rev));
    let keys = cx.listener(move |chat, event: &KeyDownEvent, window, cx| {
        let Some((seq, rev)) = asked.filter(|_| design::asks_for_menu(&event.keystroke)) else {
            return;
        };
        chat.layout.press = chat.key_spot(pane);
        chat.open_menu(pane, seq, rev, Mode::More, window, cx);
        cx.notify();
    });
    (grid.on_key_down(keys), active_id, cell)
}

/// The native list: the room's intro first once its history is all here,
/// the unread divider over the first unread message, and "No replies yet"
/// under a thread that is only its root.
fn rows(
    chat: &Chat,
    pane: Pane,
    messages: Rc<[ChatMessage]>,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement {
    let ladder = *ladder;
    let Listing {
        shape,
        intro: lead_text,
        bare,
        state,
        unread,
    } = listing(chat, pane, &messages, cx);
    let lead = lead_text.is_some();
    let (grid, active_id, cell) = grid(chat, pane, &messages, lead, &state, cx);
    let theme = *theme;
    let shared = message::Shared::of(chat);
    let list = gpui_list(
        shape.id(),
        state,
        cx.processor(move |chat, index: usize, _window, cx| {
            if let Some(text) = lead_text.as_ref().filter(|_| index == 0) {
                return intro(text, &theme).into_any_element();
            }
            let at = index - usize::from(lead);
            let Some(message) = messages.get(at).cloned() else {
                return match bare {
                    true => no_replies(&theme).into_any_element(),
                    false => div().into_any_element(),
                };
            };
            let day = new_day(&messages, at);
            let unread = unread == Some(message.seq);
            let active = (active_id.as_ref() == Some(&message.id)).then_some(cell);
            let set = (at + 1, messages.len());
            // the row is drawn again only when what it reads moved
            let deps = message::RowDeps {
                seen: message::Seen::of(chat, pane, &message),
                message: message.clone(),
                pane,
                active,
                set,
                markers: (day.clone(), unread),
                shared: shared.clone(),
                ladder,
            };
            let id = message.id.clone();
            let row = format!("chat-message-{id}-drawn");
            cx.memo(row, deps, move |chat, _, cx| {
                row_of(
                    chat,
                    message,
                    (day, unread),
                    pane,
                    active,
                    set,
                    cx,
                    &theme,
                    &ladder,
                )
            })
            .into_any_element()
        }),
    )
    .with_sizing_behavior(ListSizingBehavior::Auto)
    .flex_1()
    .min_h(px(0.))
    .w_full();
    grid.flex().flex_col().flex_1().min_h(px(0.)).child(list)
}

// A message's row in the list: its card, under the day and unread markers
// where it opens a day or the unread messages.
#[allow(
    clippy::too_many_arguments,
    reason = "the card's own arguments, and the markers above it"
)]
fn row_of(
    chat: &mut Chat,
    message: ChatMessage,
    (day, unread): (Option<String>, bool),
    pane: Pane,
    active: Option<usize>,
    set: (usize, usize),
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let day = day.map(|day| day_marker(&message.id, day, theme).into_any_element());
    let unread = unread.then(|| unread_marker(theme));
    let id = message.id.clone();
    let marked = format!("chat-message-{id}-marked");
    let (card, controls) = message::card(chat, message, pane, active, set, cx, theme, ladder);
    if active.is_some() {
        let cursor = chat.cursor_mut(pane);
        cursor.controls = controls;
        cursor.controls_of = Some(id);
    }
    if day.is_none() && unread.is_none() {
        return card;
    }
    // The markers sit outside the card, which is the grid's row,
    // so the list's row is this wrapper: it carries the message's
    // key, as a bare card does, and the card under it is filed by
    // its message wherever the row moves. Full width, as a bare
    // card is: a row shrunk to its words took the hover and the
    // action strip with it
    div()
        .id(marked)
        .w_full()
        .flex()
        .flex_col()
        .children(day)
        .children(unread)
        .child(card)
        .into_any_element()
}

/// "Jump to latest", when the timeline is not at its live tail.
fn jump_to_latest(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<AnyElement> {
    let room = chat.room.as_ref()?;
    if !room.landed && room.at_tail {
        return None;
    }
    let id = room.id.clone();
    let latest = cx.listener(move |chat, _: &ClickEvent, _, cx| {
        cx.notify();
        chat.open(id.clone(), cx)
    });
    // a strip across the list's foot that only centres the button: it has
    // no handlers, so a click beside the button still reaches the row under it
    let jump = div()
        .id("chat-jump-latest")
        .absolute()
        .left_0()
        .right_0()
        .bottom_3()
        .flex()
        .justify_center()
        .child(div().bg(theme.background).shadow_lg().child(super::button(
            "chat-jump-latest-button",
            "Jump to latest",
            ladder,
            theme,
            latest,
        )));
    Some(jump.into_any_element())
}

/// The day the messages under it were posted on, between two days.
fn day_marker(id: &str, day: String, theme: &Theme) -> impl IntoElement {
    let rule = || div().h(px(1.)).flex_1().bg(theme.border);
    div()
        .id(format!("chat-day-{id}"))
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .text_size(design::text::CAPTION)
        .text_color(theme.muted)
        .child(rule())
        .child(day)
        .child(rule())
}

fn unread_marker(theme: &Theme) -> impl IntoElement {
    div()
        .id("chat-unread-marker")
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .text_size(design::text::SECONDARY)
        .text_color(theme.accent_foreground)
        .child(div().h(px(1.)).flex_1().bg(theme.accent))
        .child("New messages")
}

/// The top of a room's history. The room's name is already its header's,
/// so this says only where the history starts ([`Chat::intro`]).
fn intro(text: &str, theme: &Theme) -> impl IntoElement {
    div()
        .id("chat-timeline-intro")
        .px_6()
        .pt_6()
        .pb_3()
        .text_size(design::text::SECONDARY)
        .text_color(theme.muted)
        .child(text.to_owned())
}

/// A thread with nothing under its root yet.
fn no_replies(theme: &Theme) -> impl IntoElement {
    div()
        .id("chat-thread-no-replies")
        .px_4()
        .py_3()
        .text_size(design::text::SECONDARY)
        .text_color(theme.muted)
        .child(words::NO_REPLIES)
}
