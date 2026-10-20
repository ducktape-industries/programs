//! The screens, the desktop's here and the phone's in `phone/`: both draw with what this module
//! holds (the badges, the folds, a pick, the front-matter editor in `setup`, `timeline`) and never
//! with each other's; their words are `words`'. Colour says something: an agent's tint, a state.
mod agent;
pub(crate) mod budget;
mod create;
mod list;
mod models;
pub(crate) mod phone;
mod setup;
mod thread;
mod timeline;

use harness::Thinking;
use view::design::Ladder;
use view::design::{fonts, space, text};
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use crate::words::{thinking_short, thinking_words};
use crate::{Agents, Picture, Screen, State};

pub(crate) fn render(view: &Agents, cx: &mut Context<Agents>) -> AnyElement {
    let theme = *cx.global::<Theme>();
    let ladder = &design::POINTER;
    let screen = match view.screen {
        Screen::List => list::render(view, cx, &theme, ladder),
        Screen::Create => create::render(view, cx, &theme, ladder),
        Screen::Agent(agent) | Screen::Part(agent, _) => {
            agent::render(view, agent, cx, &theme, ladder)
        }
        Screen::Thread(id) | Screen::Usage(id) => thread::render(view, id, cx, &theme, ladder),
        Screen::Models | Screen::SetModel => models::render(view, cx, &theme, ladder),
    };
    div()
        .id("agents")
        .size_full()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(space::BLOCK)
        .p(space::BLOCK)
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_size(text::BODY)
        .children(
            view.notice
                .clone()
                .map(|notice| panel("agents-notice", theme.warning_soft, &theme).child(notice)),
        )
        .children(view.doing.clone().map(|doing| {
            panel("agents-doing", theme.surface, &theme)
                .child(format!("{doing}…"))
                .child(caption("sent · waiting for the block", &theme))
        }))
        .child(screen)
        .children(crate::policies::sheet(view, cx, ladder, &theme))
        .into_any_element()
}

pub(crate) fn waiting<T>(
    name: &str,
    slot: &Loadable<T>,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<AnyElement> {
    match slot {
        Loadable::Idle | Loadable::Loading(_) => Some(
            design::quiet("Reading…", theme)
                .id(SharedString::from(format!("{name}-loading")))
                .when(ladder.touch, |quiet| quiet.p(space::BLOCK))
                .into_any_element(),
        ),
        Loadable::Failed(refusal) => {
            let retry = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.refresh(cx));
            Some(
                design::refused(name, design::sentence(refusal), ladder, theme, retry)
                    .into_any_element(),
            )
        }
        Loadable::Ready(_) | Loadable::Reloading(..) => None,
    }
}

pub(crate) fn back(
    id: &'static str,
    label: String,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let go = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.back(cx));
    let label = format!("‹ {label}");
    design::button(id, label, ladder, theme, go).kind(design::Kind::Quiet)
}

pub(crate) fn panel(id: &'static str, background: Hsla, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap(space::XS)
        .p(space::LG)
        .border_1()
        .border_color(theme.border)
        .bg(background)
}

pub(crate) fn caption(words: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .text_size(text::CAPTION)
        .text_color(theme.muted)
        .child(words.into())
}

pub(crate) fn agent_face(
    name: &str,
    picture: Picture,
    side: impl Into<view::design::Face>,
    theme: &Theme,
) -> Div {
    let pictured = picture.is_some();
    design::avatar_with(name, picture, side, theme)
        .bg(theme.agent_soft)
        .text_color(theme.agent)
        .when(pictured, |face| face.border_2().border_color(theme.agent))
}

pub(crate) fn fact(label: &'static str, value: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(space::HAIR)
        .min_w(px(96.))
        .child(caption(label, theme))
        .child(div().font_weight(FontWeight::MEDIUM).child(value.into()))
}

