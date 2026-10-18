//! One conversation, read only: its strip (its state, what it is doing, how full its context
//! is), what nothing recovers when it is stuck, the input held behind its loop, then its
//! timeline (`ui::timeline`, its entries' facts behind a long press). Its writes and its usage
//! open from the title row: a sheet with Open in chat, Copy link, the usage page and Close,
//! whose second press is a card in the sheet.
use harness::Body;
use view::FontWeight;
use view::design::{Ladder, space};
use view::prelude::*;

use super::agent::confirm_card;
use super::parts::{actions, fact, notice, scroller};
use crate::read::Transcript;
use crate::timeline::{self, Entry};
use crate::ui::{closed_badge, meter, state_badge, thread_gone, timeline as drawn, waiting};
use crate::words::{
    all_tokens, asking_lower, closes, context_words, conversation_label, first_line, frame_meta,
    full, held_author, mentions, no_node_words, open_label, quoted, snapshot_words, status,
    usage_words, waiting_words, work_meta,
};
use crate::{Agents, Arm, Screen, Sheet, State, links, thread_state};

/// What nothing recovers, under a stuck conversation's strip.
const STUCK: &str = "Nothing recovers a stuck conversation today. Its manager can close it.";

/// How wide the strip's meter is.
const METER: f32 = 80.;

pub(super) fn page(
    view: &Agents,
    id: u64,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let shown = view.transcript.ready().and_then(Option::as_ref);
    let roster = view.roster.ready();
    let title = match shown {
        Some(shown) => conversation_label(&shown.thread, roster),
        None => format!("Conversation {id}"),
    };
    let mut page = super::page("thread-page", title, view, cx, theme, ladder);
    if let Some(shown) = shown {
        page = page.caption(heading_caption(view, shown)).trailing(actions(
            "thread-actions",
            Sheet::Thread,
            cx,
            theme,
            ladder,
        ));
    }
    let body = match (
        waiting("thread", &view.transcript, cx, theme, ladder),
        shown,
    ) {
        (Some(waiting), _) => waiting,
        (None, None) => thread_gone(id, theme, ladder).into_any_element(),
        (None, Some(shown)) => thread(view, shown, cx, theme, ladder),
    };
    page.child(scroller("thread-scroll").child(body))
        .into_any_element()
}

/// Whose conversation it is, and whether it is the agent's current one or closed.
fn heading_caption(view: &Agents, shown: &Transcript) -> String {
    let agent = view.agent_name(shown.thread.agent);
    let current = shown
        .setup
        .as_ref()
        .is_some_and(|setup| setup.current == Some(shown.thread.id));
    match (shown.thread.closed, current) {
        (true, _) => format!("{agent} · closed"),
        (false, true) => format!("{agent} · current conversation"),
        (false, false) => format!("{agent} · open"),
    }
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
    let relaying = roster.map_or(1, |roster| roster.relaying);
    let stuck = !row.closed && thread_state(row) == State::Stuck;
    let stopped = shown
        .model()
        .and_then(|_| no_node_words(shown.serving.unwrap_or_default(), relaying));
    let problem = shown
        .setup
        .as_ref()
        .and_then(|setup| setup.configured.problem(&setup.path));
    let held = (!shown.held.is_empty()).then(|| {
        div()
            .flex()
            .flex_col()
            .child(design::section_head(
                "thread-held",
                "Waiting · follow when the turn ends",
                ladder,
                theme,
            ))
            .children(shown.held.iter().enumerate().map(|(index, body)| {
                let who = held_author(body, roster);
                design::row(format!("held-{index}"), ladder, theme)
                    .min_h(ladder.row_tall)
                    .child(design::avatar(&who, ladder.avatar, theme))
                    .child(
                        design::row_words(design::row_title(who.clone(), ladder, theme)).child(
                            design::row_caption(
                                first_line(&mentions(&body.text(), roster)),
                                ladder,
                                theme,
                            ),
                        ),
                    )
            }))
    });
    // the agent's own words on what the conversation is about, never its name
    let topic = shown.topic.as_ref().map(|topic| {
        div()
            .id("thread-topic")
            .px(space::BLOCK)
            .py(space::SM)
            .text_type(ladder.secondary)
            .text_color(theme.muted)
            .child(quoted(topic))
    });
    // the page's height, which the transcript's list fills under the strip
    div()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .children(topic)
        .child(strip(shown, relaying, theme, ladder))
        .children(
            waiting_words(shown)
                .map(|(words, lost)| notice("thread-wait", words, lost, theme, ladder)),
        )
        .when(stuck, |page| {
            page.child(notice("thread-stuck", STUCK, false, theme, ladder))
        })
        .children(stopped.map(|words| notice("thread-no-node", words, true, theme, ladder)))
        .children(problem.map(|problem| notice("thread-problem", problem, true, theme, ladder)))
        .children(held)
        .child(drawn::transcript(view, shown, cx, theme, ladder))
        .into_any_element()
}

