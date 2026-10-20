//! The routes: every provider's route harness keeps (the provider, the services that reach its
//! models in order with each one's wire, how many nodes serve the first, its owner), the form a row
//! or the title opens with its write shown for a yes, and the provider pick the setup forms draw.
use harness::Wire;
use view::design::Ladder;
use view::design::{size, space};
use view::prelude::*;
use view::{Div, FontWeight};

use super::create::pickable;
use super::list::{group, pressable};
use super::{back, caption, panel, pick, waiting};
use crate::Agents;
use crate::read::Route;
use crate::routes::{Press, Yes, wire_at};
use crate::words::serving_words;

pub(super) fn render(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let back = back("routes-back", "Agents".into(), cx, theme, ladder);
    let list = waiting("routes", &view.routes, cx, theme, ladder)
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
                        .child(design::heading("routes-title", "Routes", 1, theme))
                        .child(caption(ABOUT, theme)),
                )
                .children(view.me.map(|_| {
                    design::button("routes-set", "Set a route…", ladder, theme, set)
                        .kind(design::Kind::Primary)
                })),
        )
        .child(list)
        .children(
            view.me
                .filter(|_| view.route_draft.shown)
                .map(|_| form(view, cx, theme, ladder)),
        )
        .into_any_element()
}

pub(crate) const ABOUT: &str = "How harness reaches each provider an agent’s configuration names: its services, in the order harness tries them. A row opens its route.";

pub(super) fn pressing(
    press: Press,
    cx: &mut Context<Agents>,
) -> impl Fn(&ClickEvent, &mut view::Window, &mut App) + 'static {
    cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| view.press_route(press.clone(), cx))
}

pub(crate) fn services_words(route: &Route) -> String {
    let listed: Vec<String> = route
        .row
        .services
        .iter()
        .map(|entry| format!("{} ({})", entry.service, entry.wire.label()))
        .collect();
    listed.join(", then ")
}

