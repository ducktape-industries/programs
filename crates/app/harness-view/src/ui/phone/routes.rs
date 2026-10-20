//! The provider routes harness keeps, each a row (the provider, the services that reach its models
//! and how many nodes serve the first, its owner) that opens its form, its own page over them;
//! More opens a new one. The form's write is shown for the person's yes before it is sent.
use view::design::{Ladder, space};
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use super::parts::{glyph, scroller, section};
use crate::read::Route;
use crate::routes::{Press, Yes};
use crate::ui::routes::{
    KEPT_PROVIDER, PROVIDER_HINT, SERVICES_HINT, WIRE_HINT, place_words, pressing, services_words,
    wires,
};
use crate::ui::waiting;
use crate::words::serving_words;
use crate::{Agents, Screen};

pub(super) fn page(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let body = waiting("routes", &view.routes, cx, theme, ladder)
        .unwrap_or_else(|| rows(view, cx, theme, ladder).into_any_element());
    super::page("routes-page", "Routes", view, cx, theme, ladder)
        .caption("how harness reaches each provider")
        .child(scroller("routes-scroll").child(body))
        .into_any_element()
}

fn opening(
    press: Press,
    cx: &mut Context<Agents>,
) -> impl Fn(&ClickEvent, &mut view::Window, &mut App) + 'static {
    cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
        view.press_route(press.clone(), cx);
        view.go(Screen::SetRoute, cx)
    })
}

fn rows(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let routes = view.routes.ready().map_or(&[][..], Vec::as_slice);
    let rows: Vec<Stateful<Div>> = routes
        .iter()
        .map(|route| row(view, route, cx, theme, ladder))
        .collect();
    div()
        .flex()
        .flex_col()
        .child(section(
            "routes-list",
            "Routes",
            routes.len(),
            theme,
            ladder,
        ))
        .children(rows)
        .when(routes.is_empty(), |list| {
            list.child(
                design::quiet("Harness keeps no route yet.", theme)
                    .text_type(ladder.secondary)
                    .p(space::BLOCK),
            )
        })
        .children(view.me.map(|_| {
            let row = design::row("routes-set", ladder, theme)
                .min_h(ladder.row_tall)
                .child(glyph("plus", theme, ladder))
                .child(
                    design::row_words(design::row_title("Set a route", ladder, theme)).child(
                        design::row_caption(
                            "The services that reach a provider’s models.",
                            ladder,
                            theme,
                        ),
                    ),
                )
                .child(design::chevron(ladder, theme));
            div()
                .flex()
                .flex_col()
                .child(design::section_head("routes-more", "More", ladder, theme))
                .child(
                    design::pressed(row, ladder)
                        .role(Role::Button)
                        .focusable()
                        .on_click(opening(Press::New, cx)),
                )
        }))
}

fn row(
    view: &Agents,
    route: &Route,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let provider = route.row.provider.clone();
    let owner = crate::policies::route_words(view, route);
    let (serving, tone) = serving_words(route.serving(), theme);
    let caption = div()
        .flex()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_type(ladder.caption)
        .text_color(theme.muted)
        .child(
            div()
                .min_w_0()
                .truncate()
                .child(format!("{} · ", services_words(route))),
        )
        .child(div().flex_none().text_color(tone).child(serving));
    let line = design::row(format!("route-{provider}"), ladder, theme)
        .min_h(ladder.row_tall)
        .child(glyph("server", theme, ladder))
        .child(
            design::row_words(
                design::mono(provider)
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
        .on_click(opening(Press::Open(route.row.clone()), cx))
}

pub(super) fn form(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let draft = &view.route_draft;
    let label = |words: &'static str| {
        div()
            .text_type(ladder.caption)
            .text_color(theme.muted)
            .child(words)
    };
    let (verb_id, verb_words, verb_press) = match draft.asking {
        None => ("route-next", "Save…", Press::Ask(Yes::Save)),
        Some(Yes::Save) => ("route-set", "Save", Press::Yes),
        Some(Yes::Remove) => ("route-remove-yes", "Remove", Press::Yes),
    };
    let cancel =
        cx.listener(
            |view: &mut Agents, _: &ClickEvent, _, cx| match view.route_draft.asking {
                Some(_) => view.press_route(Press::Unask, cx),
                None => view.back(cx),
            },
        );
    let verb = design::button(verb_id, verb_words, ladder, theme, pressing(verb_press, cx))
        .kind(design::Kind::Primary)
        .busy(draft.sending.is_some());
    let asked = draft.asked(view.roster.ready()).map(|(title, lines)| {
        div()
            .id("route-confirm")
            .flex()
            .flex_col()
            .gap(space::XXS)
            .p(space::LG)
            .bg(theme.surface)
            .text_type(ladder.secondary)
            .child(div().font_weight(FontWeight::MEDIUM).child(title))
            .children(lines)
    });
    let provider = match draft.was {
        Some(_) => div()
            .flex()
            .flex_col()
            .gap(space::XS)
            .child(label("Provider"))
            .child(design::mono(draft.provider()).id("route-provider-kept"))
            .child(label(KEPT_PROVIDER))
            .into_any_element(),
        None => design::field(
            Input::new("route-provider", &draft.provider, "Provider"),
            ladder,
            theme,
        )
        .label("Provider")
        .caption(PROVIDER_HINT)
        .mono()
        .into_any_element(),
    };
    let remove = draft
        .was
        .as_ref()
        .filter(|_| draft.asking.is_none())
        .map(|_| {
            design::button(
                "route-remove",
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
        .children(crate::policies::route_row(view, cx, ladder, theme))
        .child(provider)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(space::XS)
                .child(label("Wire"))
                .child(wires(view, cx, theme, ladder))
                .child(label(WIRE_HINT)),
        )
        .child(services(view, cx, theme, ladder))
        .children(draft.problem.clone().map(|problem| {
            div()
                .id("route-problem")
                .text_type(ladder.secondary)
                .text_color(theme.danger)
                .child(problem)
        }))
        .children(asked)
        .child(design::form_verbs(
            design::button("route-cancel", "Cancel", ladder, theme, cancel)
                .kind(design::Kind::Quiet),
            verb,
        ))
        .children(remove);
    let title = match draft.was {
        Some(_) => format!("Edit {}'s route", draft.provider()),
        None => "Set a route".to_owned(),
    };
    super::page("set-route-page", title, view, cx, theme, ladder)
        .child(scroller("set-route-scroll").child(body))
        .into_any_element()
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
                let (now, tone) = serving_words(offer.serving, theme);
                let below = match place {
                    Some(place) => {
                        format!("{} · owner {owner}", place_words(place, picked[place].1))
                    }
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
