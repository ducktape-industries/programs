//! What a pressed message offers (Expo `src/chat/Actions.tsx`, design §4.4, boards 40/41/43): who
//! wrote it, quick reactions, Reply in thread, Copy text and link, Who can edit, and Edit or Delete
//! where the reader may; the kit `Dialog` a delete asks first; and who chose a reaction.
use view::prelude::*;
use view::{Div, Stateful};

use super::picker::{self, Cell};
use crate::message::ChatMessage;
use crate::words;
use crate::{CANCEL_DELETE, CONFIRM_DELETE, Chat, Mode, Pane, emoji};
use view::FontWeight;
use view::design::Ladder;

const QUICK: usize = 6;
const ALL_REACTIONS_ICON: Pixels = px(20.);
const SCRIM_LIGHT: f32 = 0.4;
const SCRIM_DARK: f32 = 0.55;

pub(super) fn sheet(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<AnyElement> {
    let menu = chat
        .menu
        .as_ref()
        .filter(|menu| matches!(menu.mode, Mode::Toolbar | Mode::More))?;
    let pane = menu.pane;
    let messages = chat.messages(pane);
    let message = messages.iter().find(|message| message.seq == menu.seq)?;
    let close = cx.listener(|chat, _: &(), _window, cx| {
        chat.close_menu();
        cx.notify();
    });
    let body = div()
        .id("chat-actions")
        .flex()
        .flex_col()
        .child(head(message, theme, ladder))
        .children((!message.deleted).then(|| quick(chat, message, pane, cx, theme)))
        .children(rows(chat, message, pane, cx, theme, ladder));
    Some(
        design::sheet("chat-actions-sheet", words::MESSAGE_ACTIONS, ladder, theme)
            .on_dismiss(close)
            .child(body)
            .into_any_element(),
    )
}

pub(super) fn reactors(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<AnyElement> {
    let menu = chat
        .menu
        .as_ref()
        .filter(|menu| menu.mode == Mode::Reactors)?;
    let messages = chat.messages(menu.pane);
    let message = messages.iter().find(|message| message.seq == menu.seq)?;
    let emoji = &chat.reactors_of;
    let reaction = message
        .reactions
        .iter()
        .find(|reaction| &reaction.emoji == emoji)?;
    let close = cx.listener(|chat, _: &(), _window, cx| {
        chat.close_menu();
        cx.notify();
    });
    let head = design::sheet_head("chat-reactors-head", theme).child(
        div()
            .text_type(ladder.body)
            .child(format!("{emoji} {}", reaction.count)),
    );
    let names = chat
        .reactor_names(menu.seq, emoji)
        .into_iter()
        .enumerate()
        .map(|(at, name)| {
            div()
                .id(format!("chat-reactors-{at}"))
                .px(design::space::BLOCK)
                .py(design::space::MD)
                .border_b_1()
                .border_color(theme.border)
                .truncate()
                .text_type(ladder.body)
                .child(name)
        });
    let body = div()
        .id("chat-reactors")
        .flex()
        .flex_col()
        .child(head)
        .children(names);
    Some(
        design::sheet("chat-reactors-sheet", words::WHO_REACTED, ladder, theme)
            .on_dismiss(close)
            .child(body)
            .into_any_element(),
    )
}

fn head(message: &ChatMessage, theme: &Theme, ladder: &Ladder) -> Stateful<Div> {
    let when = (message.time > 0).then(|| design::clock(message.time));
    let about = [
        Some(message.author.clone()),
        when,
        Some(format!("block {}", message.height)),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    design::sheet_head("chat-actions-head", theme)
        .child(
            div()
                .truncate()
                .text_type(ladder.caption)
                .text_color(theme.muted)
                .child(about),
        )
        .child(
            div()
                .truncate()
                .text_type(ladder.body)
                .child(message.body.clone()),
        )
}

fn quick(
    chat: &Chat,
    message: &ChatMessage,
    pane: Pane,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> Stateful<Div> {
    let (seq, rev) = (message.seq, message.rev);
    let cells: Vec<AnyElement> = emoji::frequent(&chat.recent_emoji)
        .into_iter()
        .take(QUICK)
        .map(|emoji| {
            let mine = message.reacted(&emoji);
            let react = emoji.clone();
            let press = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
                cx.notify();
                chat.close_menu();
                chat.react(seq, react.clone(), !mine, cx)
            });
            picker::cell(
                Cell {
                    id: format!("chat-actions-react-{emoji}").into(),
                    emoji,
                    mine,
                },
                press,
                theme,
            )
        })
        .collect();
    let all = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.show_menu(pane, seq, rev, Mode::Reactions);
    });
    let all = div()
        .id("chat-actions-all-reactions")
        .size(picker::CELL)
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(theme.border_strong)
        .role(Role::Button)
        .aria_label("All reactions")
        .focusable()
        .on_click(all)
        .child(design::icon("plus", ALL_REACTIONS_ICON).text_color(theme.muted));
    div()
        .id("chat-actions-quick")
        .flex()
        .justify_between()
        .px(design::space::BLOCK)
        .py(design::space::MD)
        .border_b_1()
        .border_color(theme.border)
        .children(cells)
        .child(all)
}

