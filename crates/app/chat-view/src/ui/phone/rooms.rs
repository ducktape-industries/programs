//! The rooms, the chat's first page (Expo `app/(tabs)/chat/index.tsx`,
//! boards 10-12): "Chat" over the network and the reader, Search and New
//! channel in its title row; then the channels, the direct rooms already
//! open and the people not yet written to, each section with its count,
//! each unread room with its dot. Reads the channel list, the roster and
//! the read cursors; a row opens its room, the title row's buttons open
//! the Search page and the new-channel form.
use view::prelude::*;
use view::{Div, Stateful};

use super::parts::{face, quiet};
use crate::Chat;
use crate::room_list::DmEntry;
use crate::words;
use chat::{ChannelInfo, Principal};
use view::design::Ladder;

/// The unread dot (mobile `src/kit/Badge.tsx:31`).
const DOT: Pixels = px(8.);

pub(super) fn screen(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let search = cx.listener(|chat, _: &ClickEvent, window, cx| {
        chat.open_search();
        window.focus("search");
        cx.notify();
    });
    let create = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.begin_create();
        cx.notify();
    });
    let page = design::page(
        "chat-rooms-page",
        words::CHAT,
        design::Nav::root(chat.reader_name()),
        ladder,
        theme,
    )
    .trailing(design::icon_button(
        "chat-rooms-search",
        design::icon("search", ladder.icon),
        "Search",
        ladder,
        theme,
        search,
    ))
    .trailing(design::icon_button(
        "chat-rooms-new-channel",
        design::icon("plus", ladder.icon),
        words::NEW_CHANNEL,
        ladder,
        theme,
        create,
    ));
    if !chat.session.connected {
        return page
            .child(quiet(
                "chat-rooms-offline",
                "Not connected to a node.",
                theme,
                ladder,
            ))
            .into_any_element();
    }
    let page = page.caption(caption(chat));
    match chat.channels.failed() {
        Some(refusal) => {
            let retry = cx.listener(|chat, _: &ClickEvent, _window, cx| {
                cx.notify();
                chat.load_channels(cx);
            });
            page.child(design::refused(
                "chat-rooms",
                design::sentence(refusal),
                ladder,
                theme,
                retry,
            ))
        }
        None => page.child(rooms(chat, cx, theme, ladder)),
    }
    .into_any_element()
}

