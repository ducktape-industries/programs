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
    // which rooms are the reader's waits on who reads
    if !chat.session_heard {
        return list.child(super::placeholders::sidebar_rooms(theme));
    }
    let no_channels = chat
        .channels
        .ready()
        .is_none_or(|channels| channels.is_empty());
    if matches!(chat.channels, Loadable::Idle | Loadable::Loading(_)) && no_channels {
        list = list.child(super::placeholders::sidebar_rooms(theme));
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
    let others = match chat.others_open {
        true => chat.more_lines(&room_list.others),
        false => Vec::new(),
    };
    let more = (!room_list.others.is_empty()).then_some(Spot::More);
    let unfolded = others.iter().map(Line::spot);
    let dms = room_list.dms.iter();
    let dms = dms.map(|dm| Spot::Room(dm.info.channel.id.clone()));
    let spots: Vec<Spot> = plain
        .iter()
        .map(Line::spot)
        .chain(more)
        .chain(unfolded)
        .chain(dms)
        .collect();
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
        // Alt+↑ and Alt+↓ move the active row up or down its level
        let up = match event.keystroke.key.as_str() {
            "up" => Some(true),
            "down" => Some(false),
            _ => None,
        };
        let alt = up.filter(|_| event.keystroke.modifiers.alt);
        if let (Some(up), Some(entry)) = (alt, spot.entry()) {
            chat.rooms_cursor = Some(spot.clone());
            return chat.step(&entry, up, cx);
        }
        let open = match event.keystroke.key.as_str() {
            "right" => true,
            "left" => false,
            _ => return,
        };
        if event.keystroke.modifiers.modified() {
            return;
        }
        match spot {
            Spot::Folder { path, .. } => chat.fold(path, open, cx),
            Spot::More => chat.fold_others(open),
            Spot::MoreFolder { path, .. } => chat.fold_more(path, open),
            Spot::Room(_) => return,
        }
        cx.notify();
    });
    // a left press on a channel or folder row holds it; dragged onto another row of its level and
    // let go, it takes that row's place
    let drawn: std::rc::Rc<[crate::room_list::Entry]> = plain.iter().map(Line::entry).collect();
    let (held, over) = (drawn.clone(), drawn);
    let hold =
        cx.listener(move |chat, event: &MouseDownEvent, _, _| chat.hold_row(event.position, &held));
    let drag = cx.listener(
        move |chat, event: &MouseMoveEvent, _, cx| match event.pressed_button {
            Some(MouseButton::Left) => chat.drag_row(event.position, &over, cx),
            _ => chat.put_down(cx),
        },
    );
    let land = cx.listener(|chat, _: &MouseUpEvent, _, cx| chat.drop_row(cx));
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
        .on_mouse_down(MouseButton::Left, hold)
        .on_mouse_move(drag)
        .on_mouse_up(MouseButton::Left, land)
        .flex()
        .flex_col()
        .gap_1();
    let mut index = 0;
    for line in &plain {
        box_ = box_.child(line_row(chat, line, open, index == at, cx, theme, ladder));
        index += 1;
    }
    if !room_list.others.is_empty() {
        let count = room_list.others.len();
        box_ = box_.child(more_row(chat, count, index == at, cx, theme));
        index += 1;
    }
    for line in &others {
        let active = index == at;
        let row = match line {
            Line::Room { info, depth } => {
                let selected = open == Some(info.channel.id.as_str());
                channel_row(chat, info, *depth, selected, active, None, cx, theme)
            }
            Line::Folder(folder) => folder_row(chat, folder, open, active, None, cx, theme, ladder),
        };
        box_ = box_.child(row);
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
    let landing = chat.landing(&line.entry());
    match line {
        Line::Room { info, depth } => {
            let selected = open == Some(info.channel.id.as_str());
            channel_row(chat, info, *depth, selected, active, landing, cx, theme)
        }
        Line::Folder(folder) => folder_row(chat, folder, open, active, landing, cx, theme, ladder),
    }
}