pub(crate) fn state_badge(
    id: impl Into<ElementId>,
    state: State,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let (label, foreground, background) = match state {
        State::Idle => ("Idle", theme.muted, theme.surface_raised),
        State::Working => ("Working", theme.success, theme.success_soft),
        State::Stuck => ("Stuck", theme.warning, theme.warning_soft),
        State::NoNode => ("No node", theme.danger, theme.danger_soft),
        State::Unconfigured => ("Not configured", theme.danger, theme.danger_soft),
    };
    dotted(id, label, foreground, background, ladder)
}

const DOT: Pixels = px(6.);

fn dotted(
    id: impl Into<ElementId>,
    label: &'static str,
    foreground: Hsla,
    background: Hsla,
    ladder: &Ladder,
) -> Stateful<Div> {
    let tag = div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(space::XXS)
        .bg(background)
        .text_color(foreground);
    match ladder.touch {
        true => tag
            .h(px(design::height::touch::BADGE as f32))
            .px(space::XS)
            .text_type(ladder.tiny)
            .font_weight(FontWeight::MEDIUM),
        false => tag.px_1().py_0p5().text_type(ladder.caption),
    }
    .child(div().size(DOT).flex_none().rounded_full().bg(foreground))
    .child(label)
}

pub(crate) fn current_badge(
    id: impl Into<ElementId>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    design::badge(id, "Current", ladder, theme.agent, theme.agent_soft)
}

pub(crate) fn bound_badge(
    id: impl Into<ElementId>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    design::badge(id, "Bound", ladder, theme.agent, theme.agent_soft)
}

pub(crate) const BOUND_WORDS: &str = "Bound to a place: it hears only messages from there";

pub(crate) fn closed_badge(
    id: impl Into<ElementId>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    design::badge(id, "Closed", ladder, theme.muted, theme.surface_raised)
}

pub(crate) fn agent_gone(agent: u64, theme: &Theme, ladder: &Ladder) -> impl IntoElement + use<> {
    design::empty_state(
        "agent-gone",
        "bot-off",
        "Not run by harness",
        format!(
            "Account {agent} has no script that calls harness: its manager took it back, or never handed it over."
        ),
        ladder,
        theme,
    )
}

pub(crate) fn thread_gone(id: u64, theme: &Theme, ladder: &Ladder) -> impl IntoElement + use<> {
    design::empty_state(
        "thread-gone",
        "message-square",
        "No such conversation",
        format!("Harness holds no conversation {id}."),
        ladder,
        theme,
    )
}

pub(crate) fn meter(
    context: harness::Context,
    window: Option<u64>,
    threshold: Option<u64>,
    width: f32,
    theme: &Theme,
) -> Option<Div> {
    let window = window.filter(|window| *window > 0)?;
    let mark = threshold.filter(|at| *at < window).map(|at| {
        div()
            .absolute()
            .top_0()
            .left(px(width * (at as f32 / window as f32)))
            .w(space::HAIR)
            .h_full()
            .bg(theme.warning)
    });
    let full = match context {
        harness::Context::Measured(tokens) => (tokens as f32 / window as f32).min(1.),
        harness::Context::Overflowed => 1.,
    };
    Some(
        div()
            .flex_none()
            .w(px(width))
            .h(px(4.))
            .relative()
            .bg(theme.border)
            .child(div().w(px(width * full)).h_full().bg(theme.foreground))
            .children(mark),
    )
}

pub(crate) fn thinking_choices() -> Vec<Option<Thinking>> {
    std::iter::once(None)
        .chain(Thinking::ALL.iter().copied().map(Some))
        .collect()
}

pub(crate) fn thinking_picker(
    id: &'static str,
    picked: Option<Thinking>,
    ladder: &Ladder,
    theme: &Theme,
    on_pick: impl Fn(Option<Thinking>, &mut view::Window, &mut App) + 'static,
) -> Stateful<Div> {
    let choices = thinking_choices();
    let at = choices.iter().position(|choice| *choice == picked);
    let labels: Vec<(ElementId, SharedString)> = choices
        .iter()
        .enumerate()
        .map(|(index, choice)| {
            (
                ElementId::Name(format!("{id}-{index}").into()),
                SharedString::from(match ladder.touch {
                    true => thinking_short(*choice),
                    false => thinking_words(*choice),
                }),
            )
        })
        .collect();
    design::segmented(
        id,
        "Thinking",
        ladder,
        theme,
        at,
        labels,
        move |index, window, app| on_pick(choices[index], window, app),
    )
}

