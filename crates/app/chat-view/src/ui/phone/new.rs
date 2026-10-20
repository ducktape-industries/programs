//! New channel (Expo `app/(tabs)/chat/new.tsx`, board 54): a name and who
//! may post, Create in the title row; the program's refusal under the
//! field, verbatim. Reads and writes the create form (`Chat::create`);
//! Create sends it, and the channel it made opens in this page's place.
use view::prelude::*;
use view::{Div, Stateful};

use crate::words;
use crate::{ChannelCreate, Chat};
use view::FontWeight;
use view::design::Ladder;

pub(super) fn screen(
    chat: &Chat,
    back: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let Some(create) = &chat.create else {
        return div().into_any_element();
    };
    design::page(
        "chat-new-page",
        words::NEW_CHANNEL,
        design::Nav::back(chat.pages().depth(), back),
        ladder,
        theme,
    )
    .trailing(create_button(create, cx, theme, ladder))
    .child(
        div()
            .id("chat-new-form")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(design::space::BLOCK)
            .p(design::space::BLOCK)
            .child(name_field(create, cx, theme, ladder))
            .child(policy(create, cx, theme, ladder)),
    )
    .into_any_element()
}

fn create_button(
    create: &ChannelCreate,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let enabled = !create.name.is_blank() && !create.busy;
    let label = match create.busy {
        true => "Creating…",
        false => "Create",
    };
    let button = div()
        .id("create")
        .h(ladder.hit)
        .px(design::space::SM)
        .flex()
        .flex_none()
        .items_center()
        .text_type(ladder.body)
        .font_weight(FontWeight::MEDIUM)
        .text_color(if enabled {
            theme.foreground
        } else {
            theme.faint
        })
        .role(Role::Button)
        .aria_label(label)
        .aria_disabled(!enabled)
        .child(label);
    match enabled {
        true => design::pressed(button.focusable(), ladder).on_click(cx.listener(
            |chat, _: &ClickEvent, _window, cx| {
                cx.notify();
                chat.create_channel(cx)
            },
        )),
        false => button,
    }
}

fn name_field(
    create: &ChannelCreate,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Div {
    let submit = cx.listener(|chat, _: &(), _window, cx| {
        cx.notify();
        chat.create_channel(cx)
    });
    let changed = cx.listener(|chat, change: &view::wire::TextChange, window, _| {
        chat.create_name_changed("chat-new-name", change, window)
    });
    let input = Input::new("chat-new-name", &create.name, words::NAME)
        .autocorrect(false)
        .autocapitalize(Autocapitalize::None)
        .on_change(changed)
        .on_submit(submit);
    let problem = (!create.error.is_empty()).then(|| create.error.clone());
    design::field(input, ladder, theme)
        .label(words::NAME)
        .when_some(problem, design::Field::problem)
        .into_element()
}

fn policy(create: &ChannelCreate, cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> Div {
    let pick = cx.processor(|chat, index: usize, _window, cx| {
        if let Some(create) = &mut chat.create {
            create.members_only = index == 1;
        }
        cx.notify();
    });
    let choices = [
        (ElementId::from("chat-new-anyone"), "Anyone".into()),
        (
            ElementId::from("chat-new-members-only"),
            words::MEMBERS_ONLY.into(),
        ),
    ];
    div()
        .flex()
        .flex_col()
        .gap(design::space::XS)
        .child(
            div()
                .text_type(ladder.caption)
                .text_color(theme.muted)
                .child(words::WHO_MAY_POST),
        )
        .child(design::segmented(
            "chat-new-policy",
            words::WHO_MAY_POST,
            ladder,
            theme,
            Some(usize::from(create.members_only)),
            choices,
            pick,
        ))
}
