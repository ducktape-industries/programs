//! Thread and channel-details panes.

use view::prelude::*;
use view::{Div, Stateful};

use super::timeline;
use crate::Chat;
use crate::ui::{button, empty_state};
use crate::ui::{notices, room};
use crate::words;
use view::design::Ladder;

pub fn thread(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let close = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.close_thread();
        cx.notify();
    });
    let mut pane = div()
        .id("chat-thread-pane")
        .w(px(chat.layout.clamped().thread))
        .h_full()
        .flex()
        .flex_col()
        // one ground under the header, the replies and the field: the
        // rows are drawn on `background`, so the pane is too
        .bg(theme.background)
        .child(
            div()
                .id("chat-thread-header")
                .flex()
                .items_center()
                .gap_2()
                .p_3()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .id("chat-thread-title")
                        .flex_1()
                        .text_size(design::text::SECTION)
                        .font_weight(view::FontWeight::SEMIBOLD)
                        .role(Role::Heading)
                        .aria_level(2)
                        .child(words::THREAD),
                )
                .child(button(
                    "chat-thread-close",
                    "Close thread",
                    ladder,
                    theme,
                    close,
                )),
        );
    if let Some(room) = &chat.room
        && let Some(thread_state) = &room.thread
    {
        pane = pane.child(timeline::list(chat, crate::Pane::Thread, cx, theme, ladder));
        if thread_state.replies.is_loading() {
            pane = pane.child(
                div()
                    .p_2()
                    .text_size(design::text::SECONDARY)
                    .text_color(theme.muted)
                    .child("Loading replies…"),
            );
        }
        if chat.may_write() {
            let target = crate::composer::Target::Post {
                channel: room.id.clone(),
                thread: Some(thread_state.root),
            };
            pane = pane.child(div().p_3().child(room::composer(
                chat,
                target,
                words::REPLY_IN_THREAD,
                chat.session.connected && !thread_state.replies.is_loading(),
                cx,
            )));
        }
    } else {
        pane = pane.child(empty_state(
            "chat-thread-empty",
            "messages-square",
            "No thread open",
            "Choose a reply from the room.",
            ladder,
            theme,
        ));
    }
    pane
}

/// The open channel's details: its name, archiving, and its members. A
/// dm seats its two peers for good: it lists them, with no way to add or
/// remove one.
pub fn details(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let (Some(details), Some(info)) = (chat.details.as_ref(), chat.room_info()) else {
        return div().id("chat-details-pane");
    };
    let dm = chat::dm_peers(&info.channel.id).is_some();
    let pane = div()
        .id("chat-details-pane")
        .w(px(chat.layout.clamped().details))
        .h_full()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_3()
        .p_3()
        .bg(theme.surface);
    let notices = |pane: Stateful<Div>, cx: &mut Context<Chat>| {
        pane.child(section(words::NOTIFICATIONS, theme))
            .child(notices::choices(
                chat,
                &info.channel.id,
                dm,
                cx,
                theme,
                ladder,
            ))
            .children(notices::managed(
                chat,
                &info.channel.id,
                dm,
                cx,
                theme,
                ladder,
            ))
            .child(rule(theme))
    };
    // a dm is its two people alike: nothing to rename, archive or seat
    let pane = match dm {
        true => {
            let pane = pane
                .child(details_header("Conversation details", cx, theme, ladder))
                .child(rule(theme));
            notices(pane, cx).child(section(words::PEOPLE, theme))
        }
        false => {
            let pane = pane
                .child(details_header(words::CHANNEL_DETAILS, cx, theme, ladder))
                .child(rule(theme))
                .children(crate::policies::room_row(chat, cx, ladder, theme));
            notices(pane, cx)
                .children(name_section(
                    &details.name_draft,
                    info.channel.archived,
                    cx,
                    theme,
                    ladder,
                ))
                .child(rule(theme))
                .children(category_section(
                    &details.category_draft,
                    crate::room_list::category_of(info).is_some(),
                    cx,
                    theme,
                    ladder,
                ))
                .child(rule(theme))
                .child(section(words::MEMBERS, theme))
                .children(member_adder(&details.member_draft, cx, theme, ladder))
        }
    };
    pane.children(members(chat, !dm, cx, theme))
}

