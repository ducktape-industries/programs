//! One conversation, read only: its strip (its state, what it waits for, how full its context
//! is, its usage on demand), the input held behind its loop, and its timeline: a person's
//! message, the agent's work (a burst of tool calls folded to one line, each call with its
//! result inside it), its answer, and the reply the model is streaming back as it grows. Its
//! manager closes it from here.
use view::FontWeight;
use view::design::Ladder;
use view::design::{space, text};
use view::prelude::*;

use super::{
    back, caption, closed_badge, current_badge, meter, no_node, opened, panel, state_badge,
    thread_gone, timeline, toggler, waiting,
};
use crate::read::{Roster, Transcript};
use crate::words::{
    armed_words, closes, context_short, context_words, conversation_label, first_line, held_author,
    mentions, open_label, opened_words, quoted, status, usage_words, waiting_words,
};
use crate::{Agents, Arm, State, links, thread_state};

pub(super) fn render(
    view: &Agents,
    id: u64,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let shown = view.transcript.ready().and_then(Option::as_ref);
    let label = match shown {
        Some(shown) => view.agent_name(shown.thread.agent),
        None => "Agents".to_owned(),
    };
    let back = back("thread-back", label, cx, theme, ladder);
    let body = match (
        waiting("thread", &view.transcript, cx, theme, ladder),
        shown,
    ) {
        (Some(waiting), _) => waiting,
        (None, None) => thread_gone(id, theme, ladder).into_any_element(),
        (None, Some(shown)) => thread(view, shown, cx, theme, ladder),
    };
    div()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .gap(space::BLOCK)
        .child(div().flex().child(back))
        .child(body)
        .into_any_element()
}

fn thread(
    view: &Agents,
    shown: &Transcript,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let roster = view.roster.ready();
    let row = &shown.thread;
    let id = row.id;
    let manages = view.me == Some(row.manager);
    let relaying = roster.map_or(1, |roster| roster.relaying);
    let current = shown
        .setup
        .as_ref()
        .is_some_and(|setup| setup.current == Some(id));
    let opened = links::place_link(&view.chain, &row.place);
    let arm = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
        view.arming = Some(Arm::Close(id));
        cx.notify();
    });
    let head = div()
        .flex()
        .flex_wrap()
        .items_start()
        .gap(space::LG)
        .child(
            div()
                .flex_1()
                .min_w(px(240.))
                .flex()
                .flex_col()
                .gap(space::HAIR)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::SM)
                        .child(design::heading(
                            "thread-place",
                            conversation_label(row, roster),
                            1,
                            theme,
                        ))
                        .when(current, |head| {
                            head.child(current_badge("thread-current", theme, ladder))
                        }),
                )
                .children(shown.topic.as_ref().map(|topic| {
                    div()
                        .id("thread-topic")
                        .text_color(theme.muted)
                        .child(quoted(topic))
                }))
                .child(caption(opened_words(shown, roster), theme)),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(space::SM)
                .children(opened.map(|(program, link)| {
                    design::button(
                        "thread-open",
                        open_label(&program),
                        ladder,
                        theme,
                        move |_, _, cx: &mut App| cx.host().open_link(&link),
                    )
                    .kind(design::Kind::Outline)
                }))
                .when(manages && !row.closed, |head| {
                    head.child(
                        design::button("thread-close", "Close…", ladder, theme, arm)
                            .kind(design::Kind::Quiet),
                    )
                }),
        );
    let reached = shown.reach();
    div()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .gap(space::BLOCK)
        .child(head)
        .children(closing(view, shown, cx, theme, ladder))
        .child(strip(view, shown, relaying, cx, theme, ladder))
        .children(reached.and_then(|reached| {
            no_node(
                "thread-no-node",
                reached.service,
                shown.serving.unwrap_or_default(),
                relaying,
                theme,
            )
        }))
        .children(shown.setup.as_ref().and_then(|setup| {
            setup.configured.problem(&setup.path).map(|problem| {
                panel("thread-problem", theme.danger_soft, theme)
                    .child(div().font_weight(FontWeight::MEDIUM).child(problem))
            })
        }))
        .children(held(shown, roster, theme))
        .child(timeline::transcript(view, shown, cx, theme, ladder))
        .into_any_element()
}

