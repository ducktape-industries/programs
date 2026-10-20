//! The roster: every account of the network under People, Agents and
//! Programs. Expo `app/(tabs)/members/index.tsx` (board 90).
//!
//! Reads the roster (`Members::roster`, `Members::shown`), the chip, the
//! filter and the session's network and reader. Writes the chip (`only`),
//! the filter, whether the filter is open (`filtering`), the arrows'
//! `cursor`, and the page stack: a row opens its member
//! (`Members::open`).
use view::design::{self, CAPSULE_CLEARANCE, Ladder, Nav, space};
use view::prelude::*;
use view::{Div, Stateful};

use super::parts::{face, name_line, quiet};
use crate::{Group, Members, Roster, Row, name_or_number, standing_badge, words};

const FILTER_ID: &str = "members-filter";
const CHIP: Pixels = px(32.);
const COUNT_ON_INK: f32 = 0.7;

pub(super) fn page(
    view: &Members,
    cx: &mut Context<Members>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let count = view.rows.ready().map_or(0, Vec::len);
    let body = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .when(count > 0, |body| body.child(chips(view, cx, ladder, theme)))
        .child(list(view, cx, ladder, theme));
    if view.filtering {
        return div()
            .id("members-roster")
            .flex()
            .flex_col()
            .size_full()
            .child(filter_row(view, cx, ladder, theme))
            .child(body)
            .into_any_element();
    }
    let open_filter = cx.listener(|view, _: &ClickEvent, window, cx| {
        view.filtering = true;
        window.focus(FILTER_ID);
        cx.notify();
    });
    let me = view
        .me
        .and_then(|me| view.all().iter().find(|row| row.number == me));
    let me_picture = me.and_then(|me| view.picture(me, cx));
    let page = design::page(
        "members-roster",
        words::MEMBERS,
        Nav::root_with(reader(view), me_picture),
        ladder,
        theme,
    );
    let page = match view.network() {
        "" => page,
        network => page.caption(format!("{network} · {}", words::accounts(count))),
    };
    page.trailing(design::icon_button(
        "members-filter-open",
        design::icon("search", ladder.icon),
        words::FILTER,
        ladder,
        theme,
        open_filter,
    ))
    .child(body)
    .into_any_element()
}

fn reader(view: &Members) -> String {
    let rows = view.rows.ready().map(Vec::as_slice).unwrap_or_default();
    view.me
        .and_then(|me| rows.iter().find(|row| row.number == me))
        .map_or_else(String::new, |row| row.name.clone())
}

fn filter_row(
    view: &Members,
    cx: &mut Context<Members>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let close = cx.listener(|view, _: &ClickEvent, _, cx| {
        view.filtering = false;
        view.filter.reset("");
        view.cursor = None;
        cx.notify();
    });
    let input = Input::new(FILTER_ID, &view.filter, words::FILTER_LABEL)
        .autocorrect(false)
        .autocapitalize(Autocapitalize::None)
        .placeholder(words::FILTER_HINT);
    let field = design::field(input, ladder, theme).lead("search");
    div()
        .id("members-roster-title")
        .flex_none()
        .h(ladder.header)
        .flex()
        .items_center()
        .gap(space::XXS)
        .px(space::SM)
        .pr(CAPSULE_CLEARANCE)
        .bg(theme.background)
        .child(div().flex_1().min_w_0().px(space::XS).child(field))
        .child(design::icon_button(
            "members-filter-close",
            design::icon("x", ladder.icon),
            words::CLOSE_FILTER,
            ladder,
            theme,
            close,
        ))
}

fn chips(
    view: &Members,
    cx: &mut Context<Members>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let chips: Vec<Div> = view
        .choices()
        .into_iter()
        .enumerate()
        .map(|(index, choice)| chip(view, index, choice, cx, ladder, theme))
        .collect();
    div()
        .id("members-chips")
        .flex_none()
        .flex()
        .gap(space::SM)
        .py(space::SM)
        .px(space::BLOCK)
        .overflow_x_scroll()
        .children(chips)
}