fn details_header(
    title: &'static str,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement {
    let close = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.toggle_details(cx);
        cx.notify();
    });
    div()
        .id("chat-details-header")
        .flex()
        .items_center()
        .child(
            div()
                .id("chat-details-title")
                .flex_1()
                .text_size(design::text::SECTION)
                .font_weight(view::FontWeight::SEMIBOLD)
                .role(Role::Heading)
                .aria_level(2)
                .child(title),
        )
        .child(button("chat-details-close", "Close", ladder, theme, close))
}

/// The name field, Rename, and Archive or Unarchive.
fn name_section(
    draft: &TextField,
    archived: bool,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    let rename = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.rename(cx)
    });
    let archive = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.set_archived(!archived, cx);
    });
    let archive_label = match archived {
        true => words::UNARCHIVE_CHANNEL,
        false => words::ARCHIVE_CHANNEL,
    };
    vec![
        section(words::NAME, theme).into_any_element(),
        field("chat-details-name-input", draft, "Channel name", theme).into_any_element(),
        button(
            "chat-details-rename-button",
            words::RENAME,
            ladder,
            theme,
            rename,
        )
        .into_any_element(),
        button(
            "chat-details-archive",
            archive_label,
            ladder,
            theme,
            archive,
        )
        .into_any_element(),
    ]
}

fn category_section(
    draft: &TextField,
    categorized: bool,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    let set = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.set_category(cx)
    });
    let mut controls = vec![
        section(words::CATEGORY, theme).into_any_element(),
        field("chat-details-category", draft, words::CATEGORY, theme)
            .placeholder(words::NO_CATEGORY)
            .into_any_element(),
        button(
            "chat-details-set-category",
            words::SET_CATEGORY,
            ladder,
            theme,
            set,
        )
        .into_any_element(),
    ];
    if categorized {
        let clear = cx.listener(|chat, _: &ClickEvent, _window, cx| {
            cx.notify();
            chat.clear_category(cx)
        });
        let clear = button(
            "chat-details-clear-category",
            words::CLEAR_CATEGORY,
            ladder,
            theme,
            clear,
        );
        controls.push(clear.into_any_element());
    }
    controls
}

/// The field and button that seat a member, and how one is removed.
fn member_adder(
    draft: &TextField,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    let add_member = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.add_member(cx);
    });
    vec![
        field("chat-details-member-input", draft, "Add member", theme).into_any_element(),
        button(
            "chat-details-add-member",
            "Add member",
            ladder,
            theme,
            add_member,
        )
        .into_any_element(),
        div()
            .text_size(design::text::CAPTION)
            .text_color(theme.faint)
            .child("Select a member below to remove it from this channel.")
            .into_any_element(),
    ]
}

fn rule(theme: &Theme) -> impl IntoElement {
    div().h(px(1.)).w_full().bg(theme.border)
}

fn section(name: &'static str, theme: &Theme) -> impl IntoElement {
    div()
        .text_size(design::text::SECONDARY)
        .text_color(theme.muted)
        .child(name)
}

fn field(id: &'static str, value: &TextField, label: &'static str, theme: &Theme) -> Input {
    Input::new(id, value, label)
        .h(design::size::CONTROL)
        .px_2()
        .py_1()
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.background)
}

/// A row per member, each with its way out where the room has one; a word
/// when there are none.
fn members(chat: &Chat, removable: bool, cx: &mut Context<Chat>, theme: &Theme) -> Vec<AnyElement> {
    let roster = chat.roster();
    if roster.is_empty() {
        let none = div()
            .id("chat-details-no-members")
            .text_size(design::text::CAPTION)
            .text_color(theme.faint)
            .child(words::NO_MEMBERS);
        return vec![none.into_any_element()];
    }
    let mut rows = Vec::new();
    for (index, (principal, label)) in roster.into_iter().enumerate() {
        let name = format!("Remove {label}");
        let remove = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
            cx.notify();
            chat.set_member(principal.clone(), false, cx);
        });
        let remove = div()
            .id(ElementId::named_usize("chat-details-remove", index))
            .px_2()
            .py_1()
            .bg(theme.surface)
            .cursor_pointer()
            .hover(|s| s.bg(theme.surface_raised))
            .role(Role::Button)
            .aria_label(name)
            .focusable()
            .child("Remove")
            .on_click(remove);
        let row = div()
            .id(ElementId::named_usize("chat-details-member", index))
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .text_size(design::text::SECONDARY)
                    .child(label),
            )
            .when(removable, |row| row.child(remove));
        rows.push(row.into_any_element());
    }
    rows
}