// The row at the channels' end: how many channels the reader is not in, which a press lists under
// it, and its chevron.
fn more_row(
    chat: &Chat,
    count: usize,
    active: bool,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> AnyElement {
    let toggle = cx.listener(|chat, _: &ClickEvent, _, cx| {
        chat.fold_others(!chat.others_open);
        cx.notify();
    });
    let open = chat.others_open;
    let text = words::more_channels(count);
    let row = div()
        .id("chat-sidebar-more")
        .flex()
        .w_full()
        .items_center()
        .gap_1()
        .min_h(design::size::CONTROL)
        .px_1()
        .bg(theme.sidebar)
        .cursor_pointer()
        .hover(|s| s.bg(theme.sidebar_raised))
        .when(active, |row| row.bg(theme.sidebar_raised))
        .aria_label(text.clone())
        .aria_expanded(open)
        .aria_selected(false)
        .on_click(toggle)
        .child(design::lead(
            div()
                .text_color(theme.sidebar_muted)
                .child(if open { "▾" } else { "▸" }),
        ))
        .child(
            div()
                .flex_1()
                .truncate()
                .text_color(theme.sidebar_muted)
                .child(text),
        );
    design::item(row, Role::ListBoxOption, active).into_any_element()
}

// A folder's chevron, the smallest box a pointer presses (the door's AX-017).
const FOLD: Pixels = px(24.);

// A folder's row: its `/` in the lead column, where a room's `#` goes, and its name, flush with the
// rooms beside it; while it is closed how many rooms inside are unread, and its chevron at the row's end. A folder named as a channel is that channel's row too: a press
// opens the channel, and the chevron opens or closes it.
#[allow(clippy::too_many_arguments)]
fn folder_row(
    chat: &Chat,
    folder: &FolderLine,
    open: Option<&str>,
    active: bool,
    landing: Option<bool>,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let id = match folder.more {
        true => format!("chat-sidebar-more-folder-{}", folder.path),
        false => format!("chat-sidebar-folder-{}", folder.path),
    };
    let (path, more) = (folder.path.clone(), folder.more);
    let toggle = cx.listener(move |chat, _: &ClickEvent, _, cx| {
        match more {
            true => chat.toggle_more_folder(&path),
            false => chat.toggle_folder(&path, cx),
        }
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
        .when_some(landing, |row, after| lands(row, after, theme))
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
                row.aria_label(folder.name.to_owned())
                    .on_click(choose)
                    .consumes_click(),
                fold,
            )
        }
        None => {
            let row = row
                .aria_label(words::folder(folder.name))
                .aria_expanded(folder.open)
                .on_click(toggle)
                .consumes_click();
            (row, glyph)
        }
    };
    let mut row = row
        .children((0..folder.depth).map(|_| design::lead(div())))
        .child(design::lead(
            div().text_color(theme.sidebar_muted).child("/"),
        ))
        .child(
            div()
                .flex_1()
                .truncate()
                .text_color(if unread || folder.unread > 0 {
                    theme.sidebar_foreground
                } else {
                    theme.sidebar_muted
                })
                .child(folder.name.to_owned()),
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
    let row = match folder.more {
        true => row,
        false => placed(
            row,
            crate::room_list::Entry::Folder(folder.path.clone()),
            cx,
        ),
    };
    let row = row.child(glyph);
    let row = match own {
        Some(own) => super::room_menu::row(row, &own.channel.id, chat, cx),
        None => row,
    };
    design::item(row, Role::ListBoxOption, active).into_any_element()
}

// A channel's row, `depth` folders in, drawn again only when what it shows moved.
#[allow(clippy::too_many_arguments)]
fn channel_row(
    chat: &Chat,
    info: &ChannelInfo,
    depth: usize,
    selected: bool,
    active: bool,
    landing: Option<bool>,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> AnyElement {
    let room = &info.channel.id;
    let read = (chat.unread(info), super::room_menu::waits(chat, room));
    let deps = (info.clone(), depth, selected, active, landing, read);
    let (info, theme) = (info.clone(), *theme);
    let id = format!("chat-sidebar-channel-{}", info.channel.id);
    cx.memo(id, deps, move |chat, _, cx| {
        channel_button(chat, &info, depth, selected, active, landing, cx, &theme)
    })
    .into_any_element()
}

// A channel's row: an empty lead column for each folder it sits in, then its `#`, its name,
// Members only and the unread dot; an accent edge where a held row lands on it.
#[allow(clippy::too_many_arguments)]
fn channel_button(
    chat: &Chat,
    info: &ChannelInfo,
    depth: usize,
    selected: bool,
    active: bool,
    landing: Option<bool>,
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
        .consumes_click()
        .when_some(landing, |row, after| lands(row, after, theme))
        .children((0..depth).map(|_| design::lead(div())))
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
    let row = placed(
        row,
        crate::room_list::Entry::Room(info.channel.id.clone()),
        cx,
    );
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

pub fn avatar(
    id: impl Into<ElementId>,
    name: &str,
    agent: bool,
    picture: Option<std::sync::Arc<view::Image>>,
    fill: view::Hsla,
    theme: &Theme,
) -> impl IntoElement {
    let pictured = picture.is_some();
    let face = design::avatar_with(name, picture, design::size::AVATAR_LG, theme).bg(fill);
    let face = match agent {
        true => design::agent_tint(face, pictured, theme),
        false => face,
    };
    face.id(id)
        .text_color(theme.foreground)
        .text_size(design::text::CAPTION)
}

// A row a held row lands on: an accent edge on the side it lands, below when it comes from above.
pub(crate) fn lands(row: Stateful<Div>, after: bool, theme: &Theme) -> Stateful<Div> {
    let row = match after {
        true => row.border_b_2(),
        false => row.border_t_2(),
    };
    row.border_color(theme.accent)
}

// `row` tells the view where layout put it, so a drag finds the row under the pointer.
pub(crate) fn placed(
    row: Stateful<Div>,
    entry: crate::room_list::Entry,
    cx: &mut Context<Chat>,
) -> Stateful<Div> {
    let id = match &entry {
        crate::room_list::Entry::Room(room) => format!("chat-sidebar-bounds-{room}"),
        crate::room_list::Entry::Folder(path) => format!("chat-sidebar-bounds-folder-{path}"),
    };
    let heard = cx.listener(move |chat, bounds: &Bounds<Pixels>, _, _| {
        chat.drawn_row(entry.clone(), *bounds)
    });
    row.relative().child(
        sensor(id, div().size_full())
            .absolute()
            .inset_0()
            .on_bounds(heard),
    )
}
