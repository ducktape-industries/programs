//! The desk's members, drawn under a pointer (the root picks this tree when
//! `window.touch()` is false; the phone's is `phone`): the list on the
//! left, the chosen account docked on the right, or floating over the list
//! when the window is too narrow for both. Colour is kept for what it says:
//! the agent tint on an agent's avatar, and the badges of a standing.
//! Everything else is the window, grey captions and hairlines.
//!
//! Reads the model (`Members`) and `words`; writes the choice, the filter,
//! the chip and the list's dragged width.
use view::design::{size, space, text};
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use crate::{
    Group, Members, Roster, Row, explorer_account, explorer_tx, name, name_or_number, opens,
    refused, standing_badge, valset_badge, words,
};
use view::design::Ladder;

pub(crate) mod phone;

/// The list pane's width until it is dragged.
const LIST: f32 = 400.;
/// The list's width bounds when docked; the detail keeps its [`DETAIL_MIN`].
const LIST_W: (f32, f32) = (320., 560.);
/// The detail's narrowest beside the list: the head and a device row with
/// its [`KEY_COLUMN`]. Narrower, it covers the whole screen.
const DETAIL_MIN: Pixels = px(440.);
/// A detail row's first column: a device's label, an agent's name. It
/// gives way first when the pane is narrow.
const KEY_COLUMN: Pixels = px(180.);
const MARK: Pixels = px(6.);

/// The desk's members, under a pointer: the list, and the chosen account
/// docked beside it or floating over it.
pub(crate) fn render(view: &Members, cx: &mut Context<Members>) -> impl IntoElement {
    let theme = *cx.global::<Theme>();
    let ladder = &design::POINTER;
    // too narrow for the detail beside the list: the list takes the pane
    // and the chosen account floats over it
    let list = list_width(view);
    let docked = design::docks(view.width, list, DETAIL_MIN.into());
    let screen = div()
        .id("members")
        .relative()
        .flex()
        .size_full()
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_size(text::BODY)
        .child(list_pane(view, docked.then_some(list), cx, &theme, ladder));
    match (docked, view.selected_row().is_some()) {
        (true, _) => screen
            .child(design::divider(
                "members-list-resize",
                words::RESIZE_LIST,
                &theme,
                cx,
                |view, dx| {
                    view.list = Some(list_width(view) + dx);
                    view.list = Some(list_width(view));
                },
            ))
            .child(detail_pane(view, false, cx, &theme, ladder).flex_1()),
        (false, true) => screen.child(design::over(
            "members-detail-over",
            detail_pane(view, true, cx, &theme, ladder),
            &theme,
        )),
        (false, false) => screen,
    }
}

/// The list's width: dragged or [`LIST`], within [`LIST_W`] and never
/// taking the detail's [`DETAIL_MIN`].
fn list_width(view: &Members) -> f32 {
    let (lo, hi) = LIST_W;
    let room = view.width - f32::from(DETAIL_MIN);
    view.list.unwrap_or(LIST).clamp(lo, room.clamp(lo, hi))
}

/// The chosen account, scrolling on its own; `over` the list it carries
/// its close.
fn detail_pane(
    view: &Members,
    over: bool,
    cx: &mut Context<Members>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    div()
        .id("members-detail")
        .min_w(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .child(detail(view, over, cx, theme, ladder))
}

fn list_pane(
    view: &Members,
    width: Option<f32>,
    cx: &mut Context<Members>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement {
    let count = match view.rows.ready() {
        Some(rows) => words::accounts(rows.len()),
        None => String::new(),
    };
    div()
        .id("members-list-pane")
        // docked at its `width`, else the list alone takes the screen
        .map(|pane| match width {
            Some(list) => pane.w(px(list)).flex_shrink_0(),
            None => pane.flex_1().min_w(px(0.)),
        })
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .items_center()
                .gap(space::SM)
                .px(space::BLOCK)
                .pt(px(14.))
                .pb(space::MD)
                .child(design::heading("members-title", words::MEMBERS, 1, theme).flex_1())
                .child(
                    div()
                        .text_size(text::CAPTION)
                        .text_color(theme.muted)
                        .child(count),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(space::SM)
                .px(space::BLOCK)
                .pb(space::MD)
                .child(
                    Input::new("members-filter", &view.filter, words::FILTER_LABEL)
                        .h(size::CONTROL)
                        .w_full()
                        .px_2()
                        .py_1()
                        .border_1()
                        .border_color(theme.border_strong)
                        .bg(theme.surface)
                        .text_color(theme.foreground)
                        .placeholder(words::FILTER_HINT),
                )
                .child(chips(view, cx, theme)),
        )
        .child(rows(view, cx, theme, ladder))
}

/// All, People, Agents, Programs, each with how many the network holds.
fn chips(view: &Members, cx: &mut Context<Members>, theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .gap(space::XS)
        .children(
            view.choices()
                .into_iter()
                .enumerate()
                .map(|(index, (group, label, count))| {
                    let on = view.only == group;
                    div()
                        .id(format!("members-chip-{}", index))
                        .flex()
                        .gap(space::XXS)
                        .px(space::SM)
                        .py(space::HAIR)
                        .border_1()
                        .border_color(if on { theme.foreground } else { theme.border })
                        .text_size(text::CAPTION)
                        .text_color(if on { theme.foreground } else { theme.muted })
                        .cursor_pointer()
                        .role(Role::Button)
                        .aria_toggled(on.into())
                        .focusable()
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                            view.only = group;
                            cx.notify();
                        }))
                        .child(label)
                        .child(div().text_color(theme.faint).child(count.to_string()))
                }),
        )
}

