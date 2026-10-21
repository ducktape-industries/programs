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
        .when_some(older(chat, theme).filter(|_| timeline), |el, older| {
            el.child(older)
        })
        .when(!messages.is_empty(), |el| {
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

fn nothing_yet(chat: &Chat, pane: Pane, theme: &Theme) -> Option<AnyElement> {
    let room = chat.room.as_ref()?;
    let rows = match pane {
        Pane::Timeline => Some(&room.messages),
        Pane::Thread => room.thread.as_ref().map(|thread| &thread.replies),
    };
    if rows.is_some_and(|rows| matches!(rows, Loadable::Loading(_))) {
        return Some(super::placeholders::messages(pane, theme).into_any_element());
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

fn older(chat: &Chat, theme: &Theme) -> Option<AnyElement> {
    chat.room.as_ref().filter(|room| room.older_loading)?;
    let row = div()
        .id("chat-load-older")
        .flex()
        .justify_center()
        .p_2()
        .text_color(theme.muted)
        .child(words::LOADING_OLDER);
    Some(row.into_any_element())
}

fn grid(
    chat: &Chat,
    pane: Pane,
    messages: &Rc<[ChatMessage]>,
    lead: bool,
    state: &ListState,
    cx: &mut Context<Chat>,
) -> (Stateful<Div>, Option<String>, usize) {
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
    .cells(cell, usize::MAX)
    .on_move(cx.processor(move |chat, index: usize, _, cx| {
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
        answering,
        busy,
        held_draft,
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
                let after = at - messages.len();
                if bare && after == 0 {
                    return no_replies(&theme).into_any_element();
                }
                return match answering.get(after - usize::from(bare)) {
                    Some(draft) => {
                        let seen: &Chat = chat;
                        let held = held_draft.as_ref().filter(|h| h.principal == draft.author);
                        let held = held.map(|holder| {
                            let said = crate::words::since(holder.height);
                            super::answering::hold(seen, pane, said, (&theme, &ladder), cx)
                        });
                        let face = message::still_face(seen, &draft.author, cx, &theme);
                        let frame = |row: Stateful<Div>| row.px_4();
                        let draw = |key: &str, blocks: &[chat::Block]| {
                            message::drawn(seen, key, blocks, cx, &theme)
                        };
                        let look = (&theme, &ladder);
                        super::answering::row(seen, draft, look, frame, face, draw, held)
                    }
                    None => match &busy {
                        Some(holder) => {
                            let frame = |row: Stateful<Div>| row.px_4();
                            super::answering::busy(chat, pane, holder, (&theme, &ladder), frame, cx)
                        }
                        None => div().into_any_element(),
                    },
                };
            };
            let day = new_day(&messages, at, chat.today);
            let unread = unread == Some(message.seq);
            let active = (active_id.as_ref() == Some(&message.id)).then_some(cell);
            let set = (at + 1, messages.len());
            let deps = message::RowDeps {
                seen: message::Seen::of(chat, pane, &message),
                message: message.clone(),
                pane,
                active,
                set,
                markers: (day.clone(), unread),
                shared: shared.clone(),
                ladder,
                face: message
                    .from
                    .account()
                    .map(|author| chat.faces.dep(author, cx)),
            };
            let id = message.id.clone();
            let row = format!("chat-message-{id}-drawn");
            let drawn = cx.memo(row, deps, move |chat, _, cx| {
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
            });
            let drawn = chat.keyed(&id, drawn.into_any_element());
            crate::listing::edged(chat, pane, at, &id, drawn, cx)
        }),
    )
    .with_sizing_behavior(ListSizingBehavior::Auto)
    .flex_1()
    .min_h(px(0.))
    .w_full();
    grid.flex().flex_col().flex_1().min_h(px(0.)).child(list)
}

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
    let latest = cx.listener(move |chat, _: &ClickEvent, _, cx| chat.jump_to_newest(cx));
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

fn no_replies(theme: &Theme) -> impl IntoElement {
    div()
        .id("chat-thread-no-replies")
        .px_4()
        .py_3()
        .text_size(design::text::SECONDARY)
        .text_color(theme.muted)
        .child(words::NO_REPLIES)
}
