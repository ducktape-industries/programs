//! The agents: the reader's, the rest of the network's, then the revoked in a closed fold, each a
//! row that says what it is doing now, who manages it and when it last moved; docked beside a wide
//! window's side pane, each row its name, state, topic and last move, the chosen one marked.
use view::design::Ladder;
use view::design::{space, text};
use view::prelude::*;
use view::{FontWeight, Stateful};

use super::{Groups, REVOKED, agent_face, caption, fold, groups, opened, state_badge, waiting};
use crate::read::{Card, Roster};
use crate::words::{agent_sentence_topic, ago, last_moved};
use crate::{Agents, Picture, Screen, agent_state};

pub(super) const ROW: Pixels = px(44.);

#[derive(Clone, Copy)]
enum Layout {
    Wide(Option<u64>),
    Docked(Option<u64>),
}

const AVATAR: f32 = 32.;

pub(super) fn render(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let body = match waiting("agents", &view.roster, cx, theme, ladder) {
        Some(shown) => shown,
        None => rows(view, Layout::Wide(view.me), cx, theme, ladder),
    };
    div()
        .flex()
        .flex_col()
        .gap(space::BLOCK)
        .child(head(view, cx, theme, ladder))
        .children(super::budget::strip(view, theme, ladder))
        .child(body)
        .into_any_element()
}

pub(super) fn pane(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let chosen = match view.screen {
        Screen::Agent(agent) => Some(agent),
        _ => None,
    };
    let body = match waiting("agents", &view.roster, cx, theme, ladder) {
        Some(shown) => shown,
        None => rows(view, Layout::Docked(chosen), cx, theme, ladder),
    };
    let budget = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.go(Screen::List, cx));
    div()
        .flex()
        .flex_col()
        .gap(space::BLOCK)
        .child(head(view, cx, theme, ladder))
        .child(
            design::button("agents-budget", "Model budget", ladder, theme, budget)
                .kind(design::Kind::Quiet)
                .selected(matches!(view.screen, Screen::List)),
        )
        .child(body)
        .into_any_element()
}

fn head(view: &Agents, cx: &mut Context<Agents>, theme: &Theme, ladder: &Ladder) -> view::Div {
    let create = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.start_create(cx));
    let routes =
        cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.go(Screen::Routes, cx));
    div()
        .flex()
        .flex_wrap()
        .items_start()
        .gap(space::LG)
        .child(
            div()
                .flex_1()
                .min_w(px(240.))
                .flex()
                .flex_col()
                .gap(space::HAIR)
                .child(design::heading("agents-title", "Agents", 1, theme))
                .child(caption(
                    "To talk to an agent, mention it or send it a DM in chat. Here you see what each one is doing, and what it did.",
                    theme,
                )),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(space::LG)
                .children(view.roster.ready().map(|roster| {
                    caption(
                        design::plural(roster.relaying as u64, "relay node", "relay nodes"),
                        theme,
                    )
                }))
                .child(
                    design::button("agents-routes", "Routes", ladder, theme, routes)
                        .kind(design::Kind::Outline),
                )
                .children(view.me.map(|_| {
                    design::button("agents-create", "Create an agent…", ladder, theme, create)
                        .kind(design::Kind::Primary)
                })),
        )
}

fn rows(
    view: &Agents,
    layout: Layout,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let Some(roster) = view.roster.ready() else {
        return div().into_any_element();
    };
    if roster.agents.is_empty() {
        let create = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.start_create(cx));
        return design::empty_state(
            "agents-empty",
            "bot",
            "No agents yet",
            "Harness runs no agent on this network. A manager creates one here; people then talk to it in chat.",
            ladder,
            theme,
        )
        .children(view.me.map(|_| {
            div().mt(space::SM).flex().child(
                design::button("agents-empty-create", "Create an agent…", ladder, theme, create)
                    .kind(design::Kind::Primary),
            )
        }))
        .into_any_element();
    }
    let Groups {
        mine,
        others,
        revoked,
    } = groups(roster, view.me);
    let mut list = div().flex().flex_col().gap(space::BLOCK);
    if !mine.is_empty() {
        list = list.child(
            group("agents-mine", "My agents", mine.len(), theme).children(mine.iter().map(
                |card| {
                    row(
                        layout,
                        card,
                        view.picture(card.setup.agent, cx),
                        roster,
                        cx,
                        theme,
                        ladder,
                    )
                },
            )),
        );
    }
    if !others.is_empty() {
        let label = match mine.is_empty() {
            true => "Agents",
            false => "Other agents",
        };
        list = list.child(group("agents-others", label, others.len(), theme).children(
            others.iter().map(|card| {
                row(
                    layout,
                    card,
                    view.picture(card.setup.agent, cx),
                    roster,
                    cx,
                    theme,
                    ladder,
                )
            }),
        ));
    }
    if !revoked.is_empty() {
        let chosen_revoked = match layout {
            Layout::Docked(Some(chosen)) => revoked.iter().any(|card| card.setup.agent == chosen),
            _ => false,
        };
        let open = chosen_revoked || opened(view, REVOKED, false);
        let label = div()
            .text_size(text::SECTION)
            .font_weight(FontWeight::SEMIBOLD)
            .child("Revoked");
        let count = Some(revoked.len().to_string());
        let head = fold(REVOKED.to_owned(), open, label, count, cx, theme, ladder)
            .pb(space::XS)
            .border_b_1()
            .border_color(theme.border);
        let mut group = div().flex().flex_col().child(head);
        if open {
            group = group.children(revoked.iter().map(|card| {
                row(
                    layout,
                    card,
                    view.picture(card.setup.agent, cx),
                    roster,
                    cx,
                    theme,
                    ladder,
                )
            }));
        }
        list = list.child(group);
    }
    list.into_any_element()
}

