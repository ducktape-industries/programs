//! One agent: who it is and what it is doing, where to talk to it, its conversations, and its
//! setup as rows that each open a page of it. Its manager's writes (edit, start or fork a
//! conversation, take it back from harness) open as a sheet from the title row; taking it back
//! asks a second press inside the sheet.
use harness::{AgentSetup, Thinking};
use view::design::{Ladder, space};
use view::prelude::*;
use view::{FontWeight, Stateful};

use super::agents::face;
use super::parts::{actions, badged, fact, glyph, notice, opening_row, scroller, section};
use crate::read::{self, Detail, Roster};
use crate::ui::{agent_gone, closed_badge, current_badge, state_badge, thinking_picker, waiting};
use crate::words::{
    PROBLEM_CAPTION, agent_sentence, answers, conversation_label, file_link, lower, may_change,
    model_more, no_node_words, prompt, prompt_words, reach, read_at, removes, starts,
    thinking_words, thread_words, tools_words, upper_first, usage_total, usage_words, wait_words,
    window_words,
};
use crate::{Agents, Arm, Part, Screen, Sheet, State, agent_state, thread_state};

pub(super) fn page(
    view: &Agents,
    agent: u64,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let name = view.agent_name(agent);
    let setup = view.detail.ready().and_then(|detail| detail.setup.as_ref());
    let mut page = super::page("agent-page", name, view, cx, theme, ladder);
    if let (Some(setup), Some(roster)) = (setup, view.roster.ready()) {
        page = page.caption(format!(
            "account {agent} · managed by {}",
            roster.name(setup.manager)
        ));
    }
    if offers(view) {
        page = page.trailing(actions("agent-actions", Sheet::Agent, cx, theme, ladder));
    }
    let body = waiting("agent", &view.detail, cx, theme, ladder)
        .or_else(|| waiting("agent-roster", &view.roster, cx, theme, ladder))
        .unwrap_or_else(|| detail(view, agent, cx, theme, ladder));
    page.child(scroller("agent-scroll").child(body))
        .into_any_element()
}

/// Whether the reader has writes on the agent: its manager's, or a conversation's start.
fn offers(view: &Agents) -> bool {
    let Some(detail) = view.detail.ready() else {
        return false;
    };
    let manages = detail
        .setup
        .as_ref()
        .is_some_and(|setup| view.me == Some(setup.manager));
    manages || starts(view, detail)
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
    let Some(setup) = detail.setup.as_ref() else {
        return agent_gone(agent, theme, ladder).into_any_element();
    };
    let name = roster.name(agent);
    let card = roster.card(agent);
    let state = card.map_or(State::Idle, |card| agent_state(card, roster));
    let sentence = card.map(|card| upper_first(&agent_sentence(card, roster)));
    let mut about = Vec::new();
    if let Some((_, config)) = setup.configured.config() {
        about.push(config.model.clone());
    }
    about.push(design::plural(detail.tools.len() as u64, "tool", "tools"));
    if let Some(note) = card.and_then(|card| card.note) {
        about.push(note.to_owned());
    }
    let head = div()
        .flex()
        .items_center()
        .gap(space::BLOCK)
        .p(space::BLOCK)
        .border_b_1()
        .border_color(theme.border)
        .child(face(&name, ladder.avatar_detail, theme))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(space::XXS)
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(space::SM)
                        .child(
                            design::heading("agent-name", name.clone(), 1, theme)
                                .text_type(ladder.heading),
                        )
                        .child(state_badge("agent-state", state, theme, ladder)),
                )
                .children(sentence.map(|sentence| div().id("agent-sentence").child(sentence)))
                .child(
                    div()
                        .text_type(ladder.caption)
                        .text_color(theme.muted)
                        .child(about.join(" · ")),
                ),
        );
    let serving = card.and_then(|card| card.serving).unwrap_or_default();
    let stopped = setup
        .configured
        .model()
        .and_then(|_| no_node_words(serving, roster.relaying));
    div()
        .flex()
        .flex_col()
        .child(head)
        .children(reach_row(view, detail, agent, theme, ladder))
        .children(setup.configured.problem(&setup.path).map(|problem| {
            notice(
                "agent-problem",
                format!("{problem} {PROBLEM_CAPTION}"),
                true,
                theme,
                ladder,
            )
        }))
        .children(stopped.map(|words| notice("agent-no-node", words, true, theme, ladder)))
        .child(threads(detail, roster, cx, theme, ladder))
        .child(setup_rows(setup, detail, roster, cx, theme, ladder))
        .into_any_element()
}

