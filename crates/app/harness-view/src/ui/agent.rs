//! One agent: who it is and what it is doing now, where to talk to it, its conversations, then
//! its setup folded (the model, thinking, tools and whom it answers, its configuration file and
//! who may change it), its system prompt folded, and what its turns used. Its manager edits its
//! configuration, starts or forks a conversation, or takes it back from harness.
use harness::{AgentSetup, Thinking};
use view::FontWeight;
use view::design::Ladder;
use view::design::{space, text};
use view::prelude::*;

use super::list::{ROW, pressable};
use super::{
    agent_gone, back, caption, closed_badge, current_badge, fact, fold, meter, no_node, opened,
    panel, state_badge, thinking_picker, waiting,
};
use crate::read::{self, Detail, Roster};
use crate::words::{
    PROBLEM_CAPTION, about, agent_sentence, ago, answers, context_short, conversation_label,
    file_link, full, may_change, model_more, prompt, prompt_words, reach, read_at, removes, starts,
    thinking_words, thread_words, tools_words, upper_first, usage_total, usage_words, wait_words,
    window_words,
};
use crate::{Agents, Arm, Screen, State, agent_state, thread_state};

pub(super) fn render(
    view: &Agents,
    agent: u64,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let back = back("agent-back", "Agents".into(), cx, theme, ladder);
    let body = waiting("agent", &view.detail, cx, theme, ladder)
        .or_else(|| waiting("agent-roster", &view.roster, cx, theme, ladder))
        .unwrap_or_else(|| detail(view, agent, cx, theme, ladder));
    div()
        .flex()
        .flex_col()
        .gap(space::BLOCK)
        .child(div().flex().child(back))
        .child(body)
        .into_any_element()
}

fn detail(
    view: &Agents,
    agent: u64,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let (Some(detail), Some(roster)) = (view.detail.ready(), view.roster.ready()) else {
        return div().into_any_element();
    };
    let Some(setup) = detail.setup.clone() else {
        return agent_gone(agent, theme, ladder).into_any_element();
    };
    let name = roster.name(agent);
    let manages = view.me == Some(setup.manager);
    let card = roster.card(agent);
    let state = card.map_or(State::Idle, |card| agent_state(card, roster));
    let sentence = card.map(|card| upper_first(&agent_sentence(card, roster)));
    let reach = reach(view, detail, agent);
    let edit_name = name.clone();
    let edit =
        cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| view.edit(&edit_name, cx));
    let arm = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
        view.arming = Some(Arm::Remove(agent));
        cx.notify();
    });
    let opens = |id: &'static str, label: &'static str, link: Option<String>| {
        link.map(|link| {
            design::button(id, label, ladder, theme, move |_, _, cx: &mut App| {
                cx.host().open_link(&link)
            })
            .kind(design::Kind::Outline)
        })
    };
    let head = div()
        .flex()
        .flex_wrap()
        .items_start()
        .gap(space::LG)
        .child(
            design::avatar(&name, px(40.), theme)
                .bg(theme.agent_soft)
                .text_color(theme.agent),
        )
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
                        .child(design::heading("agent-name", name.clone(), 1, theme))
                        .child(state_badge("agent-state", state, theme, ladder)),
                )
                .children(sentence.map(|sentence| div().id("agent-sentence").child(sentence)))
                .child(caption(
                    format!("agent · {}", about(&setup, detail, roster)),
                    theme,
                )),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(space::SM)
                .children(opens("agent-open", "Open in chat", reach.chat))
                .children(opens("agent-dm", "Send a DM", reach.dm))
                .when(manages, |actions| {
                    actions
                        .child(
                            design::button("agent-edit", "Edit…", ladder, theme, edit)
                                .kind(design::Kind::Outline),
                        )
                        .child(
                            design::button("agent-remove", "Take back…", ladder, theme, arm)
                                .kind(design::Kind::Quiet),
                        )
                }),
        );
    let window = setup
        .configured
        .model()
        .and_then(|model| model.window.tokens());
    div()
        .flex()
        .flex_col()
        .gap(space::BLOCK)
        .child(head)
        .children(removing(view, &name, agent, cx, theme, ladder))
        .children(problem(&setup, theme))
        .children(setup.configured.model().and_then(|model| {
            no_node(
                "agent-no-node",
                &model.service,
                card.and_then(|card| card.serving).unwrap_or_default(),
                roster.relaying,
                theme,
            )
        }))
        .child(threads(view, detail, roster, window, cx, theme, ladder))
        .child(setup_fold(
            view, &setup, detail, roster, &name, cx, theme, ladder,
        ))
        .child(prompt_fold(view, &setup, cx, theme, ladder))
        .child(usage_line(detail, theme))
        .into_any_element()
}

/// What keeps harness from reading the agent's configuration, and what harness does about it.
fn problem(setup: &AgentSetup, theme: &Theme) -> Option<impl IntoElement + use<>> {
    let problem = setup.configured.problem(&setup.path)?;
    Some(
        panel("agent-problem", theme.danger_soft, theme)
            .child(div().font_weight(FontWeight::MEDIUM).child(problem))
            .child(caption(PROBLEM_CAPTION, theme)),
    )
}

