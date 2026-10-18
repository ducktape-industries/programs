//! The composer band at a room's or a thread's foot (Expo
//! `src/chat/Composer.tsx`): the people `@` offers, the bar over an edit,
//! one row of the field and Send (Save edit while editing), the note a
//! refusal left, and the way back to a send a restart cut off. Or, where
//! the reader may not write, the strip that says why. Reads the target's
//! draft and the mention choices; every key and press goes through the
//! draft ([`crate::composer`]), which writes it.
use std::rc::Rc;

use view::prelude::*;
use view::{Deferred, Div, Stateful};

use super::parts::{self, NoticeKind, face};
use crate::composer::{
    Click, Draft, Event, Handle, MentionChoice, Presses, Target, chips, dropped, editor, editor_id,
    matching_choices, mention_popup, opening, picks, press, presses,
};
use crate::words;
use crate::{Chat, Pane};
use view::FontWeight;
use view::design::Ladder;

/// The field grows to five lines, then scrolls (mobile
/// `src/chat/Composer.tsx:182`).
const FIELD_LINES: f32 = 5.;
/// The pencil over an edit (mobile `src/chat/Composer.tsx:154`).
const PENCIL: Pixels = px(18.);

/// What a pane's foot holds: the composer for `post`, or for the message
/// the pane is editing in its place; or the write gate's strip.
pub(super) fn band(
    chat: &Chat,
    pane: Pane,
    post: Target,
    placeholder: &str,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    if let Some(gate) = chat.write_gate() {
        return parts::gate_strip(chat, gate, cx, theme, ladder);
    }
    let editing = chat
        .editing()
        .filter(|_| chat.menu.as_ref().is_some_and(|menu| menu.pane == pane));
    let edits = editing.is_some();
    let original = editing.as_ref().and(original(chat, pane));
    let target = editing.unwrap_or(post);
    let key = target.key();
    // the draft is seated by `Chat::render` before the frame
    let Some(draft) = chat.drafts.get(&key) else {
        return div().into_any_element();
    };
    let label = match &target {
        Target::Post { thread: None, .. } => words::NEW_MESSAGE,
        Target::Post {
            thread: Some(_), ..
        } => words::REPLY,
        Target::Edit { .. } => words::EDITED_MESSAGE,
    };
    let handle: Handle<Chat> =
        Rc::new(move |chat, event, window, cx| chat.composer(target.clone(), event, window, cx));
    let editable = chat.session.connected;
    let editor_id = editor_id(&key);
    let field = editor(draft, &editor_id, label, placeholder, editable, &handle, cx);
    let send = press(
        editable && draft.can_send(),
        "send".into(),
        &editor_id,
        &handle,
        cx,
    );
    let presses = presses(editable, &editor_id, &handle, cx);
    let attach = match edits {
        true => None,
        false => {
            let click = press(editable, "attach".into(), &editor_id, &handle, cx);
            Some(attach_button(&key, click, theme, ladder))
        }
    };
    let chips = chips(draft, &key, editable, &handle, cx, ladder);
    let restore =
        draft
            .failed_send
            .as_ref()
            .and(press(editable, "restore".into(), &editor_id, &handle, cx));
    let column = div()
        .flex()
        .flex_col()
        .border_t_1()
        .border_color(theme.border)
        .bg(theme.background)
        .children(mentions(chat, draft, &key, presses, theme, ladder))
        .children(original.map(|original| edit_bar(&key, original, cx, theme, ladder)))
        .children(chips)
        .child(field_row(draft, field, attach, send, edits, theme, ladder))
        .children((!draft.note.is_empty()).then(|| {
            div()
                .px(design::space::BLOCK)
                .pb(design::space::SM)
                .text_type(ladder.secondary)
                .text_color(theme.danger)
                .child(draft.note.clone())
        }))
        .children(restore.map(|restore| {
            let action =
                parts::notice_action(format!("{key}/restore"), words::RESTORE, ladder, restore);
            parts::notice(
                format!("{key}/failed"),
                NoticeKind::Danger,
                None,
                words::UNSENT,
                Some(action),
                theme,
                ladder,
            )
        }));
    let column = match edits {
        true => column.into_any_element(),
        false => {
            let (edge, ground) = (theme.primary, theme.accent_soft);
            column
                .id(ElementId::Name(format!("{key}/drop").into()))
                .on_file_drop(dropped(&handle, cx))
                .file_drag_over(move |style| style.border_color(edge).bg(ground))
                .into_any_element()
        }
    };
    let placed = handle.clone();
    sensor(ElementId::Name(key.into()), column)
        .on_bounds(
            cx.listener(move |chat, bounds: &Bounds<Pixels>, window, cx| {
                placed(chat, Event::Placed(bounds.into()), window, cx);
                cx.notify();
            }),
        )
        .into_any_element()
}

/// What the message being edited said, for the bar over the field.
fn original(chat: &Chat, pane: Pane) -> Option<String> {
    let seq = chat.menu.as_ref()?.seq;
    let messages = chat.messages(pane);
    let message = messages.iter().find(|message| message.seq == seq)?;
    Some(message.body.clone())
}