/// The four states of the roster: loading, refused, empty, ready.
fn rows(view: &Members, cx: &mut Context<Members>, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let shown = match view.roster() {
        Roster::Rows(shown) => shown,
        Roster::Reading => {
            return pad(design::quiet(words::READING_ROSTER, theme))
                .id("members-loading")
                .into_any_element();
        }
        Roster::Refused(refusal) => {
            return pad(refused(refusal, cx, ladder, theme)).into_any_element();
        }
        Roster::Empty { id, title, detail } => {
            return design::empty_state(id, "users", title, detail, ladder, theme)
                .into_any_element();
        }
    };
    let all = view.all();
    // one Tab stop; the arrows select as they move, and the active row is
    // the selected one, else the first shown, which Enter selects
    let numbers: Vec<u64> = shown.iter().map(|row| row.number).collect();
    let active = view
        .selected()
        .and_then(|number| numbers.iter().position(|shown| *shown == number))
        .unwrap_or(0);
    let active_number = numbers[active];
    let pressed = numbers.clone();
    let mut list = design::composite("members-list", Role::ListBox, words::MEMBERS)
        .active(active, numbers.len())
        .on_move(cx.processor(move |view, index: usize, _, cx| view.select(numbers[index], cx)))
        .on_press(cx.processor(move |view, index: usize, _, cx| view.select(pressed[index], cx)))
        .build()
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col();
    for (index, group) in Group::ALL.into_iter().enumerate() {
        let members: Vec<&Row> = shown
            .iter()
            .copied()
            .filter(|row| row.group() == group)
            .collect();
        if members.is_empty() {
            continue;
        }
        list = list.child(
            div()
                .flex()
                .justify_between()
                .px(space::BLOCK)
                .pt(space::MD)
                .pb(space::XXS)
                .text_size(text::CAPTION)
                .text_color(theme.muted)
                .when(index > 0, |head| {
                    head.border_t_1().border_color(theme.border)
                })
                .child(group.label())
                .child(design::mono(members.len().to_string()).text_size(text::CAPTION)),
        );
        for row in members {
            list = list.child(member_row(
                view,
                row,
                row.number == active_number,
                all,
                cx,
                theme,
            ));
        }
    }
    list.into_any_element()
}

fn member_row(
    view: &Members,
    row: &Row,
    active: bool,
    all: &[Row],
    cx: &mut Context<Members>,
    theme: &Theme,
) -> impl IntoElement {
    let theme = *theme;
    let number = row.number;
    let selected = view.selected() == Some(number);
    let dim = design::note(&row.kind).is_some();
    let name_of = |manager| name(all, manager);
    let kind = match row.manager() {
        // an agent: identity's words name its manager and why it no longer
        // acts
        Some(_) => identity::view::kind(&row.kind, name_of),
        None => row.kind_word(),
    };
    let me = view.me == Some(number);
    let row = div()
        .id(format!("members-row-{}", number))
        .flex()
        .items_center()
        .gap(space::MD)
        .h(size::CONTROL)
        .flex_shrink_0()
        .px(space::BLOCK)
        .cursor_pointer()
        .when(selected, |row| row.bg(theme.surface_raised))
        .when(!selected, |row| {
            row.hover(move |style| style.bg(theme.surface))
        })
        // the name, not the avatar's initial drawn before it
        .aria_label(row.name.clone())
        .aria_description(if me {
            format!("{} · {kind}", words::YOU)
        } else {
            kind.clone()
        })
        .aria_selected(selected)
        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select(number, cx)))
        .child(avatar(
            row,
            view.picture(row, cx),
            size::AVATAR,
            dim,
            &theme,
        ))
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(space::XS)
                .child(
                    div()
                        .truncate()
                        .when(dim, |name| name.text_color(theme.faint))
                        .child(row.name.clone()),
                )
                .when(me, |line| {
                    line.child(
                        div()
                            .text_size(text::CAPTION)
                            .text_color(theme.faint)
                            .child(words::YOU),
                    )
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_right()
                .text_size(text::CAPTION)
                .text_color(theme.muted)
                .child(kind),
        );
    // the list holds focus and the arrows; the active row is the one
    // assistive technology is told is active
    design::item(row, Role::ListBoxOption, active)
}

