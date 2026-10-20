//! The open room, search results, notices, and composer.

use view::prelude::*;

use chat::MsgRow;

use super::timeline;
use crate::composer::Target;
use crate::session::Gate;
use crate::ui::{badge, button, empty_state, quiet};
use crate::words;
use crate::{Chat, Hits, Pane, Room};
use view::design::Ladder;

pub fn render(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement {
    let pane = div()
        .id("chat-room")
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .flex()
        .flex_col()
        .bg(theme.background)
        .text_color(theme.foreground);
    let Some(room) = &chat.room else {
        // a search asked with no room open answers here: what the lists leave out is found so
        return match chat.search.query.is_empty() {
            true => pane.child(no_room(chat, cx, theme, ladder)),
            false => pane.child(search_results(chat, cx, theme, ladder)),
        };
    };
    let body = match chat.search.query.is_empty() {
        true => timeline::list(chat, Pane::Timeline, cx, theme, ladder).into_any_element(),
        false => search_results(chat, cx, theme, ladder).into_any_element(),
    };
    pane.child(header(chat, room, cx, theme, ladder))
        .children(notice(chat, cx, theme, ladder))
        .children(confirmation(chat, cx, theme, ladder))
        .children(super::widget::pinned(chat, cx, theme, ladder))
        .child(body)
        .child(compose(chat, room, cx, theme, ladder))
}

fn notice(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<impl IntoElement> {
    let older_failed = chat
        .room
        .as_ref()
        .and_then(|room| room.older_failed.as_deref());
    let said = match (chat.notice.is_empty(), older_failed) {
        (false, _) => chat.notice.clone(),
        (true, Some(why)) => words::unreadable_room(why),
        (true, None) => return None,
    };
    let dismiss = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.notice.clear();
        if let Some(room) = &mut chat.room {
            room.older_failed = None;
        }
        cx.notify();
    });
    Some(
        div()
            .id("chat-room-notice")
            .mx_3()
            .my_2()
            .p_2()
            .bg(theme.danger_soft)
            .text_color(theme.danger)
            .flex()
            .items_center()
            .gap_2()
            .child(div().flex_1().child(said))
            .child(button(
                "chat-room-notice-dismiss",
                words::DISMISS,
                ladder,
                theme,
                dismiss,
            )),
    )
}

fn confirmation(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<impl IntoElement> {
    if chat.confirmation.is_empty() {
        return None;
    }
    let dismiss = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.confirmation.clear();
        cx.notify();
    });
    Some(
        div()
            .id("chat-room-confirmation")
            .mx_3()
            .my_2()
            .px_2()
            .h(design::size::CONTROL)
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .text_size(design::text::SECONDARY)
            .text_color(theme.muted)
            .flex()
            .items_center()
            .gap_2()
            .child(div().flex_1().child(chat.confirmation.clone()))
            .child(design::icon_button(
                "chat-room-confirmation-dismiss",
                "✕",
                words::DISMISS,
                ladder,
                theme,
                dismiss,
            )),
    )
}

fn compose(
    chat: &Chat,
    room: &Room,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    if let Some(gate) = chat.write_gate() {
        return write_gate(gate, cx, theme, ladder).into_any_element();
    }
    let target = Target::Post {
        channel: room.id.clone(),
        thread: None,
    };
    let editable = chat.session.connected;
    div()
        .p_3()
        .children(crate::ui::rules::line(chat, cx, theme, ladder))
        .child(composer(
            chat,
            target,
            &chat.composer_hint(room),
            editable,
            cx,
        ))
        .into_any_element()
}

fn header(
    chat: &Chat,
    room: &Room,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement {
    let info = chat.info(&room.id);
    let name = info.map_or_else(
        || view::design::short_hex(&room.id),
        |info| info.channel.name.clone(),
    );
    let details = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.toggle_details(cx);
        cx.notify();
    });
    let direct = chat::dm_peers(&room.id).is_some();
    let peer = chat.dm_peer();
    let mut title = div().flex().items_center().gap_2();
    let picture = chat.dm_peer_picture(cx);
    if direct {
        let (name, agent) = peer.clone().unwrap_or((name.clone(), false));
        title = title.child(super::sidebar::avatar(
            "chat-room-avatar",
            &name,
            agent,
            picture,
            theme.surface_raised,
            theme,
        ));
    }
    title = title.child(
        div()
            .id("chat-room-title")
            .text_size(design::text::SECTION)
            .font_weight(view::FontWeight::SEMIBOLD)
            .role(Role::Heading)
            .aria_level(2)
            .child(match (direct, peer) {
                (true, Some((peer, _))) => peer,
                (true, None) => name,
                (false, _) => format!("#{name}"),
            }),
    );
    if info.is_some_and(|info| info.channel.archived) {
        title = title.child(badge(
            "chat-room-archived",
            words::ARCHIVED,
            ladder,
            theme.warning,
            theme.warning_soft,
        ));
    }
    if !direct && info.is_some_and(|info| info.channel.members_only()) {
        title = title.child(badge(
            "chat-room-members-only",
            words::MEMBERS_ONLY,
            ladder,
            theme.muted,
            theme.surface_raised,
        ));
    }
    let unanswered = chat
        .my_account()
        .and_then(|me| crate::names::dm_peer_of(me, &room.id))
        .map(chat::Principal::Account)
        .and_then(|peer| {
            let name = chat.names.ready()?.member(&peer);
            let why = chat.unanswered(&peer)?;
            Some(format!("{name} answers {why}. Messages here get no reply."))
        })
        .map(|line| {
            div()
                .id("chat-room-unanswered")
                .text_size(design::text::SECONDARY)
                .text_color(theme.muted)
                .child(line)
        });
    div()
        .id("chat-room-header")
        .flex()
        .items_center()
        .gap_2()
        .p_3()
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap_1()
                .child(title)
                .children(unanswered)
                .children(super::widget::line(chat, theme, ladder)),
        )
        .children(super::widget::side_button(chat, cx, theme, ladder))
        .child(button(
            "chat-room-details",
            if direct {
                words::CONVERSATION_DETAILS
            } else {
                words::CHANNEL_DETAILS
            },
            ladder,
            theme,
            details,
        ))
}

