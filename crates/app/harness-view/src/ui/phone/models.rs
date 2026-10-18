//! The model records harness keeps, each a row (its id, its service and how many nodes serve
//! it, its window, its owner), and the form that sets one, its own page over them: a new record
//! from More, or its owner's record from its row. The form's `SetModel` is shown for the
//! person's yes before it is sent.
use harness::Wire;
use view::design::{Ladder, space};
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use super::parts::{glyph, scroller, section};
use crate::models::ModelDraft;
use crate::read::Model;
use crate::ui::waiting;
use crate::words::{serving_words, window_short};
use crate::{Agents, Screen};

pub(super) fn page(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let body = waiting("models", &view.models, cx, theme, ladder)
        .unwrap_or_else(|| rows(view, cx, theme, ladder).into_any_element());
    super::page("models-page", "Models", view, cx, theme, ladder)
        .caption("what an agent’s configuration names")
        .child(scroller("models-scroll").child(body))
        .into_any_element()
}

fn rows(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let models = view.models.ready().map_or(&[][..], Vec::as_slice);
    let roster = view.roster.ready();
    let rows: Vec<Stateful<Div>> = models
        .iter()
        .map(|model| row(view, model, roster, cx, theme, ladder))
        .collect();
    let set = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| {
        view.model_draft = ModelDraft::default();
        view.go(Screen::SetModel, cx)
    });
    div()
        .flex()
        .flex_col()
        .child(section(
            "models-list",
            "Models",
            models.len(),
            theme,
            ladder,
        ))
        .children(rows)
        .when(models.is_empty(), |list| {
            list.child(
                design::quiet("Harness keeps no model yet.", theme)
                    .text_type(ladder.secondary)
                    .p(space::BLOCK),
            )
        })
        .children(view.me.map(|_| {
            let row = design::row("models-set", ladder, theme)
                .min_h(ladder.row_tall)
                .child(glyph("plus", theme, ladder))
                .child(
                    design::row_words(design::row_title("Set a model", ladder, theme)).child(
                        design::row_caption(
                            "A record you own; an agent’s file names it by id.",
                            ladder,
                            theme,
                        ),
                    ),
                )
                .child(design::chevron(ladder, theme));
            div()
                .flex()
                .flex_col()
                .child(design::section_head("models-more", "More", ladder, theme))
                .child(
                    design::pressed(row, ladder)
                        .role(Role::Button)
                        .focusable()
                        .on_click(set),
                )
        }))
}

/// A record: its owner's row opens the form at it; anyone else's only reads.
fn row(
    view: &Agents,
    model: &Model,
    roster: Option<&crate::read::Roster>,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let row = model.row.clone();
    let owned = view.me == Some(row.owner);
    let owner = roster.map_or_else(
        || format!("account {}", row.owner),
        |roster| roster.name(row.owner),
    );
    let (serving, tone) = serving_words(model.serving, theme);
    // one line: the service, how many nodes serve it in its tone, the window
    let caption = div()
        .flex()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_type(ladder.caption)
        .text_color(theme.muted)
        .child(format!("{} · ", row.service))
        .child(div().flex_none().text_color(tone).child(serving))
        .child(
            div()
                .min_w_0()
                .truncate()
                .child(format!(" · {}", window_short(row.window))),
        );
    let line = design::row(format!("model-{}", row.id), ladder, theme)
        .min_h(ladder.row_tall)
        .child(glyph("server", theme, ladder))
        .child(
            design::row_words(
                design::mono(row.id.clone())
                    .text_type(ladder.small)
                    .font_weight(FontWeight::MEDIUM)
                    .truncate(),
            )
            .child(caption),
        )
        .child(
            div()
                .flex_none()
                .text_type(ladder.caption)
                .text_color(theme.muted)
                .child(owner),
        );
    match owned {
        true => {
            let editing = row.clone();
            let edit = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
                view.edit_model(&editing, cx);
                view.go(Screen::SetModel, cx)
            });
            design::pressed(line.child(design::chevron(ladder, theme)), ladder)
                .role(Role::Button)
                .focusable()
                .on_click(edit)
        }
        false => line,
    }
}