/// Where to talk to the agent: its current conversation's room, and a DM with the reader.
fn reach_row(
    view: &Agents,
    detail: &Detail,
    agent: u64,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<impl IntoElement + use<>> {
    let reach = reach(view, detail, agent);
    if reach.chat.is_none() && reach.dm.is_none() {
        return None;
    }
    let opens = |id: &'static str, label: &'static str, link: Option<String>| {
        link.map(|link| {
            div().flex_1().flex().flex_col().child(
                design::button(id, label, ladder, theme, move |_, _, cx: &mut App| {
                    cx.host().open_link(&link)
                })
                .kind(design::Kind::Outline),
            )
        })
    };
    Some(
        div()
            .flex()
            .gap(space::SM)
            .p(space::BLOCK)
            .border_b_1()
            .border_color(theme.border)
            .children(opens("agent-open", "Open in chat", reach.chat))
            .children(opens("agent-dm", "Send a DM", reach.dm)),
    )
}

fn threads(
    detail: &Detail,
    roster: &Roster,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let current = detail.setup.as_ref().and_then(|setup| setup.current);
    let rows: Vec<Stateful<view::Div>> = detail
        .threads
        .iter()
        .map(|(thread, _)| {
            let id = thread.id;
            let open = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
                view.go(Screen::Thread(id), cx)
            });
            let lead = match chat::dm_peers(thread.place.strip_prefix("chat/").unwrap_or("")) {
                Some(_) => "message-square",
                None => "hash",
            };
            let title = div()
                .flex()
                .items_center()
                .gap(space::SM)
                .min_w_0()
                .child(design::row_title(
                    conversation_label(thread, Some(roster)),
                    ladder,
                    theme,
                ))
                .when(current == Some(id), |title| {
                    title.child(current_badge(format!("thread-{id}-current"), theme, ladder))
                });
            let frames = design::plural(thread.frames, "frame", "frames");
            let (badge, words) = match thread.closed {
                true => (
                    closed_badge(format!("thread-{id}-state"), theme, ladder),
                    frames,
                ),
                false => (
                    state_badge(
                        format!("thread-{id}-state"),
                        thread_state(thread),
                        theme,
                        ladder,
                    ),
                    match thread_words(thread) {
                        Some(words) => format!("{words} · {frames}"),
                        None => frames,
                    },
                ),
            };
            opening_row(
                format!("thread-{id}"),
                glyph(lead, theme, ladder),
                badged(title, Some(badge), Some(words), theme, ladder),
                Some(design::ago(roster.now, thread.last_time)),
                theme,
                ladder,
                open,
            )
        })
        .collect();
    let empty = detail.threads.is_empty().then(|| {
        design::quiet(
            "No conversation yet: a mention or a direct message opens one.",
            theme,
        )
        .text_type(ladder.secondary)
        .p(space::BLOCK)
    });
    div()
        .flex()
        .flex_col()
        .child(section(
            "agent-threads",
            "Conversations",
            detail.threads.len(),
            theme,
            ladder,
        ))
        .children(rows)
        .children(empty)
}

