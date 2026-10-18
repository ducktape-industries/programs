//! The desktop's pages, under a pointer, laid out by the pane's width
//! ([`Layout`]): narrow, a bar with the Places and History tabs over the
//! page; wide, the places down the side instead; wider still, a file or a
//! snapshot opens beside the list it is in. A folder's or a file's path
//! runs as crumbs over it, the line the last write left over the page,
//! and a write's form opens as a sheet over it all. Square corners, rows
//! split by one-pixel rules, paths and sizes in the data face, as the
//! Explorer draws. The phone's screens are `phone/`: both trees read the
//! model, `words` and the SDK, never each other.
use view::design::space;
use view::prelude::*;
use view::{Div, Stateful};

use crate::state::{ROOT, Route};
use crate::words;
use crate::write::{Form, ListChoice};
use crate::{Cx, Files, Kit};

mod file;
mod folder;
mod form;
mod history;
mod menu;
pub(crate) mod phone;
mod places;
mod side;

/// The bar's height.
const BAR_H: Pixels = px(44.);
/// A list row's height.
const ROW_H: Pixels = px(40.);
/// A detail line's label column.
const LABEL_W: Pixels = px(110.);

/// How a desktop pane lays its pages out, by its width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Layout {
    /// the tabs over one page
    Narrow,
    /// the places down the side, one page beside them; a folder lists its
    /// entries in columns
    Wide,
    /// as wide, and a file or a snapshot opens beside the list it is in
    Split,
}

/// The side's width.
const SIDE_W: f32 = 220.;
/// The narrowest a page reads beside the side: a folder's three columns.
/// With the side, 880: the width forge folds its side panes at.
const PAGE_KEEPS: f32 = 660.;
/// The narrowest a list reads beside what opened from it.
const LIST_KEEPS: f32 = 460.;
/// The narrowest a file's text or picture reads beside its list.
const DETAIL_KEEPS: f32 = 520.;

impl Layout {
    pub(crate) fn of(width: f32) -> Layout {
        if !design::docks(width, PAGE_KEEPS, SIDE_W) {
            Layout::Narrow
        } else if !design::docks(width, SIDE_W + LIST_KEEPS, DETAIL_KEEPS) {
            Layout::Wide
        } else {
            Layout::Split
        }
    }
}

pub(crate) fn render(view: &Files, cx: Cx) -> AnyElement {
    let k = &Kit {
        theme: *cx.global::<Theme>(),
        ladder: design::POINTER,
    };
    let layout = Layout::of(view.width);
    let files = div()
        .id("files")
        .size_full()
        .flex()
        .bg(k.theme.background)
        .text_color(k.theme.foreground)
        .text_size(design::text::BODY);
    let files = match layout {
        Layout::Narrow => files
            .flex_col()
            .child(bar(view, cx, k))
            .children(note(view, k))
            .child(scrolled("files-page", page(view, layout, cx, k))),
        Layout::Wide | Layout::Split => files.child(side::side(view, cx, k)).child(
            div()
                .id("files-main")
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_col()
                .children(note(view, k))
                .child(body(view, layout, cx, k)),
        ),
    };
    let form = form::sheet(view, cx, k);
    menu::over(view, files.children(form), cx, k)
}

/// The page on top of the stack, alone.
fn page(view: &Files, layout: Layout, cx: Cx, k: &Kit) -> AnyElement {
    let columns = layout != Layout::Narrow;
    match view.page().clone() {
        Route::Places => places::places(view, cx, k),
        Route::Folder(path) => folder::folder(view, &path, columns, cx, k),
        Route::File(path) => file::file(view, &path, cx, k),
        Route::History => history::history(view, None, cx, k),
        Route::Snapshot(id) => history::snapshot(view, id, cx, k),
    }
}

