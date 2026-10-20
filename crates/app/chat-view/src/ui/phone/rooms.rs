//! The rooms, the chat's first page (Expo `app/(tabs)/chat/index.tsx`, boards 10-12): the title
//! row, the channels the reader is in, in their folders, then their direct rooms newest first; a
//! folder's row opens or closes it, and a long press lifts a channel or folder row to move it.
use view::prelude::*;
use view::wire::{Action, ActionData};
use view::{Div, Stateful};

use super::parts::{face, quiet};
use crate::Chat;
use crate::room_list::{DmEntry, FolderLine, Line, label};
use crate::ui::sidebar::{lands, placed};
use crate::words;
use chat::{ChannelInfo, Entry, Principal};
use view::design::Ladder;

const DOT: Pixels = px(8.);
const MOVE_UP: i32 = 1;
const MOVE_DOWN: i32 = 2;

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
    let lines = chat.lines(&list.channels);
    // a long press lifts the channel or folder row under it; the finger dragged onto another row
    // of its level and lifted, it takes that row's place
    let drawn: std::rc::Rc<[Entry]> = lines.iter().map(|line| line.spot().entry()).collect();
    let (held, over) = (drawn.clone(), drawn);
    let lift = cx.listener(move |chat, event: &ClickEvent, _, cx| {
        chat.hold_row(event.position(), &held);
        cx.notify();
    });
    let drag = cx.listener(move |chat, event: &MouseMoveEvent, _, cx| {
        chat.drag_row(event.position, &over, cx)
    });
    let land = cx.listener(|chat, _: &MouseUpEvent, _, cx| chat.drop_row(cx));
    let mine = rows(chat, lines, cx, theme, ladder);
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
        .role(Role::Group)
        .aria_label(words::ROOMS)
        .on_aux_click(lift)
        .on_mouse_move(drag)
        .on_mouse_up(MouseButton::Left, land)
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

fn rows(
    chat: &Chat,
    lines: Vec<Line<'_>>,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    lines
        .into_iter()
        .map(|line| match line {
            Line::Room { info, depth } => channel_row(chat, info, depth, cx, theme, ladder),
            Line::Folder(folder) => folder_row(chat, &folder, cx, theme, ladder),
        })
        .collect()
}

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
    let entry = Entry::Room(channel.id.clone());
    let row = row(&id, name.clone(), channel.id.clone(), cx, theme, ladder);
    let row = movable(row, entry.clone(), chat, cx, theme);
    steps(row, entry, chat, cx)
        .pl(indent(depth))
        .child(design::lead(hash.text_color(theme.muted)))
        .child(title(name, theme, ladder))
        .child(tail)
        .into_any_element()
}

fn indent(depth: usize) -> Pixels {
    design::space::BLOCK + design::space::XL * depth as f32
}

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
    let entry = Entry::Folder(folder.path.clone());
    let line = design::row(id.clone(), ladder, theme).pl(indent(folder.depth));
    let line = movable(line, entry.clone(), chat, cx, theme);
    let name = title(folder.name.to_owned(), theme, ladder);
    let Some(own) = folder.own else {
        return steps(line, entry, chat, cx)
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
    let opens = steps(opens, entry, chat, cx);
    line.child(fold).child(opens).into_any_element()
}

// A channel or folder row the reader moves: where layout put it, raised while lifted, and an accent
// edge on the side a lifted row lands on it.
fn movable(
    row: Stateful<Div>,
    entry: Entry,
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> Stateful<Div> {
    let lifted = chat.drag.as_ref().is_some_and(|drag| drag.entry == entry);
    let landing = chat.landing(&entry);
    placed(row, entry, cx)
        .when(lifted, |row| row.bg(theme.accent_soft))
        .when_some(landing, |row, after| lands(row, after, theme))
}

// A screen reader's way to move `entry`: Move up and Move down, where its level has room.
fn steps(row: Stateful<Div>, entry: Entry, chat: &Chat, cx: &mut Context<Chat>) -> Stateful<Div> {
    let (up, down) = chat.steps(&entry);
    if !up && !down {
        return row;
    }
    let chat = cx.weak_entity();
    row.when(up, |row| row.custom_action(MOVE_UP, words::MOVE_UP))
        .when(down, |row| row.custom_action(MOVE_DOWN, words::MOVE_DOWN))
        .on_a11y_action(
            Action::CustomAction,
            move |data: Option<&ActionData>, _: &mut Window, app: &mut App| {
                let up = match data {
                    Some(ActionData::CustomAction(MOVE_UP)) => true,
                    Some(ActionData::CustomAction(MOVE_DOWN)) => false,
                    _ => return,
                };
                let _ = chat.update(app, |chat, cx| chat.step(&entry, up, cx));
            },
        )
}

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

fn title(name: String, theme: &Theme, ladder: &Ladder) -> Div {
    div()
        .flex_1()
        .min_w(px(0.))
        .truncate()
        .text_type(ladder.body)
        .text_color(theme.foreground)
        .child(name)
}

fn dot(row: &str, theme: &Theme) -> Stateful<Div> {
    div()
        .id(format!("{row}-unread"))
        .size(DOT)
        .flex_none()
        .rounded_full()
        .bg(theme.accent)
}

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