/// What the agent is set up with, folded: open for its manager, who changes it, closed for
/// everyone else.
#[allow(clippy::too_many_arguments)]
fn setup_fold(
    view: &Agents,
    setup: &AgentSetup,
    detail: &Detail,
    roster: &Roster,
    name: &str,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let manages = view.me == Some(setup.manager);
    let open = opened(view, "setup", manages);
    let label = div()
        .text_size(text::SECTION)
        .font_weight(FontWeight::SEMIBOLD)
        .child("Setup");
    let head = fold(
        "setup".into(),
        open,
        label,
        Some(read_at(&setup.configured)),
        cx,
        theme,
        ladder,
    );
    let theme = *theme;
    let muted = |words: String| div().text_color(theme.muted).child(words);
    let mut facts = div().flex().flex_wrap().gap_x(space::XL).gap_y(space::MD);
    if let Some((_, config)) = setup.configured.config() {
        facts = facts.child(
            fact_of(
                "Model",
                div()
                    .child(config.model.clone())
                    .children(model_more(setup, roster).map(muted)),
                &theme,
            )
            .w(px(240.)),
        );
        facts = match manages {
            true => facts.children(thinking(view, setup, name, cx, &theme, ladder)),
            false => facts.child(fact("Thinking", thinking_words(config.thinking), &theme).w(FACT)),
        };
        facts = facts
            .child(
                fact(
                    "Max output",
                    format!("{} tokens", design::grouped(config.max_tokens.into())),
                    &theme,
                )
                .w(FACT),
            )
            .child(fact("Wait", wait_words(config.wait), &theme).w(FACT));
    }
    facts = facts
        .child(fact("Tools", tools_words(detail, name), &theme).w(FACT))
        .child(fact("Answers", answers(&detail.ownership, roster), &theme).w(FACT));
    let file = match file_link(&view.chain, &setup.path) {
        Some(link) => {
            design::link("agent-file", setup.path.clone(), link, ladder, &theme).into_any_element()
        }
        None => div().child(setup.path.clone()).into_any_element(),
    };
    let writers = detail
        .config_writers
        .as_ref()
        .map(|writers| format!(" · {}", may_change(writers, &setup.path, roster)));
    facts = facts.child(
        fact_of(
            "Configuration file",
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .child(file)
                .children(writers.map(muted)),
            &theme,
        )
        .w(px(300.)),
    );
    if let Some(model) = setup.configured.model() {
        facts = facts.child(fact("Window", window_words(model.window), &theme).w(FACT));
    }
    div()
        .flex()
        .flex_col()
        .gap(space::SM)
        .child(head)
        .when(open, |fold| fold.child(facts.pl(space::BLOCK)))
}

/// How wide a fact is in the setup's grid.
const FACT: Pixels = px(180.);

/// A fact whose value is more than words: its label over it.
fn fact_of(label: &'static str, value: impl IntoElement, theme: &Theme) -> view::Div {
    div()
        .flex()
        .flex_col()
        .gap(space::HAIR)
        .child(caption(label, theme))
        .child(div().font_weight(FontWeight::MEDIUM).child(value))
}

