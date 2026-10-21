//! A thread (Expo `app/(tabs)/chat/[channel]/thread/[root].tsx`, boards
//! 50/51): "Thread" over the channel's name; the root with its byline and
//! reactions, how many replies it has, the replies in post order; and the
//! composer that replies in the thread, or the strip saying why the reader
//! may not write. Reads the open thread and the room's notice and gate;
//! writes, through the composer, the thread's draft.
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
        "chat-thread-page",
        words::THREAD,
        design::Nav::back(chat.pages().depth(), back),
        ladder,
        theme,
    );
    let page = match chat.info(&room.id) {
        Some(info) => page.caption(format!("#{}", info.channel.name)),
        None => page,
    };
    let Some(root) = room.thread.as_ref().map(|thread| thread.root) else {
        return page.into_any_element();
    };
    page.children(parts::names_notice(chat, theme, ladder))
        .children(parts::room_notice(chat, cx, theme, ladder))
        .child(timeline::list(chat, Pane::Thread, cx, theme, ladder))
        .child(composer::band(
            chat,
            Pane::Thread,
            Target::Post {
                channel: room.id.clone(),
                thread: Some(root),
            },
            words::REPLY_IN_THREAD,
            cx,
            theme,
            ladder,
        ))
        .into_any_element()
}
