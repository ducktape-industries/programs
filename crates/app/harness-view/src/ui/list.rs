//! The agents: the reader's, then the rest of the network's, each a row that says what it is
//! doing now, who manages it and when it last moved.
use view::design::Ladder;
use view::design::{space, text};
use view::prelude::*;
use view::{FontWeight, Stateful};

use super::{agent_face, caption, state_badge, waiting};
use crate::read::{Card, Roster, Unscripted};
use crate::words::{agent_sentence_topic, ago, last_moved};
use crate::{Agents, Picture, Screen, agent_state};

/// How tall a row is at least.
pub(super) const ROW: Pixels = px(44.);

/// How wide an agent's avatar is in a row.
const AVATAR: f32 = 32.;

pub(super) fn render(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let create = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.start_create(cx));
    let models =
        cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.go(Screen::Models, cx));
    let head = div()
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
                    design::button("agents-models", "Models", ladder, theme, models)
                        .kind(design::Kind::Outline),
                )
                .children(view.me.map(|_| {
                    design::button("agents-create", "Create an agent…", ladder, theme, create)
                        .kind(design::Kind::Primary)
                })),
        );
    let body = match waiting("agents", &view.roster, cx, theme, ladder) {
        Some(shown) => shown,
        None => rows(view, cx, theme, ladder),
    };
    div()
        .flex()
        .flex_col()
        .gap(space::BLOCK)
        .child(head)
        .children(super::budget::strip(view, cx, theme, ladder))
        .child(body)
        .into_any_element()
}

fn rows(view: &Agents, cx: &mut Context<Agents>, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let Some(roster) = view.roster.ready() else {
        return div().into_any_element();
    };
    if roster.agents.is_empty() && roster.unscripted.is_empty() {
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
    let (mine, others): (Vec<&Card>, Vec<&Card>) = roster
        .agents
        .iter()
        .partition(|card| Some(card.setup.manager) == view.me);
    let mut list = div().flex().flex_col().gap(space::BLOCK);
    if !mine.is_empty() || !roster.unscripted.is_empty() {
        list = list.child(
            group(
                "agents-mine",
                "My agents",
                mine.len() + roster.unscripted.len(),
                theme,
            )
            .children(mine.iter().map(|card| {
                row(
                    card,
                    view.picture(card.setup.agent, cx),
                    roster,
                    view.me,
                    cx,
                    theme,
                    ladder,
                )
            }))
            .children(roster.unscripted.iter().map(|unscripted| {
                unscripted_row(
                    unscripted,
                    view.picture(unscripted.number, cx),
                    cx,
                    theme,
                    ladder,
                )
            })),
        );
    }
    if !others.is_empty() {
        // with none of the reader's above them, the others are the one list
        let label = match mine.is_empty() && roster.unscripted.is_empty() {
            true => "Agents",
            false => "Other agents",
        };
        list = list.child(group("agents-others", label, others.len(), theme).children(
            others.iter().map(|card| {
                row(
                    card,
                    view.picture(card.setup.agent, cx),
                    roster,
                    view.me,
                    cx,
                    theme,
                    ladder,
                )
            }),
        ));
    }
    list.into_any_element()
}

/// A group of rows under its name and how many it holds.
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

/// A row that opens what it names.
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

fn row(
    card: &Card,
    picture: Picture,
    roster: &Roster,
    me: Option<u64>,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let agent = card.setup.agent;
    let open = card.threads.iter().filter(|thread| !thread.closed).count();
    let closed = card.threads.len() - open;
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
}

/// An agent account of the reader's that no script hands to harness: Continue picks up the
/// form at its configuration.
fn unscripted_row(
    unscripted: &Unscripted,
    picture: Picture,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let (agent, name) = (unscripted.number, unscripted.name.clone());
    let resumed = name.clone();
    // the row holds its one press, Continue, so the row itself presses nothing
    div()
        .id(SharedString::from(format!("unscripted-{agent}")))
        .flex()
        .items_center()
        .gap(space::LG)
        .min_h(ROW)
        .px(space::SM)
        .py(space::XS)
        .border_b_1()
        .border_color(theme.border)
        .child(who(name, picture, format!("account {agent}"), theme))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(space::SM)
                .child(design::badge(
                    format!("unscripted-{agent}-state"),
                    "Not handed to harness",
                    ladder,
                    theme.muted,
                    theme.surface_raised,
                ))
                .child(caption("Continue to configure it", theme)),
        )
        .child(
            design::button(
                SharedString::from(format!("unscripted-{agent}-continue")),
                "Continue",
                ladder,
                theme,
                cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
                    view.resume(agent, &resumed, cx)
                }),
            )
            .kind(design::Kind::Outline),
        )
}
