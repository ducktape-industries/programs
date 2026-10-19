//! The channel and direct-message pane, authored as native GPUI elements.

use view::prelude::*;
use view::{Div, Stateful};

use chat::{ChannelInfo, Principal};

use crate::words;
use crate::{ChannelCreate, Chat};
use view::design::Ladder;

pub fn render(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement {
    div()
        .id("chat-sidebar")
        .flex()
        .flex_col()
        .w(px(chat.layout.clamped().sidebar))
        .h_full()
        .bg(theme.sidebar)
        .text_color(theme.sidebar_foreground)
        .child(
            div()
                .id("chat-sidebar-search-row")
                .flex()
                .items_center()
                .gap_1()
                .p_2()
                .child(search(chat, cx, theme, ladder)),
        )
        .child(rooms(chat, cx, theme, ladder))
}

/// The message search field, and its clear button once it holds a search.
fn search(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> impl IntoElement {
    let submit = cx.listener(|chat, _: &(), _window, cx| {
        cx.notify();
        chat.search_submit(cx)
    });
    let input = Input::new(
        "chat-sidebar-search",
        &chat.search.draft,
        words::SEARCH_MESSAGES,
    )
    .h(design::size::CONTROL)
    .flex_1()
    .px_2()
    .py_1()
    .border_1()
    .border_color(theme.sidebar_border)
    .bg(theme.sidebar_raised)
    .text_color(theme.sidebar_foreground)
    .placeholder("Search messages…")
    .on_submit(submit);
    let searching = !chat.search.query.is_empty() || !chat.search.draft.is_blank();
    let clear = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.search_clear();
        cx.notify();
    });
    div()
        .flex()
        .items_center()
        .gap_1()
        .flex_1()
        .child(input)
        .when(searching, |el| {
            // the rail's ink at rest, as the field beside it
            el.child(
                design::icon_button(
                    "chat-sidebar-clear-search",
                    "✕",
                    "Clear search",
                    ladder,
                    theme,
                    clear,
                )
                .text_color(theme.sidebar_foreground),
            )
        })
}

/// A program's own rooms (`forge:…` review threads) are its to show.
fn rooms(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> impl IntoElement {
    let mut list = div()
        .id("chat-sidebar-rooms")
        .flex_1()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .child(section_header(
            "chat-sidebar-channels-header",
            words::CHANNELS,
            new_channel(chat, cx, theme),
            theme,
        ));
    let no_channels = chat
        .channels
        .ready()
        .is_none_or(|channels| channels.is_empty());
    if chat.channels.is_loading() && no_channels {
        list = list.child(quiet("chat-sidebar-loading", words::LOADING_ROOMS, theme));
    }
    if let Some(refusal) = chat.channels.failed() {
        list = list.child(quiet(
            "chat-sidebar-failed",
            design::sentence(refusal),
            theme,
        ));
    }
    let open = chat.room.as_ref().map(|room| room.id.as_str());
    let room_list = chat.sidebar_rooms();
    // one Tab stop under the header: ↑ ↓ walk the channels and the direct
    // messages, Enter opens the active room
    let ids = room_list.ids();
    let at = chat.rooms_at(&ids);
    let picked = ids.clone();
    // the Menu key or Shift+F10 opens the active room's menu, under its row
    let asked = ids.get(at).cloned();
    let keys = cx.listener(move |chat, event: &KeyDownEvent, window, cx| {
        if let Some(room) = asked
            .clone()
            .filter(|_| design::asks_for_menu(&event.keystroke))
        {
            chat.open_room_menu(room, None, window, cx)
        }
    });
    let mut box_ = design::composite("chat-sidebar-rooms-list", Role::ListBox, "Rooms")
        .active(at, ids.len())
        .on_move(cx.processor(move |chat, index: usize, _, cx| {
            chat.rooms_cursor = Some(ids[index].clone());
            cx.notify();
        }))
        .on_press(cx.processor(move |chat, index: usize, _, cx| {
            cx.notify();
            chat.choose(picked[index].clone(), cx)
        }))
        .build()
        .on_key_down(keys)
        .flex()
        .flex_col()
        .gap_1();
    let mut index = 0;
    for info in room_list.channels {
        let id = info.channel.id.as_str();
        let row = channel_row(chat, info, open == Some(id), index == at, cx, theme);
        box_ = box_.child(row);
        index += 1;
    }
    for section in room_list.sections {
        box_ = box_.child(section_title(
            format!("chat-sidebar-category-{}", section.category),
            section.category,
            theme,
        ));
        for info in section.channels {
            let id = info.channel.id.as_str();
            let row = channel_row(chat, info, open == Some(id), index == at, cx, theme);
            box_ = box_.child(row);
            index += 1;
        }
    }
    box_ = box_.child(section_header(
        "chat-sidebar-dm-header",
        words::DIRECT_MESSAGES,
        new_dm(cx, theme),
        theme,
    ));
    let names = std::rc::Rc::new(chat.names.ready().cloned());
    for crate::room_list::DmEntry { info, peer } in room_list.dms {
        let selected = open == Some(info.channel.id.as_str());
        let active = index == at;
        let room = &info.channel.id;
        let read = (chat.unread(info), super::room_menu::waits(chat, room));
        let deps = (info.clone(), peer, selected, active, read, names.clone());
        let (info, theme, ladder) = (info.clone(), *theme, *ladder);
        let row = cx.memo(
            format!("chat-sidebar-dm-{peer}"),
            deps,
            move |chat, _, cx| {
                dm_button(chat, &info, peer, selected, active, cx, &theme, &ladder)
                    .into_any_element()
            },
        );
        box_ = box_.child(row);
        index += 1;
    }
    list.child(box_)
}

/// "+ New channel", or "✕ Close" while the create dialog is open.
fn new_channel(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let toggle = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.create = match chat.create.take() {
            Some(_) => None,
            None => Some(ChannelCreate::default()),
        };
        cx.notify();
    });
    let label = if chat.create.is_some() {
        "✕ Close"
    } else {
        "+ New channel"
    };
    header_button("chat-sidebar-new-channel", label.to_owned(), toggle, theme)
}