fn mentions(
    chat: &Chat,
    draft: &Draft,
    key: &str,
    presses: Option<Presses>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<Deferred> {
    let (_, partial) = draft.query()?;
    let opening = opening(draft.placed?, chat.layout.pane?)?;
    let choices = chat.mention_choices();
    let names = chat.names.ready();
    let matches: Rc<[(MentionChoice, bool)]> = matching_choices(&choices, &partial)
        .into_iter()
        .map(|choice| (choice.clone(), agent(names, choice)))
        .collect();
    if matches.is_empty() {
        return None;
    }
    let row_key = key.to_owned();
    let (row_theme, row_ladder) = (*theme, *ladder);
    let rows = uniform_list(
        ElementId::Name(format!("{key}/mentions").into()),
        matches.len(),
        move |range, _, _| {
            range
                .map(|index| {
                    let (choice, agent) = &matches[index];
                    let tag = format!("mention:{}", choice.token);
                    let pick = presses.as_ref().map(|presses| picks(presses, tag));
                    mention_row(&row_key, choice, *agent, pick, &row_theme, &row_ladder)
                })
                .collect::<Vec<_>>()
        },
    )
    .track_scroll(&draft.menu_scroll.0);
    Some(mention_popup(key, opening, rows, theme))
}

fn agent(names: Option<&chat::view::Names>, choice: &MentionChoice) -> bool {
    let account = crate::names::mentioned(&choice.token);
    names.zip(account).is_some_and(|(names, account)| {
        crate::message::agent(names, &chat::Principal::Account(account))
    })
}

/// One person `@` offers.
fn mention_row(
    key: &str,
    choice: &MentionChoice,
    agent: bool,
    pick: Option<Click>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let account = crate::names::mentioned(&choice.token);
    let row = div()
        .id(ElementId::Name(
            format!("{key}/mention/{}", choice.token).into(),
        ))
        .h(ladder.row)
        .flex()
        .items_center()
        .gap(design::space::LG)
        .px(design::space::BLOCK)
        .role(Role::Button)
        .aria_label(choice.label.clone())
        .focusable()
        .child(face(&choice.label, agent, ladder.avatar_inline, theme))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_type(ladder.body)
                .child(choice.label.clone()),
        )
        .children(account.map(|account| {
            div()
                .text_type(ladder.caption)
                .text_color(theme.muted)
                .child(words::account(account))
        }));
    match pick {
        Some(pick) => design::pressed(row, ladder).on_click(pick),
        None => row.aria_disabled(true),
    }
}

/// Over the field while it edits a message: a pencil, "Editing message"
/// over what the message said, and the X that stops it.
fn edit_bar(
    key: &str,
    original: String,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Div {
    let stop = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.close_menu();
        cx.notify();
    });
    div()
        .flex()
        .items_center()
        .gap(design::space::SM)
        .pl(design::space::BLOCK)
        .pr(design::space::XS)
        .bg(theme.surface)
        .border_b_1()
        .border_color(theme.border)
        .child(design::icon("pencil", PENCIL).text_color(theme.muted))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .py(design::space::XS)
                .child(
                    div()
                        .text_type(ladder.secondary)
                        .font_weight(FontWeight::MEDIUM)
                        .child("Editing message"),
                )
                .child(
                    div()
                        .truncate()
                        .text_type(ladder.caption)
                        .text_color(theme.muted)
                        .child(original),
                ),
        )
        .child(design::icon_button(
            ElementId::Name(format!("{key}/cancel").into()),
            design::icon("x", ladder.icon),
            "Stop editing",
            ladder,
            theme,
            stop,
        ))
}

/// The field, growing to five lines (its edge danger while it holds a
/// note), and Send beside it: the ink square once there is something to
/// send; Save edit while it edits.
fn field_row(
    draft: &Draft,
    field: Textarea,
    attach: Option<AnyElement>,
    send: Option<Click>,
    editing: bool,
    theme: &Theme,
    ladder: &Ladder,
) -> Div {
    let tallest = ladder
        .body
        .line
        .map(|line| line * FIELD_LINES + design::space::MD * 2.);
    let field = field
        .flex_1()
        .min_h(ladder.control)
        .when_some(tallest, |field, tallest| field.max_h(tallest))
        .px(design::space::LG)
        .py(design::space::MD)
        .text_type(ladder.body)
        .border_1()
        .border_color(match draft.note.is_empty() {
            true => theme.border_strong,
            false => theme.danger,
        })
        .bg(theme.background);
    let (glyph, name) = match editing {
        true => ("check", "Save edit"),
        false => ("send-horizontal", words::SEND),
    };
    let sendable = send.is_some();
    let click: Click = send.unwrap_or_else(|| Box::new(|_, _, _| {}));
    let send = design::icon_button(
        "send",
        design::icon(glyph, ladder.icon),
        name,
        ladder,
        theme,
        click,
    )
    .filled(sendable)
    .enabled(sendable);
    div()
        .flex()
        .items_end()
        .gap(design::space::SM)
        .pl(design::space::BLOCK)
        .pr(design::space::LG)
        .py(design::space::SM)
        .children(attach)
        .child(field)
        .child(send)
}

// The way to the host's picker, beside the field.
fn attach_button(key: &str, click: Option<Click>, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let enabled = click.is_some();
    let click: Click = click.unwrap_or_else(|| Box::new(|_, _, _| {}));
    let glyph = design::icon("paperclip", ladder.icon);
    let id = ElementId::Name(format!("{key}/attach").into());
    design::icon_button(id, glyph, words::ATTACH, ladder, theme, click)
        .enabled(enabled)
        .into_any_element()
}
