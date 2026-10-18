//! A conversation's timeline, as both trees draw it: a person's message, the agent's work (a
//! burst of tool calls folded to one line, each call with its result inside it), its answer, a
//! summary's mark, what harness noted, and the reply the model is streaming back as it grows.
//! Under a pointer each entry ends its head with where it is in the transcript, which a press
//! copies; under a finger that line is not drawn, and a long press on the entry opens its facts
//! as a sheet. Reads the transcript and the folds; writes the folds and the sheet.
use harness::{Body, CallRow, Forming, Frame, Partial, Said};
use view::design::Ladder;
use view::design::{fonts, space};
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use super::{copy_chip, fold, opened, toggler};
use crate::read::{Roster, Transcript};
use crate::timeline::{self, Entry, Work};
use crate::words::{
    author, call_state, first_line, flow_failed, flow_words, frame_meta, mentions, ran,
    result_head, size_words, stop_words, summary_words, usage_words, work_meta, work_size,
};
use crate::{Agents, Sheet, State, thread_state};

/// How wide an entry's avatar is.
const AVATAR: f32 = 24.;

/// How tall a call's header is at least under a pointer; under a finger it is a press's
/// [`Ladder::hit`].
const CALL_HEADER: f32 = 32.;

pub(crate) fn transcript(
    view: &Agents,
    shown: &Transcript,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let (theme, ladder) = (*theme, *ladder);
    // a row a frame: an entry draws on the row of the frame it begins with, the frames it holds
    // draw nothing, and the row after the last is the reply the model is streaming back
    let rows = view.rows_for(&shown.thread);
    list(
        "thread-transcript",
        rows,
        cx.processor(move |view, index: usize, _window, cx| {
            let drawn = row(view, index as u64 + 1, cx, &theme, &ladder);
            view.read_wanted(cx);
            drawn
        }),
    )
    .with_sizing_behavior(ListSizingBehavior::Auto)
    .flex_1()
    .min_h(px(0.))
    .w_full()
}

/// The frame an entry begins with: what its row's key and its sheet name it by.
pub(crate) fn first_seq(entry: &Entry) -> u64 {
    match entry {
        Entry::Worked(work) => work.first().seq,
        Entry::Wrote(frame)
        | Entry::Answered(frame)
        | Entry::Summary(frame)
        | Entry::Noted(frame)
        | Entry::Stray(frame) => frame.seq,
    }
}

// the list's row of frame `seq`, or past the last frame the streaming reply's; a frame above
// those read asks for them
fn row(
    view: &Agents,
    seq: u64,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let Some(Some(shown)) = view.transcript.ready() else {
        return div().into_any_element();
    };
    let agent = view.agent_name(shown.thread.agent);
    if seq > shown.thread.frames {
        let Some(partial) = &shown.partial else {
            return div().into_any_element();
        };
        for said in &partial.said {
            if let Said::ToolUse { id, .. } = said {
                view.want_call(id);
            }
        }
        return partial_row(partial, &shown.calls, &agent, theme, ladder).into_any_element();
    }
    if seq < shown.from {
        view.want_from(seq);
        return reading(seq, theme, ladder);
    }
    if seq >= shown.next() {
        return reading(seq, theme, ladder);
    }
    let entries = timeline::entries(&shown.frames);
    let Some(entry) = entries.iter().find(|entry| first_seq(entry) == seq) else {
        return div().into_any_element();
    };
    // the burst the loop is still in opens of itself
    let working = !shown.thread.closed && thread_state(&shown.thread) != State::Idle;
    let live = working && entries.last().map(first_seq) == Some(seq);
    let roster = view.roster.ready();
    entry_row(view, entry, live, shown, roster, &agent, cx, theme, ladder)
}

// a frame on its way: one muted line, so the list has a row to show while it is read
fn reading(seq: u64, theme: &Theme, ladder: &Ladder) -> AnyElement {
    small("Reading…", theme, ladder)
        .id(SharedString::from(format!("entry-{seq}-reading")))
        .px(space::SM)
        .py(space::XS)
        .into_any_element()
}

/// One muted line at the ladder's caption size.
fn small(words: impl Into<SharedString>, theme: &Theme, ladder: &Ladder) -> Div {
    div()
        .text_type(ladder.caption)
        .text_color(theme.muted)
        .child(words.into())
}

