//! The sheets over a screen: the page's writes as a list (Expo `Sheet`
//! with `SheetRow`s), and one write's form (Expo `Sheet` holding a form:
//! its title, its fields, Cancel and Commit). New design in the kit's
//! look; nearest Expo screen
//! `mobile/app/(tabs)/forge/[repo]/code/[...path].tsx`. Reads the page's
//! writes and the open form; writes the form's fields, `begin`, `submit`
//! and `close_sheet`.
use view::Autocapitalize;
use view::Div;
use view::design::space;

use super::*;
use crate::state::file_glyph;
use crate::upload;
use crate::write::{Form, ListChoice, Write};

/// Where the list of writes stops when it opens, and how far it may be
/// pulled. Expo's `Sheet` sizes a short list to its content
/// (`src/kit/Sheet.tsx:27`); a host sheet stops at fixed fractions, and
/// half the body holds the four writes a page offers at most.
const DETENTS: [f32; 2] = [0.5, 1.0];

/// A form opens at the body's full height, so Commit shows under its
/// fields without a pull.
const FORM_DETENTS: [f32; 1] = [1.0];

/// How tall the text field starts: about ten lines.
const TEXT_H: Pixels = px(200.);

/// The page's writes, each opening its form; none to offer, no sheet.
pub(super) fn actions(view: &Files, cx: Cx, k: &Kit) -> Option<AnyElement> {
    let offered = view.offered();
    if offered.is_empty() {
        return None;
    }
    let dismiss = cx.listener(|view: &mut Files, _: &(), _, cx| view.close_sheet(cx));
    let rows = offered.into_iter().map(|write| {
        let id = format!("files/begin/{}", write.slug());
        let danger = matches!(write, Write::Remove { .. });
        let glyph = write.glyph();
        let label = words::verb(&write);
        let begin = cx
            .listener(move |view: &mut Files, _: &ClickEvent, _, cx| view.begin(write.clone(), cx));
        design::sheet_row(id, label, &k.ladder, &k.theme, begin)
            .icon(glyph)
            .when(danger, design::SheetRow::danger)
    });
    Some(
        design::sheet("files/actions-sheet", words::ACTIONS, &k.ladder, &k.theme)
            .detents(DETENTS)
            .on_dismiss(dismiss)
            .child(div().flex().flex_col().pb(space::BLOCK).children(rows))
            .into_any_element(),
    )
}

/// One write's form.
pub(super) fn form(form: &Form, cx: Cx, k: &Kit) -> AnyElement {
    let title = words::form_title(&form.write);
    let dismiss = cx.listener(|view: &mut Files, _: &(), _, cx| view.close_sheet(cx));
    let removes = match &form.write {
        Write::Remove { path, folder } => Some(
            div()
                .text_type(k.ladder.secondary)
                .text_color(k.theme.muted)
                .child(words::removes(path, *folder)),
        ),
        _ => None,
    };
    let refused = form.refused.as_ref().map(|why| {
        div()
            .id("files/write/refused")
            .text_type(k.ladder.secondary)
            .text_color(k.theme.danger)
            .role(Role::Alert)
            .aria_live(view::accesskit::Live::Assertive)
            .child(why.clone())
    });
    let body = div()
        .flex()
        .flex_col()
        .gap(space::LG)
        .p(design::SHEET_INSET)
        .children(removes)
        .children(naming(form, cx, k))
        .children(text(form, k))
        .children(list(form, cx, k))
        .children(picked(form, k))
        .child(message(form, cx, k))
        .children(refused)
        .child(verbs(form, cx, k));
    design::sheet("files/write", title.clone(), &k.ladder, &k.theme)
        .detents(FORM_DETENTS)
        .on_dismiss(dismiss)
        .child(
            div()
                .id("files/write/body")
                .size_full()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .child(design::sheet_title(
                    "files/write/head",
                    title,
                    &k.ladder,
                    &k.theme,
                ))
                .child(body),
        )
        .into_any_element()
}