/// The form that sets a record, its own page over the models.
pub(super) fn form(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let draft = &view.model_draft;
    let wires = design::segmented(
        "model-wire",
        "Wire",
        ladder,
        theme,
        Some(draft.wire),
        Wire::ALL.iter().enumerate().map(|(index, wire)| {
            (
                ElementId::Name(format!("model-wire-{index}").into()),
                SharedString::from(wire.label()),
            )
        }),
        cx.processor(|view: &mut Agents, index: usize, _, cx| {
            view.model_draft.wire = index;
            cx.notify();
        }),
    );
    let label = |words: &'static str| {
        div()
            .text_type(ladder.caption)
            .text_color(theme.muted)
            .child(words)
    };
    let cancel = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.back(cx));
    // its first press shows the record for a yes, its second sends it
    let (verb_id, verb_words) = match draft.confirming {
        true => ("model-set", "Set"),
        false => ("model-next", "Set model…"),
    };
    let next =
        cx.listener(
            |view: &mut Agents, _: &ClickEvent, _, cx| match view.model_draft.confirming {
                true => view.set_model(cx),
                false => view.confirm_model(cx),
            },
        );
    let verb = design::button(verb_id, verb_words, ladder, theme, next)
        .kind(design::Kind::Primary)
        .busy(draft.sending.is_some());
    // the record as it is sent, for the person's yes
    let confirming = draft.confirming.then(|| {
        div()
            .id("model-confirm")
            .flex()
            .flex_col()
            .gap(space::XXS)
            .p(space::LG)
            .bg(theme.surface)
            .text_type(ladder.secondary)
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(format!("Set model · {}", draft.id.text().trim())),
            )
            .child(format!("wire {}", draft.wire().label()))
            .child(format!(
                "service {}",
                draft.service.clone().unwrap_or_default()
            ))
            .child(format!("vendor's name {}", draft.name.text().trim()))
    });
    let body = div()
        .flex()
        .flex_col()
        .gap(space::LG)
        .p(space::BLOCK)
        .child(
            design::field(Input::new("model-id", &draft.id, "Model id"), ladder, theme)
                .label("Model id")
                .caption(
                    "What an agent's configuration names: lowercase letters, digits, -, _ and .",
                )
                .mono(),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(space::XS)
                .child(label("Wire"))
                .child(wires),
        )
        .child(services(view, cx, theme, ladder))
        .child(
            design::field(
                Input::new("model-name", &draft.name, "Vendor's name"),
                ladder,
                theme,
            )
            .label("Vendor's name")
            .caption("The model as the vendor names it."),
        )
        .children(draft.problem.clone().map(|problem| {
            div()
                .id("model-problem")
                .text_type(ladder.secondary)
                .text_color(theme.danger)
                .child(problem)
        }))
        .children(confirming)
        .child(design::form_verbs(
            design::button("model-cancel", "Cancel", ladder, theme, cancel)
                .kind(design::Kind::Quiet),
            verb,
        ));
    let title = match draft.id.text().trim().is_empty() {
        true => "Set a model".to_owned(),
        false => format!("Set {}", draft.id.text().trim()),
    };
    super::page("set-model-page", title, view, cx, theme, ladder)
        .child(scroller("set-model-scroll").child(body))
        .into_any_element()
}

/// The registered services, one pick each: its name, its owner and how many member nodes serve
/// it now.
fn services(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let list = match waiting("model-services", &view.offers, cx, theme, ladder) {
        Some(shown) => shown,
        None => {
            let offers = view.offers.ready().map_or(&[][..], Vec::as_slice);
            let roster = view.roster.ready();
            let rows = offers.iter().map(|offer| {
                let name = offer.row.name.clone();
                let picked = view.model_draft.service.as_deref() == Some(name.as_str());
                let owner = roster.map_or_else(
                    || format!("account {}", offer.row.owner),
                    |roster| roster.name(offer.row.owner),
                );
                let (now, tone) = serving_words(offer.serving, theme);
                let chose = name.clone();
                let pick = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
                    view.model_draft.service = Some(chose.clone());
                    cx.notify();
                });
                super::create::pick(format!("service-{name}"), picked, theme, ladder)
                    .on_click(pick)
                    .child(
                        design::mono(name)
                            .text_type(ladder.small)
                            .font_weight(FontWeight::MEDIUM),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_type(ladder.caption)
                            .text_color(theme.muted)
                            .child(format!("owner {owner}")),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_type(ladder.caption)
                            .text_color(tone)
                            .child(now),
                    )
            });
            div()
                .flex()
                .flex_col()
                .children(rows)
                .when(offers.is_empty(), |list| {
                    list.child(design::quiet(
                        "No service is registered: an account registers one in egress, and assigns the nodes that serve it.",
                        theme,
                    ))
                })
                .into_any_element()
        }
    };
    div()
        .flex()
        .flex_col()
        .gap(space::XS)
        .child(
            div()
                .text_type(ladder.caption)
                .text_color(theme.muted)
                .child("Service"),
        )
        .child(list)
}