/// Beside the side: the page on top, or, split, the list it is in (a
/// folder's, the history) with what opened from it beside the list, or
/// what would.
fn body(view: &Files, layout: Layout, cx: Cx, k: &Kit) -> AnyElement {
    if layout != Layout::Split {
        return scrolled("files-page", page(view, layout, cx, k)).into_any_element();
    }
    let (list, detail) = match view.page().clone() {
        Route::Places => {
            return scrolled("files-page", places::places(view, cx, k)).into_any_element();
        }
        Route::Folder(path) => (
            folder::folder(view, &path, true, cx, k),
            hint(words::PICK_A_FILE, k),
        ),
        Route::File(path) => (
            folder::beside(view, &path, cx, k),
            file::detail(view, &path, cx, k),
        ),
        Route::History => (
            history::history(view, None, cx, k),
            hint(words::PICK_A_SNAPSHOT, k),
        ),
        Route::Snapshot(id) => (
            history::history(view, Some(id), cx, k),
            history::snapshot(view, id, cx, k),
        ),
    };
    div()
        .id("files-split")
        .flex_1()
        .min_h(px(0.))
        .flex()
        .child(scrolled("files-list", list).min_w(px(LIST_KEEPS)))
        .child(
            scrolled("files-detail", detail)
                .min_w(px(DETAIL_KEEPS))
                .border_l_1()
                .border_color(k.theme.border),
        )
        .into_any_element()
}

/// A pane that scrolls what it holds, sharing the room it is in.
fn scrolled(id: &'static str, content: AnyElement) -> Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .min_w_0()
        .min_h(px(0.))
        .overflow_y_scroll()
        .child(content)
}

/// What the detail pane says before anything opens in it.
fn hint(sentence: &'static str, k: &Kit) -> AnyElement {
    design::quiet(sentence, &k.theme)
        .id("files-hint")
        .px_5()
        .py_4()
        .into_any_element()
}

/// The tabs: the places (and every folder and file under them), and the
/// history (and every snapshot in it).
fn bar(view: &Files, cx: Cx, k: &Kit) -> impl IntoElement {
    let tabs = [
        ("places", words::PLACES, Route::Places),
        ("history", words::HISTORY, Route::History),
    ];
    let shown = match view.page() {
        Route::History | Route::Snapshot(_) => 1,
        Route::Places | Route::Folder(_) | Route::File(_) => 0,
    };
    let routes: Vec<Route> = tabs.iter().map(|(_, _, route)| route.clone()).collect();
    let count = routes.len();
    let items = tabs
        .into_iter()
        .enumerate()
        .map(|(index, (key, label, route))| {
            let active = index == shown;
            let go = cx.listener(move |view: &mut Files, _: &ClickEvent, _, cx| {
                view.open_from_places(route.clone(), cx)
            });
            design::item(
                design::tab(
                    format!("files/tab/{key}"),
                    label,
                    active,
                    &k.ladder,
                    &k.theme,
                    go,
                )
                .h_full()
                .mx_1(),
                Role::Tab,
                active,
            )
        });
    let pick = move |view: &mut Files, index: usize, _: &mut Window, cx: &mut Context<Files>| {
        view.open_from_places(routes[index].clone(), cx)
    };
    let moved = pick.clone();
    div()
        .id("files-bar")
        .w_full()
        .h(BAR_H)
        .flex()
        .items_center()
        .px_3()
        .border_b_1()
        .border_color(k.theme.border)
        .child(
            // one Tab stop; ← → open the next tab's page, Enter or Space
            // the tab's own from the top
            design::composite("files-tabs", Role::TabList, words::TABS)
                .orientation(design::Orientation::Horizontal)
                .wrap()
                .active(shown, count)
                .on_move(cx.processor(moved))
                .on_press(cx.processor(pick))
                .build()
                .h_full()
                .flex()
                .items_center()
                .children(items),
        )
}

/// The line the last write left.
fn note(view: &Files, k: &Kit) -> Option<Stateful<Div>> {
    let note = view.note.as_ref()?;
    Some(
        div()
            .id("files-note")
            .px_5()
            .py_2()
            .border_b_1()
            .border_color(k.theme.border)
            .bg(k.theme.surface)
            .text_size(design::text::SECONDARY)
            .text_color(k.theme.muted)
            .role(Role::Status)
            .aria_live(view::accesskit::Live::Polite)
            .child(words::note(note)),
    )
}

