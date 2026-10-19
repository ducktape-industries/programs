//! A room (Expo `app/(tabs)/chat/[channel]/index.tsx`, boards 20-22): the
//! title row names it (a channel by `#name` over its members and policy; a
//! direct room by the other person's face and name), its info button opens
//! Channel details or the person's profile; under it the room's notice,
//! the timeline, and the composer or the strip saying why the reader may
//! not write. Reads the open room, its members and the write gate; writes
//! what its controls do (details, the profile link) and, through the
//! composer, the room's draft.
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
        // a direct room's title is the person: their face beside the name
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

/// A channel's caption: how many members it seats, and who may post.
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

/// The title row's info button: a direct room's opens the other person's
/// profile, a channel's its details.
fn info(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let glyph = design::icon("info", ladder.icon);
    let peer = chat
        .my_account()
        .and_then(|me| crate::names::dm_peer_of(me, &chat.room_id()));
    match peer {
        Some(peer) => design::icon_button(
            "chat-room-details",
            glyph,
            "Profile",
            ladder,
            theme,
            cx.listener(move |chat, _: &ClickEvent, _window, cx| {
                chat.open_link(peer.to_string(), cx);
            }),
        )
        .into_any_element(),
        None => design::icon_button(
            "chat-room-details",
            glyph,
            words::CHANNEL_DETAILS,
            ladder,
            theme,
            cx.listener(|chat, _: &ClickEvent, _window, cx| {
                chat.toggle_details(cx);
                cx.notify();
            }),
        )
        .into_any_element(),
    }
}
