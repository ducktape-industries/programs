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

/// The filter field's id: opened, the view gives it the keyboard.
const FILTER_ID: &str = "members-filter";
/// A filter chip is this tall (`theme.ts:78`, `size.chip`).
// SDK gap: the kit's `Chips`; the SDK builds it later
const CHIP: Pixels = px(32.);
/// A chosen chip's count shows at this opacity on the ink (`Choice.tsx:72`).
const COUNT_ON_INK: f32 = 0.7;

/// `Members` over the network and how many accounts it holds, Filter in
/// the title row; the chips while anyone is listed, then the rows.
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
    // SDK gap: the network caption; the SDK builds it later
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

/// The reader's name for the first page's avatar, as the roster names
/// their account (Expo `members/index.tsx:84` `me.name`); empty until the
/// session and the roster both say who the reader is.
fn reader(view: &Members) -> String {
    let rows = view.rows.ready().map(Vec::as_slice).unwrap_or_default();
    view.me
        .and_then(|me| rows.iter().find(|row| row.number == me))
        .map_or_else(String::new, |row| row.name.clone())
}

/// The title row while the filter is open (the kit's `Header` with its
/// `center`): the field, its magnifier inside the edge, and Close filter,
/// which empties it.
// SDK gap: a `center` slot on `design::page`; the SDK builds it later
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
    // Expo's testID; a name or a number wants no autocorrection and no
    // capitals
    let input = Input::new(FILTER_ID, &view.filter, words::FILTER_LABEL)
        .autocorrect(false)
        .autocapitalize(Autocapitalize::None)
        .placeholder(words::FILTER_HINT);
    // the kit's `Field`, its magnifier the lead (Expo `app/(tabs)/members/index.tsx:90-98`)
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

/// The kit's `Chips`, one per `Members::choices`; the row scrolls
/// sideways.
// SDK gap: the kit's `Chips`; the SDK builds it later
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

/// One chip: an outline, the chosen one in ink, its count in the data face
/// two spaces after the label (`Choice.tsx:51-78`); its press reaches 44
/// up and down.
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
    // the press reaches 6 past the 32 chip, its hairline included, into
    // the row's 8 above and below (44), never over the row's neighbours;
    // it is as wide as the chip, every chip being wider than 44, so it
    // never reaches the next chip either
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

/// The rows under their groups' section heads, or why there are none: the
/// first read, a refusal with Retry, nobody on the network, nobody the
/// filter and the chip let through.
fn list(view: &Members, cx: &mut Context<Members>, ladder: &Ladder, theme: &Theme) -> AnyElement {
    let shown = match view.roster() {
        Roster::Rows(shown) => shown,
        Roster::Reading => return quiet("members-loading", words::READING_ROSTER, ladder, theme),
        Roster::Refused(refusal) => {
            return crate::refused(refusal, cx, ladder, theme).into_any_element();
        }
        Roster::Empty { id, title, detail } => {
            // Expo's `Users` (`app/(tabs)/members/index.tsx:33`, `:37`)
            return design::empty_state(id, "users", title, detail, ladder, theme)
                .into_any_element();
        }
    };
    // one Tab stop: the arrows move a cursor, shown only once they have
    // moved, and a press or Enter opens the member
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
    // the list's foot (`index.tsx:46`)
    list.child(div().flex_none().h(space::BLOCK))
        .into_any_element()
}

/// Where a row stands in the list the arrows walk.
#[derive(Clone, Copy)]
struct Place {
    /// the list's active row: the one Enter opens
    active: bool,
    /// the arrows have moved onto it
    cursor: bool,
}

/// One account (the kit's `Row`): its face, its name (`you` after mine),
/// who manages an agent, then an agent's standing or what the account is,
/// and the chevron; the press opens the member.
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
    // an agent's standing, else what the account is (`index.tsx:64-71`)
    let (tail, what) = match standing_badge("members-row-standing", row, ladder, theme) {
        Some(badge) => (badge.into_any_element(), row.life()),
        // a person or a program: no standing. The roster says only which
        // (`index.tsx:69`), never the program a program account runs
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
    // the name, not the avatar's initial drawn before it
    .aria_label(row.name.clone())
    .aria_description(description)
    // a press opens the member: the option the arrows rest on is the
    // chosen one
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