/// The manager's thinking: a press writes the configuration file again with that level.
fn thinking(
    view: &Agents,
    setup: &AgentSetup,
    name: &str,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<impl IntoElement + use<>> {
    let (_, config) = setup.configured.config()?;
    let rethinking = view.draft.rethink && view.draft.created == Some(setup.agent);
    let sending = rethinking && view.draft.sending.is_some();
    let picked = match sending {
        true => view.draft.thinking,
        false => config.thinking,
    };
    let problem = view.draft.problem.clone().filter(|_| rethinking);
    let name = name.to_owned();
    let pick = cx.processor(
        move |view: &mut Agents, thinking: Option<Thinking>, _, cx| {
            view.rethink(&name, thinking, cx)
        },
    );
    Some(
        div()
            .flex()
            .flex_col()
            .gap(space::HAIR)
            .child(caption("Thinking", theme))
            .child(div().flex().child(thinking_picker(
                "agent-thinking-level",
                picked,
                ladder,
                theme,
                pick,
            )))
            .children(problem.map(|problem| {
                panel("agent-thinking-problem", theme.danger_soft, theme).child(problem)
            })),
    )
}

fn prompt_fold(
    view: &Agents,
    setup: &AgentSetup,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let open = opened(view, "system", false);
    let shown = prompt(&setup.configured)
        .ok()
        .filter(|prompt| !prompt.is_empty())
        .map(str::to_owned);
    let head = fold(
        "system".into(),
        open,
        div().child("System prompt"),
        Some(prompt_words(&setup.configured)),
        cx,
        theme,
        ladder,
    );
    div()
        .flex()
        .flex_col()
        .gap(space::XS)
        .child(head)
        .children(shown.filter(|_| open).map(|prompt| {
            div()
                .id("agent-system")
                .p(space::LG)
                .border_1()
                .border_color(theme.border)
                .bg(theme.surface)
                .child(prompt)
        }))
}

fn usage_line(detail: &Detail, theme: &Theme) -> impl IntoElement + use<> {
    let total = read::sum(detail.threads.iter().map(|(_, usage)| usage));
    div()
        .id("agent-usage")
        .flex()
        .flex_wrap()
        .gap(space::XS)
        .text_size(text::CAPTION)
        .child(
            div()
                .text_color(theme.muted)
                .child(format!("Usage · {}", usage_total(detail))),
        )
        .child(div().text_color(theme.faint).child(usage_words(&total)))
}

/// Take back's second press, and what it costs.
fn removing(
    view: &Agents,
    name: &str,
    agent: u64,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<impl IntoElement + use<>> {
    if view.arming != Some(Arm::Remove(agent)) {
        return None;
    }
    let confirm = cx
        .listener(move |view: &mut Agents, _: &ClickEvent, _, cx| view.act(Arm::Remove(agent), cx));
    let cancel = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| {
        view.arming = None;
        cx.notify();
    });
    Some(
        panel("agent-removing", theme.danger_soft, theme)
            .child(removes(name))
            .child(
                div()
                    .flex()
                    .gap(space::SM)
                    .child(
                        design::button("agent-remove-cancel", "Cancel", ladder, theme, cancel)
                            .kind(design::Kind::Outline),
                    )
                    .child(
                        design::button("agent-remove-confirm", "Take back", ladder, theme, confirm)
                            .kind(design::Kind::Primary)
                            .enabled(view.acting.is_none()),
                    ),
            ),
    )
}

fn threads(
    view: &Agents,
    detail: &Detail,
    roster: &Roster,
    window: Option<u64>,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let current = detail.setup.as_ref().and_then(|setup| setup.current);
    let rows = detail.threads.iter().map(|(thread, _)| {
        let id = thread.id;
        let theme = *theme;
        let state = match thread.closed {
            true => closed_badge(format!("thread-{id}-state"), &theme, ladder),
            false => state_badge(
                format!("thread-{id}-state"),
                thread_state(thread),
                &theme,
                ladder,
            ),
        };
        let fill = match (
            meter(thread.context, window, 96., &theme),
            full(thread.context, window),
        ) {
            (Some(bar), Some(pct)) => div()
                .flex()
                .items_center()
                .gap(space::SM)
                .child(bar)
                .child(caption(format!("{pct}%"), &theme))
                .into_any_element(),
            _ => caption(context_short(thread.context, window), &theme).into_any_element(),
        };
        pressable(format!("thread-{id}"), &theme)
            .when(thread.closed, |row| row.text_color(theme.muted))
            .on_click(
                cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
                    view.go(Screen::Thread(id), cx)
                }),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(220.))
                    .flex()
                    .items_center()
                    .gap(space::SM)
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .when(!thread.closed, |label| {
                                label.font_weight(FontWeight::MEDIUM)
                            })
                            .child(conversation_label(thread, Some(roster))),
                    )
                    .when(current == Some(id), |label| {
                        label.child(current_badge(
                            SharedString::from(format!("thread-{id}-current")),
                            &theme,
                            ladder,
                        ))
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(260.))
                    .flex()
                    .items_center()
                    .gap(space::SM)
                    .child(state)
                    .children(thread_words(thread).map(|words| caption(words, &theme).truncate())),
            )
            .child(
                caption(design::plural(thread.frames, "frame", "frames"), &theme)
                    .flex_none()
                    .w(px(90.)),
            )
            .child(div().flex_none().w(px(140.)).child(fill))
            .child(
                caption(ago(roster.now, thread.last_time), &theme)
                    .flex_1()
                    .text_right(),
            )
            .child(design::chevron(ladder, &theme))
    });
    let agent = detail.setup.as_ref().map(|setup| setup.agent);
    let actions = agent.filter(|_| starts(view, detail)).map(|agent| {
        let renew = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
            view.converse(harness::Op::NewThread { agent }, cx)
        });
        let fork = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
            view.converse(harness::Op::ForkThread { agent }, cx)
        });
        let idle = view.acting.is_none();
        div()
            .flex()
            .gap(space::SM)
            .child(
                design::button("agent-new-thread", "New conversation", ladder, theme, renew)
                    .kind(design::Kind::Outline)
                    .enabled(idle),
            )
            .child(
                design::button("agent-fork-thread", "Fork", ladder, theme, fork)
                    .kind(design::Kind::Outline)
                    .enabled(idle && current.is_some()),
            )
    });
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(space::SM)
                .min_h(ROW)
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(space::SM)
                        .child(design::heading("agent-threads", "Conversations", 2, theme))
                        .child(caption(detail.threads.len().to_string(), theme)),
                )
                .children(actions),
        )
        .children(rows)
        .when(detail.threads.is_empty(), |list| {
            list.child(div().py(space::SM).child(design::quiet(
                "No conversation yet: a mention or a direct message opens one.",
                theme,
            )))
        })
}