/// The path over a folder or a file: every folder above it a crumb that
/// opens it (`files/crumb:<its path>`), the last one where the reader is.
fn crumbs(path: &str, cx: Cx, k: &Kit) -> Stateful<Div> {
    let mut folders = vec![(ROOT.to_owned(), words::EVERYTHING.to_owned())];
    let mut at = String::new();
    for segment in path.split('/').filter(|segment| !segment.is_empty()) {
        at = format!("{at}/{segment}");
        folders.push((at.clone(), segment.to_owned()));
    }
    let last = folders.len() - 1;
    let crumbs = folders
        .into_iter()
        .enumerate()
        .map(|(index, (folder, label))| {
            let here = index == last;
            let crumb = div()
                .id(ElementId::Name(format!("files/crumb:{folder}").into()))
                .min_h(k.ladder.hit)
                .flex()
                .items_center()
                .child(label);
            match here {
                true => crumb.text_color(k.theme.foreground),
                false => crumb
                    .text_color(k.theme.muted)
                    .cursor_pointer()
                    .hover(|style| style.text_decoration_1())
                    .role(Role::Link)
                    .focusable()
                    .on_click(cx.listener(move |view: &mut Files, _: &ClickEvent, _, cx| {
                        view.open_from_places(Route::Folder(folder.clone()), cx)
                    })),
            }
        });
    div()
        .id("files-crumbs")
        .flex()
        .flex_wrap()
        .items_center()
        .gap(space::XS)
        .px_5()
        .pt_3()
        .font_family(design::fonts::FAMILY_MONO)
        .text_size(design::text::SECONDARY)
        .role(Role::Navigation)
        .aria_label(words::PATH)
        .children(with_slashes(crumbs, k))
}

/// The crumbs with a slash between each two.
fn with_slashes(crumbs: impl Iterator<Item = Stateful<Div>>, k: &Kit) -> Vec<AnyElement> {
    let mut out = Vec::new();
    for (index, crumb) in crumbs.enumerate() {
        if index > 0 {
            out.push(
                div()
                    .text_color(k.theme.faint)
                    .child("/")
                    .into_any_element(),
            );
        }
        out.push(crumb.into_any_element());
    }
    out
}

/// A section's title row: its name, then what sits on its right.
fn heading(id: &str, title: &str, right: Option<String>, k: &Kit) -> Stateful<Div> {
    design::section_head(
        ElementId::Name(id.to_owned().into()),
        title.to_owned(),
        &k.ladder,
        &k.theme,
    )
    .children(right)
}

/// A page's title, over what it is.
fn title(id: &str, text: &str, under: Option<String>, k: &Kit) -> Div {
    div()
        .px_5()
        .pt_4()
        .pb_2()
        .flex()
        .flex_col()
        .gap_1()
        .child(design::heading(
            ElementId::Name(id.to_owned().into()),
            text.to_owned(),
            1,
            &k.theme,
        ))
        .children(under.map(|under| {
            div()
                .text_size(design::text::SECONDARY)
                .text_color(k.theme.muted)
                .child(under)
        }))
}

/// A row's look: a glyph, its name, what it is on the right.
fn row_look(
    id: String,
    glyph: &'static str,
    label: String,
    aside: impl IntoElement,
    k: &Kit,
) -> Stateful<Div> {
    div()
        .id(ElementId::Name(id.into()))
        .h(ROW_H)
        .flex()
        .items_center()
        .gap_3()
        .px_5()
        .border_b_1()
        .border_color(k.theme.border)
        .child(design::icon(glyph, k.ladder.icon_row).text_color(k.theme.muted))
        .child(div().flex_1().min_w_0().truncate().child(label))
        .child(
            div()
                .flex_none()
                .text_size(design::text::SECONDARY)
                .text_color(k.theme.muted)
                .child(aside),
        )
}