fn no_room(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> AnyElement {
    match &chat.channels {
        _ if !chat.session_heard => super::placeholders::no_room().into_any_element(),
        Loadable::Idle | Loadable::Loading(_) => super::placeholders::no_room().into_any_element(),
        Loadable::Failed(refusal) => empty_state(
            "chat-no-room-failed",
            "message-square",
            "Couldn’t read the channels",
            design::sentence(refusal),
            ladder,
            theme,
        )
        .into_any_element(),
        Loadable::Ready(rooms) | Loadable::Reloading(rooms, _) if !rooms.is_empty() => empty_state(
            "chat-no-room",
            "message-square",
            "No channel open",
            "Choose a channel from the sidebar.",
            ladder,
            theme,
        )
        .into_any_element(),
        Loadable::Ready(_) | Loadable::Reloading(_, _) => {
            let open = cx.listener(|chat, _: &ClickEvent, _window, cx| {
                chat.create = Some(Default::default());
                cx.notify();
            });
            div()
                .id("chat-no-room-empty")
                .flex()
                .flex_col()
                .gap_2()
                .p_6()
                .child(div().text_size(design::text::BODY).child("No channels"))
                .child(
                    div()
                        .text_size(design::text::SECONDARY)
                        .text_color(theme.muted)
                        .child("Create the first channel in this network."),
                )
                .child(button(
                    "chat-no-room-create",
                    "Create a channel",
                    ladder,
                    theme,
                    open,
                ))
                .into_any_element()
        }
    }
    .into_any_element()
}

fn search_results(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement {
    let found = match &chat.search.hits {
        Loadable::Idle | Loadable::Loading(_) => {
            vec![quiet("Searching…", theme).into_any_element()]
        }
        Loadable::Failed(refusal) => {
            vec![quiet(design::sentence(refusal), theme).into_any_element()]
        }
        Loadable::Ready(hits) | Loadable::Reloading(hits, _) if hits.rows.is_empty() => vec![
            empty_state(
                "chat-search-empty",
                "search-x",
                "No results",
                "Nothing matched this message search.",
                ladder,
                theme,
            )
            .into_any_element(),
        ],
        Loadable::Ready(hits) | Loadable::Reloading(hits, _) => {
            hit_list(chat, hits, cx, theme, ladder)
        }
    };
    let clear = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.search_clear();
        cx.notify();
    });
    div()
        .id("chat-search-results")
        .flex_1()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .children(room_matches(chat, cx, theme))
        .children(found)
        .child(button(
            "chat-search-clear",
            "Clear message search",
            ladder,
            theme,
            clear,
        ))
}