/// An entry: its author's face beside a head (name, what they did, when, and under a pointer
/// where in the transcript), and what it holds under the head. Under a finger a long press on
/// it opens the facts of the entry, which begins at frame `seq`.
#[allow(clippy::too_many_arguments)]
fn entry(
    seq: u64,
    face: Div,
    head: Vec<AnyElement>,
    meta: Option<AnyElement>,
    held: impl IntoElement,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let facts = ladder.touch.then(|| {
        // a press of a call or a fold inside passes out to here: only a finger held opens it
        cx.listener(move |view: &mut Agents, event: &ClickEvent, _, cx| {
            if let ClickEvent::Touch(touch) = event
                && touch.long_press
            {
                view.show_sheet(Some(Sheet::Frame(seq)), cx);
            }
        })
    });
    div()
        .id(SharedString::from(format!("entry-{seq}")))
        .flex()
        .gap(space::LG)
        .px(match ladder.touch {
            true => space::BLOCK,
            false => space::SM,
        })
        .py(space::LG)
        .border_b_1()
        .border_color(theme.border)
        // a press as tall as a finger's, whatever the entry holds
        .when_some(facts, |entry, facts| {
            entry
                .min_h(ladder.hit)
                .role(Role::Group)
                .focusable()
                .on_click(facts)
        })
        .child(face)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(space::XS)
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_baseline()
                        .gap(space::SM)
                        .children(head)
                        .child(div().flex_1())
                        .children(meta),
                )
                .child(held),
        )
}

/// What an entry's head says: its author, then what they did and when, muted.
fn said_by(
    name: String,
    what: String,
    time: u64,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    vec![
        div()
            .font_weight(FontWeight::MEDIUM)
            .child(name)
            .into_any_element(),
        small(what, theme, ladder).into_any_element(),
        small(design::clock(time), theme, ladder).into_any_element(),
    ]
}

fn agent_face(name: &str, theme: &Theme) -> Div {
    design::avatar(name, px(AVATAR), theme)
        .bg(theme.agent_soft)
        .text_color(theme.agent)
}

#[allow(clippy::too_many_arguments)]
fn entry_row(
    view: &Agents,
    entry_of: &Entry,
    live: bool,
    shown: &Transcript,
    roster: Option<&Roster>,
    agent: &str,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    // where the entry is, a press that copies it: a pointer's alone
    let meta = |seq: u64, words: String, cx: &mut Context<Agents>| {
        (!ladder.touch).then(|| {
            copy_chip(
                SharedString::from(format!("meta-{seq}")),
                words.clone(),
                words,
                cx,
                theme,
                ladder,
            )
            .into_any_element()
        })
    };
    match entry_of {
        Entry::Wrote(frame) => {
            let Body::Post { author: who, text } = &frame.body else {
                return div().into_any_element();
            };
            let name = author(who, roster);
            let face = design::avatar(&name, px(AVATAR), theme);
            let meta = meta(frame.seq, frame_meta(frame), cx);
            let head = said_by(name, "wrote".into(), frame.time, theme, ladder);
            // the post whole, harness's framing line included (owner, 10-18)
            let text = div().child(mentions(text, roster));
            entry(frame.seq, face, head, meta, text, cx, theme, ladder).into_any_element()
        }
        Entry::Worked(work) => {
            let first = work.first();
            let meta = meta(first.seq, work_meta(work), cx);
            let head = said_by(agent.to_owned(), "worked".into(), first.time, theme, ladder);
            let held = worked(view, work, live, shown, cx, theme, ladder);
            entry(
                first.seq,
                agent_face(agent, theme),
                head,
                meta,
                held,
                cx,
                theme,
                ladder,
            )
            .bg(theme.surface)
            .into_any_element()
        }
        Entry::Answered(frame) => {
            let Body::Turn {
                said, stop, usage, ..
            } = &frame.body
            else {
                return div().into_any_element();
            };
            let meta = meta(frame.seq, frame_meta(frame), cx);
            let head = said_by(
                agent.to_owned(),
                stop_words(*stop).into(),
                frame.time,
                theme,
                ladder,
            );
            let saids = said
                .iter()
                .enumerate()
                .map(|(index, said)| {
                    said_block(view, frame.seq, index, said, shown, cx, theme, ladder)
                })
                .collect::<Vec<_>>();
            // a turn's tokens: a pointer's faint line; a finger reads them in its facts
            let usage = (!ladder.touch).then(|| {
                div()
                    .text_type(ladder.caption)
                    .text_color(theme.faint)
                    .child(usage_words(usage))
            });
            let held = div()
                .flex()
                .flex_col()
                .gap(space::XS)
                .children(saids)
                .children(usage);
            entry(
                frame.seq,
                agent_face(agent, theme),
                head,
                meta,
                held,
                cx,
                theme,
                ladder,
            )
            .into_any_element()
        }
        Entry::Summary(frame) => summary_mark(view, frame, cx, theme, ladder),
        Entry::Noted(frame) => {
            let meta = meta(frame.seq, frame_meta(frame), cx);
            let head = said_by("harness".into(), "noted".into(), frame.time, theme, ladder);
            let face = design::avatar("harness", px(AVATAR), theme);
            let text = div().child(frame.body.text());
            entry(frame.seq, face, head, meta, text, cx, theme, ladder)
                .bg(theme.danger_soft)
                .into_any_element()
        }
        Entry::Stray(frame) => {
            let (tool, what, failed) = match &frame.body {
                Body::Result { tool, failed, .. } => (tool.clone(), "result", *failed),
                Body::Refused { tool, .. } => (tool.clone(), "refused", true),
                _ => return div().into_any_element(),
            };
            let meta = meta(frame.seq, frame_meta(frame), cx);
            let head = said_by(tool.clone(), what.into(), frame.time, theme, ladder);
            let face = design::avatar(&tool, px(AVATAR), theme);
            let text = div()
                .when(failed, |text| text.text_color(theme.danger))
                .child(frame.body.text());
            entry(frame.seq, face, head, meta, text, cx, theme, ladder).into_any_element()
        }
    }
}