fn chip(
    view: &Members,
    index: usize,
    (group, label, count): (Option<Group>, &'static str, usize),
    cx: &mut Context<Members>,
    ladder: &Ladder,
    theme: &Theme,
) -> Div {
    let chosen = view.only == group;
    let (edge, words, number) = match chosen {
        true => (
            theme.primary,
            theme.primary_foreground,
            theme.primary_foreground.opacity(COUNT_ON_INK),
        ),
        false => (theme.border_strong, theme.foreground, theme.muted),
    };
    let pick = cx.listener(move |view, _: &ClickEvent, _, cx| {
        view.only = group;
        view.cursor = None;
        cx.notify();
    });
    div()
        .relative()
        .flex_none()
        .h(CHIP)
        .flex()
        .items_center()
        .px(space::MD)
        .border_1()
        .border_color(edge)
        .when(chosen, |chip| chip.bg(theme.primary))
        .text_type(ladder.secondary)
        .text_color(words)
        .whitespace_nowrap()
        .child(label)
        .child(
            div()
                .font_family(design::fonts::FAMILY_MONO)
                .text_size(ladder.tiny.size)
                .text_color(number)
                .child(format!("  {count}")),
        )
        .child(
            design::hit(
                format!("members-chip-{index}"),
                ladder.hit,
                CHIP,
                px(1.),
                ladder,
            )
            .role(Role::Button)
            .aria_label(format!("{label} {count}"))
            .aria_toggled(chosen.into())
            .focusable()
            .on_click(pick),
        )
}

fn list(view: &Members, cx: &mut Context<Members>, ladder: &Ladder, theme: &Theme) -> AnyElement {
    let shown = match view.roster() {
        Roster::Rows(shown) => shown,
        Roster::Reading => return quiet("members-loading", words::READING_ROSTER, ladder, theme),
        Roster::Refused(refusal) => {
            return crate::refused(refusal, cx, ladder, theme).into_any_element();
        }
        Roster::Empty { id, title, detail } => {
            return design::empty_state(id, "users", title, detail, ladder, theme)
                .into_any_element();
        }
    };
    let numbers: Vec<u64> = shown.iter().map(|row| row.number).collect();
    let cursor = view.cursor.filter(|index| *index < numbers.len());
    let active = cursor.unwrap_or(0);
    let mut list = design::composite("members-list", Role::ListBox, words::MEMBERS)
        .active(active, numbers.len())
        .on_move(cx.processor(|view, index: usize, _, cx| {
            view.cursor = Some(index);
            cx.notify();
        }))
        .on_press(cx.processor(move |view, index: usize, _, cx| view.open(numbers[index], cx)))
        .build()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col();
    let mut index = 0;
    for group in Group::ALL {
        let members: Vec<&Row> = shown
            .iter()
            .copied()
            .filter(|row| row.group() == group)
            .collect();
        if members.is_empty() {
            continue;
        }
        list = list.child(
            design::section_head(
                format!("members-group-{}", group.label().to_lowercase()),
                group.label(),
                ladder,
                theme,
            )
            .child(members.len().to_string()),
        );
        for row in members {
            let place = Place {
                active: index == active,
                cursor: cursor == Some(index),
            };
            list = list.child(member_row(view, row, place, cx, ladder, theme));
            index += 1;
        }
    }
    list.child(div().flex_none().h(space::BLOCK))
        .into_any_element()
}

#[derive(Clone, Copy)]
struct Place {
    active: bool,
    cursor: bool,
}

fn member_row(
    view: &Members,
    row: &Row,
    place: Place,
    cx: &mut Context<Members>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let all = view.all();
    let number = row.number;
    let me = view.me == Some(number);
    let managed_by = row
        .manager()
        .map(|manager| format!("{} {}", words::MANAGED_BY, name_or_number(all, manager)));
    let (tail, what) = match standing_badge("members-row-standing", row, ladder, theme) {
        Some(badge) => (badge.into_any_element(), row.life()),
        None => {
            let word = row.group().word();
            let tail = design::row_caption(word, ladder, theme).flex_none();
            (tail.into_any_element(), word)
        }
    };
    let description = [me.then_some(words::YOU), managed_by.as_deref(), Some(what)]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    let element = design::pressed(
        design::row(format!("members-row-{number}"), ladder, theme),
        ladder,
    )
    .when(managed_by.is_some(), |row| row.min_h(ladder.row_tall))
    .when(place.cursor, |row| row.bg(theme.surface_raised))
    .aria_label(row.name.clone())
    .aria_description(description)
    .aria_selected(place.cursor)
    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.open(number, cx)))
    .child(design::lead(face(
        row,
        view.picture(row, cx),
        ladder.avatar,
        theme,
    )))
    .child(
        design::row_words(name_line(row, me, ladder.body, ladder, theme))
            .children(managed_by.map(|words| design::row_caption(words, ladder, theme))),
    )
    .child(tail)
    .child(design::chevron(ladder, theme));
    design::item(element, Role::ListBoxOption, place.active)
}
