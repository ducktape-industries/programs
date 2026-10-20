//! What a room's details offer for its notices, alike on the desktop's pane and the phone's page:
//! the reader's own radio group, then a row for each agent they manage (and one for all of them)
//! whose button names its choice and opens the four, a menu at it or, under a finger, a sheet.
use chat::Notices;
use chat::view::Names;
use view::design::Ladder;
use view::prelude::*;
use view::{Div, Stateful};

use crate::notifier::Choice;
use crate::{Chat, DetailsSheet, NoticesMenu, words};

pub(crate) const OFFERED: &[Choice] = &[
    Choice::Every,
    Choice::Default,
    Choice::Cut(Notices::Mentions),
    Choice::Cut(Notices::Off),
];

pub(crate) const MENU: &str = "chat-notices-menu";

fn offered(dm: bool) -> &'static [Choice] {
    const DIRECT: &[Choice] = &[Choice::Default, Choice::Cut(Notices::Off)];
    if dm { DIRECT } else { OFFERED }
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
    };
    radios(group, cx, theme, ladder)
}

pub(crate) fn agents(
    chat: &Chat,
    dm: bool,
    phone: bool,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    let Some(chosen) = chat.managed_choices.ready().filter(|_| !dm) else {
        return Vec::new();
    };
    let writes = chat.holds_account() && chat.session.connected;
    let mut rows = Vec::new();
    if chosen.len() > 1 {
        let all = Agent {
            whose: None,
            name: words::ALL_AGENTS.to_owned(),
            choice: chat.agents_choice(None),
        };
        rows.push(agent_row(chat, all, writes, phone, cx, theme, ladder));
    }
    for (account, choice) in chosen {
        let agent = Agent {
            whose: Some(*account),
            name: name_of(chat, *account),
            choice: Some(*choice),
        };
        rows.push(agent_row(chat, agent, writes, phone, cx, theme, ladder));
    }
    rows
}

struct Agent {
    whose: Option<u64>,
    name: String,
    choice: Option<Choice>,
}

fn agent_row(
    chat: &Chat,
    agent: Agent,
    writes: bool,
    phone: bool,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let Agent {
        whose,
        name,
        choice,
    } = agent;
    let said = choice.map_or(words::MIXED, |choice| words::managed_choice(choice).0);
    let menu = chat
        .notices_menu
        .as_ref()
        .filter(|menu| menu.whose == whose);
    let open = menu.is_some();
    let waits = menu.is_some_and(|menu| menu.at.is_none());
    let press = cx.listener(move |chat, _: &ClickEvent, _window, cx| match phone {
        true => {
            chat.open_details_sheet(DetailsSheet::Notices(whose));
            cx.notify();
        }
        false => chat.open_notices_menu(whose, cx),
    });
    let placed = cx.listener(|chat, bounds: &Bounds<Pixels>, window, cx| {
        chat.place_notices_menu(*bounds, window, cx)
    });
    let button = div()
        .id(pick_id(whose))
        .flex_none()
        .flex()
        .items_center()
        .gap(design::space::XS)
        .h(ladder.control)
        .px(design::space::SM)
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.background)
        .text_type(ladder.secondary)
        .whitespace_nowrap()
        .role(Role::Button)
        .aria_label(format!("{}: {said}", heard(chat, whose)))
        .aria_expanded(open)
        .focusable()
        .child(said)
        .child(design::icon("chevron-down", ladder.icon).text_color(theme.muted))
        .map(|button| match writes {
            true => design::pressed(button.cursor_pointer().on_click(press), ladder),
            false => button.aria_disabled(true),
        })
        .when(waits, |button| {
            button.relative().child(
                sensor("chat-notices-menu-anchor", div().size_full())
                    .absolute()
                    .inset_0()
                    .on_bounds(placed),
            )
        });
    design::row(row_id(whose), ladder, theme)
        .child(design::row_words(design::row_title(name, ladder, theme)))
        .child(button)
        .into_any_element()
}

