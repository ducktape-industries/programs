//! New message, a page of its own: the search, then each person and agent a direct message
//! may go to (`Chat::dm_choices`). Reads the picker and the roster; a row, or the keyboard's
//! return on the first match, opens their room in this page's place.
use view::prelude::*;

use super::parts::{face, quiet};
use crate::room_list::DmChoice;
use crate::ui::dialogs::DM_PICK_FIELD;
use crate::words;
use crate::{Chat, DmPick};
use view::Div;
use view::design::Ladder;

pub(super) fn screen(
    chat: &Chat,
    back: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let Some(pick) = &chat.dm_pick else {
        return div().into_any_element();
    };
    let choices = chat.dm_choices(&pick.query.text());
    design::page(
        "chat-dm-pick-page",
        words::NEW_MESSAGE,
        design::Nav::back(chat.pages().depth(), back),
        ladder,
        theme,
    )
    .child(field(pick, cx, theme, ladder))
    .child(choices_list(chat, &choices, cx, theme, ladder))
    .into_any_element()
}

fn field(pick: &DmPick, cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> Div {
    let submit = cx.listener(|chat, _: &(), _window, cx| {
        cx.notify();
        chat.open_first_dm(cx)
    });
    let input = Input::new(DM_PICK_FIELD, &pick.query, words::FIND_SOMEONE)
        .autocorrect(false)
        .autocapitalize(Autocapitalize::None)
        .placeholder(words::SEARCH_PEOPLE)
        .on_submit(submit);
    div()
        .flex_none()
        .p(design::space::BLOCK)
        .child(design::field(input, ladder, theme).lead("search"))
}

fn choices_list(
    chat: &Chat,
    choices: &[DmChoice],
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let note = |id: &'static str, text: &'static str| quiet(id, text, theme, ladder);
    if !chat.holds_account() {
        return note("chat-dm-pick-no-account", words::DM_NEEDS_ACCOUNT).into_any_element();
    }
    if chat.names.ready().is_none() {
        return note("chat-dm-pick-loading", words::LOADING_PEOPLE).into_any_element();
    }
    if choices.is_empty() {
        return note("chat-dm-pick-none", words::NO_ONE_MATCHES).into_any_element();
    }
    div()
        .id("chat-dm-pick-list")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .children(choices.iter().map(|choice| {
            let picture = chat.faces.of(choice.peer, cx);
            choice_row(choice, picture, cx, theme, ladder)
        }))
        .into_any_element()
}

fn choice_row(
    choice: &DmChoice,
    picture: Option<std::sync::Arc<view::Image>>,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let peer = choice.peer;
    let id = format!("chat-dm-pick-{peer}");
    let open = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.open_dm(peer, cx)
    });
    let words_ = div()
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .child(
            div()
                .truncate()
                .text_type(ladder.body)
                .child(choice.name.clone()),
        )
        .child(
            div()
                .truncate()
                .text_type(ladder.caption)
                .text_color(theme.muted)
                .child(words::account(peer)),
        );
    design::row(id.clone(), ladder, theme)
        .min_h(ladder.row_tall)
        .role(Role::Button)
        .aria_label(choice.name.clone())
        .when(choice.agent, |row| row.aria_description(words::AGENT))
        .focusable()
        .on_click(open)
        .child(design::lead(face(
            &choice.name,
            choice.agent,
            picture,
            ladder.avatar,
            theme,
        )))
        .child(words_)
        .when(choice.agent, |row| {
            row.child(design::badge(
                format!("{id}-agent"),
                words::AGENT,
                ladder,
                theme.agent,
                theme.agent_soft,
            ))
        })
        .into_any_element()
}
