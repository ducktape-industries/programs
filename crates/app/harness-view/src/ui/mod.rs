//! The screens: the agents, one agent, one thread, the create form, the models. Colour is
//! kept for what it says: an agent's tint, and a state's badge.
mod agent;
mod create;
mod list;
mod models;
mod thread;

use harness::{Asking, Thinking, ThreadRow, Usage, Window};
use view::design::Ladder;
use view::design::{space, text};
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use crate::read::{Card, Roster};
use crate::{Agents, Screen, State, thread_state};

pub(crate) fn render(
    view: &Agents,
    touch: bool,
    cx: &mut Context<Agents>,
) -> impl IntoElement + use<> {
    let theme = *cx.global::<Theme>();
    let ladder = &design::ladder(touch);
    let screen = match view.screen {
        Screen::List => list::render(view, cx, &theme, ladder),
        Screen::Create => create::render(view, cx, &theme, ladder),
        Screen::Agent(agent) => agent::render(view, agent, cx, &theme, ladder),
        Screen::Thread(id) => thread::render(view, id, cx, &theme, ladder),
        Screen::Models => models::render(view, cx, &theme, ladder),
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
        .child(screen)
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

/// A press back to `screen`.
pub(crate) fn back(
    id: &'static str,
    label: SharedString,
    screen: Screen,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let go = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| view.go(screen, cx));
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
    design::badge(id, label, ladder, foreground, background)
}

/// An agent's state: its configuration and a node before anything, then the busiest of its
/// open threads.
pub(crate) fn agent_state(card: &Card, roster: &Roster) -> State {
    let Some(serving) = card.serving else {
        return State::Unconfigured;
    };
    if serving == 0 || roster.relaying == 0 {
        return State::NoNode;
    }
    let states = card.threads.iter().map(thread_state);
    states.fold(State::Idle, |state, next| match (state, next) {
        (State::Stuck, _) | (_, State::Stuck) => State::Stuck,
        (State::Working, _) | (_, State::Working) => State::Working,
        _ => State::Idle,
    })
}

/// What a thread is doing, in words.
pub(crate) fn asking(asking: Asking) -> String {
    match asking {
        Asking::Idle => "Idle".into(),
        Asking::Learning => "Learning the model’s context window".into(),
        Asking::Compacting { through } => format!("Summarizing through frame {through}"),
        Asking::Asked { .. } => "Waiting for the model".into(),
        Asking::Running => "Running tool calls".into(),
    }
}

/// A conversation as people read it, by its number. Its title is what harness tells the model,
/// and one opened before an agent kept one conversation names its room there by the room's id.
pub(crate) fn conversation_label(thread: &ThreadRow) -> String {
    format!("Conversation {}", thread.id)
}

pub(crate) fn began_words(thread: &ThreadRow, roster: Option<&Roster>) -> Option<String> {
    let began = !thread.place.is_empty();
    began.then(|| format!("began in {}", place_label(thread, roster)))
}

/// A thread's place as people read it: `#general` by the room's name in chat, `DM · Ada`, else
/// the place as is.
pub(crate) fn place_label(thread: &ThreadRow, roster: Option<&Roster>) -> String {
    let Some(channel) = thread.place.strip_prefix("chat/") else {
        return thread.place.clone();
    };
    match chat::dm_peers(channel) {
        Some((a, b)) => {
            let other = if a == thread.agent { b } else { a };
            let name = roster.map_or_else(|| format!("account {other}"), |r| r.name(other));
            format!("DM · {name}")
        }
        None => roster
            .and_then(|roster| roster.rooms.get(channel))
            .map_or_else(|| "a chat room".to_owned(), |name| format!("#{name}")),
    }
}

/// What a thread's context holds, against its model's window once harness learned it.
pub(crate) fn context_words(context: harness::Context, window: Option<u64>) -> String {
    match (context, window) {
        (harness::Context::Measured(tokens), Some(window)) if window > 0 => format!(
            "{} / {} tokens · {}%",
            design::grouped(tokens),
            design::grouped(window),
            tokens.saturating_mul(100) / window
        ),
        (harness::Context::Measured(tokens), _) => format!("{} tokens", design::grouped(tokens)),
        (harness::Context::Overflowed, _) => "over the model’s window".to_owned(),
    }
}

/// The context against the window, as a bar: drawn only once the window is known.
pub(crate) fn meter(context: harness::Context, window: Option<u64>, theme: &Theme) -> Option<Div> {
    const WIDTH: f32 = 120.;
    let window = window.filter(|window| *window > 0)?;
    let full = match context {
        harness::Context::Measured(tokens) => (tokens as f32 / window as f32).min(1.),
        harness::Context::Overflowed => 1.,
    };
    Some(
        div()
            .w(px(WIDTH))
            .h(px(4.))
            .bg(theme.border)
            .child(div().w(px(WIDTH * full)).h_full().bg(theme.foreground)),
    )
}

pub(crate) fn thinking_choices() -> Vec<Option<Thinking>> {
    std::iter::once(None)
        .chain(Thinking::ALL.iter().copied().map(Some))
        .collect()
}

pub(crate) fn thinking_words(thinking: Option<Thinking>) -> &'static str {
    match thinking {
        None => "Model's default",
        Some(Thinking::Off) => "Off",
        Some(Thinking::Low) => "Low",
        Some(Thinking::Medium) => "Medium",
        Some(Thinking::High) => "High",
        Some(Thinking::Max) => "Max",
    }
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
                SharedString::from(thinking_words(*choice)),
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

pub(crate) fn window_words(window: Window) -> String {
    match window {
        Window::Unasked => "not asked yet".to_owned(),
        Window::Unreported => "not reported".to_owned(),
        Window::Tokens(tokens) => format!("{} tokens", design::grouped(tokens)),
    }
}

pub(crate) fn snapshot_words(snapshot: &harness::BlobId) -> String {
    let short: String = snapshot
        .digest()
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("snapshot {short}")
}

pub(crate) fn usage_words(usage: &Usage) -> String {
    let all = usage.input + usage.output + usage.cache_read + usage.cache_write;
    format!(
        "input {} · output {} · cache read {} · cache write {} · all {}",
        design::grouped(usage.input),
        design::grouped(usage.output),
        design::grouped(usage.cache_read),
        design::grouped(usage.cache_write),
        design::grouped(all),
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
