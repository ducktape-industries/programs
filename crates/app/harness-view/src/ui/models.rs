//! The models: every model record harness keeps (its id, the vendor's name when it is another,
//! its wire, the services that reach it and how many nodes serve the first, its window and
//! compact_at, its owner), and the form a row or the title opens, its writes shown for a yes.
use harness::Wire;
use view::design::Ladder;
use view::design::{size, space, text};
use view::prelude::*;
use view::{Div, FontWeight};

use super::create::pickable;
use super::list::{group, pressable};
use super::{back, caption, panel, waiting};
use crate::Agents;
use crate::models::{Press, Yes};
use crate::read::Model;
use crate::words::{compact_short, serving_words, window_words};

pub(super) fn render(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let back = back("models-back", "Agents".into(), cx, theme, ladder);
    let list = waiting("models", &view.models, cx, theme, ladder)
        .unwrap_or_else(|| rows(view, cx, theme, ladder).into_any_element());
    let set = pressing(Press::Toggle, cx);
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

// What the models are, under their title.
pub(crate) const ABOUT: &str = "What an agent’s configuration names. A row opens its record: its services, its compact_at default, or removing it.";

// A press that hands `press` to the models form.
pub(super) fn pressing(
    press: Press,
    cx: &mut Context<Agents>,
) -> impl Fn(&ClickEvent, &mut view::Window, &mut App) + 'static {
    cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| view.press_model(press.clone(), cx))
}

// What opens a record's form: the record and the services that reach it.
pub(super) fn open(model: &Model) -> Press {
    let services = model.services.iter().map(|(name, _)| name.clone());
    Press::Open(model.row.clone(), services.collect())
}

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
        let (serving, tone) = serving_words(model.serving(), theme);
        let more = model.services.iter().skip(1).map(|(name, _)| name.as_str());
        let more: Vec<&str> = more.collect();
        let then =
            (!more.is_empty()).then(|| caption(format!(" · then {}", more.join(", ")), theme));
        pressable(format!("model-{}", row.id), theme)
            .on_click(pressing(open(model), cx))
            .child(
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
                    .w(px(220.))
                    .flex()
                    .flex_wrap()
                    .child(caption(format!("{} · ", row.service), theme))
                    .child(caption(serving, theme).text_color(tone))
                    .children(then),
            )
            .child(
                caption(format!("window {}", window_words(row.window)), theme)
                    .flex_none()
                    .w(px(170.)),
            )
            .children(
                compact_short(row.compact_at)
                    .map(|words| caption(words, theme).flex_none().w(px(140.))),
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
            .child(design::chevron(ladder, theme))
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

// What the form says of each field, both trees alike.
pub(crate) const ID_HINT: &str =
    "What an agent's configuration names: lowercase letters, digits, -, _ and .";
pub(crate) const KEPT_ID: &str = "A record keeps its id: set another id for another record.";
pub(crate) const SERVICES_HINT: &str = "The egress services that reach it, in the order harness tries them: the first is the record's. A press picks one or drops it.";
pub(crate) const NAME_HINT: &str =
    "The model as the vendor names it: letters, digits, - . _ @ and :.";
pub(crate) const COMPACT_HINT: &str = "The tokens of context at which its conversations start compacting, unless an agent's configuration names its own. Empty: when the window fills.";

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
            view.press_model(Press::Wire(index), cx)
        }),
    );
    let (title, id) = match draft.was {
        Some(_) => (
            format!("Edit {}", draft.id()),
            labelled(
                "Model id",
                KEPT_ID,
                design::mono(draft.id()).id("model-id-kept"),
                theme,
            ),
        ),
        None => (
            "Set a model".to_owned(),
            labelled(
                "Model id",
                ID_HINT,
                input(Input::new("model-id", &draft.id, "Model id"), theme).placeholder("sonnet"),
                theme,
            ),
        ),
    };
    div()
        .flex()
        .flex_col()
        .gap(space::LG)
        .child(design::heading("model-form", title, 2, theme))
        .child(id)
        .child(labelled(
            "Wire",
            "The API the model speaks.",
            div().flex().child(wires),
            theme,
        ))
        .child(services(view, cx, theme, ladder))
        .child(labelled(
            "Vendor's name",
            NAME_HINT,
            input(
                Input::new("model-name", &draft.name, "Vendor's name"),
                theme,
            )
            .placeholder("claude-sonnet-4-5"),
            theme,
        ))
        .child(labelled(
            "Compacts at",
            COMPACT_HINT,
            input(
                Input::new("model-compact-at", &draft.compact_at, "Compacts at"),
                theme,
            )
            .placeholder("650000"),
            theme,
        ))
        .children(draft.problem.clone().map(|problem| {
            div()
                .id("model-problem")
                .text_color(theme.danger)
                .child(problem)
        }))
        .child(match draft.asking {
            Some(_) => asked(view, cx, theme, ladder).into_any_element(),
            None => div()
                .flex()
                .gap(space::SM)
                .child(
                    design::button(
                        "model-next",
                        "Save…",
                        ladder,
                        theme,
                        pressing(Press::Ask(Yes::Save), cx),
                    )
                    .kind(design::Kind::Primary),
                )
                .children(draft.was.as_ref().map(|_| {
                    design::button(
                        "model-remove",
                        "Remove…",
                        ladder,
                        theme,
                        pressing(Press::Ask(Yes::Remove), cx),
                    )
                    .kind(design::Kind::Quiet)
                }))
                .into_any_element(),
        })
}

// The registered services, one press each: its owner, the nodes assigned to it, and how
// many member nodes serve it now; a picked one says its place in the list.
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
                let now = match offer.serving {
                    0 => "no member node serves it now".to_owned(),
                    serving => format!("{serving} serving now"),
                };
                pickable(format!("service-{name}"), place.is_some(), theme)
                    .on_click(pressing(Press::Service(name.clone()), cx))
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
                    .children(place.map(|place| caption(place_words(place), theme)))
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
        .child(div().font_weight(FontWeight::MEDIUM).child("Services"))
        .child(caption(SERVICES_HINT, theme))
        .child(list)
}

// A picked service's place in the list.
pub(crate) fn place_words(place: usize) -> String {
    match place {
        0 => "first · the record's".to_owned(),
        place => format!("then, {}", place + 1),
    }
}

fn asked(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let draft = &view.model_draft;
    let (title, lines) = draft.asked(view.roster.ready()).unwrap_or_default();
    let (yes_id, yes_words) = match draft.asking {
        Some(Yes::Remove) => ("model-remove-yes", "Remove"),
        _ => ("model-set", "Save"),
    };
    panel("model-confirm", theme.surface, theme)
        .child(div().font_weight(FontWeight::MEDIUM).child(title))
        .children(lines)
        .child(
            div()
                .flex()
                .gap(space::SM)
                .child(design::button(
                    "model-confirm-back",
                    "Back",
                    ladder,
                    theme,
                    pressing(Press::Unask, cx),
                ))
                .child(
                    design::button(yes_id, yes_words, ladder, theme, pressing(Press::Yes, cx))
                        .kind(design::Kind::Primary)
                        .busy(draft.sending.is_some()),
                ),
        )
}