fn new_dm(cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let open = cx.listener(|chat, _: &ClickEvent, window, cx| {
        chat.begin_dm();
        window.focus(super::dialogs::DM_PICK_FIELD);
        cx.notify();
    });
    header_button(
        "chat-sidebar-new-dm",
        format!("+ {}", words::NEW_MESSAGE),
        open,
        theme,
    )
}

fn header_button(
    id: &'static str,
    label: String,
    press: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    theme: &Theme,
) -> impl IntoElement {
    div()
        .id(id)
        // the smallest box a pointer presses, each way (the door's AX-017)
        .min_w(px(24.))
        .min_h(px(24.))
        .flex()
        .items_center()
        .px_1()
        .py_0p5()
        .cursor_pointer()
        .hover(|s| s.bg(theme.sidebar_raised))
        .role(view::Role::Button)
        .focusable()
        .on_click(press)
        .child(label)
}

fn section_header(
    id: impl Into<ElementId>,
    label: &str,
    control: impl IntoElement,
    theme: &Theme,
) -> impl IntoElement {
    section_title(id, label, theme).child(control)
}

fn section_title(id: impl Into<ElementId>, label: &str, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_1()
        .px_1()
        .py_1()
        .text_size(design::text::CAPTION)
        .text_color(theme.sidebar_muted)
        .child(div().flex_1().child(label.to_owned()))
}

fn quiet(id: impl Into<ElementId>, text: impl Into<String>, theme: &Theme) -> impl IntoElement {
    div()
        .id(id)
        .px_1()
        .py_1()
        .text_size(design::text::CAPTION)
        .text_color(theme.sidebar_muted)
        .child(text.into())
}

