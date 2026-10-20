//! One agent: who it is and what it is doing, where to talk to it, its conversations, and its
//! setup as rows that each open a page of it (the model's edits the front matter); the writes on
//! it open as a sheet from the title row, taking it back asking a second press there.
use harness::AgentSetup;
use view::design::{Ladder, space};
use view::prelude::*;
use view::{FontWeight, Stateful};

use super::parts::{actions, badged, fact, glyph, notice, opening_row, scroller, section};
use crate::policies::{CLAIM, CLAIM_HINT};
use crate::read::{self, Detail, Roster};
use crate::setup::Press;
use crate::ui::agent_face;
use crate::ui::setup::editor;
use crate::ui::{
    BOUND_WORDS, agent_gone, bound_badge, closed_badge, current_badge, state_badge, waiting,
};
use crate::words::{
    PROBLEM_CAPTION, agent_sentence, answers, compacts_line, conversation_label, file_link, lower,
    may_change, model_more, no_node_words, prompt, prompt_words, quoted, reach, read_at, removes,
    starts, thinking_words, thread_words, tools_words, upper_first, usage_total, usage_words,
    wait_words, window_words,
};
use crate::words::{configured_words, listen_words, server_tools_words, settle_words};
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
        .id("agent-head")
        .flex()
        .items_center()
        .gap(space::BLOCK)
        .p(space::BLOCK)
        .border_b_1()
        .border_color(theme.border)
        .child(agent_face(
            &name,
            view.picture(agent, cx),
            ladder.avatar_detail,
            theme,
        ))
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
        .reached()
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
        .children(crate::policies::conversations_row(view, cx, ladder, theme))
        .child(threads(detail, roster, cx, theme, ladder))
        .child(setup_rows(setup, detail, roster, cx, theme, ladder))
        .into_any_element()
}

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
                })
                .when(detail.bound.contains(&id), |title| {
                    title.child(bound_badge(format!("thread-{id}-bound"), theme, ladder))
                });
            let frames = design::plural(thread.frames, "frame", "frames");
            let topic = detail.topics.get(&id).map(|topic| quoted(topic));
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
                badged(
                    title,
                    Some(badge),
                    Some(topic.unwrap_or(words)),
                    theme,
                    ladder,
                ),
                Some(design::ago(roster.now, thread.last_time)),
                theme,
                ladder,
                open,
            )
            .when(detail.bound.contains(&id), |row| {
                row.aria_description(BOUND_WORDS)
            })
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

fn part_key(part: Part) -> &'static str {
    match part {
        Part::Model => "model",
        Part::File => "file",
        Part::Prompt => "prompt",
        Part::Usage => "usage",
    }
}

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
    if view.setup_draft.agent == Some(setup.agent) {
        return editor(view, cx, theme, ladder)
            .p(space::BLOCK)
            .into_any_element();
    }
    let name = roster.name(setup.agent);
    let edit = (view.me.is_some() && setup.configured.config().is_some()).then(|| {
        let opening = Press::Open(Box::new(setup.clone()));
        let open = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
            view.press_setup(opening.clone(), cx)
        });
        let button = design::button("setup-edit", "Edit front matter", ladder, theme, open)
            .kind(design::Kind::Outline);
        div().flex().flex_col().p(space::BLOCK).child(button)
    });
    let muted = |words: String| div().text_color(theme.muted).child(words);
    let mut facts = div().flex().flex_col().child(fact(
        "part-configured",
        "Configured",
        configured_words(&setup.configured),
        theme,
        ladder,
    ));
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
                thinking_words(config.thinking),
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
            ))
            .child(fact(
                "part-server-tools",
                "Server tools",
                server_tools_words(&config.server_tools),
                theme,
                ladder,
            ))
            .child(fact(
                "part-listen",
                "Listens to",
                listen_words(&config.listen, roster),
                theme,
                ladder,
            ))
            .child(fact(
                "part-settle",
                "Settle",
                settle_words(config.settle),
                theme,
                ladder,
            ));
    }
    if let Some(reached) = setup.configured.reached() {
        facts = facts.child(fact(
            "part-window",
            "Window",
            window_words(reached.window),
            theme,
            ladder,
        ));
    }
    if setup.configured.config().is_some() {
        facts = facts.child(fact(
            "part-compacts",
            "Compacts at",
            compacts_line(setup.configured.compact_at()),
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
        .children(edit)
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
        .child(agent_face(
            &name,
            view.picture(agent, cx),
            ladder.avatar_sheet,
            theme,
        ))
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
            let claims = view.claims(agent, setup.manager);
            let claim = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
                view.converse(harness::Op::ClaimConversations { agent }, cx)
            });
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
                .when(claims, |rows| {
                    rows.child(
                        design::sheet_row("agent-sheet-claim", CLAIM, ladder, theme, claim)
                            .icon("shield")
                            .hint(CLAIM_HINT),
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
