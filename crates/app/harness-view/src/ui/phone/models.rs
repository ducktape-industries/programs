//! The model records harness keeps, each a row (its id, the services that reach it and how many
//! nodes serve the first, its window, its owner) that opens its form, its own page over them;
//! More opens a new one. The form's writes are shown for the person's yes before they are sent.
use harness::Wire;
use view::design::{Ladder, space};
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use super::parts::{glyph, scroller, section};
use crate::models::{Press, Yes};
use crate::read::Model;
use crate::ui::models::{
    COMPACT_HINT, ID_HINT, KEPT_ID, NAME_HINT, SERVICES_HINT, open, place_words, pressing,
};
use crate::ui::waiting;
use crate::words::{compact_short, serving_words, window_short};
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

// A press that opens the form page with `press`.
fn opening(
    press: Press,
    cx: &mut Context<Agents>,
) -> impl Fn(&ClickEvent, &mut view::Window, &mut App) + 'static {
    cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
        view.press_model(press.clone(), cx);
        view.go(Screen::SetModel, cx)
    })
}

fn rows(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let models = view.models.ready().map_or(&[][..], Vec::as_slice);
    let rows: Vec<Stateful<Div>> = models
        .iter()
        .map(|model| row(view, model, cx, theme, ladder))
        .collect();
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
                        design::row_caption("A record agents’ files name by id.", ladder, theme),
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
                        .on_click(opening(Press::New, cx)),
                )
        }))
}

// A record: whose it is and its policy; a press opens its form.
fn row(
    view: &Agents,
    model: &Model,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let row = model.row.clone();
    let owner = crate::policies::model_words(view, model);
    let (serving, tone) = serving_words(model.serving(), theme);
    let more = match model.services.len() {
        0 | 1 => String::new(),
        services => format!(" +{}", services - 1),
    };
    // one line: the service, how many nodes serve it in its tone, the window
    let caption = div()
        .flex()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_type(ladder.caption)
        .text_color(theme.muted)
        .child(format!("{}{more} · ", row.service))
        .child(div().flex_none().text_color(tone).child(serving))
        .child(
            div()
                .min_w_0()
                .truncate()
                .child(match compact_short(row.compact_at) {
                    Some(compacts) => format!(" · {} · {compacts}", window_short(row.window)),
                    None => format!(" · {}", window_short(row.window)),
                }),
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
    design::pressed(line.child(design::chevron(ladder, theme)), ladder)
        .role(Role::Button)
        .focusable()
        .on_click(opening(open(model), cx))
}

// The form that sets, edits or removes a record, its own page over the models.
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
            view.press_model(Press::Wire(index), cx)
        }),
    );
    let label = |words: &'static str| {
        div()
            .text_type(ladder.caption)
            .text_color(theme.muted)
            .child(words)
    };
    // its first press shows what it sends for a yes, its second sends it
    let (verb_id, verb_words, verb_press) = match draft.asking {
        None => ("model-next", "Save…", Press::Ask(Yes::Save)),
        Some(Yes::Save) => ("model-set", "Save", Press::Yes),
        Some(Yes::Remove) => ("model-remove-yes", "Remove", Press::Yes),
    };
    // Cancel leaves the yes, else the page
    let cancel =
        cx.listener(
            |view: &mut Agents, _: &ClickEvent, _, cx| match view.model_draft.asking {
                Some(_) => view.press_model(Press::Unask, cx),
                None => view.back(cx),
            },
        );
    let verb = design::button(verb_id, verb_words, ladder, theme, pressing(verb_press, cx))
        .kind(design::Kind::Primary)
        .busy(draft.sending.is_some());
    let asked = draft.asked(view.roster.ready()).map(|(title, lines)| {
        div()
            .id("model-confirm")
            .flex()
            .flex_col()
            .gap(space::XXS)
            .p(space::LG)
            .bg(theme.surface)
            .text_type(ladder.secondary)
            .child(div().font_weight(FontWeight::MEDIUM).child(title))
            .children(lines)
    });
    let id = match draft.was {
        Some(_) => div()
            .flex()
            .flex_col()
            .gap(space::XS)
            .child(label("Model id"))
            .child(design::mono(draft.id()).id("model-id-kept"))
            .child(label(KEPT_ID))
            .into_any_element(),
        None => design::field(Input::new("model-id", &draft.id, "Model id"), ladder, theme)
            .label("Model id")
            .caption(ID_HINT)
            .mono()
            .into_any_element(),
    };
    let remove = draft
        .was
        .as_ref()
        .filter(|_| draft.asking.is_none())
        .map(|_| {
            design::button(
                "model-remove",
                "Remove…",
                ladder,
                theme,
                pressing(Press::Ask(Yes::Remove), cx),
            )
            .kind(design::Kind::Quiet)
        });
    let body = div()
        .flex()
        .flex_col()
        .gap(space::LG)
        .p(space::BLOCK)
        .children(crate::policies::model_row(view, cx, ladder, theme))
        .child(id)
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
            .caption(NAME_HINT),
        )
        .child(
            design::field(
                Input::new("model-compact-at", &draft.compact_at, "Compacts at"),
                ladder,
                theme,
            )
            .label("Compacts at")
            .caption(COMPACT_HINT),
        )
        .children(draft.problem.clone().map(|problem| {
            div()
                .id("model-problem")
                .text_type(ladder.secondary)
                .text_color(theme.danger)
                .child(problem)
        }))
        .children(asked)
        .child(design::form_verbs(
            design::button("model-cancel", "Cancel", ladder, theme, cancel)
                .kind(design::Kind::Quiet),
            verb,
        ))
        .children(remove);
    let title = match draft.was {
        Some(_) => format!("Edit {}", draft.id()),
        None => "Set a model".to_owned(),
    };
    super::page("set-model-page", title, view, cx, theme, ladder)
        .child(scroller("set-model-scroll").child(body))
        .into_any_element()
}

// The registered services, one pick each: its name, its owner, how many member nodes serve it
// now, and a picked one's place in the list.
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
            let picked = &view.model_draft.services;
            let rows = offers.iter().map(|offer| {
                let name = offer.row.name.clone();
                let place = picked.iter().position(|service| *service == name);
                let owner = roster.map_or_else(
                    || format!("account {}", offer.row.owner),
                    |roster| roster.name(offer.row.owner),
                );
                let (now, tone) = serving_words(offer.serving, theme);
                let below = match place {
                    Some(place) => format!("{} · owner {owner}", place_words(place)),
                    None => format!("owner {owner}"),
                };
                crate::ui::pick(format!("service-{name}"), place.is_some(), theme, ladder)
                    .on_click(pressing(Press::Service(name.clone()), cx))
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
                            .child(below),
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
                .child("Services"),
        )
        .child(
            div()
                .text_type(ladder.caption)
                .text_color(theme.muted)
                .child(SERVICES_HINT),
        )
        .child(list)
}
