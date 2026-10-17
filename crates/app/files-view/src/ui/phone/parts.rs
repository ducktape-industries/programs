//! The pieces two or more phone screens draw: a page's frame and the line
//! the last write left, the Expo kit's `Row` (a lead glyph, a title over a
//! caption, a chevron), `KeyValue`, `Quiet` and the states a read is in.
//! Each draws what it is handed.
use view::design::space;
use view::{Div, Stateful};

use super::*;

/// A key's column in a key/value row (Expo `src/kit/Facts.tsx:64`).
const KEY_W: Pixels = px(96.);

/// A page: its title row (Back by where it sits, the page's writes behind
/// one control when it offers any), the line the last write left, then
/// what the caller adds.
pub(super) fn page(
    id: &'static str,
    title: impl Into<SharedString>,
    view: &Files,
    cx: Cx,
    k: &Kit,
) -> design::Page {
    let nav = nav(view, cx);
    let page = design::page(id, title, nav, &k.ladder, &k.theme);
    let page = match actions_control(view, cx, k) {
        Some(control) => page.trailing(control),
        None => page,
    };
    match note(view, k) {
        Some(note) => page.child(note),
        None => page,
    }
}

/// The line the last write left (Expo `Notice`): a quiet strip under the
/// title row.
fn note(view: &Files, k: &Kit) -> Option<Stateful<Div>> {
    let note = view.note.as_ref()?;
    Some(
        div()
            .id("files-note")
            .px(space::BLOCK)
            .py(space::MD)
            .bg(k.theme.surface)
            .text_type(k.ladder.secondary)
            .text_color(k.theme.muted)
            .role(Role::Status)
            .aria_live(view::accesskit::Live::Polite)
            .child(words::note(note)),
    )
}

/// What scrolls under a page's title row.
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

/// A row that opens a page (Expo `Row` with `chevron`): a still row with
/// its glyph in the muted tone, the chevron; the whole row the press.
pub(super) fn opening_row(
    id: impl Into<ElementId>,
    glyph: &'static str,
    title: impl Into<SharedString>,
    caption: impl Into<SharedString>,
    k: &Kit,
    press: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let row = lined(id, glyph, k.theme.muted, title, caption, k)
        .child(design::chevron(&k.ladder, &k.theme));
    design::pressed(row, &k.ladder)
        .role(Role::Button)
        .focusable()
        .on_click(press)
}

/// A row that opens nothing: a glyph, faint, its title over its caption.
pub(super) fn still_row(
    id: impl Into<ElementId>,
    glyph: &'static str,
    title: impl Into<SharedString>,
    caption: impl Into<SharedString>,
    k: &Kit,
) -> Stateful<Div> {
    lined(id, glyph, k.theme.faint, title, caption, k)
}

/// A row's glyph in `tint`, then its title over its caption.
fn lined(
    id: impl Into<ElementId>,
    glyph: &'static str,
    tint: Hsla,
    title: impl Into<SharedString>,
    caption: impl Into<SharedString>,
    k: &Kit,
) -> Stateful<Div> {
    design::row(id, &k.ladder, &k.theme)
        .min_h(k.ladder.row_tall)
        .child(design::lead(
            design::icon(glyph, k.ladder.icon_row).text_color(tint),
        ))
        .child(
            design::row_words(design::row_title(title, &k.ladder, &k.theme))
                .child(design::row_caption(caption, &k.ladder, &k.theme)),
        )
}

/// A key and its value, on one row (Expo `KeyValue`).
pub(super) fn key_value(key: &'static str, value: impl IntoElement, k: &Kit) -> Div {
    div()
        .flex()
        .items_start()
        .gap(space::LG)
        .px(space::BLOCK)
        .py(space::MD)
        .border_b_1()
        .border_color(k.theme.border)
        .child(
            div()
                .w(KEY_W)
                .flex_none()
                .text_type(k.ladder.small)
                .text_color(k.theme.muted)
                .child(key),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_type(k.ladder.small)
                .child(value),
        )
}

/// Text in the data face: a path, an id.
pub(super) fn data(text: impl Into<SharedString>, k: &Kit) -> Div {
    div()
        .font_family(design::fonts::FAMILY_MONO)
        .text_type(k.ladder.code)
        .child(text.into())
}

/// One quiet line (Expo `Quiet`): a page still reading, a list with
/// nothing to say.
pub(super) fn quiet(id: &'static str, sentence: impl Into<SharedString>, k: &Kit) -> AnyElement {
    div()
        .id(id)
        .p(space::BLOCK)
        .text_type(k.ladder.secondary)
        .text_color(k.theme.muted)
        .child(sentence.into())
        .into_any_element()
}

/// A refused read, and Retry.
pub(super) fn refused(name: &str, refusal: &Error, cx: Cx, k: &Kit) -> AnyElement {
    let retry = cx.listener(|view: &mut Files, _: &ClickEvent, _, cx| view.read_page(cx));
    design::refused(name, design::sentence(refusal), &k.ladder, &k.theme, retry)
        .m(space::BLOCK)
        .into_any_element()
}

/// The page's writes behind one control in its title row, when it has
/// any (Expo `Header` right `Hit`). They are a mix (new, edit, rename,
/// remove), so it draws `ellipsis-vertical`: the host's capsule beside
/// it draws `ellipsis` ([`design::Page::trailing`]).
fn actions_control(view: &Files, cx: Cx, k: &Kit) -> Option<AnyElement> {
    if view.offered().is_empty() {
        return None;
    }
    let open = cx.listener(|view: &mut Files, _: &ClickEvent, _, cx| view.open_actions(cx));
    Some(
        design::icon_button(
            "files/actions",
            design::icon("ellipsis-vertical", k.ladder.icon),
            words::ACTIONS,
            &k.ladder,
            &k.theme,
            open,
        )
        .into_any_element(),
    )
}