fn strip(
    view: &Agents,
    shown: &Transcript,
    relaying: usize,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let row = &shown.thread;
    let window = shown.reach().and_then(|reached| reached.window.tokens());
    let threshold = shown.threshold();
    let status = status(shown, relaying);
    let stuck = thread_state(row) == State::Stuck && !row.closed;
    let wait = waiting_words(shown);
    let lost = wait.as_ref().is_some_and(|(_, lost)| *lost);
    let badge = match status.state {
        Some(state) => state_badge("thread-state", state, theme, ladder),
        None => closed_badge("thread-state", theme, ladder),
    };
    let usage_open = opened(view, "usage", false);
    let summaries = shown.tally.summaries;
    let usage = design::button(
        "thread-usage-toggle",
        if usage_open { "Usage ▾" } else { "Usage ▸" },
        ladder,
        theme,
        toggler("usage".into(), cx),
    )
    .kind(design::Kind::Quiet);
    let line = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(space::LG)
        .child(badge)
        .child(
            div()
                .id("thread-asking")
                .font_weight(FontWeight::MEDIUM)
                .child(status.words),
        )
        .child(caption(status.caption, theme).flex_1().min_w(px(120.)))
        .children(meter(row.context, window, threshold, 96., theme))
        .child(caption(context_short(row.context, window), theme))
        .child(usage);
    div()
        .id("thread-strip")
        .flex()
        .flex_col()
        .gap(space::XS)
        .min_h(px(36.))
        .justify_center()
        .px(space::LG)
        .py(space::XXS)
        .border_1()
        .border_color(theme.border)
        .bg(if stuck || lost {
            theme.warning_soft
        } else {
            theme.surface
        })
        .child(line)
        .children(wait.map(|(words, lost)| {
            caption(words, theme)
                .id("thread-wait")
                .pb(space::XS)
                .truncate()
                .when(lost, |words| words.text_color(theme.warning))
        }))
        .children(armed_words(shown).map(|words| {
            caption(words, theme)
                .id("thread-armed")
                .pb(space::XS)
                .truncate()
        }))
        .when(usage_open, |strip| {
            strip.child(
                div()
                    .id("thread-usage")
                    .flex()
                    .flex_wrap()
                    .gap(space::SM)
                    .pb(space::XS)
                    .text_size(text::CAPTION)
                    .text_color(theme.muted)
                    .child(usage_words(&shown.tally.usage))
                    .child(design::plural(summaries, "summary", "summaries"))
                    .child(context_words(row.context, window)),
            )
        })
}

fn closing(
    view: &Agents,
    shown: &Transcript,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<impl IntoElement + use<>> {
    let id = shown.thread.id;
    if view.arming != Some(Arm::Close(id)) {
        return None;
    }
    let confirm =
        cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| view.act(Arm::Close(id), cx));
    let cancel = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| {
        view.arming = None;
        cx.notify();
    });
    Some(
        panel("thread-closing", theme.danger_soft, theme)
            .child(closes(shown))
            .child(
                div()
                    .flex()
                    .gap(space::SM)
                    .child(
                        design::button("thread-close-cancel", "Cancel", ladder, theme, cancel)
                            .kind(design::Kind::Outline),
                    )
                    .child(
                        design::button(
                            "thread-close-confirm",
                            "Close conversation",
                            ladder,
                            theme,
                            confirm,
                        )
                        .kind(design::Kind::Primary)
                        .enabled(view.acting.is_none()),
                    ),
            ),
    )
}

fn held(
    shown: &Transcript,
    roster: Option<&Roster>,
    theme: &Theme,
) -> Option<impl IntoElement + use<>> {
    if shown.held.is_empty() {
        return None;
    }
    let lines = shown.held.iter().enumerate().map(|(index, body)| {
        div()
            .id(SharedString::from(format!("held-{index}")))
            .flex()
            .gap(space::SM)
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(held_author(body, roster)),
            )
            .child(first_line(&mentions(&body.text(), roster)))
    });
    Some(
        panel("thread-held", theme.surface, theme)
            .child(caption("Waiting · they follow when the turn ends", theme))
            .children(lines),
    )
}