// The rooms whose names hold the search, under how many, a row each that opens it; nothing when no
// room's name does.
fn room_matches(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> Vec<AnyElement> {
    let found = chat.found_rooms();
    if found.is_empty() {
        return Vec::new();
    }
    let count = design::plural(found.len() as u64, "room", "rooms");
    let mut list = vec![
        div()
            .id("chat-search-rooms-head")
            .text_size(design::text::SECONDARY)
            .text_color(theme.muted)
            .child(format!("{count} for “{}”", chat.search.query))
            .into_any_element(),
    ];
    for info in found {
        let room = info.channel.id.clone();
        let open = cx.listener(move |chat, _: &ClickEvent, _, cx| {
            cx.notify();
            chat.open_found(room.clone(), cx)
        });
        let name = format!("#{}", info.channel.name);
        let row = div()
            .id(format!("chat-search-room-{}", info.channel.id))
            .p_2()
            .bg(theme.surface)
            .cursor_pointer()
            .hover(|s| s.bg(theme.surface_raised))
            .role(view::Role::Button)
            .aria_label(name.clone())
            .focusable()
            .on_click(open)
            .child(name);
        list.push(row.into_any_element());
    }
    list
}

/// How many hits, a row per hit, and the way to more.
fn hit_list(
    chat: &Chat,
    hits: &Hits,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    let count = design::plural(hits.rows.len() as u64, "result", "results");
    let mut list = vec![
        div()
            .text_size(design::text::SECONDARY)
            .text_color(theme.muted)
            .child(format!("{count} for “{}”", chat.search.query))
            .into_any_element(),
    ];
    list.extend(hits.rows.iter().map(|row| hit(chat, row, cx, theme)));
    if hits.has_more {
        let more = cx.listener(|chat, _: &ClickEvent, _window, cx| {
            cx.notify();
            chat.search_more(cx)
        });
        list.push(
            button("chat-search-more", words::MORE_RESULTS, ladder, theme, more).into_any_element(),
        );
    }
    list
}

fn hit(chat: &Chat, row: &MsgRow, cx: &mut Context<Chat>, theme: &Theme) -> AnyElement {
    let names = chat.names.ready();
    let room = match chat::dm_peers(&row.channel_id) {
        Some(_) => chat
            .my_account()
            .and_then(|me| crate::names::dm_peer_of(me, &row.channel_id))
            .zip(names)
            .map(|(peer, names)| names.member(&chat::Principal::Account(peer))),
        None => chat
            .info(&row.channel_id)
            .map(|info| format!("#{}", info.channel.name)),
    };
    let author = names.map(|names| names.member(&row.author));
    let place = [room, author]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    let id = row.channel_id.clone();
    let seq = row.seq;
    let key = format!("chat-search-hit-{id}-{seq}");
    let open = cx.listener(move |chat, _: &ClickEvent, _, cx| {
        cx.notify();
        chat.open_hit(id.clone(), seq, cx)
    });
    div()
        .id(key)
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .bg(theme.surface)
        .cursor_pointer()
        .hover(|s| s.bg(theme.surface_raised))
        .role(view::Role::Button)
        .focusable()
        .on_click(open)
        .child(
            div()
                .text_size(design::text::SECONDARY)
                .child(row.text.clone()),
        )
        .child(
            div()
                .text_size(design::text::CAPTION)
                .text_color(theme.muted)
                .child(place),
        )
        .into_any_element()
}

fn write_gate(gate: Gate, cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let why = words::gate(gate);
    let mut notice = div()
        .id("chat-room-write-refusal")
        .mx_3()
        .my_2()
        .p_3()
        .bg(match gate {
            Gate::Archived => theme.surface,
            Gate::NotMember | Gate::NoAccount => theme.warning_soft,
        })
        .text_color(theme.muted)
        .flex()
        .items_center()
        .gap_2()
        .child(div().flex_1().child(why));
    if gate == Gate::Archived {
        let reopen = cx.listener(|chat, _: &ClickEvent, _window, cx| {
            cx.notify();
            chat.set_archived(false, cx);
        });
        notice = notice.child(button(
            "chat-room-unarchive",
            words::UNARCHIVE,
            ladder,
            theme,
            reopen,
        ));
    }
    notice.into_any_element()
}

pub fn composer(
    chat: &Chat,
    target: Target,
    hint: &str,
    editable: bool,
    cx: &mut Context<Chat>,
) -> impl IntoElement {
    let key = target.key();
    let draft = chat
        .drafts
        .get(&key)
        .expect("seated by `Chat::render` before the frame");
    let choices = chat.mention_choices();
    let (commit, cancel) = match target {
        Target::Post { .. } => (words::SEND, None),
        Target::Edit { .. } => (
            "Save",
            Some(Box::new(cx.listener(|chat, _: &ClickEvent, _, cx| {
                chat.close_menu();
                cx.notify();
            })) as crate::composer::Click),
        ),
    };
    let label = match target {
        Target::Post { thread: None, .. } => words::NEW_MESSAGE,
        Target::Post {
            thread: Some(_), ..
        } => words::REPLY,
        Target::Edit { .. } => words::EDITED_MESSAGE,
    };
    crate::composer::view(
        draft,
        &key,
        label,
        hint,
        commit,
        cancel,
        editable,
        &choices,
        chat.layout.pane,
        cx,
        move |chat, event, window, cx| chat.composer(target.clone(), event, window, cx),
    )
}

pub fn selection_bar(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement {
    let count = chat.copy_count();
    if count == 0 {
        return div().into_any_element();
    }
    let copy = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.copy_range(cx)
    });
    div()
        .id("chat-selection-bar")
        .flex()
        .items_center()
        .gap_2()
        .p_2()
        .bg(theme.accent_soft)
        .occlude()
        .child(div().flex_1().child(format!(
            "{} selected",
            design::plural(count as u64, "message", "messages")
        )))
        .child(button("chat-selection-copy", "Copy", ladder, theme, copy))
        .into_any_element()
}
