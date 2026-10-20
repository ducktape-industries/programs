//! New change, or Edit change #n (Expo
//! `app/(tabs)/forge/[repo]/changes/new.tsx`, board 72): its source and
//! target with how they compare, the title, the body, the reviewers; Open
//! (or Save) in the title row. The source and target are picked on the
//! ref sheet (`RefSheet` "Source" / "Target"); an edit keeps its own.
//!
//! Reads the change form and the comparison of its endpoints; writes the
//! form, and opens or saves the change (`submit_change`).
use view::design::{Ladder, Nav};
use view::prelude::*;
use view::{Div, FontWeight, Stateful, wire};

use super::parts;
use crate::Forge;
use crate::id;
use crate::state::{ChangeForm, Menu, Sheet, change_key};
use crate::words::{self, ref_label};
use forge::Reply;

const CHIP_CROSS: Pixels = px(14.);

pub(super) fn render(
    forge: &Forge,
    form: &ChangeForm,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let (title, verb, scope) = match form.edit {
        Some(n) => (words::edit_change(n), "Save", change_key(&form.repo, n)),
        None => (words::NEW_CHANGE.to_owned(), "Open", "changes".to_owned()),
    };
    let back = cx.listener(|forge, _: &ClickEvent, _, cx| forge.cancel_change(cx));
    let body = div()
        .id(id("forge-change-form"))
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(design::space::XL)
        .p(design::space::BLOCK)
        .child(endpoints(forge, form, cx, ladder, theme))
        .child(title_field(form, cx, ladder, theme))
        .child(body_field(form, ladder, theme))
        .child(reviewers(forge, form, cx, ladder, theme));
    design::page(
        "forge-page-change-form",
        title,
        Nav::back(forge.nav().depth() + 1, back),
        ladder,
        theme,
    )
    .caption(form.repo.clone())
    .trailing(send(verb, ready(forge, form, &scope), cx, ladder, theme))
    .children(parts::notices(forge, ladder, theme))
    .children(parts::pending(forge, &scope, cx, ladder, theme))
    .child(body)
    .into_any_element()
}

fn ready(forge: &Forge, form: &ChangeForm, scope: &str) -> bool {
    !forge.busy(scope) && (form.edit.is_some() || !form.from.is_empty())
}

fn send(
    verb: &'static str,
    ready: bool,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let submit = cx.listener(|forge, _: &ClickEvent, _, cx| forge.submit_change(cx));
    let words = div()
        .id(id("forge-change-send"))
        .h(ladder.hit)
        .flex()
        .flex_none()
        .items_center()
        .px(design::space::SM)
        .font_weight(FontWeight::MEDIUM)
        .text_color(if ready { theme.foreground } else { theme.faint })
        .role(Role::Button)
        .child(verb);
    match ready {
        true => design::pressed(words, ladder).focusable().on_click(submit),
        false => words.aria_disabled(true),
    }
}