pub(crate) fn no_node(
    id: &'static str,
    service: &str,
    serving: usize,
    relaying: usize,
    theme: &Theme,
) -> Option<Stateful<Div>> {
    let shown = panel(id, theme.danger_soft, theme);
    let shown = match (serving, relaying) {
        (0, _) => shown
                .child(div().font_weight(FontWeight::MEDIUM).child(format!(
                    "No member node serves {service}. The model is not asked, and the agent stays silent in chat."
                )))
                .child(caption("What an operator does:", theme))
                .child(format!(
                    "1. The service owner registers {service} and assigns node keys to it."
                ))
                .child(format!(
                    "2. Each assigned node’s operator writes egress.toml with {service} and relay = true, then restarts the node."
                )),
        (_, 0) => shown
            .child(div().font_weight(FontWeight::MEDIUM).child(
                "No member node serves relay, so no tool call can run, chat_post included.",
            ))
            .child(caption("What an operator does:", theme))
            .child(
                "A member node’s operator writes relay = true in egress.toml, then restarts the node.",
            ),
        _ => return None,
    };
    Some(shown)
}

pub(crate) fn opened(view: &Agents, key: &str, starts_open: bool) -> bool {
    starts_open != view.open.contains(key)
}

pub(crate) fn toggler(
    key: String,
    cx: &mut Context<Agents>,
) -> impl Fn(&ClickEvent, &mut view::Window, &mut App) + 'static {
    cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
        if !view.open.remove(&key) {
            view.open.insert(key.clone());
        }
        cx.notify();
    })
}

pub(crate) fn fold(
    key: String,
    open: bool,
    label: impl IntoElement,
    caption: Option<String>,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let toggle = toggler(key.clone(), cx);
    div()
        .id(SharedString::from(key))
        .flex()
        .items_center()
        .gap(space::SM)
        .min_h(ladder.hit)
        .cursor_pointer()
        .role(Role::Button)
        .aria_expanded(open)
        .focusable()
        .on_click(toggle)
        .map(|row| design::pressed(row, ladder))
        .child(
            div()
                .flex_none()
                .text_type(ladder.caption)
                .text_color(theme.faint)
                .child(if open { "▾" } else { "▸" }),
        )
        .child(label)
        .children(caption.map(|caption| {
            div()
                .min_w_0()
                .truncate()
                .text_type(ladder.caption)
                .text_color(theme.muted)
                .child(caption)
        }))
}

pub(crate) fn pick(id: String, picked: bool, theme: &Theme, ladder: &Ladder) -> Stateful<Div> {
    let row = div()
        .id(SharedString::from(id))
        .flex()
        .items_center()
        .gap(space::LG)
        .min_h(ladder.row)
        .px(space::LG)
        .border_1()
        .border_color(match picked {
            true => theme.foreground,
            false => theme.border,
        });
    design::pressed(row, ladder)
        .role(Role::Button)
        .aria_toggled(picked.into())
        .focusable()
}

pub(crate) fn copy_chip(
    id: impl Into<ElementId>,
    words: String,
    copied: String,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let theme = *theme;
    let copy =
        cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| view.copy(copied.clone(), cx));
    div()
        .id(id)
        .flex_none()
        .min_h(ladder.hit)
        .flex()
        .items_center()
        .font_family(fonts::FAMILY_MONO)
        .text_type(ladder.caption)
        .text_color(theme.faint)
        .whitespace_nowrap()
        .cursor_pointer()
        .hover(move |style| style.text_color(theme.muted))
        .role(Role::Button)
        .aria_label(format!("Copy {words}"))
        .focusable()
        .on_click(copy)
        .child(words)
}
