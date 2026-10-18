//! The desktop's dialogs: creating a channel, and New message, whom a
//! direct message goes to.

use view::prelude::*;

use crate::room_list::DmChoice;
use crate::words;
use crate::{ChannelCreate, Chat};
use view::design::Ladder;

/// How wide the dialog's card grows.
const CARD_MAX_WIDTH: f32 = 480.;

pub(crate) const INSET: Pixels = px(24.);
pub(crate) const DM_PICK_FIELD: &str = "chat-dm-pick-search";

pub fn channel_create(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<AnyElement> {
    let create = chat.create.as_ref()?;
    let busy = create.busy;
    let can_submit = !busy && chat.session.connected && chat.holds_account();
    let cancel = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.create = None;
        cx.notify();
    });
    let submit = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.create_channel(cx)
    });
    let card = div()
        .id("chat-create-card")
        .max_w(px(CARD_MAX_WIDTH))
        .flex()
        .flex_col()
        .gap_2()
        .p_5()
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .shadow_lg()
        .child(
            div()
                .text_size(design::text::TITLE)
                .child("Create a channel"),
        )
        .child(
            div()
                .text_size(design::text::SECONDARY)
                .text_color(theme.muted)
                .child("Channel name"),
        )
        .child(name_field(create, can_submit, cx, theme))
        .children(notes(chat, create, theme))
        // one row, as the other views' dialogs: the option, then Cancel
        // beside the primary action
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(members_only(create, cx, theme, ladder))
                .child(div().flex_1())
                .child(
                    design::button("chat-create-cancel", words::CANCEL, ladder, theme, cancel)
                        .enabled(!busy),
                )
                .child(
                    design::button(
                        "chat-create-submit",
                        "Create channel",
                        ladder,
                        theme,
                        submit,
                    )
                    .kind(design::Kind::Primary)
                    .enabled(can_submit),
                ),
        );
    Some(card.into_any_element())
}

/// The name, typed; Enter creates where the dialog may.
fn name_field(
    create: &ChannelCreate,
    can_submit: bool,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> Input {
    let name = Input::new("chat-create-name", &create.name, "Name the new channel")
        .h(design::size::CONTROL)
        .px_2()
        .py_1()
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.surface)
        .placeholder("Channel name")
        .disabled(create.busy);
    match can_submit {
        true => name.on_submit(cx.listener(|chat, _: &(), _window, cx| {
            cx.notify();
            chat.create_channel(cx)
        })),
        false => name,
    }
}

/// Members only, a toggle: on, posting takes a seat.
fn members_only(
    create: &ChannelCreate,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let toggle = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        if let Some(create) = &mut chat.create {
            create.members_only = !create.members_only;
        }
        cx.notify();
    });
    design::button(
        "chat-create-members",
        words::MEMBERS_ONLY,
        ladder,
        theme,
        toggle,
    )
    .selected(create.members_only)
    .enabled(!create.busy)
    .into_any_element()
}

/// Why the dialog failed, and why it cannot create at all.
fn notes(chat: &Chat, create: &ChannelCreate, theme: &Theme) -> Vec<AnyElement> {
    let note = |text: String, color| {
        div()
            .text_size(design::text::SECONDARY)
            .text_color(color)
            .child(text)
            .into_any_element()
    };
    let mut notes = Vec::new();
    if !create.error.is_empty() {
        notes.push(note(create.error.clone(), theme.danger));
    }
    if !chat.holds_account() {
        let why = "Create an account to create a channel".to_owned();
        notes.push(note(why, theme.muted));
    }
    notes
}

