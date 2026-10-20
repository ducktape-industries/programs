//! What both phone screens draw: a page's scroller, a press on a motion,
//! a vote's outcome strip and the confirm sheet a press that enacts
//! raises (Expo `src/network/voting.tsx` `Outcomes`, `usePress`, and
//! `src/kit/Confirm.tsx`), the node's last answer, the kit's small button
//! as a vote draws it, and the Expo kit's pieces the SDK has no helper for
//! yet: the notice strip (`src/kit/Notice.tsx`) and the quiet line
//! (`src/kit/States.tsx`). Reads the vote's outcome and what waits for a
//! yes; writes the press, the yes, Not now and Dismiss.
use valset::Motion;
use view::design::{Kind, SHEET_INSET, space};
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use super::LADDER;
use crate::row::Status;
use crate::{Nodes, words};

const NOTICE_ACTION: Pixels = px(30.);
const NOTICE_REACH: Pixels = px(8.);
const CONFIRM_ROW: Pixels = px(48.);

pub(super) fn scroller(id: &'static str, body: Div) -> Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .child(body.flex().flex_col().pb(space::BLOCK))
}

pub(super) fn press(
    cx: &Context<Nodes>,
    motion: Motion,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    cx.listener(move |view: &mut Nodes, _: &ClickEvent, _, cx| view.press(motion.clone(), cx))
}

pub(super) fn outcome(
    view: &Nodes,
    cx: &mut Context<Nodes>,
    theme: &Theme,
) -> Option<Stateful<Div>> {
    let outcome = view.voting.outcome.as_ref()?;
    if outcome.is_ok() && view.voting.pressed_on != view.open {
        return None;
    }
    let dismiss = cx.listener(|view: &mut Nodes, _: &ClickEvent, _, cx| view.dismiss_outcome(cx));
    let action = action("nodes-outcome-dismiss", words::DISMISS, dismiss);
    Some(match outcome {
        Ok(said) => notice("nodes-outcome", theme.success_soft, theme.success, said).child(action),
        Err(said) => alert(notice(
            "nodes-outcome",
            theme.danger_soft,
            theme.danger,
            said,
        ))
        .child(action),
    })
}

pub(super) fn last_answer(view: &Nodes, id: &'static str, theme: &Theme) -> Option<Stateful<Div>> {
    let silent = view.status.ready().is_some() && !view.answering();
    silent.then(|| quiet(id, words::last_answer(view.ticks - view.answered), theme))
}

pub(super) fn confirm(view: &Nodes, cx: &mut Context<Nodes>, theme: &Theme) -> Option<AnyElement> {
    let motion = view.voting.confirming.as_ref()?;
    let title = view.voting.words(motion);
    let hold = cx.listener(|view: &mut Nodes, _: &(), _, cx| view.hold(cx));
    Some(
        design::sheet("nodes-confirm-sheet", title.clone(), &LADDER, theme)
            .on_dismiss(hold)
            .child(confirm_card(title, cx, theme))
            .into_any_element(),
    )
}

fn confirm_card(title: String, cx: &mut Context<Nodes>, theme: &Theme) -> Stateful<Div> {
    let no = cx.listener(|view: &mut Nodes, _: &ClickEvent, _, cx| view.hold(cx));
    let yes = cx.listener(|view: &mut Nodes, _: &ClickEvent, _, cx| view.confirm(cx));
    let not_now = design::button("nodes-confirm/cancel", words::NOT_NOW, &LADDER, theme, no)
        .kind(Kind::Quiet);
    let enact = design::button(
        "nodes-confirm/submit",
        words::VOTE_AND_ENACT,
        &LADDER,
        theme,
        yes,
    )
    .kind(Kind::Primary);
    div()
        .id("nodes-confirm")
        .flex()
        .flex_col()
        .pt(space::XS)
        .pb(space::LG)
        .child(
            div()
                .px(SHEET_INSET)
                .pb(space::MD)
                .text_type(LADDER.title)
                .font_weight(FontWeight::MEDIUM)
                .child(title),
        )
        .child(
            div()
                .px(SHEET_INSET)
                .pt(space::MD)
                .text_type(LADDER.secondary)
                .text_color(theme.warning)
                .child(words::ENACTS_NOW),
        )
        .child(
            div()
                .flex()
                .gap(space::SM)
                .px(SHEET_INSET)
                .pt(space::BLOCK)
                .child(
                    share(1.)
                        .h(CONFIRM_ROW)
                        .flex()
                        .flex_col()
                        .justify_center()
                        .child(not_now),
                )
                .child(share(2.).flex().flex_col().child(enact)),
        )
        .child(
            div()
                .px(SHEET_INSET)
                .pt(space::MD)
                .text_type(LADDER.caption)
                .text_color(theme.muted)
                .child("This phone's key signs it. The node runs the program's rules; a refusal comes back in its words."),
        )
}

pub(super) fn share(parts: f32) -> Div {
    let mut part = div().flex_basis(px(0.));
    part.style().flex_grow = Some(parts);
    part
}

pub(super) fn tint(status: &Status, live: bool, theme: &Theme) -> (Hsla, Hsla) {
    match (live, status) {
        (true, Status::InSync) => (theme.success, theme.success_soft),
        (true, Status::Behind(_)) => (theme.warning, theme.warning_soft),
        (true, Status::Quiet { .. }) => (theme.danger, theme.danger_soft),
        _ => (theme.muted, theme.surface_raised),
    }
}

fn alert(strip: Stateful<Div>) -> Stateful<Div> {
    strip
        .role(Role::Alert)
        .aria_live(view::accesskit::Live::Assertive)
}

fn notice(id: impl Into<ElementId>, ground: Hsla, tone: Hsla, sentence: &str) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(space::MD)
        .py(space::MD)
        .px(space::BLOCK)
        .bg(ground)
        .text_type(LADDER.secondary)
        .text_color(tone)
        .child(div().flex_1().child(sentence.to_owned()))
}

fn action(
    id: impl Into<ElementId>,
    label: &'static str,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    div()
        .relative()
        .flex_none()
        .h(NOTICE_ACTION)
        .flex()
        .items_center()
        .pl(space::SM)
        .font_weight(FontWeight::MEDIUM)
        .child(label)
        .child(
            div()
                .id(id)
                .absolute()
                .top(-NOTICE_REACH)
                .bottom(-NOTICE_REACH)
                .left(-NOTICE_REACH)
                .right(-NOTICE_REACH)
                .role(Role::Button)
                .aria_label(label)
                .focusable()
                .on_click(click),
        )
}

pub(super) fn small_button(
    id: ElementId,
    label: &'static str,
    pressable: bool,
    theme: &Theme,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    design::button(id, label, &LADDER, theme, click)
        .kind(Kind::Outline)
        .small()
        .enabled(pressable)
}

pub(super) fn quiet(
    id: &'static str,
    text: impl Into<SharedString>,
    theme: &Theme,
) -> Stateful<Div> {
    design::quiet(text, theme)
        .id(id)
        .p(space::BLOCK)
        .text_type(LADDER.secondary)
}