fn rows(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let routes = view.routes.ready().map_or(&[][..], Vec::as_slice);
    let rows = routes.iter().map(|route| {
        let provider = route.row.provider.clone();
        let owner = crate::policies::route_words(view, route);
        let (serving, tone) = serving_words(route.serving(), theme);
        pressable(format!("route-{provider}"), theme)
            .on_click(pressing(Press::Open(route.row.clone()), cx))
            .child(
                design::mono(provider)
                    .flex_none()
                    .w(px(160.))
                    .font_weight(FontWeight::MEDIUM),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_wrap()
                    .child(caption(format!("{} · ", services_words(route)), theme))
                    .child(caption(serving, theme).text_color(tone)),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(200.))
                    .flex()
                    .gap(space::XXS)
                    .text_size(view::design::text::CAPTION)
                    .text_color(theme.muted)
                    .child(owner),
            )
            .child(design::chevron(ladder, theme))
    });
    group("routes-list", "Routes", routes.len(), theme)
        .children(rows)
        .when(routes.is_empty(), |list| {
            list.child(div().py(space::SM).child(design::quiet(
                "Harness keeps no route yet: set the first below.",
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

pub(crate) const PROVIDER_HINT: &str =
    "What an agent's configuration names as its provider: lowercase letters, digits, -, _ and .";
pub(crate) const KEPT_PROVIDER: &str =
    "A route keeps its provider: set another provider for another route.";
pub(crate) const WIRE_HINT: &str = "The API the next service you pick speaks.";
pub(crate) const SERVICES_HINT: &str = "The egress services that reach its models, in the order harness tries them, each with the wire picked when it was. A press picks one or drops it.";

pub(crate) fn wires(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    design::segmented(
        "route-wire",
        "Wire",
        ladder,
        theme,
        Some(view.route_draft.wire),
        Wire::ALL.iter().enumerate().map(|(place, wire)| {
            (
                ElementId::Name(format!("route-wire-{place}").into()),
                SharedString::from(wire.label()),
            )
        }),
        cx.processor(|view: &mut Agents, place: usize, _, cx| {
            view.press_route(Press::Wire(place), cx)
        }),
    )
    .into_any_element()
}

fn form(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let draft = &view.route_draft;
    let (title, provider) = match draft.was {
        Some(_) => (
            format!("Edit {}'s route", draft.provider()),
            labelled(
                "Provider",
                KEPT_PROVIDER,
                design::mono(draft.provider()).id("route-provider-kept"),
                theme,
            ),
        ),
        None => (
            "Set a route".to_owned(),
            labelled(
                "Provider",
                PROVIDER_HINT,
                input(
                    Input::new("route-provider", &draft.provider, "Provider"),
                    theme,
                )
                .placeholder("anthropic"),
                theme,
            ),
        ),
    };
    div()
        .flex()
        .flex_col()
        .gap(space::LG)
        .child(design::heading("route-form", title, 2, theme))
        .children(crate::policies::route_row(view, cx, ladder, theme))
        .child(provider)
        .child(labelled(
            "Wire",
            WIRE_HINT,
            div().flex().child(wires(view, cx, theme, ladder)),
            theme,
        ))
        .child(services(view, cx, theme, ladder))
        .children(draft.problem.clone().map(|problem| {
            div()
                .id("route-problem")
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
                        "route-next",
                        "Save…",
                        ladder,
                        theme,
                        pressing(Press::Ask(Yes::Save), cx),
                    )
                    .kind(design::Kind::Primary),
                )
                .children(draft.was.as_ref().map(|_| {
                    design::button(
                        "route-remove",
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

fn services(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let list = match waiting("route-services", &view.offers, cx, theme, ladder) {
        Some(shown) => shown,
        None => {
            let offers = view.offers.ready().map_or(&[][..], Vec::as_slice);
            let roster = view.roster.ready();
            let picked = &view.route_draft.services;
            let rows = offers.iter().map(|offer| {
                let name = offer.row.name.clone();
                let place = picked.iter().position(|(service, _)| *service == name);
                let owner = roster.map_or_else(
                    || format!("account {}", offer.row.owner),
                    |roster| roster.name(offer.row.owner),
                );
                let now = match offer.serving {
                    0 => "no member node serves it now".to_owned(),
                    serving => format!("{serving} serving now"),
                };
                let placed = place.map(|place| place_words(place, picked[place].1));
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
                    .children(placed.map(|placed| caption(placed, theme)))
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

pub(crate) fn place_words(place: usize, wire: usize) -> String {
    let wire = wire_at(wire).label();
    match place {
        0 => format!("first · {wire}"),
        place => format!("then, {} · {wire}", place + 1),
    }
}

fn asked(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let draft = &view.route_draft;
    let (title, lines) = draft.asked(view.roster.ready()).unwrap_or_default();
    let (yes_id, yes_words) = match draft.asking {
        Some(Yes::Remove) => ("route-remove-yes", "Remove"),
        _ => ("route-set", "Save"),
    };
    panel("route-confirm", theme.surface, theme)
        .child(div().font_weight(FontWeight::MEDIUM).child(title))
        .children(lines)
        .child(
            div()
                .flex()
                .gap(space::SM)
                .child(design::button(
                    "route-confirm-back",
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

// the routes a setup form picks its provider from: each provider with its services and how many
// nodes serve the first; `set` takes the provider a press picked
pub(crate) fn providers(
    view: &Agents,
    picked: Option<&str>,
    id: &'static str,
    set: impl Fn(&mut Agents, String, &mut Context<Agents>) + Clone + 'static,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    if let Some(shown) = waiting(id, &view.routes, cx, theme, ladder) {
        return shown;
    }
    let routes = view.routes.ready().map_or(&[][..], Vec::as_slice);
    let rows: Vec<_> = routes
        .iter()
        .map(|route| {
            let provider = route.row.provider.clone();
            let (now, tone) = serving_words(route.serving(), theme);
            let chose = provider.clone();
            let set = set.clone();
            let choose = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
                set(view, chose.clone(), cx);
                cx.notify();
            });
            pick(
                format!("{id}-{provider}"),
                picked == Some(provider.as_str()),
                theme,
                ladder,
            )
            .on_click(choose)
            .child(
                design::mono(provider)
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
                    .child(services_words(route)),
            )
            .child(
                div()
                    .flex_none()
                    .text_type(ladder.caption)
                    .text_color(tone)
                    .child(now),
            )
        })
        .collect();
    let unrouted = picked
        .filter(|provider| !routes.iter().any(|route| route.row.provider == *provider))
        .map(|provider| {
            div()
                .id(SharedString::from(format!("{id}-unrouted")))
                .text_type(ladder.caption)
                .text_color(theme.danger)
                .child(format!(
                    "The file names provider {provider}, which harness keeps no route for: pick one above."
                ))
        });
    div()
        .flex()
        .flex_col()
        .gap(space::XS)
        .children(rows)
        .children(unrouted)
        .when(routes.is_empty(), |list| {
            list.child(design::quiet(
                "Harness keeps no route yet: set one on the Routes page.",
                theme,
            ))
        })
        .into_any_element()
}