/// The title's caption: the network, then the reader's name, the one the
/// avatar reads (Expo `chat/index.tsx:116`, `[network, myName]`).
fn caption(chat: &Chat) -> String {
    [chat.network().to_owned(), chat.reader_name()]
        .into_iter()
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The three sections: Channels always (its loading line or empty state
/// under it), then Direct messages and People where they have rows.
fn rooms(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> Stateful<Div> {
    let list = chat.room_list();
    let opened = list.opened();
    let (dms, people) = list.dms.split_at(opened);
    let count = |rows: usize| (rows > 0).then(|| rows.to_string());
    let loading = matches!(chat.channels, Loadable::Idle | Loadable::Loading(_));
    // with every channel archived the network still holds channels: the
    // empty state's words would be false
    let empty = (!loading && list.channels.is_empty() && list.archived == 0)
        .then(|| no_rooms(chat, cx, theme, ladder));
    div()
        .id("chat-rooms")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .child(
            design::section_head("chat-rooms-channels-head", words::CHANNELS, ladder, theme)
                .children(count(list.channels.len())),
        )
        .children(
            list.channels
                .iter()
                .map(|info| channel_row(chat, info, cx, theme, ladder)),
        )
        .when(loading, |rooms| {
            rooms.child(quiet(
                "chat-rooms-loading",
                words::LOADING_ROOMS,
                theme,
                ladder,
            ))
        })
        .children(empty)
        .when(!dms.is_empty(), |rooms| {
            rooms.child(
                design::section_head("chat-rooms-dms-head", words::DIRECT_MESSAGES, ladder, theme)
                    .children(count(dms.len())),
            )
        })
        .children(
            dms.iter()
                .map(|dm| person_row(chat, dm, "dm", cx, theme, ladder)),
        )
        .when(!people.is_empty(), |rooms| {
            rooms.child(
                design::section_head("chat-rooms-people-head", words::PEOPLE, ladder, theme)
                    .children(count(people.len())),
            )
        })
        .children(
            people
                .iter()
                .map(|person| person_row(chat, person, "person", cx, theme, ladder)),
        )
}

// A channel's row: its hash muted in the lead column, the name, Members
// only, and the dot while it holds what the reader has not read.
fn channel_row(
    chat: &Chat,
    info: &ChannelInfo,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let channel = &info.channel;
    let id = format!("chat-rooms-channel-{}", channel.id);
    let hash = design::icon("hash", ladder.icon_row);
    let tail = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(design::space::SM)
        .when(channel.members_only(), |tail| {
            tail.child(design::badge(
                format!("{id}-members-only"),
                words::MEMBERS_ONLY,
                ladder,
                theme.muted,
                theme.surface_raised,
            ))
        })
        .when(chat.unread(info), |tail| tail.child(dot(&id, theme)));
    row(
        &id,
        channel.name.clone(),
        channel.id.clone(),
        cx,
        theme,
        ladder,
    )
    .child(design::lead(hash.text_color(theme.muted)))
    .child(title(channel.name.clone(), theme, ladder))
    .child(tail)
    .into_any_element()
}

/// A person's row: their face and name; an open direct room's also wears
/// Agent for an agent and the unread dot. `kind` names the section in the
/// row's id (`dm`, `person`).
fn person_row(
    chat: &Chat,
    entry: &DmEntry,
    kind: &str,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let peer = Principal::Account(entry.peer);
    let names = chat.names.ready();
    let name = names.map_or_else(|| words::account(entry.peer), |n| n.member(&peer));
    let agent = names.is_some_and(|names| crate::message::agent(names, &peer));
    let id = format!("chat-rooms-{kind}-{}", entry.peer);
    let unread = entry.info.is_some_and(|info| chat.unread(info));
    let tail = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(design::space::SM)
        .when(agent && entry.info.is_some(), |tail| {
            tail.child(design::badge(
                format!("{id}-agent"),
                words::AGENT,
                ladder,
                theme.agent,
                theme.agent_soft,
            ))
        })
        .when(unread, |tail| tail.child(dot(&id, theme)));
    row(&id, name.clone(), entry.id.clone(), cx, theme, ladder)
        .when(agent, |row| row.aria_description(words::AGENT))
        .child(design::lead(face(&name, agent, ladder.avatar, theme)))
        .child(title(name, theme, ladder))
        .child(tail)
        .into_any_element()
}

/// The kit `Row` as a room's: the whole row opens `room`.
fn row(
    id: &str,
    name: String,
    room: String,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let open = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.choose(room.clone(), cx)
    });
    design::row(id.to_owned(), ladder, theme)
        .role(Role::Button)
        .aria_label(name)
        .focusable()
        .on_click(open)
}

// A row's title (the kit `Row`'s): body text on one line.
fn title(name: String, theme: &Theme, ladder: &Ladder) -> Div {
    div()
        .flex_1()
        .min_w(px(0.))
        .truncate()
        .text_type(ladder.body)
        .text_color(theme.foreground)
        .child(name)
}

// SDK gap: the kit Dot, kept local until the kit takes it (kit.md §2)
/// The unread dot (the kit `Dot`): the accent, never a count.
fn dot(row: &str, theme: &Theme) -> Stateful<Div> {
    div()
        .id(format!("{row}-unread"))
        .size(DOT)
        .flex_none()
        .rounded_full()
        .bg(theme.accent)
}

/// No channel on the network yet (the kit `EmptyState`, mobile
/// `app/(tabs)/chat/index.tsx:142-148`): what is missing, what would fill
/// it, and New channel.
fn no_rooms(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> Stateful<Div> {
    let create = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.begin_create();
        cx.notify();
    });
    let network = match chat.network() {
        "" => "this network",
        network => network,
    };
    let detail = format!(
        "Nobody has created a channel on {network}. Open one, or message a person from the list below."
    );
    let create = design::button(
        "chat-rooms-empty-new-channel",
        words::NEW_CHANNEL,
        ladder,
        theme,
        create,
    )
    .kind(design::Kind::Outline)
    .small();
    design::empty_state(
        "chat-rooms-empty",
        "message-square",
        "No rooms yet",
        detail,
        ladder,
        theme,
    )
    .child(div().mt(design::space::SM).child(create))
}