/// A burst of calls: the model's words while it worked, the fold line that says what the
/// burst did, and, open, each call with its result.
#[allow(clippy::too_many_arguments)]
fn worked(
    view: &Agents,
    work: &Work,
    live: bool,
    shown: &Transcript,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let key = format!("work-{}", work.first().seq);
    let open = opened(view, &key, live);
    let words: Vec<String> = work
        .turns
        .iter()
        .flat_map(|turn| match &turn.body {
            Body::Turn { said, .. } => said.as_slice(),
            _ => &[],
        })
        .filter_map(|said| match said {
            Said::Text(text) | Said::Cited { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    let summary = div().font_weight(FontWeight::MEDIUM).child(work.summary());
    let summary = match ladder.touch {
        true => summary.text_type(ladder.small),
        false => summary,
    };
    let line = fold(key, open, summary, Some(work_size(work)), cx, theme, ladder);
    let mut calls = div().flex().flex_col().gap(space::XS);
    if open {
        for turn in &work.turns {
            let Body::Turn { said, .. } = &turn.body else {
                continue;
            };
            for (index, said) in said.iter().enumerate() {
                if let Said::Text(_) | Said::Cited { .. } = said {
                    continue;
                }
                calls = calls.child(said_block(
                    view, turn.seq, index, said, shown, cx, theme, ladder,
                ));
            }
        }
    }
    div()
        .flex()
        .flex_col()
        .gap(space::XS)
        .children(words.into_iter().map(|words| div().child(words)))
        .child(line)
        .when(open, |work| work.child(calls))
}

/// One block of what the model said in a turn: its words, its thinking folded, a call.
#[allow(clippy::too_many_arguments)]
fn said_block(
    view: &Agents,
    seq: u64,
    index: usize,
    said: &Said,
    shown: &Transcript,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    match said {
        Said::Text(text) => div().child(text.clone()).into_any_element(),
        Said::Thinking { text, .. } => {
            let key = format!("thinking-{seq}-{index}");
            let open = opened(view, &key, false);
            let line = fold(
                key,
                open,
                small("Thinking", theme, ladder),
                Some(design::plural(text.chars().count() as u64, "char", "chars")),
                cx,
                theme,
                ladder,
            );
            div()
                .flex()
                .flex_col()
                .child(line)
                .when(open, |thinking| {
                    thinking.child(div().text_color(theme.muted).child(text.clone()))
                })
                .into_any_element()
        }
        Said::RedactedThinking(_) => small("Thinking, redacted", theme, ladder).into_any_element(),
        Said::ToolUse { id, name, input } => {
            let call = shown.calls.iter().find(|call| &call.id == id);
            // a call drawn is read, once
            if call.is_none() {
                view.want_call(id);
            }
            call_box(view, (id, name, input), call, shown, cx, theme, ladder).into_any_element()
        }
        Said::Cited { text, citations } => {
            let sources = timeline::cited_sources(citations);
            let named = (!sources.is_empty())
                .then(|| small(format!("Sources: {}", sources.join(", ")), theme, ladder));
            div()
                .flex()
                .flex_col()
                .child(div().child(text.clone()))
                .children(named)
                .into_any_element()
        }
        Said::ServerToolUse { id, name, input } => {
            let result = timeline::server_result(&shown.frames, id);
            server_box(view, (id, name, input), result, cx, theme, ladder).into_any_element()
        }
        Said::ServerToolResult(block) => {
            let call = timeline::result_of(block);
            match timeline::server_use_in(&shown.frames, &call) {
                // its call's line draws it
                true => div().into_any_element(),
                false => {
                    let words = timeline::server_outcome(block).words;
                    small(format!("Result of {call} · {words}"), theme, ladder).into_any_element()
                }
            }
        }
    }
}

// a call the model vendor ran (web search, web fetch): what it did and what came back, and,
// opened, each source; harness neither runs it nor keeps a row for it
fn server_box(
    view: &Agents,
    (id, name, input): (&str, &str, &str),
    result: Option<&str>,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let key = format!("server-{id}");
    let open = view.open.contains(&key);
    let outcome = result.map(timeline::server_outcome);
    let (words, colour) = match &outcome {
        None => ("not run yet".to_owned(), theme.muted),
        Some(outcome) if outcome.failed => (outcome.words.clone(), theme.danger),
        Some(outcome) => (outcome.words.clone(), theme.muted),
    };
    let sources = match (open, outcome) {
        (true, Some(outcome)) => outcome.sources,
        _ => Vec::new(),
    };
    let header = div()
        .id(SharedString::from(key.clone()))
        .flex()
        .items_center()
        .gap(space::SM)
        .px(space::SM)
        .min_h(match ladder.touch {
            true => ladder.hit,
            false => px(CALL_HEADER),
        })
        .cursor_pointer()
        .role(Role::Button)
        .aria_expanded(open)
        .focusable()
        .on_click(toggler(key, cx))
        .map(|header| design::pressed(header, ladder))
        .child(
            div()
                .text_type(ladder.caption)
                .text_color(theme.faint)
                .child(if open { "▾" } else { "▸" }),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .font_weight(FontWeight::MEDIUM)
                .child(timeline::server_head(name, input, false)),
        )
        .child(small(words, theme, ladder).text_color(colour));
    let listed = sources
        .into_iter()
        .map(|(title, url)| small(format!("{title} · {url}"), theme, ladder));
    div()
        .flex()
        .flex_col()
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .child(header)
        .when(open, |server| {
            server.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space::XS)
                    .p(space::SM)
                    .border_t_1()
                    .border_color(theme.border)
                    .children(listed),
            )
        })
}

/// A tool call: its name, its input on one line and how far it got; opened, its whole input,
/// its result, the blocks it ran at and (under a pointer) its id.
#[allow(clippy::too_many_arguments)]
fn call_box(
    view: &Agents,
    (id, tool, input): (&str, &str, &str),
    call: Option<&CallRow>,
    shown: &Transcript,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let key = format!("call-{id}");
    let open = view.open.contains(&key);
    let (state, foreground, background) = call_state(call.map(|call| &call.state), theme);
    let header = div()
        .id(SharedString::from(key.clone()))
        .flex()
        .items_center()
        .gap(space::SM)
        .px(space::SM)
        .min_h(match ladder.touch {
            true => ladder.hit,
            false => px(CALL_HEADER),
        })
        .cursor_pointer()
        .role(Role::Button)
        .aria_expanded(open)
        .focusable()
        .on_click(toggler(key.clone(), cx))
        .map(|header| design::pressed(header, ladder))
        .child(
            div()
                .text_type(ladder.caption)
                .text_color(theme.faint)
                .child(if open { "▾" } else { "▸" }),
        )
        .child(
            design::mono(tool.to_owned())
                .text_type(ladder.code)
                .font_weight(FontWeight::MEDIUM),
        )
        .child(
            design::mono(first_line(input))
                .text_type(ladder.code)
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .text_color(theme.muted),
        )
        .child(design::badge(
            SharedString::from(format!("{key}-state")),
            state,
            ladder,
            foreground,
            background,
        ));
    let result = shown.frames.iter().find(|frame| match &frame.body {
        Body::Result { call, .. } | Body::Refused { call, .. } => call == id,
        _ => false,
    });
    let opened_body = open.then(|| {
        let chip = (!ladder.touch).then(|| {
            copy_chip(
                SharedString::from(format!("{key}-id")),
                id.to_owned(),
                id.to_owned(),
                cx,
                theme,
                ladder,
            )
        });
        div()
            .flex()
            .flex_col()
            .gap(space::XS)
            .p(space::SM)
            .border_t_1()
            .border_color(theme.border)
            .child(small("Input", theme, ladder))
            .child(wrapped_mono(input, theme, ladder))
            .children(result.map(|frame| result_part(view, frame, shown, cx, theme, ladder)))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(space::SM)
                    .children(call.map(|call| small(ran(call), theme, ladder)))
                    .children(chip),
            )
    });
    div()
        .flex()
        .flex_col()
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .child(header)
        .children(opened_body)
}