/// The conversation's state on one line: its badge, what it does, how full its context is. A
/// stuck one wears the warning ground.
fn strip(
    shown: &Transcript,
    relaying: usize,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let row = &shown.thread;
    let window = shown.model().and_then(|model| model.window.tokens());
    let status = status(shown, relaying);
    let stuck = !row.closed && thread_state(row) == State::Stuck;
    let badge = match status.state {
        Some(state) => state_badge("thread-state", state, theme, ladder),
        None => closed_badge("thread-state", theme, ladder),
    };
    let tail = |words: String| {
        div()
            .flex_none()
            .text_type(ladder.caption)
            .text_color(theme.muted)
            .child(words)
    };
    // a stuck one says what it is still doing, and how many wait, where the meter goes
    let (words, meter, fill) = match stuck {
        true => (
            format!("Still {}", asking_lower(row.asking)),
            None,
            Some(tail(format!("{} waiting", row.held))),
        ),
        false => (
            status.words,
            meter(row.context, window, METER, theme),
            full(row.context, window).map(|pct| tail(format!("{pct}%"))),
        ),
    };
    div()
        .id("thread-strip")
        .flex()
        .items_center()
        .gap(space::SM)
        .min_h(ladder.control)
        .px(space::BLOCK)
        .border_b_1()
        .border_color(theme.border)
        .bg(match stuck {
            true => theme.warning_soft,
            false => theme.surface,
        })
        .child(badge)
        .child(
            div()
                .id("thread-asking")
                .flex_1()
                .min_w_0()
                .truncate()
                .text_type(ladder.small)
                .font_weight(FontWeight::MEDIUM)
                .when(stuck, |words| words.text_color(theme.warning))
                .child(words),
        )
        .children(meter)
        .children(fill)
}

