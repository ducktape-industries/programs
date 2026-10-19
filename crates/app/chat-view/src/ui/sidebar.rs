//! The channel and direct-message pane, authored as native GPUI elements.

use view::prelude::*;
use view::{Div, Stateful};

use chat::{ChannelInfo, Principal};

use crate::room_list::{FolderLine, Line, Spot, label};
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
    let room_list = chat.room_list();
    if room_list.channels.is_empty() && room_list.left_out > 0 {
        list = list.child(quiet("chat-sidebar-none-held", words::NONE_HELD, theme));
    }
    let plain = chat.lines(&room_list.channels);
    // one Tab stop under the header: ↑ ↓ walk the channels, their folders
    // and the direct messages, Enter opens the active room or folder, → and
    // ← open and close the active folder
    let dms = room_list.dms.iter();
    let dms = dms.map(|dm| Spot::Room(dm.info.channel.id.clone()));
    let spots: Vec<Spot> = plain.iter().map(Line::spot).chain(dms).collect();
    let at = chat.rooms_at(&spots);
    let picked = spots.clone();
    // the Menu key or Shift+F10 opens the active room's menu, under its row
    let asked = spots.get(at).cloned();
    let keys = cx.listener(move |chat, event: &KeyDownEvent, window, cx| {
        let Some(spot) = &asked else {
            return;
        };
        if let Some(room) = spot.room()
            && design::asks_for_menu(&event.keystroke)
        {
            return chat.open_room_menu(room.to_owned(), None, window, cx);
        }
        let Spot::Folder { path, .. } = spot else {
            return;
        };
        let open = match event.keystroke.key.as_str() {
            "right" => true,
            "left" => false,
            _ => return,
        };
        if !event.keystroke.modifiers.modified() {
            chat.fold(path, open, cx);
            cx.notify();
        }
    });
    let mut box_ = design::composite("chat-sidebar-rooms-list", Role::ListBox, "Rooms")
        .active(at, spots.len())
        .on_move(cx.processor(move |chat, index: usize, _, cx| {
            chat.rooms_cursor = Some(spots[index].clone());
            cx.notify();
        }))
        .on_press(cx.processor(move |chat, index: usize, _, cx| {
            cx.notify();
            chat.press_spot(&picked[index], cx)
        }))
        .build()
        .on_key_down(keys)
        .flex()
        .flex_col()
        .gap_1();
    let mut index = 0;
    for line in &plain {
        box_ = box_.child(line_row(chat, line, open, index == at, cx, theme, ladder));
        index += 1;
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
        let face = chat.faces.dep(peer, cx);
        let deps = (
            info.clone(),
            peer,
            selected,
            active,
            read,
            names.clone(),
            face,
        );
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

// A line of the channel list: a channel's row, or a folder's.
fn line_row(
    chat: &Chat,
    line: &Line,
    open: Option<&str>,
    active: bool,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    match line {
        Line::Room { info, .. } => {
            let selected = open == Some(info.channel.id.as_str());
            channel_row(chat, info, selected, active, cx, theme)
        }
        Line::Folder(folder) => folder_row(chat, folder, open, active, cx, theme, ladder),
    }
}

// A folder's chevron, the smallest box a pointer presses (the door's AX-017).
const FOLD: Pixels = px(24.);

// A folder's row: its name with its `/` after the lead column, while it is closed how many rooms
// inside are unread, and its chevron at the row's end. A folder named as a channel is that channel's row too: a press
// opens the channel, and the chevron opens or closes it.
fn folder_row(
    chat: &Chat,
    folder: &FolderLine,
    open: Option<&str>,
    active: bool,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let id = format!("chat-sidebar-folder-{}", folder.path);
    let path = folder.path.clone();
    let toggle = cx.listener(move |chat, _: &ClickEvent, _, cx| {
        chat.toggle_folder(&path, cx);
        cx.notify();
    });
    let own = folder.own;
    let selected = own.is_some_and(|own| open == Some(own.channel.id.as_str()));
    let unread = own.is_some_and(|own| chat.unread(own)) && !selected;
    let glyph = div()
        .id(format!("{id}-fold"))
        .min_w(FOLD)
        .min_h(FOLD)
        .flex()
        .items_center()
        .justify_center()
        .text_color(theme.sidebar_muted)
        .child(if folder.open { "▾" } else { "▸" });
    let row = div()
        .id(id.clone())
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
        .when(folder.unread > 0, |row| {
            row.aria_description(words::unread_rooms(folder.unread))
        });
    let (row, glyph) = match own {
        // the chevron is a press of its own, which the row's leaves alone
        Some(own) => {
            let room = own.channel.id.clone();
            let choose = cx.listener(move |chat, _: &ClickEvent, _, cx| {
                cx.notify();
                chat.choose(room.clone(), cx)
            });
            let fold = glyph
                .role(Role::Button)
                .aria_label(words::folder(folder.name))
                .aria_expanded(folder.open)
                .on_click(toggle)
                .consumes_click();
            (
                row.aria_label(folder.name.to_owned()).on_click(choose),
                fold,
            )
        }
        None => {
            let row = row
                .aria_label(words::folder(folder.name))
                .aria_expanded(folder.open)
                .on_click(toggle);
            (row, glyph)
        }
    };
    // the kit row's lead column, empty, as a room row's holds its `#`: the folder's name starts
    // where its rooms' names do
    let mut row = row.child(design::lead(div())).child(
        div()
            .flex_1()
            .truncate()
            .text_color(if unread || folder.unread > 0 {
                theme.sidebar_foreground
            } else {
                theme.sidebar_muted
            })
            .child(format!("{}/", folder.name)),
    );
    if own.is_some_and(|own| own.channel.members_only()) {
        row = row.child(
            div()
                .text_size(design::text::CAPTION)
                .text_color(theme.sidebar_muted)
                .child(words::MEMBERS_ONLY),
        );
    }
    if folder.unread > 0 {
        row = row.child(super::badge(
            format!("{id}-count"),
            folder.unread.to_string(),
            ladder,
            theme.foreground,
            theme.accent_soft,
        ));
    }
    if unread {
        row = row.child(
            div()
                .id(format!("{id}-unread"))
                .size_2()
                .rounded_full()
                .bg(theme.accent),
        );
    }
    let row = row.child(glyph);
    let row = match own {
        Some(own) => super::room_menu::row(row, &own.channel.id, chat, cx),
        None => row,
    };
    design::item(row, Role::ListBoxOption, active).into_any_element()
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
        .child(design::lead(
            div().text_color(theme.sidebar_muted).child("#"),
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
                .child(label(&info.channel.name).to_owned()),
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
    let picture = chat.faces.of(peer, cx);
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
            picture,
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

// A person's round initials, or their picture once read (an agent's
// ringed in its tint): a direct room's face, in the sidebar and over the
// room.
pub fn avatar(
    id: impl Into<ElementId>,
    name: &str,
    agent: bool,
    picture: Option<std::sync::Arc<view::Image>>,
    fill: view::Hsla,
    theme: &Theme,
) -> impl IntoElement {
    let ringed = agent && picture.is_some();
    design::avatar_with(name, picture, design::size::AVATAR_LG, theme)
        .id(id)
        .bg(if agent { theme.agent_soft } else { fill })
        .text_color(theme.foreground)
        .text_size(design::text::CAPTION)
        .when(ringed, |face| face.border_2().border_color(theme.agent))
}