// A channel's row, drawn again only when what it shows moved.
fn channel_row(
    chat: &Chat,
    info: &ChannelInfo,
    selected: bool,
    active: bool,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> AnyElement {
    let room = &info.channel.id;
    let read = (chat.unread(info), super::room_menu::waits(chat, room));
    let deps = (info.clone(), selected, active, read);
    let (info, theme) = (info.clone(), *theme);
    let id = format!("chat-sidebar-channel-{}", info.channel.id);
    cx.memo(id, deps, move |chat, _, cx| {
        channel_button(chat, &info, selected, active, cx, &theme)
    })
    .into_any_element()
}

fn channel_button(
    chat: &Chat,
    info: &ChannelInfo,
    selected: bool,
    active: bool,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> AnyElement {
    let id = info.channel.id.clone();
    let click = cx.listener(move |chat, _: &ClickEvent, _, cx| {
        cx.notify();
        chat.choose(id.clone(), cx)
    });
    let unread = chat.unread(info) && !selected;
    let mut row = div()
        .id(format!("chat-sidebar-channel-{}", info.channel.id))
        .flex()
        .w_full()
        .items_center()
        .gap_1()
        .min_h(design::size::CONTROL)
        .px_1()
        .bg(if selected {
            theme.sidebar_raised
        } else {
            theme.sidebar
        })
        .cursor_pointer()
        .hover(|s| s.bg(theme.sidebar_raised))
        .when(active && !selected, |row| row.bg(theme.sidebar_raised))
        .aria_selected(selected)
        .on_click(click)
        .child(div().text_color(theme.sidebar_muted).child("#"))
        .child(
            div()
                .flex_1()
                .truncate()
                .text_color(if unread {
                    theme.sidebar_foreground
                } else {
                    theme.sidebar_muted
                })
                .child(info.channel.name.clone()),
        );
    if info.channel.members_only() {
        row = row.child(
            div()
                .text_size(design::text::CAPTION)
                .text_color(theme.sidebar_muted)
                .child(words::MEMBERS_ONLY),
        );
    }
    if unread {
        row = row.child(
            div()
                .id(format!("chat-sidebar-channel-{}-unread", info.channel.id))
                .size_2()
                .rounded_full()
                .bg(theme.accent),
        );
    }
    let row = super::room_menu::row(row, &info.channel.id, chat, cx);
    design::item(row, Role::ListBoxOption, active).into_any_element()
}

// A direct message's row: the room `info` with `peer`.
#[allow(clippy::too_many_arguments)]
fn dm_button(
    chat: &Chat,
    info: &ChannelInfo,
    peer: u64,
    selected: bool,
    active: bool,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement {
    let names = chat.names.ready();
    let name = names.map_or_else(
        || words::account(peer),
        |n| n.member(&Principal::Account(peer)),
    );
    let agent = names.is_some_and(|n| crate::message::agent(n, &Principal::Account(peer)));
    let unread = chat.unread(info) && !selected;
    let id = info.channel.id.clone();
    let room = id.clone();
    let click = cx.listener(move |chat, _: &ClickEvent, _, cx| {
        cx.notify();
        chat.choose(id.clone(), cx)
    });
    let mut row = div()
        .id(format!("chat-sidebar-dm-{peer}"))
        .flex()
        .items_center()
        .gap_1()
        .min_h(design::size::CONTROL)
        .px_1()
        .bg(if selected {
            theme.sidebar_raised
        } else {
            theme.sidebar
        })
        .cursor_pointer()
        .hover(|s| s.bg(theme.sidebar_raised))
        .when(active && !selected, |row| row.bg(theme.sidebar_raised))
        // the peer's name, not the avatar's initial drawn before it
        .aria_label(name.clone())
        .when(agent, |row| row.aria_description(words::AGENT))
        .aria_selected(selected)
        .on_click(click)
        .child(avatar(
            format!("chat-sidebar-dm-{peer}-avatar"),
            &name,
            agent,
            theme.sidebar_raised,
            theme,
        ))
        .child(
            div()
                .flex_1()
                .truncate()
                .text_color(if unread {
                    theme.sidebar_foreground
                } else {
                    theme.sidebar_muted
                })
                .child(name),
        );
    if agent {
        row = row.child(super::badge(
            format!("chat-sidebar-dm-{peer}-agent"),
            words::AGENT,
            ladder,
            theme.agent,
            theme.agent_soft,
        ));
    }
    if unread {
        row = row.child(
            div()
                .id(format!("chat-sidebar-dm-{peer}-unread"))
                .size_2()
                .rounded_full()
                .bg(theme.accent),
        );
    }
    let row = super::room_menu::row(row, &room, chat, cx);
    design::item(row, Role::ListBoxOption, active)
}

/// A person's round initials: a direct room's face, in the sidebar and
/// over the room.
pub fn avatar(
    id: impl Into<ElementId>,
    name: &str,
    agent: bool,
    fill: view::Hsla,
    theme: &Theme,
) -> impl IntoElement {
    design::avatar(name, design::size::AVATAR_LG, theme)
        .id(id)
        .bg(if agent { theme.agent_soft } else { fill })
        .text_color(theme.foreground)
        .text_size(design::text::CAPTION)
}