/// The setup, a row for each part, each opening its page.
fn setup_rows(
    setup: &AgentSetup,
    detail: &Detail,
    roster: &Roster,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let agent = setup.agent;
    let row = |part: Part, icon: &'static str, title: &'static str, caption: String| {
        let open = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
            view.go(Screen::Part(agent, part), cx)
        });
        opening_row(
            format!("agent-part-{}", part_key(part)),
            glyph(icon, theme, ladder),
            design::row_words(design::row_title(title, ladder, theme))
                .child(design::row_caption(caption, ladder, theme)),
            None,
            theme,
            ladder,
            open,
        )
    };
    let mut model = Vec::new();
    if let Some((_, config)) = setup.configured.config() {
        model.push(config.model.clone());
        model.push(lower(thinking_words(config.thinking)));
    }
    model.push(design::plural(detail.tools.len() as u64, "tool", "tools"));
    let mut file = setup.path.clone();
    if let Some(writers) = &detail.config_writers {
        file.push_str(&format!(" · {}", may_change(writers, &setup.path, roster)));
    }
    div()
        .flex()
        .flex_col()
        .child(design::section_head("agent-setup", "Setup", ladder, theme))
        .child(row(
            Part::Model,
            "settings",
            "Model, thinking, tools",
            model.join(" · "),
        ))
        .child(row(Part::File, "file", "Configuration file", file))
        .child(row(
            Part::Prompt,
            "file-text",
            "System prompt",
            prompt_words(&setup.configured),
        ))
        .child(row(Part::Usage, "info", "Usage", usage_total(detail)))
}

/// A part's name in its row's key.
fn part_key(part: Part) -> &'static str {
    match part {
        Part::Model => "model",
        Part::File => "file",
        Part::Prompt => "prompt",
        Part::Usage => "usage",
    }
}