pub fn dm_pick(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<AnyElement> {
    let pick = chat.dm_pick.as_ref()?;
    let choices = chat.dm_choices(&pick.query.text());
    let at = pick
        .cursor
        .and_then(|peer| choices.iter().position(|choice| choice.peer == peer))
        .unwrap_or(0);
    let submit = cx.listener(|chat, _: &(), _window, cx| {
        cx.notify();
        chat.open_first_dm(cx)
    });
    let cancel = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.dm_pick = None;
        cx.notify();
    });
    let field = Input::new(DM_PICK_FIELD, &pick.query, words::FIND_SOMEONE)
        .h(design::size::CONTROL)
        .px_2()
        .py_1()
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.surface)
        .placeholder(words::SEARCH_PEOPLE)
        .on_submit(submit);
    let tallest = chat.layout.viewport.1 - 2. * f32::from(INSET);
    let card = div()
        .id("chat-dm-pick-card")
        .w_full()
        .max_w(px(CARD_MAX_WIDTH))
        .max_h(px(tallest))
        .flex()
        .flex_col()
        .gap_2()
        .p_5()
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .shadow_lg()
        .child(
            div()
                .text_size(design::text::TITLE)
                .child(words::NEW_MESSAGE),
        )
        .child(field)
        .child(choices_list(chat, &choices, at, cx, theme, ladder))
        .child(
            div().flex().child(div().flex_1()).child(design::button(
                "chat-dm-pick-cancel",
                words::CANCEL,
                ladder,
                theme,
                cancel,
            )),
        );
    Some(card.into_any_element())
}

fn choices_list(
    chat: &Chat,
    choices: &[DmChoice],
    at: usize,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let note = |id: &'static str, text: &'static str| {
        div()
            .id(id)
            .text_size(design::text::SECONDARY)
            .text_color(theme.muted)
            .child(text)
            .into_any_element()
    };
    if !chat.holds_account() {
        return note("chat-dm-pick-no-account", words::DM_NEEDS_ACCOUNT);
    }
    if chat.names.ready().is_none() {
        return note("chat-dm-pick-loading", words::LOADING_PEOPLE);
    }
    if choices.is_empty() {
        return note("chat-dm-pick-none", words::NO_ONE_MATCHES);
    }
    let peers: Vec<u64> = choices.iter().map(|choice| choice.peer).collect();
    let walked = peers.clone();
    let list = design::composite("chat-dm-pick-list", Role::ListBox, words::NEW_MESSAGE)
        .active(at, peers.len())
        .on_move(cx.processor(move |chat, index: usize, _, cx| {
            if let Some(pick) = &mut chat.dm_pick {
                pick.cursor = walked.get(index).copied();
            }
            cx.notify();
        }))
        .on_press(cx.processor(move |chat, index: usize, _, cx| {
            let Some(peer) = peers.get(index).copied() else {
                return;
            };
            cx.notify();
            chat.open_dm(peer, cx)
        }))
        .build()
        .flex()
        .flex_col()
        .gap_1()
        .children(
            choices
                .iter()
                .enumerate()
                .map(|(index, choice)| choice_row(choice, index == at, cx, theme, ladder)),
        );
    div()
        .id("chat-dm-pick-scroll")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .child(list)
        .into_any_element()
}

fn choice_row(
    choice: &DmChoice,
    active: bool,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let peer = choice.peer;
    let id = format!("chat-dm-pick-{peer}");
    let click = cx.listener(move |chat, _: &ClickEvent, _, cx| {
        cx.notify();
        chat.open_dm(peer, cx)
    });
    let row = div()
        .id(id.clone())
        .flex()
        .items_center()
        .gap_2()
        .min_h(design::size::CONTROL)
        .px_1()
        .hover(|s| s.bg(theme.surface_raised))
        .when(active, |row| row.bg(theme.surface_raised))
        .aria_label(choice.name.clone())
        .when(choice.agent, |row| row.aria_description(words::AGENT))
        .aria_selected(active)
        .on_click(click)
        .child(super::sidebar::avatar(
            format!("{id}-avatar"),
            &choice.name,
            choice.agent,
            theme.surface_raised,
            theme,
        ))
        .child(div().flex_1().truncate().child(choice.name.clone()))
        .when(choice.agent, |row| {
            row.child(super::badge(
                format!("{id}-agent"),
                words::AGENT,
                ladder,
                theme.agent,
                theme.agent_soft,
            ))
        })
        .child(
            div()
                .text_size(design::text::CAPTION)
                .text_color(theme.muted)
                .child(words::account(peer)),
        );
    design::item(row, Role::ListBoxOption, active).into_any_element()
}