fn endpoints(
    forge: &Forge,
    form: &ChangeForm,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Div {
    let source = match form.from.is_empty() {
        true => "Pick a source".to_owned(),
        false => ref_label(&form.from),
    };
    let editing = form.edit.is_some();
    let picker = |menu: Menu, label: String| {
        let open = cx.listener(move |forge, event: &ClickEvent, window, cx| {
            forge.open_dropdown(menu, event, window, cx)
        });
        parts::small_button(id(menu.key()), label, "git-branch", ladder, theme, open)
            .enabled(!editing)
    };
    let pair = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(design::space::LG)
        .child(picker(Menu::Source, source))
        .child(design::icon("arrow-right", ladder.icon_row).text_color(theme.muted))
        .child(picker(Menu::Target, ref_label(&form.into)));
    div()
        .flex()
        .flex_col()
        .gap(design::space::SM)
        .child(parts::caption("Source → target", ladder, theme))
        .child(pair)
        .children(comparison(forge).map(|said| parts::caption(said, ladder, theme)))
}

fn comparison(forge: &Forge) -> Option<String> {
    let Reply::Compare { comparison, .. } = forge.ready(&forge.form_compare_query()?)? else {
        return None;
    };
    Some(words::compared(comparison))
}

fn title_field(
    form: &ChangeForm,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> impl IntoElement {
    let typed = cx.listener(|forge, _: &wire::TextChange, _, cx| {
        if let Some(form) = &mut forge.form {
            form.error.clear();
        }
        cx.notify();
    });
    let input = Input::new(id("forge-change-title"), &form.title, "Title")
        .autocorrect(true)
        .autocapitalize(Autocapitalize::Sentences)
        .placeholder(words::TITLE_PROMPT)
        .on_change(typed);
    design::field(input, ladder, theme)
        .label("Title")
        .when(!form.error.is_empty(), |field| {
            field.problem(form.error.clone())
        })
}

fn body_field(form: &ChangeForm, ladder: &Ladder, theme: &Theme) -> impl IntoElement {
    let field = Textarea::new(id("forge-change-body"), &form.body, "Body")
        .autocorrect(true)
        .autocapitalize(Autocapitalize::Sentences)
        .placeholder(words::BODY_PROMPT);
    design::field(field, ladder, theme).label("Body")
}

fn reviewers(
    forge: &Forge,
    form: &ChangeForm,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Div {
    let chips = form.reviewers.iter().map(|who| {
        let name = forge.principal_name(who);
        let gone = who.clone();
        let remove = cx.listener(move |forge, _: &ClickEvent, _, cx| {
            if let Some(form) = &mut forge.form {
                form.reviewers.retain(|kept| *kept != gone);
            }
            cx.notify();
        });
        removable_chip(&name, ladder, theme, remove)
    });
    let add = cx.listener(|forge, _: &ClickEvent, _, cx| forge.open_sheet(Sheet::Reviewers, cx));
    let row = div()
        .flex()
        .flex_wrap()
        .gap(design::space::SM)
        .children(chips)
        .child(parts::small_button(
            id("forge-change-add-reviewer"),
            "Add",
            "plus",
            ladder,
            theme,
            add,
        ));
    div()
        .flex()
        .flex_col()
        .gap(design::space::SM)
        .child(parts::caption("Reviewers", ladder, theme))
        .child(row)
}

fn removable_chip(
    name: &str,
    ladder: &Ladder,
    theme: &Theme,
    remove: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    let label = format!("Remove {name}");
    div()
        .relative()
        .h(parts::CHIP)
        .flex()
        .flex_none()
        .items_center()
        .gap(design::space::XS)
        .px(design::space::MD)
        .border_1()
        .border_color(theme.border_strong)
        .text_type(ladder.secondary)
        .whitespace_nowrap()
        .child(name.to_owned())
        .child(design::icon("x", CHIP_CROSS).text_color(theme.foreground))
        .child(
            design::hit(
                id(format!("forge-reviewer-{name}")),
                parts::CHIP,
                parts::CHIP,
                px(1.),
                ladder,
            )
            .left_0()
            .right_0()
            .role(Role::Button)
            .aria_label(label)
            .focusable()
            .on_click(remove),
        )
}

pub(super) fn reviewers_sheet(
    forge: &Forge,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let skip = forge
        .form
        .as_ref()
        .map(|form| form.reviewers.clone())
        .unwrap_or_default();
    let ask = parts::PeopleAsk {
        key: "forge-reviewers-sheet",
        title: "Reviewers",
        caption: None,
        skip: &skip,
    };
    let add = |forge: &mut Forge, person: forge::Principal, cx: &mut Context<Forge>| {
        if let Some(form) = &mut forge.form {
            form.reviewers.push(person);
        }
        cx.notify();
    };
    parts::people_sheet(ask, add, forge, cx, ladder, theme)
}