/// A call's result inside its box: its size (and that the model reads it in parts, when it
/// does), then its first lines, and all of it behind a press.
fn result_part(
    view: &Agents,
    frame: &Frame,
    shown: &Transcript,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let (text, failed, refused) = match &frame.body {
        Body::Result { text, failed, .. } => (text.clone(), *failed, false),
        Body::Refused { problem, .. } => (problem.clone(), true, true),
        _ => (String::new(), false, false),
    };
    let withheld = shown
        .withheld
        .iter()
        .find(|withheld| withheld.seq == frame.seq)
        .map(|withheld| {
            design::badge(
                SharedString::from(format!("frame-{}-withheld", frame.seq)),
                format!(
                    "Read in parts · {}",
                    design::plural(withheld.size, "byte", "bytes")
                ),
                ladder,
                theme.agent,
                theme.agent_soft,
            )
        });
    let key = format!("all-{}", frame.seq);
    let whole = view.open.contains(&key);
    let (head, more) = result_head(&text);
    let label = match refused {
        true => "Refused".to_owned(),
        false => format!("Result · {}", size_words(&text)),
    };
    div()
        .flex()
        .flex_col()
        .gap(space::XS)
        .child(
            div()
                .flex()
                .items_center()
                .gap(space::SM)
                .child(small(label, theme, ladder))
                .children(withheld),
        )
        .child(
            wrapped_mono(if whole { &text } else { &head }, theme, ladder)
                .when(failed, |text| text.text_color(theme.danger)),
        )
        .when(more, |result| {
            result.child(
                div().flex().child(
                    design::button(
                        SharedString::from(key.clone()),
                        if whole { "Show less" } else { "Show all" },
                        ladder,
                        theme,
                        toggler(key, cx),
                    )
                    .kind(design::Kind::Quiet),
                ),
            )
        })
}