// An account's initial, or its picture once read: round for a person,
// tinted for an agent, square and monospaced for a program; faint when it
// no longer acts.
fn avatar(
    row: &Row,
    picture: Option<std::sync::Arc<view::Image>>,
    size: Pixels,
    dim: bool,
    theme: &Theme,
) -> Div {
    let pictured = picture.is_some();
    let avatar = design::avatar_with(&row.name, picture, size, theme);
    match dim {
        true => avatar.bg(theme.background).text_color(theme.faint),
        false => row.group().face(avatar, pictured, theme),
    }
}

fn pad(inner: impl IntoElement) -> Div {
    div().px(space::BLOCK).py(space::SM).child(inner)
}

fn detail(
    view: &Members,
    over: bool,
    cx: &mut Context<Members>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let (Some(rows), Some(row)) = (view.rows.ready(), view.selected_row()) else {
        return div()
            .id("members-none")
            .px(px(20.))
            .py(space::BLOCK)
            .child(design::quiet(words::CHOOSE_MEMBER, theme))
            .into_any_element();
    };
    let mut detail = div()
        .flex()
        .flex_col()
        .child(head(view, row, rows, over, cx, theme, ladder))
        .child(devices(row, theme));
    let managed = view.managed(row.number);
    if !managed.is_empty() {
        detail = detail.child(manages(view, &managed, cx, theme, ladder));
    }
    if !row.devices.is_empty() {
        detail = detail.child(activity(view, theme, ladder));
    }
    if row.group() == Group::Agents {
        detail = detail.child(
            div()
                .id("members-managed-in-settings")
                .px(px(20.))
                .pt(space::LG)
                .text_size(text::SECONDARY)
                .text_color(theme.muted)
                .child(words::MANAGED_IN_SETTINGS),
        );
    }
    detail.into_any_element()
}