/// One part of the agent's setup, its own page over the agent.
pub(super) fn part(
    view: &Agents,
    agent: u64,
    part: Part,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let title = match part {
        Part::Model => "Model, thinking, tools",
        Part::File => "Configuration file",
        Part::Prompt => "System prompt",
        Part::Usage => "Usage",
    };
    let page =
        super::page("part-page", title, view, cx, theme, ladder).caption(view.agent_name(agent));
    let body = waiting("agent", &view.detail, cx, theme, ladder)
        .or_else(|| waiting("agent-roster", &view.roster, cx, theme, ladder))
        .unwrap_or_else(|| {
            let (Some(detail), Some(roster)) = (view.detail.ready(), view.roster.ready()) else {
                return div().into_any_element();
            };
            let Some(setup) = detail.setup.as_ref() else {
                return agent_gone(agent, theme, ladder).into_any_element();
            };
            match part {
                Part::Model => model_part(view, setup, detail, roster, cx, theme, ladder),
                Part::File => file_part(view, setup, detail, roster, theme, ladder),
                Part::Prompt => prompt_part(setup, theme, ladder),
                Part::Usage => usage_part(detail, roster, theme, ladder),
            }
        });
    page.child(scroller("part-scroll").child(body))
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn model_part(
    view: &Agents,
    setup: &AgentSetup,
    detail: &Detail,
    roster: &Roster,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let name = roster.name(setup.agent);
    let muted = |words: String| div().text_color(theme.muted).child(words);
    let mut facts = div().flex().flex_col();
    if let Some((_, config)) = setup.configured.config() {
        facts = facts
            .child(fact(
                "part-model",
                "Model",
                div()
                    .child(config.model.clone())
                    .children(model_more(setup, roster).map(muted)),
                theme,
                ladder,
            ))
            .child(fact(
                "part-thinking",
                "Thinking",
                thinking(view, setup, &name, cx, theme, ladder),
                theme,
                ladder,
            ))
            .child(fact(
                "part-max-output",
                "Max output",
                format!("{} tokens", design::grouped(config.max_tokens.into())),
                theme,
                ladder,
            ))
            .child(fact(
                "part-wait",
                "Wait",
                wait_words(config.wait),
                theme,
                ladder,
            ));
    }
    if let Some(model) = setup.configured.model() {
        facts = facts.child(fact(
            "part-window",
            "Window",
            window_words(model.window),
            theme,
            ladder,
        ));
    }
    facts
        .child(fact(
            "part-tools",
            "Tools",
            tools_words(detail, &name),
            theme,
            ladder,
        ))
        .children(detail.tools.iter().map(|tool| {
            design::row(format!("tool-{}", tool.name), ladder, theme)
                .child(
                    design::mono(tool.name.clone())
                        .text_type(ladder.code)
                        .flex_1()
                        .min_w_0(),
                )
                .when(tool.writes, |row| {
                    row.child(
                        div()
                            .text_type(ladder.caption)
                            .text_color(theme.muted)
                            .child(format!("writes as {name}")),
                    )
                })
        }))
        .into_any_element()
}

/// The thinking level: its manager's press writes the configuration file again with that level;
/// anyone else reads it.
fn thinking(
    view: &Agents,
    setup: &AgentSetup,
    name: &str,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let Some((_, config)) = setup.configured.config() else {
        return div().into_any_element();
    };
    if view.me != Some(setup.manager) {
        return div()
            .child(thinking_words(config.thinking))
            .into_any_element();
    }
    let rethinking = view.draft.rethink && view.draft.created == Some(setup.agent);
    let picked = match rethinking && view.draft.sending.is_some() {
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
    div()
        .flex()
        .flex_col()
        .gap(space::XS)
        .pt(space::XS)
        .child(thinking_picker(
            "agent-thinking-level",
            picked,
            ladder,
            theme,
            pick,
        ))
        .children(problem.map(|problem| {
            div()
                .id("agent-thinking-problem")
                .text_type(ladder.caption)
                .text_color(theme.danger)
                .child(problem)
        }))
        .into_any_element()
}

fn file_part(
    view: &Agents,
    setup: &AgentSetup,
    detail: &Detail,
    roster: &Roster,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let link = file_link(&view.chain, &setup.path);
    let path = design::mono(setup.path.clone())
        .text_type(ladder.code)
        .whitespace_normal();
    let file = match link {
        Some(link) => {
            let row = design::row("agent-file", ladder, theme)
                .min_h(ladder.row_tall)
                .child(
                    design::row_words(div().child(path)).child(design::row_caption(
                        read_at(&setup.configured),
                        ladder,
                        theme,
                    )),
                )
                .child(design::icon("external-link", ladder.icon_row).text_color(theme.faint));
            design::pressed(row, ladder)
                .role(Role::Link)
                .focusable()
                .on_click(move |_, _, cx: &mut App| cx.host().open_link(&link))
                .into_any_element()
        }
        None => fact("agent-file", "Path", path, theme, ladder).into_any_element(),
    };
    div()
        .flex()
        .flex_col()
        .child(file)
        .children(detail.config_writers.as_ref().map(|writers| {
            fact(
                "part-writers",
                "Who may change it",
                upper_first(&may_change(writers, &setup.path, roster)),
                theme,
                ladder,
            )
        }))
        .child(fact(
            "part-answers",
            "Answers",
            answers(&detail.ownership, roster),
            theme,
            ladder,
        ))
        .into_any_element()
}

fn prompt_part(setup: &AgentSetup, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let shown = prompt(&setup.configured)
        .ok()
        .filter(|prompt| !prompt.is_empty())
        .map(str::to_owned);
    match shown {
        Some(prompt) => div()
            .id("agent-system")
            .p(space::BLOCK)
            .child(prompt)
            .into_any_element(),
        None => design::quiet(prompt_words(&setup.configured), theme)
            .text_type(ladder.secondary)
            .p(space::BLOCK)
            .into_any_element(),
    }
}

fn usage_part(detail: &Detail, roster: &Roster, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let total = read::sum(detail.threads.iter().map(|(_, usage)| usage));
    div()
        .flex()
        .flex_col()
        .child(fact(
            "agent-usage",
            "Usage",
            usage_total(detail),
            theme,
            ladder,
        ))
        .child(fact(
            "part-tokens",
            "Tokens",
            usage_words(&total),
            theme,
            ladder,
        ))
        .child(design::section_head(
            "part-conversations",
            "Conversations",
            ladder,
            theme,
        ))
        .children(detail.threads.iter().map(|(thread, usage)| {
            design::row(format!("usage-{}", thread.id), ladder, theme)
                .min_h(ladder.row_tall)
                .child(
                    design::row_words(design::row_title(
                        conversation_label(thread, Some(roster)),
                        ladder,
                        theme,
                    ))
                    .child(design::row_caption(
                        usage_words(usage),
                        ladder,
                        theme,
                    )),
                )
        }))
        .into_any_element()
}

/// The agent's writes: its manager's edit and take back, and starting or forking a
/// conversation for whoever may. Take back's second press is a card in the sheet.
pub(super) fn sheet(
    view: &Agents,
    agent: u64,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<AnyElement> {
    if view.sheet != Some(Sheet::Agent) || !offers(view) {
        return None;
    }
    let detail = view.detail.ready()?;
    let setup = detail.setup.as_ref()?;
    let name = view.agent_name(agent);
    let manages = view.me == Some(setup.manager);
    let current = setup.current;
    let dismiss = cx.listener(|view: &mut Agents, _: &(), _, cx| view.show_sheet(None, cx));
    let head = design::sheet_head("agent-sheet-head", theme)
        .flex_row()
        .items_center()
        .gap(space::LG)
        .child(face(&name, ladder.avatar_sheet, theme))
        .child(
            div()
                .flex()
                .flex_col()
                .child(div().text_type(ladder.title).child(name.clone()))
                .child(
                    div()
                        .text_type(ladder.caption)
                        .text_color(theme.muted)
                        .child(match manages {
                            true => "manager actions",
                            false => "conversation actions",
                        }),
                ),
        );
    let body = match view.arming == Some(Arm::Remove(agent)) {
        true => removing(view, &name, agent, cx, theme, ladder).into_any_element(),
        false => {
            let edit_name = name.clone();
            let edit = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
                view.edit(&edit_name, cx)
            });
            let renew = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
                view.converse(harness::Op::NewThread { agent }, cx)
            });
            let fork = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
                view.converse(harness::Op::ForkThread { agent }, cx)
            });
            let arm = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
                view.arming = Some(Arm::Remove(agent));
                cx.notify();
            });
            let begins = starts(view, detail);
            div()
                .flex()
                .flex_col()
                .when(manages, |rows| {
                    rows.child(
                        design::sheet_row(
                            "agent-sheet-edit",
                            "Edit configuration",
                            ladder,
                            theme,
                            edit,
                        )
                        .icon("pencil"),
                    )
                })
                .when(begins, |rows| {
                    rows.child(
                        design::sheet_row(
                            "agent-sheet-new",
                            "New conversation",
                            ladder,
                            theme,
                            renew,
                        )
                        .icon("plus")
                        .hint("starts from nothing"),
                    )
                })
                // a fork keeps the current conversation's context: none, nothing to fork
                .when(begins && current.is_some(), |rows| {
                    rows.child(
                        design::sheet_row("agent-sheet-fork", "Fork", ladder, theme, fork)
                            .icon("git-branch")
                            .hint("keeps the current context"),
                    )
                })
                .when(manages, |rows| {
                    rows.child(
                        design::sheet_row(
                            "agent-sheet-remove",
                            "Take back from harness",
                            ladder,
                            theme,
                            arm,
                        )
                        .icon("x")
                        .hint("second press confirms")
                        .danger(),
                    )
                })
                .into_any_element()
        }
    };
    Some(
        design::sheet("agent-sheet", "Agent actions", ladder, theme)
            .on_dismiss(dismiss)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .pb(space::BLOCK)
                    .child(head)
                    .child(body),
            )
            .into_any_element(),
    )
}

/// Take back's second press, in the sheet: what it does, then Cancel and Take back.
fn removing(
    view: &Agents,
    name: &str,
    agent: u64,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let confirm = cx
        .listener(move |view: &mut Agents, _: &ClickEvent, _, cx| view.act(Arm::Remove(agent), cx));
    let cancel = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| {
        view.arming = None;
        cx.notify();
    });
    confirm_card(
        "agent-removing",
        removes(name),
        design::form_verbs(
            design::button("agent-remove-cancel", "Cancel", ladder, theme, cancel)
                .kind(design::Kind::Quiet),
            design::button("agent-remove-confirm", "Take back", ladder, theme, confirm)
                .kind(design::Kind::Primary)
                .busy(view.acting.is_some()),
        ),
        theme,
        ladder,
    )
}

/// A write's second press inside a sheet: what it does, over its verbs.
pub(super) fn confirm_card(
    id: &'static str,
    words: String,
    verbs: view::Div,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap(space::LG)
        .p(design::SHEET_INSET)
        .child(
            div()
                .text_type(ladder.secondary)
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.danger)
                .child(words),
        )
        .child(verbs)
}
