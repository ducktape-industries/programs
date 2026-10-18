//! The vote on the sheet, for a reader who votes: what a press about to
//! enact asks first, what the last vote did, the motions that wait, and
//! the scheduled changes and who made each. `render` reads the state and
//! changes nothing; presses land in `voting.rs`.
use view::design::Kind;
use view::prelude::*;
use view::{Div, Stateful};

use crate::{Nodes, ui, voting, words};
use view::design::Ladder;

/// A press about to enact: what will happen, asked once.
pub(crate) fn confirm(
    view: &Nodes,
    cx: &mut Context<Nodes>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<Stateful<Div>> {
    let motion = view.voting.confirming.as_ref()?;
    let yes = cx.listener(|view: &mut Nodes, _: &ClickEvent, _, cx| view.confirm(cx));
    let no = cx.listener(|view: &mut Nodes, _: &ClickEvent, _, cx| view.hold(cx));
    Some(
        div()
            .id("nodes-confirm")
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .p_3()
            .border_1()
            .border_color(theme.warning)
            .bg(theme.warning_soft)
            .child(
                div()
                    .flex_1()
                    .min_w(px(240.))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .font_weight(view::FontWeight::MEDIUM)
                            .child(view.voting.words(motion)),
                    )
                    .child(
                        div()
                            .text_size(design::text::SECONDARY)
                            .child(words::ENACTS_NOW),
                    ),
            )
            .child(
                design::button(
                    "nodes-confirm-yes",
                    words::VOTE_AND_ENACT,
                    ladder,
                    theme,
                    yes,
                )
                .kind(Kind::Primary),
            )
            .child(
                design::button("nodes-confirm-no", words::NOT_NOW, ladder, theme, no)
                    .kind(Kind::Outline),
            ),
    )
}