/// The conversation's usage and context, its own page over it: how full the context is, what
/// every turn used, and each turn's own.
pub(super) fn usage(
    view: &Agents,
    id: u64,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let shown = view.transcript.ready().and_then(Option::as_ref);
    let roster = view.roster.ready();
    let mut page = super::page("usage-page", "Usage and context", view, cx, theme, ladder);
    if let Some(shown) = shown {
        page = page.caption(conversation_label(&shown.thread, roster));
    }
    let body = match (
        waiting("thread", &view.transcript, cx, theme, ladder),
        shown,
    ) {
        (Some(waiting), _) => waiting,
        (None, None) => thread_gone(id, theme, ladder).into_any_element(),
        (None, Some(shown)) => {
            let row = &shown.thread;
            let window = shown.model().and_then(|model| model.window.tokens());
            let summaries = shown.tally.summaries;
            let turns = shown.frames.iter().filter_map(|frame| match &frame.body {
                Body::Turn { usage, .. } => Some((frame, usage)),
                _ => None,
            });
            div()
                .flex()
                .flex_col()
                .child(fact(
                    "thread-context",
                    "Context",
                    div()
                        .flex()
                        .flex_col()
                        .gap(space::XS)
                        .child(context_words(row.context, window))
                        .children(meter(row.context, window, METER * 2., theme)),
                    theme,
                    ladder,
                ))
                .child(fact(
                    "thread-usage",
                    "Tokens",
                    usage_words(&shown.tally.usage),
                    theme,
                    ladder,
                ))
                .child(fact(
                    "thread-summaries",
                    "Summaries",
                    design::plural(summaries, "summary", "summaries"),
                    theme,
                    ladder,
                ))
                .child(design::section_head("usage-turns", "Turns", ladder, theme))
                .children(turns.map(|(frame, usage)| {
                    design::row(format!("turn-{}", frame.seq), ladder, theme)
                        .min_h(ladder.row_tall)
                        .child(
                            design::row_words(design::row_title(
                                format!("#{} · {}", frame.seq, design::clock(frame.time)),
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
    };
    page.child(scroller("usage-scroll").child(body))
        .into_any_element()
}

/// A link in a sheet row's hint: its network left out, `duck://…/harness/12/chat/general`, so
/// the row's label keeps its room.
fn short_link(link: &str) -> String {
    match link
        .strip_prefix("duck://")
        .and_then(|rest| rest.split_once('/'))
    {
        Some((_, tail)) => format!("duck://…/{tail}"),
        None => link.to_owned(),
    }
}

/// The sheet over the conversation: its writes, or one entry's facts.
pub(super) fn sheet(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<AnyElement> {
    let shown = view.transcript.ready().and_then(Option::as_ref)?;
    match view.sheet? {
        Sheet::Thread => Some(actions_sheet(view, shown, cx, theme, ladder)),
        Sheet::Frame(seq) => facts_sheet(shown, seq, cx, theme, ladder),
        Sheet::Agent => None,
    }
}

fn actions_sheet(
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
    let dismiss = cx.listener(|view: &mut Agents, _: &(), _, cx| view.show_sheet(None, cx));
    let head = design::sheet_head("thread-sheet-head", theme)
        .child(
            div()
                .text_type(ladder.title)
                .child(conversation_label(row, roster)),
        )
        .child(
            div()
                .text_type(ladder.caption)
                .text_color(theme.muted)
                .child(format!(
                    "{} · {} · opened at block {}",
                    view.agent_name(row.agent),
                    design::plural(row.frames, "frame", "frames"),
                    design::grouped(row.opened)
                )),
        );
    let body = match view.arming == Some(Arm::Close(id)) {
        true => {
            let confirm = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
                view.act(Arm::Close(id), cx)
            });
            let cancel = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| {
                view.arming = None;
                cx.notify();
            });
            confirm_card(
                "thread-closing",
                closes(shown),
                design::form_verbs(
                    design::button("thread-close-cancel", "Cancel", ladder, theme, cancel)
                        .kind(design::Kind::Quiet),
                    design::button(
                        "thread-close-confirm",
                        "Close conversation",
                        ladder,
                        theme,
                        confirm,
                    )
                    .kind(design::Kind::Primary)
                    .busy(view.acting.is_some()),
                ),
                theme,
                ladder,
            )
            .into_any_element()
        }
        false => {
            let opened = links::place_link(&view.chain, &row.place);
            let link = links::work_link(&view.chain, row.agent, &row.place);
            let usage = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
                view.go(Screen::Usage(id), cx)
            });
            let arm = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
                view.arming = Some(Arm::Close(id));
                cx.notify();
            });
            let total = all_tokens(&shown.tally.usage);
            div()
                .flex()
                .flex_col()
                .children(opened.map(|(program, link)| {
                    design::sheet_row(
                        "thread-sheet-open",
                        open_label(&program),
                        ladder,
                        theme,
                        move |_, _, cx: &mut App| cx.host().open_link(&link),
                    )
                    .icon("message-square")
                }))
                .children(link.map(|link| {
                    let copied = link.clone();
                    let copy = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
                        view.copy(copied.clone(), cx)
                    });
                    design::sheet_row("thread-sheet-copy", "Copy link", ladder, theme, copy)
                        .icon("link")
                        .hint(short_link(&link))
                }))
                .child(
                    design::sheet_row(
                        "thread-sheet-usage",
                        "Usage and context",
                        ladder,
                        theme,
                        usage,
                    )
                    .icon("info")
                    .hint(format!("{} tokens", design::grouped(total))),
                )
                .when(manages && !row.closed, |rows| {
                    rows.child(
                        design::sheet_row(
                            "thread-sheet-close",
                            "Close conversation",
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
    design::sheet("thread-sheet", "Conversation actions", ladder, theme)
        .on_dismiss(dismiss)
        .child(
            div()
                .flex()
                .flex_col()
                .pb(space::BLOCK)
                .child(head)
                .child(body),
        )
        .into_any_element()
}

/// One entry's facts, each a row whose press copies it: where it is in the transcript, its
/// blocks, the configuration it read, its calls' ids, its tokens.
fn facts_sheet(
    shown: &Transcript,
    seq: u64,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<AnyElement> {
    let entries = timeline::entries(&shown.frames);
    let entry = entries
        .iter()
        .find(|entry| drawn::first_seq(entry) == seq)?;
    let (title, facts) = facts(entry);
    let dismiss = cx.listener(|view: &mut Agents, _: &(), _, cx| view.show_sheet(None, cx));
    let rows = facts
        .into_iter()
        .enumerate()
        .map(|(index, (label, value))| {
            let copied = value.clone();
            let copy = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
                view.copy(copied.clone(), cx)
            });
            design::sheet_row(format!("fact-{index}"), label, ladder, theme, copy)
                .icon("copy")
                .hint(value)
        });
    Some(
        design::sheet("frame-sheet", "Entry facts", ladder, theme)
            .on_dismiss(dismiss)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .pb(space::BLOCK)
                    .child(design::sheet_title(
                        "frame-sheet-head",
                        title,
                        ladder,
                        theme,
                    ))
                    .children(rows),
            )
            .into_any_element(),
    )
}

/// An entry's facts, each a label and the words a press copies, under the entry's place.
fn facts(entry: &Entry) -> (String, Vec<(String, String)>) {
    match entry {
        Entry::Worked(work) => {
            let (first, last) = (work.first(), work.last());
            let mut facts = vec![
                ("Frames".to_owned(), format!("#{}–#{}", first.seq, last.seq)),
                (
                    "Blocks".to_owned(),
                    format!(
                        "{}–{}",
                        design::grouped(first.height),
                        design::grouped(last.height)
                    ),
                ),
            ];
            if let Body::Turn { config, .. } = &first.body {
                facts.push(("Configuration".to_owned(), snapshot_words(config)));
            }
            facts.extend(
                work.calls()
                    .map(|call| (call.tool.to_owned(), call.id.to_owned())),
            );
            (work_meta(work), facts)
        }
        Entry::Wrote(frame)
        | Entry::Answered(frame)
        | Entry::Summary(frame)
        | Entry::Noted(frame)
        | Entry::Stray(frame) => {
            let mut facts = vec![
                ("Frame".to_owned(), format!("#{}", frame.seq)),
                ("Block".to_owned(), design::grouped(frame.height)),
                (
                    "Time".to_owned(),
                    format!("{}, {}", design::clock(frame.time), design::day(frame.time)),
                ),
            ];
            if let Body::Turn { config, usage, .. } = &frame.body {
                facts.push(("Configuration".to_owned(), snapshot_words(config)));
                facts.push(("Tokens".to_owned(), usage_words(usage)));
            }
            if let Body::Result { call, .. } | Body::Refused { call, .. } = &frame.body {
                facts.push(("Call".to_owned(), call.clone()));
            }
            (frame_meta(frame), facts)
        }
    }
}
