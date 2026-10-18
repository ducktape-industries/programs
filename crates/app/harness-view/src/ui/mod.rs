//! The screens: the agents, one agent, one conversation, the create form, the models, the
//! desktop's here and the phone's in `phone/`. Both trees draw with what this module holds
//! (the badges, the folds, a conversation's timeline in `timeline`) and never with each
//! other's; the words they say are `words`'. Colour is kept for what it says: an agent's tint,
//! and a state's badge.
mod agent;
mod create;
mod list;
mod models;
pub(crate) mod phone;
mod thread;
mod timeline;

use harness::Thinking;
use view::design::Ladder;
use view::design::{fonts, space, text};
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use crate::words::{thinking_short, thinking_words};
use crate::{Agents, Screen, State};

/// The desktop's screens, under a pointer: what the last write left over the screen, then the
/// screen whole.
pub(crate) fn render(view: &Agents, cx: &mut Context<Agents>) -> AnyElement {
    let theme = *cx.global::<Theme>();
    let ladder = &design::POINTER;
    let screen = match view.screen {
        Screen::List => list::render(view, cx, &theme, ladder),
        Screen::Create => create::render(view, cx, &theme, ladder),
        // the phone's pages of an agent or a conversation are parts of the desktop's screen
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
        .into_any_element()
}

/// What a read in flight or refused shows in place of its screen.
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
                // a phone's page runs edge to edge: its line sits in as its rows' words do
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

/// A press back to the screen this one was opened from, named for it.
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

/// A box of text that says something about the screen: a warning, a cost.
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

/// A line under a heading, in the muted colour.
pub(crate) fn caption(words: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .text_size(text::CAPTION)
        .text_color(theme.muted)
        .child(words.into())
}

/// One fact: its label over its value.
pub(crate) fn fact(label: &'static str, value: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(space::HAIR)
        .min_w(px(96.))
        .child(caption(label, theme))
        .child(div().font_weight(FontWeight::MEDIUM).child(value.into()))
}

/// A state's badge: a dot in its colour, then its word.
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

/// How wide a badge's dot is.
const DOT: Pixels = px(6.);

/// `design::badge` with a dot before its word, at the badge's sizes.
// SDK gap: a badge with a leading dot; `design::badge` holds its words alone
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

/// The agent's current conversation's badge, in the agent's tint.
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

/// A closed conversation's badge: raised, no dot, as it does nothing more.
pub(crate) fn closed_badge(
    id: impl Into<ElementId>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    design::badge(id, "Closed", ladder, theme.muted, theme.surface_raised)
}

/// An account no script hands to harness any more.
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

/// A conversation harness does not hold.
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

/// The context against the window, as a bar `width` wide: drawn only once the window is known.
pub(crate) fn meter(
    context: harness::Context,
    window: Option<u64>,
    width: f32,
    theme: &Theme,
) -> Option<Div> {
    let window = window.filter(|window| *window > 0)?;
    let full = match context {
        harness::Context::Measured(tokens) => (tokens as f32 / window as f32).min(1.),
        harness::Context::Overflowed => 1.,
    };
    Some(
        div()
            .flex_none()
            .w(px(width))
            .h(px(4.))
            .bg(theme.border)
            .child(div().w(px(width * full)).h_full().bg(theme.foreground)),
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
                // a phone's six segments share its width: the levels in few letters
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

/// What is missing when no member node serves `service`, or relay, and the steps an
/// operator takes: a registered service is served by the nodes its owner assigns; relay is
/// egress's own, served by any member node that turns it on.
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

/// Whether the fold under `key` is open: a key in `Agents.open` turns it from how it starts.
pub(crate) fn opened(view: &Agents, key: &str, starts_open: bool) -> bool {
    starts_open != view.open.contains(key)
}

/// A press that turns the fold under `key`.
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

/// A row that opens and closes what follows it: its triangle, its label, a caption after it.
/// The caller adds what it holds, open.
// SDK gap: a disclosure row; the kit has none yet
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

/// An id in the data face that a press copies: a frame's place, a call's id.
// SDK gap: a copyable id; the kit has none yet
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