/// A row that opens what it names.
fn row(
    id: String,
    glyph: &'static str,
    label: String,
    aside: impl IntoElement,
    k: &Kit,
    press: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let theme = k.theme;
    row_look(id, glyph, label, aside, k)
        .cursor_pointer()
        .hover(move |style| style.bg(theme.surface))
        .role(Role::Button)
        .focusable()
        .on_click(press)
}

/// A row of what is open beside its list, marked.
fn mark(row: Stateful<Div>, marked: bool, k: &Kit) -> Stateful<Div> {
    row.when(marked, |row| {
        row.bg(k.theme.accent_soft)
            .aria_current(view::accesskit::AriaCurrent::Page)
    })
}

/// One line of a detail: its label, then its value.
fn line(label: &'static str, value: impl IntoElement, k: &Kit) -> Div {
    div()
        .min_h(ROW_H)
        .flex()
        .items_center()
        .px_5()
        .border_b_1()
        .border_color(k.theme.border)
        .child(
            div()
                .w(LABEL_W)
                .flex_none()
                .text_size(design::text::SECONDARY)
                .text_color(k.theme.muted)
                .child(label),
        )
        .child(div().flex_1().min_w_0().child(value))
}

/// A page still reading.
fn reading(id: &'static str, sentence: &'static str, k: &Kit) -> AnyElement {
    design::quiet(sentence, &k.theme)
        .id(id)
        .px_5()
        .py_4()
        .into_any_element()
}

/// A refused read, and Retry.
fn refused(name: &str, refusal: &Error, cx: Cx, k: &Kit) -> AnyElement {
    let retry = cx.listener(|view: &mut Files, _: &ClickEvent, _, cx| view.read_page(cx));
    design::refused(name, design::sentence(refusal), &k.ladder, &k.theme, retry)
        .m_5()
        .into_any_element()
}

/// The writes the page offers, as a row of buttons.
fn writes(view: &Files, cx: Cx, k: &Kit) -> Option<Div> {
    let offered = view.offered();
    if offered.is_empty() {
        return None;
    }
    let buttons = offered.into_iter().map(|write| {
        let id = format!("files/begin/{}", write.slug());
        let label = words::verb(&write);
        let begin = cx
            .listener(move |view: &mut Files, _: &ClickEvent, _, cx| view.begin(write.clone(), cx));
        design::button(id, label, &k.ladder, &k.theme, begin).kind(design::Kind::Outline)
    });
    Some(
        div()
            .flex()
            .flex_wrap()
            .gap_2()
            .px_5()
            .py_2()
            .children(buttons),
    )
}

/// Text that reads as a path, a size or an id.
fn data(text: impl Into<SharedString>) -> Div {
    div()
        .font_family(design::fonts::FAMILY_MONO)
        .text_size(design::text::MONO)
        .child(text.into())
}

pub(crate) fn list_choices(form: &Form, cx: Cx, k: &Kit) -> Stateful<Div> {
    let picked = ListChoice::ALL
        .iter()
        .position(|choice| *choice == form.list);
    let choices = ListChoice::ALL.map(|choice| {
        (
            ElementId::Name(format!("files/write/list/{}", choice.slug()).into()),
            SharedString::from(words::list_choice(choice)),
        )
    });
    design::segmented(
        "files/write/list",
        words::WHO_WRITES,
        &k.ladder,
        &k.theme,
        picked,
        choices,
        cx.processor(|view: &mut Files, index: usize, _, cx| {
            if let Some(choice) = ListChoice::ALL.get(index) {
                view.pick_list(*choice, cx);
            }
        }),
    )
}

pub(crate) fn listed_input(form: &Form) -> Input {
    Input::new("files/write/listed", &form.listed, words::LISTED)
        .w_full()
        .placeholder("7, 12, harness")
        .autocapitalize(view::Autocapitalize::None)
}