/// A new name, or a rename's new path. Return commits, unless the form
/// takes text still to be typed below it.
fn naming(form: &Form, cx: Cx, k: &Kit) -> Option<impl IntoElement> {
    let naming = form.write.naming()?;
    let label = words::naming(naming);
    let input = Input::new("files/write/name", &form.name, label)
        .w_full()
        .autocapitalize(Autocapitalize::None)
        .autocorrect(false);
    let input = match form.write.takes_text() {
        true => input,
        false => input.on_submit(cx.listener(|view: &mut Files, _: &(), _, cx| view.submit(cx))),
    };
    Some(
        design::field(input, &k.ladder, &k.theme)
            .label(label)
            .when_some(form.problem.clone(), design::Field::problem),
    )
}

/// A new file's text, or an edit's, in the data face: many lines, so
/// Return is a new line and Commit is the only send.
fn text(form: &Form, k: &Kit) -> Option<impl IntoElement> {
    if !form.write.takes_text() {
        return None;
    }
    let area = Textarea::new("files/write/text", &form.text, words::TEXT)
        .w_full()
        .h(TEXT_H)
        .autocapitalize(Autocapitalize::None)
        .autocorrect(false);
    Some(
        design::field(area, &k.ladder, &k.theme)
            .label(words::TEXT)
            .mono(),
    )
}

fn list(form: &Form, cx: Cx, k: &Kit) -> Option<Div> {
    if !form.write.takes_list() {
        return None;
    }
    let choices = crate::ui::list_choices(form, cx, k);
    let below = match form.list {
        ListChoice::Listed => Some(
            design::field(crate::ui::listed_input(form), &k.ladder, &k.theme)
                .label(words::LISTED)
                .into_any_element(),
        ),
        ListChoice::AsItsFolder => Some(
            div()
                .text_type(k.ladder.secondary)
                .text_color(k.theme.muted)
                .child(words::AS_ITS_FOLDER_NOTE)
                .into_any_element(),
        ),
        ListChoice::OnlyMe | ListChoice::Anyone => None,
    };
    Some(
        div()
            .flex()
            .flex_col()
            .gap(space::SM)
            .child(
                div()
                    .text_type(k.ladder.secondary)
                    .text_color(k.theme.muted)
                    .child(words::WHO_WRITES),
            )
            .child(choices)
            .children(below),
    )
}

/// An upload's files, each a row: its name over its size, whether it
/// replaces one there, or why it is left out.
fn picked(form: &Form, k: &Kit) -> Option<Div> {
    if form.upload.is_empty() {
        return None;
    }
    let rows = form.upload.iter().enumerate().map(|(index, file)| {
        let size = words::size(file.size);
        let caption = match (&file.path, &file.refused) {
            (Err(why), _) | (Ok(_), Some(why)) => why.clone(),
            (Ok(_), None) if file.replaces => format!("{size} · {}", words::REPLACES),
            (Ok(_), None) => size,
        };
        still_row(
            ElementId::Name(format!("files/upload/{index}").into()),
            file_glyph(&file.name),
            file.name.clone(),
            caption,
            k,
        )
    });
    let none = upload::sent(&form.upload)
        .is_empty()
        .then(|| quiet("files/upload/none", words::NOTHING_TO_UPLOAD, k));
    Some(div().flex().flex_col().children(rows).children(none))
}

/// The commit's message; its placeholder is what it says left empty.
fn message(form: &Form, cx: Cx, k: &Kit) -> impl IntoElement {
    let submit = cx.listener(|view: &mut Files, _: &(), _, cx| view.submit(cx));
    let input = Input::new("files/write/message", &form.message, words::MESSAGE)
        .w_full()
        .placeholder(form.placeholder())
        .on_submit(submit);
    design::field(input, &k.ladder, &k.theme).label(words::MESSAGE)
}

/// Cancel, then Commit, which waits on its answer.
fn verbs(form: &Form, cx: Cx, k: &Kit) -> Div {
    let cancel = cx.listener(|view: &mut Files, _: &ClickEvent, _, cx| view.close_sheet(cx));
    let commit = cx.listener(|view: &mut Files, _: &ClickEvent, _, cx| view.submit(cx));
    let verb = words::commit(form.sending, form.progress());
    design::form_verbs(
        design::button(
            "files/write/cancel",
            words::CANCEL,
            &k.ladder,
            &k.theme,
            cancel,
        )
        .kind(design::Kind::Quiet),
        design::button("files/write/submit", verb, &k.ladder, &k.theme, commit)
            .kind(design::Kind::Primary)
            .enabled(form.committable()),
    )
}