/// The avatar, the name, one caption line of what the account is, its bio,
/// and the two ways out of this screen.
fn head(
    view: &Members,
    row: &Row,
    rows: &[Row],
    over: bool,
    cx: &mut Context<Members>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement {
    let text = |text: String| div().child(text).into_any_element();
    // each part keeps its words (and the manager its link) on one line;
    // a narrow pane wraps between parts
    let mut caption: Vec<Vec<AnyElement>> = vec![vec![text(words::account(row.number))]];
    match row.manager() {
        None => caption.push(vec![text(row.kind_word())]),
        Some(manager) => {
            // what the agent is and `managed by`, then the manager's name
            // as a link that chooses the manager here
            let what = format!("{} · {}", row.kind_word(), words::MANAGED_BY);
            let label = name_or_number(rows, manager);
            let link = div()
                .id("members-manager")
                .text_color(theme.foreground)
                .text_decoration_1()
                .cursor_pointer()
                .role(Role::Link)
                .focusable()
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select(manager, cx)))
                .child(label)
                .into_any_element();
            caption.push(vec![text(what), link]);
            caption.extend(
                standing_badge("members-standing", row, ladder, theme)
                    .map(|badge| vec![badge.into_any_element()]),
            );
        }
    }
    if row.group() != Group::Programs {
        let devices = words::devices(row.devices.len());
        caption.push(vec![text(devices)]);
    }
    if let Some(seat) = row.seat {
        caption.push(vec![valset_badge(seat, ladder, theme).into_any_element()]);
    }
    let last = caption.len() - 1;
    let line = caption.into_iter().enumerate().map(|(index, part)| {
        div()
            .flex()
            .items_center()
            .gap(space::XS)
            .whitespace_nowrap()
            .children(part)
            .when(index < last, |part| part.child("·"))
    });
    let number = row.number;
    let explorer = explorer_account(number);
    let dm = view.dm_link(row);
    div()
        .id("members-detail-head")
        .flex()
        .items_start()
        .gap(space::LG)
        .px(px(20.))
        .pt(space::BLOCK)
        .pb(px(14.))
        .border_b_1()
        .border_color(theme.border)
        .child(avatar(
            row,
            view.picture(row, cx),
            px(32.),
            design::note(&row.kind).is_some(),
            theme,
        ))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .id("members-detail-title")
                        // one line: a long name ends in an ellipsis
                        .truncate()
                        .text_size(text::TITLE)
                        .font_weight(FontWeight::SEMIBOLD)
                        .role(Role::Heading)
                        .aria_level(2)
                        .child(row.name.clone()),
                )
                .child(
                    div()
                        .id("members-caption")
                        .mt(px(3.))
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_x(space::XS)
                        .gap_y(space::HAIR)
                        .font_family(design::fonts::FAMILY_MONO)
                        .text_size(text::CAPTION)
                        .text_color(theme.muted)
                        .children(line),
                )
                .when_some(
                    row.bio.clone().filter(|bio| !bio.trim().is_empty()),
                    |who, bio| {
                        who.child(
                            div()
                                .id("members-bio")
                                .mt(space::SM)
                                .max_w(px(460.))
                                .child(bio),
                        )
                    },
                ),
        )
        .child(
            div()
                .flex()
                .flex_shrink_0()
                .gap(space::XS)
                .when_some(dm, |buttons, link| {
                    buttons.child(outline_button(
                        "members-open-dm",
                        words::OPEN_DM,
                        link,
                        theme,
                    ))
                })
                .child(outline_button(
                    "members-explorer",
                    words::EXPLORER,
                    explorer,
                    theme,
                ))
                .when(over, |buttons| {
                    let close = cx.listener(|view, _: &ClickEvent, _, cx| view.close(cx));
                    buttons.child(design::button(
                        "members-detail-close",
                        words::CLOSE,
                        ladder,
                        theme,
                        close,
                    ))
                }),
        )
}

/// A 1px-edged button that opens `link` through the host.
fn outline_button(
    id: &'static str,
    label: &'static str,
    link: String,
    theme: &Theme,
) -> Stateful<Div> {
    let theme = *theme;
    div()
        .id(id)
        .flex()
        .items_center()
        .h(size::CONTROL)
        .px(space::MD)
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.background)
        .text_size(text::SECONDARY)
        .whitespace_nowrap()
        .cursor_pointer()
        .hover(move |style| style.bg(theme.surface))
        .role(Role::Button)
        .focusable()
        .on_click(opens(link))
        .child(label)
}

/// A detail section: its heading and count over its rows, a hairline under.
fn section(
    id: &'static str,
    title: &'static str,
    count: String,
    body: Vec<AnyElement>,
    theme: &Theme,
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .flex_col()
        .pb(space::XS)
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .flex()
                .items_center()
                .px(px(20.))
                .pt(space::LG)
                .pb(space::XS)
                .child(
                    design::heading(SharedString::from(format!("{id}-heading")), title, 2, theme)
                        .flex_1(),
                )
                .child(
                    design::mono(count)
                        .text_size(text::CAPTION)
                        .text_color(theme.muted),
                ),
        )
        .children(body)
}

/// One line of a section.
fn line() -> Div {
    div()
        .flex()
        .items_center()
        .gap(space::MD)
        .h(size::ROW)
        .px(px(20.))
}

/// A section with nothing in it says so, quietly.
fn empty(text: &'static str, theme: &Theme) -> AnyElement {
    div()
        .px(px(20.))
        .pt(space::HAIR)
        .pb(space::SM)
        .text_size(text::SECONDARY)
        .text_color(theme.muted)
        .child(text)
        .into_any_element()
}