/// The divider a summary draws: the model sees what is above it only as the summary, which a
/// press shows.
fn summary_mark(
    view: &Agents,
    frame: &Frame,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let key = format!("summary-{}", frame.seq);
    let open = view.open.contains(&key);
    let rule = || div().flex_1().h(px(1.)).bg(theme.border);
    div()
        .id(SharedString::from(format!("entry-{}", frame.seq)))
        .flex()
        .flex_col()
        .gap(space::XS)
        .py(space::SM)
        .when(ladder.touch, |mark| mark.px(space::BLOCK))
        .child(
            div()
                .flex()
                .items_center()
                .gap(space::SM)
                .text_type(ladder.secondary)
                .text_color(theme.muted)
                .child(rule())
                .child("Summary")
                .child(format!("· {} ·", summary_words(frame)))
                .child(
                    design::button(
                        SharedString::from(key.clone()),
                        if open { "Hide" } else { "Show" },
                        ladder,
                        theme,
                        toggler(key, cx),
                    )
                    .kind(design::Kind::Quiet),
                )
                .child(rule()),
        )
        .when(open, |mark| {
            mark.child(wrapped_mono(&frame.body.text(), theme, ladder))
        })
        .into_any_element()
}

/// The reply the model is streaming back, as it grows: its words, its thinking, each call as it
/// forms.
fn partial_row(
    partial: &Partial,
    calls: &[CallRow],
    agent: &str,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let forming = match partial.forming {
        Forming::Kept { .. } => partial.said.len().checked_sub(1),
        Forming::Nothing | Forming::Skipped { .. } => None,
    };
    let (words, so_far) = flow_words(partial);
    let blocks = partial.said.iter().enumerate().map(|(index, said)| {
        let key = format!("partial-{index}");
        partial_block(key, said, forming == Some(index), calls, theme, ladder)
    });
    let head = div()
        .flex()
        .flex_wrap()
        .items_baseline()
        .gap(space::SM)
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(agent.to_owned()),
        )
        .child(
            small(words, theme, ladder)
                .when(flow_failed(partial), |words| words.text_color(theme.danger)),
        )
        .child(small("now", theme, ladder));
    div()
        .id("thread-partial")
        .flex()
        .gap(space::LG)
        .px(match ladder.touch {
            true => space::BLOCK,
            false => space::SM,
        })
        .py(space::LG)
        .border_b_1()
        .border_color(theme.border)
        .child(agent_face(agent, theme))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(space::XS)
                .child(head)
                .children(blocks)
                .children(so_far.map(|so_far| small(so_far, theme, ladder))),
        )
}

