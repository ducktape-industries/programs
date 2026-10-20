//! What a room's details offer for its notices: one radio group, the same
//! on the desktop's pane and the phone's page. A choice is chat's, on chain
//! (`Chat::choose_notices`), so every device and server tells the same.
use chat::Notices;
use chat::view::Names;
use view::design::Ladder;
use view::prelude::*;
use view::{Div, Stateful};

use crate::Chat;
use crate::notifier::Choice;
use crate::words;

fn offered(dm: bool) -> &'static [Choice] {
    const ROOM: &[Choice] = &[
        Choice::Every,
        Choice::Default,
        Choice::Cut(Notices::Mentions),
        Choice::Cut(Notices::Off),
    ];
    const DIRECT: &[Choice] = &[Choice::Default, Choice::Cut(Notices::Off)];
    if dm { DIRECT } else { ROOM }
}

pub(crate) fn choices(
    chat: &Chat,
    room: &str,
    dm: bool,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let chosen = chat.choice(room).map(|choice| match choice {
        Choice::Every | Choice::Cut(Notices::Mentions) if dm => Choice::Default,
        choice => choice,
    });
    let rows = offered(dm)
        .iter()
        .map(|&choice| (choice, words::choice(choice, dm)))
        .collect();
    let group = Group {
        id: "chat-details-notices".into(),
        label: words::NOTIFICATIONS.into(),
        rows,
        chosen,
        at: chat.details.as_ref().and_then(|details| details.notices_at),
        writes: chat.holds_account() && chat.session.connected,
        whose: None,
    };
    radios(group, cx, theme, ladder)
}

pub(crate) fn managed(
    chat: &Chat,
    room: &str,
    dm: bool,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<Stateful<Div>> {
    let Some(managed) = chat.managed_choices.ready().filter(|_| !dm) else {
        return Vec::new();
    };
    let writes = chat.holds_account() && chat.session.connected;
    let mut groups = Vec::new();
    for (account, choices) in managed {
        let account = *account;
        let principal = chat::Principal::Account(account);
        let name = chat.names.ready().map_or_else(
            || Names::empty().member(&principal),
            |names| names.member(&principal),
        );
        let label = words::managed_notices(&name);
        let rows = offered(false)
            .iter()
            .map(|&choice| (choice, words::managed_choice(choice)))
            .collect();
        let at = chat.details.as_ref().map(|details| &details.managed_at);
        let group = Group {
            id: format!("chat-details-notices-{account}"),
            label: label.clone(),
            rows,
            chosen: Some(choices.get(room).copied().unwrap_or_default()),
            at: at.and_then(|at| at.get(&account).copied()),
            writes,
            whose: Some(account),
        };
        let head = format!("chat-details-notices-{account}-head");
        groups.push(
            div()
                .id(format!("chat-details-managed-{account}"))
                .flex()
                .flex_col()
                .child(design::section_head(head, label, ladder, theme))
                .child(radios(group, cx, theme, ladder)),
        );
    }
    groups
}

struct Group {
    id: String,
    label: String,
    rows: Vec<(Choice, (&'static str, &'static str))>,
    chosen: Option<Choice>,
    at: Option<usize>,
    writes: bool,
    whose: Option<u64>,
}

fn radios(group: Group, cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> Stateful<Div> {
    let Group {
        id,
        label,
        rows,
        chosen,
        at,
        writes,
        whose,
    } = group;
    let offered: Vec<Choice> = rows.iter().map(|(choice, _)| *choice).collect();
    let at = at
        .or_else(|| offered.iter().position(|choice| Some(*choice) == chosen))
        .unwrap_or_default()
        .min(offered.len() - 1);
    let count = offered.len();
    let mut radios = design::composite(id.clone(), Role::RadioGroup, label)
        .active(at, count)
        .wrap()
        .on_move(cx.processor(move |chat, index: usize, _, cx| {
            chat.step_notices(whose, index, cx);
        }))
        .on_press(cx.processor(move |chat, index: usize, _, cx| {
            if writes && let Some(choice) = offered.get(index) {
                chat.choose_notices(whose, *choice, cx);
            }
        }))
        .build()
        .flex()
        .flex_col();
    for (index, (choice, (label, about))) in rows.into_iter().enumerate() {
        let on = chosen == Some(choice);
        let pick = cx.listener(move |chat, _: &ClickEvent, _, cx| {
            chat.choose_notices(whose, choice, cx);
        });
        let row = design::row(format!("{id}-{}", slug(choice)), ladder, theme)
            .min_h(ladder.row_tall)
            .child(
                design::row_words(design::row_title(label, ladder, theme))
                    .child(design::row_caption(about, ladder, theme)),
            )
            .child(mark(on, theme))
            .aria_toggled(on.into())
            .map(|row| match writes {
                true => design::pressed(row.cursor_pointer().on_click(pick), ladder),
                false => row.aria_disabled(true),
            });
        radios = radios.child(design::item(row, Role::RadioButton, index == at));
    }
    radios
}

fn slug(choice: Choice) -> &'static str {
    match choice {
        Choice::Every => "every",
        Choice::Default => "default",
        Choice::Cut(Notices::Mentions) => "mentions",
        Choice::Cut(Notices::Off) => "off",
    }
}

fn mark(on: bool, theme: &Theme) -> Div {
    div()
        .size(px(14.))
        .flex_none()
        .rounded_full()
        .border_1()
        .border_color(if on {
            theme.foreground
        } else {
            theme.border_strong
        })
        .when(on, |dot| dot.bg(theme.foreground))
}
