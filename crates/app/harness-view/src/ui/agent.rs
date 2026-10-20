//! One agent: who it is and what it is doing, where to talk to it, its conversations, its setup
//! folded (its front matter, read or edited in place, whom it answers, its file and who may change
//! it), its prompt folded, what its turns used; its manager's edit, take back and conversations.
use harness::AgentSetup;
use view::FontWeight;
use view::design::Ladder;
use view::design::{space, text};
use view::prelude::*;

use super::list::{ROW, pressable};
use super::setup::editor;
use super::{
    BOUND_WORDS, agent_face, agent_gone, back, bound_badge, caption, closed_badge, current_badge,
    fact, fold, meter, no_node, opened, panel, state_badge, waiting,
};
use crate::policies::{CLAIM, CLAIM_HINT};
use crate::read::{self, Detail, Roster};
use crate::setup::Press;
use crate::words::{
    PROBLEM_CAPTION, about, agent_sentence, ago, answers, compacts_words, context_short,
    conversation_label, file_link, full, may_change, model_more, prompt, prompt_words, quoted,
    reach, read_at, removes, starts, thinking_words, thread_words, threshold_source, tools_words,
    upper_first, usage_total, usage_words, wait_words, window_words,
};
use crate::words::{configured_words, listen_words, server_tools_words, settle_words};
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
        .id("agent-head")
        .flex()
        .flex_wrap()
        .items_start()
        .gap(space::LG)
        .child(agent_face(&name, view.picture(agent, cx), px(40.), theme))
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

fn problem(setup: &AgentSetup, theme: &Theme) -> Option<impl IntoElement + use<>> {
    let problem = setup.configured.problem(&setup.path)?;
    Some(
        panel("agent-problem", theme.danger_soft, theme)
            .child(div().font_weight(FontWeight::MEDIUM).child(problem))
            .child(caption(PROBLEM_CAPTION, theme)),
    )
}

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
        facts = facts.child(fact("Thinking", thinking_words(config.thinking), &theme).w(FACT));
        facts = facts
            .child(
                fact(
                    "Max output",
                    format!("{} tokens", design::grouped(config.max_tokens.into())),
                    &theme,
                )
                .w(FACT),
            )
            .child(fact("Wait", wait_words(config.wait), &theme).w(FACT))
            .child(
                fact(
                    "Server tools",
                    server_tools_words(&config.server_tools),
                    &theme,
                )
                .w(FACT),
            )
            .child(fact("Listens to", listen_words(&config.listen, roster), &theme).w(px(240.)))
            .child(fact("Settle", settle_words(config.settle), &theme).w(FACT));
    }
    facts =
        facts.child(fact("Configured", configured_words(&setup.configured), &theme).w(px(300.)));
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
    if setup.configured.config().is_some() {
        let threshold = setup.configured.threshold();
        let compacts = div()
            .child(compacts_words(threshold))
            .children(threshold_source(threshold).map(muted));
        facts = facts.child(fact_of("Compacts at", compacts, &theme).w(px(240.)));
    }
    let editing = view.setup_draft.agent == Some(setup.agent);
    let readable = setup.configured.config().is_some();
    let edit = (view.me.is_some() && readable && !editing).then(|| {
        let opening = Press::Open(Box::new(setup.clone()));
        let open = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
            view.press_setup(opening.clone(), cx)
        });
        let button = design::button("setup-edit", "Edit front matter…", ladder, &theme, open)
            .kind(design::Kind::Outline);
        div().flex().pl(space::BLOCK).child(button)
    });
    let shown = match editing {
        true => editor(view, cx, &theme, ladder).max_w(px(560.)),
        false => facts,
    };
    div()
        .flex()
        .flex_col()
        .gap(space::SM)
        .child(head)
        .when(open, |fold| {
            fold.child(shown.pl(space::BLOCK)).children(edit)
        })
}

const FACT: Pixels = px(180.);

fn fact_of(label: &'static str, value: impl IntoElement, theme: &Theme) -> view::Div {
    div()
        .flex()
        .flex_col()
        .gap(space::HAIR)
        .child(caption(label, theme))
        .child(div().font_weight(FontWeight::MEDIUM).child(value))
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

fn claim(
    view: &Agents,
    detail: &Detail,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<impl IntoElement + use<>> {
    let setup = detail.setup.as_ref()?;
    let agent = setup.agent;
    if !view.claims(agent, setup.manager) {
        return None;
    }
    let press = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
        view.converse(harness::Op::ClaimConversations { agent }, cx)
    });
    Some(
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(space::SM)
            .py(space::SM)
            .child(
                design::button("agent-claim", CLAIM, ladder, theme, press)
                    .kind(design::Kind::Outline)
                    .enabled(view.acting.is_none()),
            )
            .child(caption(CLAIM_HINT, theme)),
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
    let policy = crate::policies::conversations_row(view, cx, ladder, theme);
    let claim = claim(view, detail, cx, theme, ladder);
    let current = detail.setup.as_ref().and_then(|setup| setup.current);
    let threshold = detail
        .setup
        .as_ref()
        .and_then(|setup| setup.configured.threshold());
    let threshold = threshold.map(harness::Threshold::tokens);
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
            meter(thread.context, window, threshold, 96., &theme),
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
        let topic = detail.topics.get(&id).map(|topic| {
            caption(quoted(topic), &theme)
                .id(SharedString::from(format!("thread-{id}-topic")))
                .truncate()
        });
        pressable(format!("thread-{id}"), &theme)
            .when(thread.closed, |row| row.text_color(theme.muted))
            .when(detail.bound.contains(&id), |row| {
                row.aria_description(BOUND_WORDS)
            })
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
                    .flex_col()
                    .child(
                        div()
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
                            })
                            .when(detail.bound.contains(&id), |label| {
                                label.child(bound_badge(
                                    SharedString::from(format!("thread-{id}-bound")),
                                    &theme,
                                    ladder,
                                ))
                            }),
                    )
                    .children(topic),
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
        .children(policy)
        .children(claim)
        .children(rows)
        .when(detail.threads.is_empty(), |list| {
            list.child(div().py(space::SM).child(design::quiet(
                "No conversation yet: a mention or a direct message opens one.",
                theme,
            )))
        })
}
