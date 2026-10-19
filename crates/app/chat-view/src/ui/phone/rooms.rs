//! The rooms, the chat's first page (Expo `app/(tabs)/chat/index.tsx`, boards 10-12): the title
//! row, the channels the reader is in, in their folders, then their direct rooms newest first; a
//! folder's row opens or closes it.
use view::prelude::*;
use view::{Div, Stateful};

use super::parts::{face, quiet};
use crate::Chat;
use crate::room_list::{DmEntry, FolderLine, Line, label};
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
    let reader = chat.my_account().and_then(|me| chat.faces.of(me, cx));
    let page = design::page(
        "chat-rooms-page",
        words::CHAT,
        design::Nav::root_with(chat.reader_name(), reader),
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

// The channels the reader is in (their loading line, the empty state, or where the rest are, under
// them), then Direct messages always.
fn rooms(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> Stateful<Div> {
    let list = chat.room_list();
    let count = |rows: usize| (rows > 0).then(|| rows.to_string());
    let loading = matches!(chat.channels, Loadable::Idle | Loadable::Loading(_));
    // the empty state says the network holds no channel: keyed on every room, the ones left out
    // of the list too
    let none = list.channels.is_empty() && list.left_out == 0;
    let empty = (!loading && none).then(|| no_rooms(chat, cx, theme, ladder));
    let elsewhere = (list.channels.is_empty() && list.left_out > 0)
        .then(|| quiet("chat-rooms-none-held", words::NONE_HELD, theme, ladder));
    let mine = lines(chat, &list.channels, cx, theme, ladder);
    let mut dms = Vec::new();
    for dm in &list.dms {
        dms.push(dm_row(chat, dm, cx, theme, ladder));
    }
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
        .children(mine)
        .when(loading, |rooms| {
            rooms.child(quiet(
                "chat-rooms-loading",
                words::LOADING_ROOMS,
                theme,
                ladder,
            ))
        })
        .children(empty)
        .children(elsewhere)
        .child(
            design::section_head("chat-rooms-dms-head", words::DIRECT_MESSAGES, ladder, theme)
                .children(count(list.dms.len()))
                .child(new_message(cx, theme, ladder)),
        )
        .children(dms)
}

// `channels` as rows, each under the folders its name gives.
fn lines(
    chat: &Chat,
    channels: &[&ChannelInfo],
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    let lines = chat.lines(channels).into_iter();
    lines
        .map(|line| match line {
            Line::Room { info, depth } => channel_row(chat, info, depth, cx, theme, ladder),
            Line::Folder(folder) => folder_row(chat, &folder, cx, theme, ladder),
        })
        .collect()
}

// New message at the Direct messages head: the page of whom to write to.
fn new_message(cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> impl IntoElement {
    let open = cx.listener(|chat, _: &ClickEvent, window, cx| {
        chat.begin_dm();
        window.focus(crate::ui::dialogs::DM_PICK_FIELD);
        cx.notify();
    });
    design::icon_button(
        "chat-rooms-new-dm",
        design::icon("plus", ladder.icon),
        words::NEW_MESSAGE,
        ladder,
        theme,
        open,
    )
}

// A channel's row: its hash muted in the lead column, the name, Members
// only, and the dot while it holds what the reader has not read.
fn channel_row(
    chat: &Chat,
    info: &ChannelInfo,
    depth: usize,
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
    let name = label(&channel.name).to_owned();
    row(&id, name.clone(), channel.id.clone(), cx, theme, ladder)
        .pl(indent(depth))
        .child(design::lead(hash.text_color(theme.muted)))
        .child(title(name, theme, ladder))
        .child(tail)
        .into_any_element()
}

// Where a row `depth` folders deep starts: a lead column further each.
fn indent(depth: usize) -> Pixels {
    design::space::BLOCK + design::space::XL * depth as f32
}

// A folder's row: its chevron, its name, and while it is closed how many
// rooms inside are unread; a press opens or closes it. A folder named as a
// channel opens that channel, and its chevron opens or closes the folder.
fn folder_row(
    chat: &Chat,
    folder: &FolderLine,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let id = format!("chat-rooms-folder-{}", folder.path);
    let path = folder.path.clone();
    let toggle = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
        chat.toggle_folder(&path, cx);
        cx.notify();
    });
    let shape = if folder.open {
        "chevron-down"
    } else {
        "chevron-right"
    };
    let chevron = design::lead(design::icon(shape, ladder.icon_row).text_color(theme.muted));
    let unread = folder.own.is_some_and(|own| chat.unread(own));
    let tail = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(design::space::SM)
        .when(folder.unread > 0, |tail| {
            tail.child(design::badge(
                format!("{id}-count"),
                folder.unread.to_string(),
                ladder,
                theme.foreground,
                theme.accent_soft,
            ))
        })
        .when(unread, |tail| tail.child(dot(&id, theme)));
    let counted = (folder.unread > 0).then(|| words::unread_rooms(folder.unread));
    let line = design::row(id.clone(), ladder, theme).pl(indent(folder.depth));
    let name = title(folder.name.to_owned(), theme, ladder);
    let Some(own) = folder.own else {
        return line
            .role(Role::Button)
            .aria_label(words::folder(folder.name))
            .aria_expanded(folder.open)
            .when_some(counted, |line, counted| line.aria_description(counted))
            .focusable()
            .on_click(toggle)
            .child(chevron)
            .child(name)
            .child(tail)
            .into_any_element();
    };
    let room = own.channel.id.clone();
    let choose = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.choose(room.clone(), cx)
    });
    let fold = chevron
        .id(format!("{id}-fold"))
        .min_h(ladder.hit)
        .role(Role::Button)
        .aria_label(words::folder(folder.name))
        .aria_expanded(folder.open)
        .focusable()
        .on_click(toggle);
    let opens = div()
        .id(format!("{id}-open"))
        .flex_1()
        .min_w(px(0.))
        .min_h(ladder.hit)
        .flex()
        .items_center()
        .gap(design::space::LG)
        .role(Role::Button)
        .aria_label(folder.name.to_owned())
        .when_some(counted, |opens, counted| opens.aria_description(counted))
        .focusable()
        .on_click(choose)
        .child(name)
        .child(tail);
    line.child(fold).child(opens).into_any_element()
}

// A direct room's row: the other person's face and name, Agent for an
// agent, and the unread dot.
fn dm_row(
    chat: &Chat,
    entry: &DmEntry,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let peer = Principal::Account(entry.peer);
    let names = chat.names.ready();
    let name = names.map_or_else(|| words::account(entry.peer), |n| n.member(&peer));
    let agent = names.is_some_and(|names| crate::message::agent(names, &peer));
    let id = format!("chat-rooms-dm-{}", entry.peer);
    let unread = chat.unread(entry.info);
    let tail = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(design::space::SM)
        .when(agent, |tail| {
            tail.child(design::badge(
                format!("{id}-agent"),
                words::AGENT,
                ladder,
                theme.agent,
                theme.agent_soft,
            ))
        })
        .when(unread, |tail| tail.child(dot(&id, theme)));
    let room = entry.info.channel.id.clone();
    let picture = chat.faces.of(entry.peer, cx);
    row(&id, name.clone(), room, cx, theme, ladder)
        .when(agent, |row| row.aria_description(words::AGENT))
        .child(design::lead(face(
            &name,
            agent,
            picture,
            ladder.avatar,
            theme,
        )))
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
        "Nobody has created a channel on {network}. Open one, or start a direct message below."
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