fn partial_block(
    key: String,
    said: &Said,
    forming: bool,
    calls: &[CallRow],
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    match said {
        Said::Text(text) | Said::Cited { text, .. } => div()
            .id(SharedString::from(key))
            .child(text.clone())
            .into_any_element(),
        Said::ServerToolUse { name, input, .. } => {
            small(timeline::server_head(name, input, true), theme, ladder)
                .id(SharedString::from(key))
                .into_any_element()
        }
        Said::ServerToolResult(block) => {
            small(timeline::server_outcome(block).words, theme, ladder)
                .id(SharedString::from(key))
                .into_any_element()
        }
        Said::Thinking { text, .. } => div()
            .id(SharedString::from(key))
            .flex()
            .flex_col()
            .child(small("Thinking", theme, ladder))
            .child(div().text_color(theme.muted).child(text.clone()))
            .into_any_element(),
        Said::RedactedThinking(_) => small("Thinking, redacted", theme, ladder).into_any_element(),
        Said::ToolUse { id, name, input } => {
            let call = calls.iter().find(|call| &call.id == id);
            let (state, foreground, background) = match (call, forming) {
                (Some(call), _) => call_state(Some(&call.state), theme),
                (None, true) => ("Forming", theme.warning, theme.warning_soft),
                (None, false) => ("Formed", theme.muted, theme.surface_raised),
            };
            div()
                .id(SharedString::from(key.clone()))
                .flex()
                .flex_col()
                .border_1()
                .border_color(theme.border)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::SM)
                        .px(space::SM)
                        .min_h(px(CALL_HEADER))
                        .child(
                            design::mono(name.clone())
                                .text_type(ladder.code)
                                .font_weight(FontWeight::MEDIUM),
                        )
                        .child(div().flex_1())
                        .child(design::badge(
                            SharedString::from(format!("{key}-state")),
                            state,
                            ladder,
                            foreground,
                            background,
                        )),
                )
                .child(div().p(space::SM).child(wrapped_mono(input, theme, ladder)))
                .into_any_element()
        }
    }
}

fn wrapped_mono(text: &str, theme: &Theme, ladder: &Ladder) -> Div {
    div()
        .p(space::SM)
        .bg(theme.surface)
        .font_family(fonts::FAMILY_MONO)
        .text_type(ladder.code)
        .child(text.to_owned())
}