fn rows(
    chat: &Chat,
    message: &ChatMessage,
    pane: Pane,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    let (seq, rev, deleted) = (message.seq, message.rev, message.deleted);
    let in_thread = pane == Pane::Thread || message.thread.is_some();
    let edit = chat.editable(pane, seq) && chat.may_edit(pane, seq);
    let delete = chat.may_delete(pane, seq);
    let link = chat.message_link(seq);
    let row =
        |id: &'static str, icon: &'static str, label: &'static str, danger: bool, click: Press| {
            design::sheet_row(id, label, ladder, theme, click)
                .icon(icon)
                .when(danger, |row| row.danger())
                .into_any_element()
        };
    let mut rows = Vec::new();
    if !deleted && !in_thread {
        let reply = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
            cx.notify();
            chat.reply_in_thread(seq, cx);
        });
        rows.push(row(
            "chat-actions-reply",
            "corner-up-left",
            words::REPLY_IN_THREAD,
            false,
            Box::new(reply),
        ));
    }
    if !deleted {
        let text = message.body.clone();
        let copy = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
            cx.notify();
            chat.close_menu();
            chat.copy_text(text.clone(), "message text", cx);
        });
        rows.push(row(
            "chat-actions-copy",
            "copy",
            "Copy text",
            false,
            Box::new(copy),
        ));
    }
    if let Some(link) = link {
        let copy = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
            cx.notify();
            chat.close_menu();
            chat.copy_text(link.clone(), words::MESSAGE_LINK, cx);
        });
        rows.push(row(
            "chat-actions-copy-link",
            "link-2",
            words::COPY_LINK,
            false,
            Box::new(copy),
        ));
    }
    let reopen = |mode: Mode, cx: &mut Context<Chat>| -> Press {
        Box::new(cx.listener(move |chat, _: &ClickEvent, window, cx| {
            cx.notify();
            open(chat, pane, seq, rev, mode, window)
        }))
    };
    let who = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.who_can_edit(pane, seq, cx);
    });
    rows.push(row(
        "chat-actions-who-can-edit",
        "shield",
        policy_view::item::WHO,
        false,
        Box::new(who),
    ));
    if !deleted && edit {
        rows.push(row(
            "chat-actions-edit",
            "pencil",
            words::EDIT_MESSAGE,
            false,
            reopen(Mode::Editing, cx),
        ));
    }
    if !deleted && delete {
        rows.push(row(
            "chat-actions-delete",
            "trash",
            words::DELETE_MESSAGE,
            true,
            reopen(Mode::Delete, cx),
        ));
    }
    rows
}

fn open(chat: &mut Chat, pane: Pane, seq: u64, rev: u32, mode: Mode, window: &mut Window) {
    if !chat.show_menu(pane, seq, rev, mode) {
        return;
    }
    match chat.editing() {
        Some(target) => crate::composer::focus(window, &target.key()),
        None => window.focus(CANCEL_DELETE),
    }
}

type Press = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

pub(super) fn confirm(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<Stateful<Div>> {
    chat.menu
        .as_ref()
        .filter(|menu| menu.mode == Mode::Delete)
        .map(|_| delete_dialog(cx, theme, ladder))
}

pub(super) fn with_confirm(
    screen: AnyElement,
    dialog: Option<Stateful<Div>>,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> AnyElement {
    let cancel = cx.listener(|chat, _: &(), _window, cx| {
        chat.close_menu();
        cx.notify();
    });
    let scrim = match theme.dark {
        true => SCRIM_DARK,
        false => SCRIM_LIGHT,
    };
    modal_overlay(
        "chat-delete-overlay",
        words::DELETE_QUESTION,
        screen,
        dialog,
    )
    .flex()
    .flex_col()
    .justify_center()
    .px(design::space::XL)
    .backdrop(hsla(0., 0., 0., scrim))
    .on_dismiss(cancel)
    .into_any_element()
}

fn delete_dialog(cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> Stateful<Div> {
    let cancel = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.close_menu();
        cx.notify();
    });
    let delete = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.delete_armed(cx)
    });
    let delete = div()
        .id(CONFIRM_DELETE)
        .h(ladder.control)
        .px(design::space::BLOCK)
        .flex()
        .items_center()
        .border_1()
        .border_color(theme.danger)
        .text_color(theme.danger)
        .text_type(ladder.label)
        .font_weight(FontWeight::MEDIUM)
        .role(Role::Button)
        .focusable()
        .on_click(delete)
        .child(words::DELETE);
    let delete = design::pressed(delete, ladder);
    div()
        .id("chat-delete-dialog")
        .w_full()
        .flex()
        .flex_col()
        .pt(design::SHEET_INSET)
        .px(design::SHEET_INSET)
        .pb(design::space::LG)
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.background)
        .role(Role::AlertDialog)
        .aria_label(words::DELETE_QUESTION)
        .occlude()
        .child(
            div()
                .mb(design::space::SM)
                .text_type(ladder.title)
                .font_weight(FontWeight::MEDIUM)
                .child(words::DELETE_QUESTION),
        )
        .child(
            div()
                .text_type(ladder.small)
                .text_color(theme.muted)
                .child("It leaves the room for everyone. What stays is its place in the timeline."),
        )
        .child(
            div()
                .flex()
                .justify_end()
                .gap(design::space::XXS)
                .mt(design::space::BLOCK)
                .child(
                    design::button(CANCEL_DELETE, words::CANCEL, ladder, theme, cancel)
                        .kind(design::Kind::Quiet),
                )
                .child(delete),
        )
}
