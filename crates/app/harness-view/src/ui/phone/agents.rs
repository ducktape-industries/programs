//! The root: the reader's agents, then the rest of the network's, each a row with its state and
//! what it is doing; then what else the view holds, the create form and the models.
use view::design::Ladder;
use view::prelude::*;

use super::parts::{badged, glyph, opening_row, scroller, section};
use crate::read::{Card, Roster, Unscripted};
use crate::ui::{agent_face, state_badge, waiting};
use crate::words::{agent_sentence_topic, last_moved, network};
use crate::{Agents, Picture, Screen, agent_state};

const PAGE: &str = "agents-page";

pub(super) fn page(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let mut caption = network(&view.chain).to_owned();
    if let Some(roster) = view.roster.ready() {
        caption.push_str(&format!(
            " · {}",
            design::plural(roster.relaying as u64, "relay node", "relay nodes")
        ));
    }
    let body = match waiting("agents", &view.roster, cx, theme, ladder) {
        Some(shown) => shown,
        None => rows(view, cx, theme, ladder),
    };
    super::page(PAGE, "Agents", view, cx, theme, ladder)
        .caption(caption)
        .child(
            scroller("agents-list")
                .children(crate::ui::budget::strip(view, cx, theme, ladder))
                .child(body),
        )
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
            div().mt(design::space::SM).child(
                design::button("agents-empty-create", "Create an agent", ladder, theme, create)
                    .kind(design::Kind::Outline)
                    .small(),
            )
        }))
        .into_any_element();
    }
    let (mine, others): (Vec<&Card>, Vec<&Card>) = roster
        .agents
        .iter()
        .partition(|card| Some(card.setup.manager) == view.me);
    let mut list = div().flex().flex_col();
    if !mine.is_empty() || !roster.unscripted.is_empty() {
        let count = mine.len() + roster.unscripted.len();
        list = list
            .child(section("agents-mine", "My agents", count, theme, ladder))
            .children(mine.iter().map(|card| {
                row(
                    card,
                    view.picture(card.setup.agent, cx),
                    roster,
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
            }));
    }
    if !others.is_empty() {
        let label = match mine.is_empty() && roster.unscripted.is_empty() {
            true => "Agents",
            false => "Other agents",
        };
        list = list
            .child(section("agents-others", label, others.len(), theme, ladder))
            .children(others.iter().map(|card| {
                row(
                    card,
                    view.picture(card.setup.agent, cx),
                    roster,
                    cx,
                    theme,
                    ladder,
                )
            }));
    }
    list.child(more(view, cx, theme, ladder))
        .child(
            design::quiet(
                "To talk to an agent, mention it or send it a DM in Chat.",
                theme,
            )
            .text_type(ladder.secondary)
            .p(design::space::BLOCK),
        )
        .into_any_element()
}

fn row(
    card: &Card,
    picture: Picture,
    roster: &Roster,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let agent = card.setup.agent;
    let open = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
        view.go(Screen::Agent(agent), cx)
    });
    let badge = state_badge(
        format!("agent-{agent}-state"),
        agent_state(card, roster),
        theme,
        ladder,
    );
    let words = badged(
        design::row_title(card.name.clone(), ladder, theme),
        Some(badge),
        Some(agent_sentence_topic(card, roster)),
        theme,
        ladder,
    );
    let moved = last_moved(card).map(|time| design::ago(roster.now, time));
    opening_row(
        format!("agent-{agent}"),
        agent_face(&card.name, picture, ladder.avatar, theme),
        words,
        moved,
        theme,
        ladder,
        open,
    )
}

fn unscripted_row(
    unscripted: &Unscripted,
    picture: Picture,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let (agent, name) = (unscripted.number, unscripted.name.clone());
    let resume = name.clone();
    let open = cx
        .listener(move |view: &mut Agents, _: &ClickEvent, _, cx| view.resume(agent, &resume, cx));
    let badge = design::badge(
        format!("unscripted-{agent}-state"),
        "Not handed to harness",
        ladder,
        theme.muted,
        theme.surface_raised,
    );
    let words = badged(
        design::row_title(name.clone(), ladder, theme),
        Some(badge),
        Some("Continue to configure it".to_owned()),
        theme,
        ladder,
    );
    opening_row(
        format!("unscripted-{agent}"),
        agent_face(&name, picture, ladder.avatar, theme),
        words,
        None,
        theme,
        ladder,
        open,
    )
}

fn more(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let create = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.start_create(cx));
    let models =
        cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.go(Screen::Models, cx));
    let records = match view.models.ready() {
        Some(models) if !models.is_empty() => {
            let ids: Vec<&str> = models.iter().map(|model| model.row.id.as_str()).collect();
            format!(
                "{} · {}",
                design::plural(models.len() as u64, "record", "records"),
                ids.join(", ")
            )
        }
        _ => "What an agent’s configuration names".to_owned(),
    };
    let line = |title: &'static str, caption: String| {
        design::row_words(design::row_title(title, ladder, theme))
            .child(design::row_caption(caption, ladder, theme))
    };
    div()
        .flex()
        .flex_col()
        .child(design::section_head("agents-more", "More", ladder, theme))
        .children(view.me.map(|_| {
            opening_row(
                "agents-create",
                glyph("plus", theme, ladder),
                line("Create an agent", crate::create::CREATE_INTRO.to_owned()),
                None,
                theme,
                ladder,
                create,
            )
        }))
        .child(opening_row(
            "agents-models",
            glyph("server", theme, ladder),
            line("Models", records),
            None,
            theme,
            ladder,
            models,
        ))
}