fn devices(row: &Row, theme: &Theme) -> impl IntoElement {
    let body = if row.devices.is_empty() {
        vec![empty(words::NO_DEVICES, theme)]
    } else {
        row.devices
            .iter()
            .enumerate()
            .map(|(index, device)| {
                let label = device.title(index);
                let hex = device.hex();
                line()
                    .child(div().w(KEY_COLUMN).min_w(px(0.)).truncate().child(label))
                    .child(
                        design::mono(design::short_hex(&hex))
                            .text_size(text::CAPTION)
                            .text_color(theme.muted),
                    )
                    .child(
                        design::mono(words::added(device.added_at))
                            .ml_auto()
                            .text_size(text::CAPTION)
                            .text_color(theme.faint),
                    )
                    .into_any_element()
            })
            .collect()
    };
    section(
        "members-devices",
        words::DEVICES,
        row.devices.len().to_string(),
        body,
        theme,
    )
}

/// The agents a person manages; pressing one chooses it here.
fn manages(
    view: &Members,
    managed: &[&Row],
    cx: &mut Context<Members>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement {
    let body = managed
        .iter()
        .filter_map(|agent| {
            let identity::Kind::Program {
                category: identity::Category::Agent,
                ..
            } = agent.kind
            else {
                return None;
            };
            let number = agent.number;
            let theme = *theme;
            let about = words::agent(number);
            let row = line()
                .id(format!("members-manages-{}", number))
                .cursor_pointer()
                .hover(move |style| style.bg(theme.surface))
                .role(Role::Button)
                // the agent's name, not the avatar's initial drawn before it
                .aria_label(agent.name.clone())
                .aria_description(format!("{about} · {}", agent.life()))
                .focusable()
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select(number, cx)))
                .child(avatar(
                    agent,
                    view.picture(agent, cx),
                    size::AVATAR_SM,
                    design::note(&agent.kind).is_some(),
                    &theme,
                ))
                .child(
                    div()
                        .w(KEY_COLUMN)
                        .min_w(px(0.))
                        .truncate()
                        .child(agent.name.clone()),
                )
                .child(
                    design::mono(about)
                        .text_size(text::CAPTION)
                        .text_color(theme.muted),
                )
                .child(div().ml_auto().children(standing_badge(
                    "members-manages-standing",
                    agent,
                    ladder,
                    &theme,
                )));
            Some(row.into_any_element())
        })
        .collect();
    section(
        "members-manages",
        words::MANAGES,
        managed.len().to_string(),
        body,
        theme,
    )
}

fn activity(view: &Members, theme: &Theme, ladder: &Ladder) -> impl IntoElement {
    let body = match &view.activity {
        Loadable::Idle | Loadable::Loading(_) => vec![
            div()
                .id("members-activity-loading")
                .px(px(20.))
                .pb(space::SM)
                .child(design::quiet(words::READING_ACTIVITY, theme))
                .into_any_element(),
        ],
        Loadable::Failed(refusal) => vec![
            div()
                .px(px(20.))
                .pb(space::SM)
                .child(design::quiet(design::sentence(refusal), theme))
                .into_any_element(),
        ],
        Loadable::Ready(recent) | Loadable::Reloading(recent, _) if recent.items.is_empty() => {
            vec![
                div()
                    .id("members-no-activity")
                    .px(px(20.))
                    .pb(space::SM)
                    .child(design::quiet(words::NO_ACTIVITY, theme))
                    .into_any_element(),
            ]
        }
        Loadable::Ready(recent) | Loadable::Reloading(recent, _) => recent
            .items
            .iter()
            .enumerate()
            .map(|(index, signed)| {
                let place = design::grouped(signed.height);
                let label = ::activity::view::label(
                    &signed.target,
                    signed.applied,
                    &place,
                    signed.position,
                );
                let color = verdict(signed.applied, theme);
                let mark = div().size(MARK).flex_shrink_0().bg(color);
                line()
                    .child(mark)
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .child(signed.target.clone()),
                    )
                    .child(
                        design::link(
                            format!("members-tx-{index}"),
                            format!("{} · {}", words::block(&place), signed.position),
                            explorer_tx(signed.height, signed.position),
                            ladder,
                            theme,
                        )
                        .aria_label(words::open_in_explorer(&label)),
                    )
                    .child(
                        div()
                            .w(px(52.))
                            .flex()
                            .justify_end()
                            .text_size(text::CAPTION)
                            .text_color(theme.faint)
                            .child(design::ago(recent.now, signed.time)),
                    )
                    .into_any_element()
            })
            .collect(),
    };
    section(
        "members-activity",
        words::RECENT_ACTIVITY,
        words::NEWEST_FIRST.to_owned(),
        body,
        theme,
    )
}

fn verdict(applied: bool, theme: &Theme) -> Hsla {
    match applied {
        true => theme.success,
        false => theme.danger,
    }
}
