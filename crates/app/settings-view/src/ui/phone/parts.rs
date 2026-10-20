//! The pieces both phone files draw that are this view's own, beside the
//! SDK's kit (`design`). They draw only what they are handed; none reads
//! the state or writes. The ones marked "SDK gap" are the kit's still to
//! come.
use view::design::Ladder;
use view::prelude::*;
use view::{Div, ElementId, FontWeight, Stateful, Svg};

use crate::state::Form;
use crate::words;

pub(super) fn column(id: impl Into<ElementId>) -> Stateful<Div> {
    div().id(id).flex().flex_col().w_full()
}

pub(super) fn name_input(
    id: impl Into<ElementId>,
    form: &Form,
    label: impl Into<String>,
    prompt: &str,
) -> Input {
    Input::new(id, &form.text, label)
        .placeholder(prompt)
        .disabled(form.busy)
        .autocapitalize(Autocapitalize::Sentences)
}

pub(super) fn problem_of(form: &Form, empty: &str) -> Option<String> {
    form.problem
        .as_ref()
        .map(|problem| words::problem_text(problem, empty))
}

pub(super) fn quiet(
    id: &str,
    text: impl Into<SharedString>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id(id.to_owned())
        .p(design::space::BLOCK)
        .text_type(ladder.secondary)
        .text_color(theme.muted)
        .child(text.into())
}

pub(super) fn note(text: impl Into<SharedString>, tone: Hsla, ladder: &Ladder) -> Div {
    div()
        .text_type(ladder.secondary)
        .text_color(tone)
        .child(text.into())
}

pub(super) fn mono(text: impl Into<SharedString>, ladder: &Ladder, theme: &Theme) -> Div {
    div()
        .font_family(design::fonts::FAMILY_MONO)
        .text_type(ladder.caption)
        .text_color(theme.muted)
        .child(text.into())
}

pub(super) fn muted_icon(name: &str, ladder: &Ladder, theme: &Theme) -> Svg {
    design::icon(name, ladder.icon_row).text_color(theme.muted)
}

pub(super) fn named(
    face: Div,
    name: impl IntoElement,
    about: String,
    ladder: &Ladder,
    theme: &Theme,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap(design::space::LG)
        .child(face)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(name)
                .child(mono(about, ladder, theme)),
        )
}

pub(super) fn name_line(name: &str, size: design::Type) -> Div {
    div()
        .truncate()
        .text_type(size)
        .font_weight(FontWeight::MEDIUM)
        .child(name.to_owned())
}