pub(crate) fn heard(chat: &Chat, whose: Option<u64>) -> String {
    match whose {
        Some(account) => words::managed_notices(&name_of(chat, account)),
        None => words::ALL_AGENTS_HEAR.to_owned(),
    }
}

fn name_of(chat: &Chat, account: u64) -> String {
    let principal = chat::Principal::Account(account);
    chat.names.ready().map_or_else(
        || Names::empty().member(&principal),
        |names| names.member(&principal),
    )
}

fn row_id(whose: Option<u64>) -> String {
    match whose {
        Some(account) => format!("chat-details-agent-{account}"),
        None => "chat-details-agents-all".to_owned(),
    }
}

pub(crate) fn pick_id(whose: Option<u64>) -> String {
    format!("{}-pick", row_id(whose))
}

pub(crate) fn over(
    chat: &Chat,
    screen: AnyElement,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let menu = chat
        .notices_menu
        .as_ref()
        .filter(|_| chat.details.is_some())
        .and_then(|menu| popup(chat, menu, cx, theme, ladder));
    let close = cx.listener(|chat, _: &(), window, cx| chat.close_notices_menu(window, cx));
    modal_overlay(
        "chat-notices-menu-overlay",
        words::AGENTS_NOTICES,
        screen,
        menu,
    )
    .on_dismiss(close)
    .into_any_element()
}

fn popup(
    chat: &Chat,
    menu: &NoticesMenu,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<design::Menu> {
    let (whose, at) = (menu.whose, menu.at?);
    let held = chat.agents_choice(whose);
    let moved = cx.processor(|chat, index: usize, _, cx| chat.move_in_notices_menu(index, cx));
    let popup = design::menu(MENU, heard(chat, whose), at, theme)
        .active(menu.active)
        .on_move(moved);
    let popup = OFFERED.iter().fold(popup, |popup, &choice| {
        let pick = cx.listener(move |chat, _: &ClickEvent, window, cx| {
            chat.pick_in_notices_menu(whose, choice, window, cx)
        });
        let id = format!("{MENU}/{}", slug(choice));
        let item = design::menu_item(id, words::managed_choice(choice).0, ladder, theme, pick);
        popup.item(match held == Some(choice) {
            true => item.icon("check"),
            false => item,
        })
    });
    Some(popup)
}

pub(crate) fn sheet_rows(
    chat: &Chat,
    whose: Option<u64>,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    let held = chat.agents_choice(whose);
    OFFERED
        .iter()
        .map(|&choice| {
            let pick = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
                chat.close_details_sheet();
                chat.choose_for_agents(whose, choice, cx);
            });
            let (label, about) = words::managed_choice(choice);
            let id = format!("chat-details-notices-sheet/{}", slug(choice));
            let row = design::sheet_row(id, label, ladder, theme, pick).hint(about);
            match held == Some(choice) {
                true => row.icon("check").into_any_element(),
                false => row.into_any_element(),
            }
        })
        .collect()
}

struct Group {
    id: String,
    label: String,
    rows: Vec<(Choice, (&'static str, &'static str))>,
    chosen: Option<Choice>,
    at: Option<usize>,
    writes: bool,
}

fn radios(group: Group, cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> Stateful<Div> {
    let Group {
        id,
        label,
        rows,
        chosen,
        at,
        writes,
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
            chat.step_notices(index, cx);
        }))
        .on_press(cx.processor(move |chat, index: usize, _, cx| {
            if writes && let Some(choice) = offered.get(index) {
                chat.choose_notices(*choice, cx);
            }
        }))
        .build()
        .flex()
        .flex_col();
    for (index, (choice, (label, about))) in rows.into_iter().enumerate() {
        let on = chosen == Some(choice);
        let pick = cx.listener(move |chat, _: &ClickEvent, _, cx| {
            chat.choose_notices(choice, cx);
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