pub(super) fn group(
    id: &'static str,
    label: &'static str,
    count: usize,
    theme: &Theme,
) -> Stateful<view::Div> {
    div().id(id).flex().flex_col().child(
        div()
            .id(SharedString::from(format!("{id}-head")))
            .flex()
            .items_baseline()
            .justify_between()
            .pb(space::XS)
            .border_b_1()
            .border_color(theme.border)
            .role(Role::Heading)
            .aria_level(2)
            .child(
                div()
                    .text_size(text::SECTION)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(label),
            )
            .child(caption(count.to_string(), theme)),
    )
}

pub(super) fn pressable(id: String, theme: &Theme) -> Stateful<view::Div> {
    let theme = *theme;
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(space::LG)
        .min_h(ROW)
        .px(space::SM)
        .py(space::XS)
        .border_b_1()
        .border_color(theme.border)
        .cursor_pointer()
        .hover(move |style| style.bg(theme.surface))
        .role(Role::Button)
        .focusable()
}

fn who(name: String, picture: Picture, below: String, theme: &Theme) -> impl IntoElement + use<> {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(space::LG)
        .w(px(220.))
        .child(agent_face(&name, picture, px(AVATAR), theme))
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().truncate().font_weight(FontWeight::MEDIUM).child(name))
                .child(caption(below, theme).truncate()),
        )
}

fn docked_row(
    card: &Card,
    picture: Picture,
    roster: &Roster,
    chosen: bool,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let agent = card.setup.agent;
    let moved = last_moved(card).map(|time| ago(roster.now, time));
    let goes = match chosen {
        true => Screen::List,
        false => Screen::Agent(agent),
    };
    let named = div()
        .flex()
        .items_center()
        .gap(space::SM)
        .child(
            div()
                .min_w_0()
                .truncate()
                .font_weight(FontWeight::MEDIUM)
                .child(card.name.clone()),
        )
        .child(state_badge(
            format!("agent-{agent}-state"),
            agent_state(card, roster),
            theme,
            ladder,
        ));
    pressable(format!("agent-{agent}"), theme)
        .aria_toggled(chosen.into())
        .when(chosen, |row| row.bg(theme.surface_raised))
        .on_click(cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| view.go(goes, cx)))
        .child(agent_face(&card.name, picture, px(AVATAR), theme))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(named)
                .child(caption(agent_sentence_topic(card, roster), theme).truncate()),
        )
        .child(caption(moved.unwrap_or_default(), theme).flex_none())
        .into_any_element()
}

fn row(
    layout: Layout,
    card: &Card,
    picture: Picture,
    roster: &Roster,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let me = match layout {
        Layout::Wide(me) => me,
        Layout::Docked(chosen) => {
            let chosen = chosen == Some(card.setup.agent);
            return docked_row(card, picture, roster, chosen, cx, theme, ladder);
        }
    };
    let agent = card.setup.agent;
    let (open, closed) = (card.open, card.closed);
    let counts = match closed {
        0 => format!("{open} open"),
        closed => format!("{open} open · {closed} closed"),
    };
    let mut below = format!("account {agent}");
    if let Some((_, config)) = card.setup.configured.config() {
        below.push_str(&format!(" · {}", config.model));
    }
    if let Some(note) = card.note {
        below.push_str(&format!(" · {note}"));
    }
    let moved = last_moved(card).map(|time| ago(roster.now, time));
    pressable(format!("agent-{agent}"), theme)
        .on_click(
            cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
                view.go(Screen::Agent(agent), cx)
            }),
        )
        .child(who(card.name.clone(), picture, below, theme))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(space::SM)
                .child(state_badge(
                    format!("agent-{agent}-state"),
                    agent_state(card, roster),
                    theme,
                    ladder,
                ))
                .child(
                    caption(agent_sentence_topic(card, roster), theme)
                        .min_w_0()
                        .truncate(),
                ),
        )
        .child(
            div()
                .flex_none()
                .w(px(120.))
                .flex()
                .gap(space::XXS)
                .child(div().truncate().child(roster.name(card.setup.manager)))
                .when(Some(card.setup.manager) == me, |manager| {
                    manager.child(div().text_color(theme.muted).child("(you)"))
                }),
        )
        .child(caption(counts, theme).flex_none().w(px(130.)))
        .child(
            caption(moved.unwrap_or_default(), theme)
                .flex_none()
                .w(px(72.))
                .text_right(),
        )
        .child(design::chevron(ladder, theme))
        .into_any_element()
}
