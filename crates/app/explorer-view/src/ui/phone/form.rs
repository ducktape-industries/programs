//! Expo `src/network/explorer/programs.tsx` `FormSheet` (mobile dev
//! 266633c): the open form as a sheet over the Programs page or a program's
//! page. Its title, the fields its edit reads, its notes, why it builds
//! nothing, Close and Schedule or Propose. Reads the form (`acting.form`),
//! the programs role and the tip; writes the fields, the
//! program or view choice (`add_view`), the send (`submit_form`) and Close
//! (`close_form`).
use view::Div;
use view::design::space;
use view::prelude::*;

use crate::acting::{Asked, Form, FormField, Problem, Takes};
use crate::{Cx, Explorer};

use super::Look;

pub(super) fn sheet(view: &Explorer, cx: Cx, k: &Look) -> Option<AnyElement> {
    let form = view.acting.form.as_ref()?;
    let network = view.network.ready()?;
    let tip = view.status.ready().map_or(0, |status| status.height);
    let notes = form.notes(network, tip).into_iter().map(|note| {
        div()
            .text_type(k.ladder.secondary)
            .text_color(k.theme.muted)
            .child(note)
    });
    let problem = form.problem.as_ref().map(|problem| {
        div()
            .id("explorer-form-problem")
            .text_type(k.ladder.secondary)
            .text_color(k.theme.danger)
            .child(problem.sentence.clone())
    });
    let dismiss = cx.listener(|view: &mut Explorer, _: &(), _, cx| view.close_form(cx));
    let body = div()
        .flex()
        .flex_col()
        .gap(space::LG)
        .p(design::SHEET_INSET)
        .children(fields(form, cx, k))
        .children(notes)
        .children(problem)
        .child(buttons(view, form, cx, k));
    Some(
        design::sheet("explorer-form-sheet", form.title(), &k.ladder, &k.theme)
            .on_dismiss(dismiss)
            .child(
                // as tall as what it holds, as Expo's form sheet has no
                // snaps (`programs.tsx:459`)
                div()
                    .id("explorer-form")
                    .flex()
                    .flex_col()
                    .child(design::sheet_title(
                        "explorer-form-head",
                        form.title(),
                        &k.ladder,
                        &k.theme,
                    ))
                    .child(body),
            )
            .into_any_element(),
    )
}

/// What its edit asks for, in the Expo form's order.
fn fields(form: &Form, cx: Cx, k: &Look) -> Vec<AnyElement> {
    form.asks()
        .into_iter()
        .map(|asked| match asked {
            Asked::Field(field) => labelled(form, field, k),
            Asked::Kind => program_or_view(form, cx, k),
        })
        .collect()
}

/// One labelled field (Expo `Field label` + `mono`): the label over the
/// box, the text in the data face, the keys it takes. A problem that is
/// the field's marks it invalid and says why to assistive tech; the eye
/// reads it once, under the form, as Expo draws it (`programs.tsx:500`).
fn labelled(form: &Form, field: FormField, k: &Look) -> AnyElement {
    let input = Input::new(field.id(), form.text(field), field.label()).placeholder(field.hint());
    let input = match field.takes() {
        Takes::Number => input.virtual_keyboard(VirtualKeyboard::Numeric),
        Takes::Id => input
            .autocapitalize(Autocapitalize::None)
            .autocorrect(false),
        Takes::Hex => input.autocapitalize(Autocapitalize::None),
    };
    let input = match Problem::of(form.problem.as_ref(), field) {
        Some(sentence) => input
            .invalid(view::accesskit::Invalid::True)
            .description(sentence.to_owned()),
        None => input,
    };
    design::field(input, &k.ladder, &k.theme)
        .label(field.label())
        .mono()
        .into_any_element()
}

/// What an Add adds: a program, or a view (Expo `Segmented`).
fn program_or_view(form: &Form, cx: Cx, k: &Look) -> AnyElement {
    let pick =
        cx.processor(|view: &mut Explorer, index: usize, _, cx| view.add_view(index == 1, cx));
    design::segmented(
        "explorer-form-kind",
        "What it adds",
        &k.ladder,
        &k.theme,
        Some(usize::from(form.view)),
        [
            (
                ElementId::from("explorer-form-program"),
                SharedString::from("program"),
            ),
            (
                ElementId::from("explorer-form-view"),
                SharedString::from("view"),
            ),
        ],
        pick,
    )
    .into_any_element()
}

/// Close, quiet; and Schedule or Propose, the primary, busy while a write
/// is out.
fn buttons(view: &Explorer, form: &Form, cx: Cx, k: &Look) -> Div {
    let send = cx.listener(|view: &mut Explorer, _: &ClickEvent, _, cx| view.submit_form(cx));
    let close = cx.listener(|view: &mut Explorer, _: &ClickEvent, _, cx| view.close_form(cx));
    design::form_verbs(
        design::button("explorer-form-close", "Close", &k.ladder, &k.theme, close)
            .kind(design::Kind::Quiet),
        design::button(
            "explorer-form-submit",
            form.verb(),
            &k.ladder,
            &k.theme,
            send,
        )
        .kind(design::Kind::Primary)
        .busy(view.acting.busy),
    )
}
