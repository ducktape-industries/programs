//! A pane's messages (Expo `chat/[channel]/index.tsx` `Timeline`, `thread/[root].tsx`): the room's
//! newest at the foot, a day's separator, "New" over the first unread, "Latest"; a thread's root
//! over its reply count, then its replies. The first row in view pages older in (`listing::edged`).
use view::list as gpui_list;
use view::prelude::*;

use super::message;
use super::parts::{self, HAIRLINE, quiet};
use crate::listing::{Listing, listing};
use crate::message::{ChatMessage, day_of, new_day};
use crate::words;
use crate::{Chat, Pane};
use view::FontWeight;
use view::design::Ladder;

pub(super) fn list(
    chat: &Chat,
    pane: Pane,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let frame = div()
        .id(match pane {
            Pane::Timeline => "chat-timeline",
            Pane::Thread => "chat-thread-messages",
        })
        .relative()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col();
    let Some(room) = &chat.room else {
        return frame.into_any_element();
    };
    let read = match pane {
        Pane::Timeline => Some(&room.messages),
        Pane::Thread => room.thread.as_ref().map(|thread| &thread.replies),
    };
    let loading = match pane {
        Pane::Timeline => words::LOADING_MESSAGES,
        Pane::Thread => "Loading the thread…",
    };
    if read.is_some_and(|read| matches!(read, Loadable::Loading(_))) {
        return frame
            .child(quiet("chat-messages-loading", loading, theme, ladder))
            .into_any_element();
    }
    if let Some(refusal) = read.and_then(Loadable::failed) {
        let retry = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
            cx.notify();
            chat.retry_read(pane, cx)
        });
        let refused = design::refused(
            "chat-messages",
            design::sentence(refusal),
            ladder,
            theme,
            retry,
        );
        return frame.child(refused).into_any_element();
    }
    let timeline = pane == Pane::Timeline;
    frame
        .when(timeline && room.older_loading, |frame| {
            frame.child(quiet(
                "chat-load-older",
                words::LOADING_OLDER,
                theme,
                ladder,
            ))
        })
        .when_some(
            room.older_failed
                .as_deref()
                .filter(|_| timeline && !room.older_loading),
            |frame, why| {
                frame.child(quiet(
                    "chat-older-failed",
                    words::unreadable_room(why),
                    theme,
                    ladder,
                ))
            },
        )
        .child(rows(chat, pane, cx, theme, ladder))
        .when(timeline && (room.landed || !room.at_tail), |frame| {
            frame.child(latest(room.id.clone(), cx, theme, ladder))
        })
        .into_any_element()
}

fn rows(
    chat: &Chat,
    pane: Pane,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement {
    let messages = chat.messages(pane);
    let Listing {
        shape,
        intro,
        bare,
        state,
        unread,
        answering,
    } = listing(chat, pane, &messages, cx);
    let lead = usize::from(intro.is_some());
    let (theme, ladder) = (*theme, *ladder);
    gpui_list(
        shape.id(),
        state,
        cx.processor(move |chat, index: usize, _window, cx| {
            if let Some(intro) = intro.as_ref().filter(|_| index == 0) {
                return quiet("chat-timeline-intro", intro.clone(), &theme, &ladder)
                    .into_any_element();
            }
            let at = index - lead;
            let Some(message) = messages.get(at) else {
                let after = at - messages.len();
                if bare && after == 0 {
                    return quiet("chat-thread-no-replies", words::NO_REPLIES, &theme, &ladder)
                        .into_any_element();
                }
                return match answering.get(after - usize::from(bare)) {
                    Some(draft) => {
                        let seen: &Chat = chat;
                        crate::ui::answering::row(seen, draft, &theme, &ladder, |key, blocks| {
                            super::rich::drawn(seen, key, blocks, cx, &theme, &ladder)
                        })
                    }
                    None => div().into_any_element(),
                };
            };
            let marks = match pane {
                Pane::Timeline => marks(chat, &messages, at, unread, &theme, &ladder),
                Pane::Thread => Vec::new(),
            };
            let count =
                (pane == Pane::Thread && at == 0).then(|| replies(message, &theme, &ladder));
            let set = (at + 1, messages.len());
            let row = message::row(chat, message, pane, set, cx, &theme, &ladder);
            let row = match marks.is_empty() && count.is_none() {
                true => row,
                false => div()
                    .id(format!("chat-message-{}-marked", message.id))
                    .w_full()
                    .flex()
                    .flex_col()
                    .children(marks)
                    .child(row)
                    .children(count)
                    .into_any_element(),
            };
            crate::listing::edged(chat, pane, at, &message.id, row, cx)
        }),
    )
    .with_sizing_behavior(ListSizingBehavior::Auto)
    .map(|list| match pane {
        Pane::Timeline => list.py(design::space::SM),
        Pane::Thread => list.pb(design::space::LG),
    })
    .flex_1()
    .min_h(px(0.))
    .w_full()
}

fn marks(
    chat: &Chat,
    messages: &[ChatMessage],
    at: usize,
    unread: Option<u64>,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    let message = &messages[at];
    let day = new_day(messages, at).map(|day| {
        let today = chat.today == Some(day_of(message.time));
        let label = if today { "Today".to_owned() } else { day };
        separator(
            format!("chat-day-{}", message.id),
            label,
            false,
            theme,
            ladder,
        )
    });
    let new = (unread == Some(message.seq)).then(|| {
        separator(
            "chat-unread-marker".into(),
            "New".into(),
            true,
            theme,
            ladder,
        )
    });
    day.into_iter().chain(new).collect()
}

fn separator(
    id: String,
    label: String,
    danger: bool,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let tone = match danger {
        true => theme.danger,
        false => theme.muted,
    };
    let rule = || {
        div()
            .h(HAIRLINE)
            .flex_1()
            .bg(if danger { theme.danger } else { theme.border })
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(design::space::LG)
        .px(design::space::BLOCK)
        .py(design::space::SM)
        .text_type(ladder.caption)
        .text_color(tone)
        .child(rule())
        .child(label)
        .child(rule())
        .into_any_element()
}

fn replies(root: &ChatMessage, theme: &Theme, ladder: &Ladder) -> AnyElement {
    div()
        .id(format!("chat-message-{}-reply-separator", root.id))
        .flex()
        .items_center()
        .px(design::space::BLOCK)
        .pt(design::space::LG)
        .pb(design::space::XS)
        .border_b_1()
        .border_color(theme.border)
        .text_type(ladder.caption)
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.muted)
        .child(design::plural(root.reply_count, "reply", "replies"))
        .into_any_element()
}

fn latest(room: String, cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let open = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.open(room.clone(), cx)
    });
    div()
        .absolute()
        .left_0()
        .right_0()
        .bottom(design::space::LG)
        .flex()
        .justify_center()
        .child(parts::small_button(
            "chat-jump-latest-button",
            "arrow-down",
            "Latest",
            theme,
            ladder,
            open,
        ))
        .into_any_element()
}
