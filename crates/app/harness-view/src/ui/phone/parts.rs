//! What two or more phone pages draw the same way: the line the last write left under a title
//! row, what scrolls under it, the control that opens a page's sheet, a section's head, a row
//! that opens a page, and a notice strip. Each draws what it is handed.
use view::design::{Ladder, space};
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use crate::{Agents, Sheet};

pub(super) fn note(view: &Agents, theme: &Theme, ladder: &Ladder) -> Option<AnyElement> {
    let strip = |id: &'static str, background: Hsla| {
        div()
            .id(id)
            .flex()
            .flex_col()
            .px(space::BLOCK)
            .py(space::MD)
            .bg(background)
            .text_type(ladder.secondary)
            .role(Role::Status)
            .aria_live(view::accesskit::Live::Polite)
    };
    if let Some(notice) = &view.notice {
        return Some(
            strip("agents-notice", theme.warning_soft)
                .child(notice.clone())
                .into_any_element(),
        );
    }
    let doing = view.doing.as_ref()?;
    Some(
        strip("agents-doing", theme.surface)
            .child(format!("{doing}…"))
            .child(
                div()
                    .text_type(ladder.caption)
                    .text_color(theme.muted)
                    .child("sent · waiting for the block"),
            )
            .into_any_element(),
    )
}

pub(super) fn scroller(id: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .pb(space::BLOCK)
}

pub(super) fn actions(
    id: &'static str,
    sheet: Sheet,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let open = cx
        .listener(move |view: &mut Agents, _: &ClickEvent, _, cx| view.show_sheet(Some(sheet), cx));
    design::icon_button(
        id,
        design::icon("ellipsis-vertical", ladder.icon),
        "Actions",
        ladder,
        theme,
        open,
    )
}

pub(super) fn section(
    id: &'static str,
    label: &'static str,
    count: usize,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    design::section_head(id, label, ladder, theme).child(count.to_string())
}

pub(super) fn opening_row(
    id: impl Into<ElementId>,
    lead: impl IntoElement,
    words: Div,
    tail: Option<String>,
    theme: &Theme,
    ladder: &Ladder,
    press: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let row = design::row(id, ladder, theme)
        .min_h(ladder.row_tall)
        .child(lead)
        .child(words)
        .children(tail.map(|tail| {
            div()
                .flex_none()
                .text_type(ladder.caption)
                .text_color(theme.muted)
                .child(tail)
        }))
        .child(design::chevron(ladder, theme));
    design::pressed(row, ladder)
        .role(Role::Button)
        .focusable()
        .on_click(press)
}

pub(super) fn glyph(name: &str, theme: &Theme, ladder: &Ladder) -> Div {
    design::lead(design::icon(name, ladder.icon_row).text_color(theme.muted))
}

pub(super) fn badged(
    title: impl IntoElement,
    badge: Option<Stateful<Div>>,
    words: Option<String>,
    theme: &Theme,
    ladder: &Ladder,
) -> Div {
    design::row_words(title).child(
        div()
            .flex()
            .items_center()
            .gap(space::SM)
            .min_w_0()
            .children(badge)
            .children(words.map(|words| design::row_caption(words, ladder, theme).min_w_0())),
    )
}

pub(super) fn notice(
    id: &'static str,
    words: impl Into<SharedString>,
    danger: bool,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let (background, foreground) = match danger {
        true => (theme.danger_soft, theme.danger),
        false => (theme.surface, theme.muted),
    };
    div()
        .id(id)
        .flex()
        .items_start()
        .gap(space::MD)
        .px(space::BLOCK)
        .py(space::MD)
        .bg(background)
        .text_type(ladder.secondary)
        .text_color(foreground)
        .when(danger, |strip| {
            strip
                .role(Role::Alert)
                .aria_live(view::accesskit::Live::Assertive)
                .child(design::icon("triangle-alert", ladder.icon_row).text_color(foreground))
        })
        .child(div().flex_1().min_w_0().child(words.into()))
}

pub(super) fn fact(
    id: impl Into<ElementId>,
    label: &'static str,
    value: impl IntoElement,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    design::row(id, ladder, theme).min_h(ladder.row_tall).child(
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(1.))
            .child(
                div()
                    .text_type(ladder.caption)
                    .text_color(theme.muted)
                    .child(label),
            )
            .child(div().font_weight(FontWeight::MEDIUM).child(value)),
    )
}
