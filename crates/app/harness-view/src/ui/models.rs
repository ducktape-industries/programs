//! The models: every model record harness keeps (its id, the vendor's name when it is another,
//! its wire, its service and how many nodes serve it, the context window harness learned, its
//! owner), and the form that sets one, opened from the title, its `SetModel` shown for the
//! person's yes before it is sent.
use harness::Wire;
use view::design::Ladder;
use view::design::{size, space, text};
use view::prelude::*;
use view::{Div, FontWeight};

use super::create::pickable;
use super::list::{ROW, group, pressable};
use super::{back, caption, panel, waiting};
use crate::Agents;
use crate::words::{serving_words, window_words};

pub(super) fn render(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let back = back("models-back", "Agents".into(), cx, theme, ladder);
    let list = waiting("models", &view.models, cx, theme, ladder)
        .unwrap_or_else(|| rows(view, cx, theme, ladder).into_any_element());
    let set = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| {
        view.model_draft.shown = !view.model_draft.shown;
        cx.notify();
    });
    div()
        .flex()
        .flex_col()
        .gap(space::BLOCK)
        .child(div().flex().child(back))
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_start()
                .gap(space::LG)
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(space::HAIR)
                        .child(design::heading("models-title", "Models", 1, theme))
                        .child(caption(ABOUT, theme)),
                )
                .children(view.me.map(|_| {
                    design::button("models-set", "Set a model…", ladder, theme, set)
                        .kind(design::Kind::Primary)
                })),
        )
        .child(list)
        .children(
            view.me
                .filter(|_| view.model_draft.shown)
                .map(|_| form(view, cx, theme, ladder)),
        )
        .into_any_element()
}

/// What the models are, under their title.
pub(crate) const ABOUT: &str = "What an agent’s configuration names. The first account to set an id owns the record and can change it.";

fn rows(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let models = view.models.ready().map_or(&[][..], Vec::as_slice);
    let roster = view.roster.ready();
    let rows = models.iter().map(|model| {
        let row = model.row.clone();
        let owned = view.me == Some(row.owner);
        let owner = roster.map_or_else(
            || format!("account {}", row.owner),
            |roster| roster.name(row.owner),
        );
        let (serving, tone) = serving_words(model.serving, theme);
        let editing = row.clone();
        let id = format!("model-{}", row.id);
        let line = match owned {
            true => pressable(id, theme).on_click(cx.listener(
                move |view: &mut Agents, _: &ClickEvent, _, cx| view.edit_model(&editing, cx),
            )),
            false => div()
                .id(SharedString::from(id))
                .flex()
                .items_center()
                .gap(space::LG)
                .min_h(ROW)
                .px(space::SM)
                .border_b_1()
                .border_color(theme.border),
        };
        line.child(
            div()
                .flex_none()
                .w(px(220.))
                .flex()
                .items_baseline()
                .gap(space::SM)
                .child(design::mono(row.id.clone()).font_weight(FontWeight::MEDIUM))
                .when(row.name != row.id, |id| {
                    id.child(design::mono(row.name.clone()).text_color(theme.muted))
                }),
        )
        .child(div().flex_none().w(px(160.)).child(row.wire.label()))
        .child(
            div()
                .flex_none()
                .w(px(170.))
                .flex()
                .child(caption(format!("{} · ", row.service), theme))
                .child(caption(serving, theme).text_color(tone)),
        )
        .child(
            caption(format!("window {}", window_words(row.window)), theme)
                .flex_none()
                .w(px(170.)),
        )
        .child(
            div()
                .flex_1()
                .flex()
                .gap(space::XXS)
                .text_size(text::CAPTION)
                .text_color(theme.muted)
                .child(owner)
                .when(owned, |owner| owner.child("(you)")),
        )
        .when(owned, |line| line.child(design::chevron(ladder, theme)))
    });
    group("models-list", "Models", models.len(), theme)
        .children(rows)
        .when(models.is_empty(), |list| {
            list.child(div().py(space::SM).child(design::quiet(
                "Harness keeps no model yet: set the first below.",
                theme,
            )))
        })
}

fn input(field: Input, theme: &Theme) -> Input {
    field
        .h(size::CONTROL)
        .w_full()
        .px(space::SM)
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.background)
        .text_color(theme.foreground)
}

fn labelled(
    label: &'static str,
    hint: &'static str,
    field: impl IntoElement,
    theme: &Theme,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(space::XXS)
        .child(div().font_weight(FontWeight::MEDIUM).child(label))
        .child(field)
        .child(caption(hint, theme))
}

fn form(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
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
    let confirm = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.confirm_model(cx));
    div()
        .flex()
        .flex_col()
        .gap(space::LG)
        .child(design::heading("model-form", "Set a model", 2, theme))
        .child(labelled(
            "Model id",
            "What an agent's configuration names: lowercase letters, digits, -, _ and .",
            input(Input::new("model-id", &draft.id, "Model id"), theme).placeholder("sonnet"),
            theme,
        ))
        .child(labelled(
            "Wire",
            "The API the model speaks.",
            div().flex().child(wires),
            theme,
        ))
        .child(services(view, cx, theme, ladder))
        .child(labelled(
            "Vendor's name",
            "The model as the vendor names it: letters, digits, - . _ @ and :.",
            input(
                Input::new("model-name", &draft.name, "Vendor's name"),
                theme,
            )
            .placeholder("claude-sonnet-4-5"),
            theme,
        ))
        .children(draft.problem.clone().map(|problem| {
            div()
                .id("model-problem")
                .text_color(theme.danger)
                .child(problem)
        }))
        .child(match draft.confirming {
            true => confirming(view, cx, theme, ladder).into_any_element(),
            false => div()
                .flex()
                .child(
                    design::button("model-next", "Set model…", ladder, theme, confirm)
                        .kind(design::Kind::Primary),
                )
                .into_any_element(),
        })
}

/// The registered services, one press each: its owner, the nodes assigned to it, and how
/// many member nodes serve it now.
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
                let now = match offer.serving {
                    0 => "no member node serves it now".to_owned(),
                    serving => format!("{serving} serving now"),
                };
                let pick = name.clone();
                pickable(format!("service-{name}"), picked, theme)
                    .on_click(
                        cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
                            view.model_draft.service = Some(pick.clone());
                            cx.notify();
                        }),
                    )
                    .child(design::mono(name).w(px(160.)))
                    .child(div().w(px(160.)).child(format!("owner {owner}")))
                    .child(caption(
                        design::plural(
                            offer.row.nodes.len() as u64,
                            "node assigned",
                            "nodes assigned",
                        ),
                        theme,
                    ))
                    .child(
                        div()
                            .text_color(if offer.serving == 0 {
                                theme.danger
                            } else {
                                theme.success
                            })
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
        .gap(space::XXS)
        .child(div().font_weight(FontWeight::MEDIUM).child("Service"))
        .child(caption(
            "How the model is reached: an egress service the network registered.",
            theme,
        ))
        .child(list)
}

fn confirming(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let draft = &view.model_draft;
    let set = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.set_model(cx));
    let cancel = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| {
        view.model_draft.confirming = false;
        cx.notify();
    });
    panel("model-confirm", theme.surface, theme)
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
        .child(
            div()
                .flex()
                .gap(space::SM)
                .child(design::button(
                    "model-confirm-back",
                    "Back",
                    ladder,
                    theme,
                    cancel,
                ))
                .child(
                    design::button("model-set", "Set", ladder, theme, set)
                        .kind(design::Kind::Primary)
                        .enabled(draft.sending.is_none()),
                ),
        )
}