/// What the last vote did, or the refusal it met, until dismissed.
pub(crate) fn outcome(
    view: &Nodes,
    cx: &mut Context<Nodes>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<Stateful<Div>> {
    let outcome = view.voting.outcome.as_ref()?;
    let (text, color, background) = match outcome {
        Ok(text) => (text.clone(), theme.foreground, theme.surface),
        Err(text) => (text.clone(), theme.danger, theme.danger_soft),
    };
    let dismiss = cx.listener(|view: &mut Nodes, _: &ClickEvent, _, cx| view.dismiss_outcome(cx));
    Some(
        div()
            .id("nodes-outcome")
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .min_h(design::size::CONTROL)
            .bg(background)
            .text_color(color)
            .text_size(design::text::SECONDARY)
            .child(div().flex_1().child(text))
            .child(design::icon_button(
                "nodes-outcome-dismiss",
                "✕",
                words::DISMISS,
                ladder,
                theme,
                dismiss,
            )),
    )
}

/// The vote's sections under the table, and where a program is changed:
/// nothing for a reader who does not vote.
pub(crate) fn sections(
    view: &Nodes,
    cx: &mut Context<Nodes>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<Div> {
    if !view.voting.votes() {
        return None;
    }
    Some(
        div()
            .flex()
            .flex_col()
            .gap_3()
            .pt_4()
            .child(pending(view, cx, theme, ladder))
            .child(scheduled(view, cx, theme, ladder))
            .child(quiet(
                "nodes-change-elsewhere",
                words::CHANGE_ELSEWHERE,
                theme,
            )),
    )
}

/// One line of a section: what it is, a quiet caption, a press.
fn line(id: impl Into<ElementId>, title: String, caption: String, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x_3()
        .gap_y_1()
        .px_2()
        .py_1()
        .min_h(design::size::CONTROL + design::space::SM)
        .border_b_1()
        .border_color(theme.border)
        .text_size(design::text::SECONDARY)
        .child(div().flex_1().min_w(px(200.)).child(title))
        .child(div().text_color(theme.muted).child(caption))
}

/// Each motion that waits: its words, its votes of the quorum, which of the
/// reader's validators voted, and Vote while one has not.
fn pending(view: &Nodes, cx: &mut Context<Nodes>, theme: &Theme, ladder: &Ladder) -> Div {
    let voting = &view.voting;
    let lines: Vec<AnyElement> = match &voting.motions {
        Loadable::Ready(motions) | Loadable::Reloading(motions, _)
            if motions.pending.is_empty() =>
        {
            vec![quiet("nodes-motions-none", words::NO_MOTION, theme)]
        }
        Loadable::Ready(motions) | Loadable::Reloading(motions, _) => motions
            .pending
            .iter()
            .enumerate()
            .map(|(index, pending)| {
                let caption = words::pending_caption(voting, pending, motions.needed);
                let title = voting.words(&pending.motion);
                let row = line(
                    ElementId::Name(format!("nodes-motion-{index}").into()),
                    title,
                    caption,
                    theme,
                );
                let open = voting.uncast(&pending.motion) > 0;
                let press = open.then(|| {
                    let motion = pending.motion.clone();
                    let vote = cx.listener(move |view: &mut Nodes, _: &ClickEvent, _, cx| {
                        view.press(motion.clone(), cx)
                    });
                    design::button(
                        ElementId::Name(format!("nodes-motion-vote-{index}").into()),
                        words::VOTE,
                        ladder,
                        theme,
                        vote,
                    )
                    .kind(Kind::Outline)
                    .enabled(voting.pressable())
                });
                row.children(press).into_any_element()
            })
            .collect(),
        Loadable::Failed(refusal) => vec![quiet(
            "nodes-motions-refused",
            design::sentence(refusal),
            theme,
        )],
        Loadable::Idle | Loadable::Loading(_) => vec![quiet(
            "nodes-motions-loading",
            words::READING_MOTIONS,
            theme,
        )],
    };
    div()
        .flex()
        .flex_col()
        .child(ui::section("nodes-motions", words::MOTIONS.into(), theme))
        .children(lines)
}

fn scheduled(view: &Nodes, cx: &mut Context<Nodes>, theme: &Theme, ladder: &Ladder) -> Div {
    let voting = &view.voting;
    let status = view.status.ready();
    let entries = voting.entries();
    let lines: Vec<AnyElement> = match &voting.scheduled {
        Loadable::Ready(schedule) | Loadable::Reloading(schedule, _) => {
            let ahead = view.scheduled_ahead().unwrap_or_default();
            if ahead.is_empty() {
                vec![quiet("nodes-scheduled-none", words::NO_CHANGE, theme)]
            } else {
                ahead
                    .into_iter()
                    .enumerate()
                    .map(|(index, change)| {
                        let motion = voting::cancel(change);
                        let caption = words::scheduled_caption(change, schedule, status);
                        let row = line(
                            ElementId::Name(format!("nodes-scheduled-{index}").into()),
                            voting::scheduled_words(change.height, &change.change, entries),
                            caption,
                            theme,
                        );
                        let press = cx.listener(move |view: &mut Nodes, _: &ClickEvent, _, cx| {
                            view.press(motion.clone(), cx)
                        });
                        row.child(
                            design::button(
                                ElementId::Name(format!("nodes-scheduled-cancel-{index}").into()),
                                words::PROPOSE_CANCEL,
                                ladder,
                                theme,
                                press,
                            )
                            .kind(Kind::Outline)
                            .enabled(voting.pressable()),
                        )
                        .into_any_element()
                    })
                    .collect()
            }
        }
        Loadable::Failed(refusal) => vec![quiet(
            "nodes-scheduled-refused",
            design::sentence(refusal),
            theme,
        )],
        Loadable::Idle | Loadable::Loading(_) => vec![quiet(
            "nodes-scheduled-loading",
            words::READING_SCHEDULE,
            theme,
        )],
    };
    div()
        .flex()
        .flex_col()
        .child(ui::section(
            "nodes-scheduled",
            words::SCHEDULED.into(),
            theme,
        ))
        .children(lines)
}

fn quiet(id: &'static str, text: impl Into<SharedString>, theme: &Theme) -> AnyElement {
    design::quiet(text, theme)
        .id(id)
        .px_2()
        .py_1()
        .into_any_element()
}
