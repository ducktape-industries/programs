//! A room (Expo `app/(tabs)/chat/[channel]/index.tsx`, boards 20-22): its title row (a channel's
//! `#name` over its members and policy, a direct room's person), whose info button opens the room's
//! details; under it the room's notice, the timeline, and the composer or the strip saying why not.
use view::prelude::*;

use super::{composer, parts, timeline};
use crate::composer::Target;
use crate::words;
use crate::{Chat, Pane, Room};
use view::design::Ladder;

pub(super) fn screen(
    chat: &Chat,
    room: &Room,
    back: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let page = design::page(
        "chat-room-page",
        chat.room_name(&room.id),
        design::Nav::back(chat.pages().depth(), back),
        ladder,
        theme,
    )
    .trailing(info(chat, cx, theme, ladder));
    let picture = chat.dm_peer_picture(cx);
    let page = match chat.dm_peer() {
        Some((peer, agent)) => page.leading(
            parts::face(&peer, agent, picture, ladder.avatar_inline, theme).id("chat-room-avatar"),
        ),
        None => match caption(chat, room) {
            Some(caption) => page.caption(caption),
            None => page,
        },
    };
    page.children(parts::room_notice(chat, cx, theme, ladder))
        .children(crate::ui::widget::line(chat, theme, ladder))
        .children(crate::ui::widget::pinned(chat, cx, theme, ladder))
        .child(timeline::list(chat, Pane::Timeline, cx, theme, ladder))
        .children(
            crate::ui::rules::line(chat, cx, theme, ladder)
                .map(|line| div().px(design::space::BLOCK).child(line)),
        )
        .child(composer::band(
            chat,
            Pane::Timeline,
            Target::Post {
                channel: room.id.clone(),
                thread: None,
            },
            &chat.composer_hint(room),
            cx,
            theme,
            ladder,
        ))
        .into_any_element()
}

fn caption(chat: &Chat, room: &Room) -> Option<String> {
    let channel = &chat.info(&room.id)?.channel;
    let policy = words::policy(channel);
    let seated = room.members.ready().map_or(0, Vec::len);
    Some(match seated {
        0 => policy.to_owned(),
        seated => format!(
            "{} · {policy}",
            design::plural(seated as u64, "member", "members")
        ),
    })
}

fn info(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let glyph = design::icon("info", ladder.icon);
    let label = match chat::dm_peers(&chat.room_id()) {
        Some(_) => words::CONVERSATION_DETAILS,
        None => words::CHANNEL_DETAILS,
    };
    design::icon_button(
        "chat-room-details",
        glyph,
        label,
        ladder,
        theme,
        cx.listener(|chat, _: &ClickEvent, _window, cx| {
            chat.toggle_details(cx);
            cx.notify();
        }),
    )
    .into_any_element()
}
