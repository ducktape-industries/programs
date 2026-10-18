//! Expo `app/(tabs)/explorer/index.tsx`, board `09-explorer`: Explorer's
//! root. The title row says "Explorer" over the network and the reader,
//! with Search (and Add on the Programs page); Search turns the row into
//! the field. Under it the five pages on a strip, the notice, and the
//! page's list ([`pages::body`]). Reads the session, the root page and the
//! search field; writes the root page (`go`), the search (`search`) and
//! whether the field is open.
use view::design::{CAPSULE_CLEARANCE, space};
use view::prelude::*;
use view::{Div, Stateful};

use crate::words;
use crate::{Cx, Explorer, Note, Route};

use super::parts::note;
use super::{Look, pages};

/// The five pages, in the strip's order: each one's key, label and list.
const PAGES: [(&str, &str, Route); 5] = [
    ("overview", "Overview", Route::Overview),
    ("blocks", "Blocks", Route::Blocks),
    ("transactions", "Transactions", Route::Transactions(None)),
    ("accounts", "Accounts", Route::Accounts),
    ("programs", "Programs", Route::Programs),
];

pub(super) fn root(view: &Explorer, cx: Cx, k: &Look) -> AnyElement {
    let strip = strip(view, cx, k);
    // a write's outcome on the Programs page sits in its scroll
    // (`programs::programs`)
    let outcome = view.note.as_ref().is_some_and(Note::of_a_write);
    let note = note(view, cx, k).filter(|_| !(outcome && view.pages.first() == &Route::Programs));
    let page = pages::body(view, cx, k);
    if view.searching {
        searching(view, cx, k)
            .child(strip)
            .children(note)
            .child(page)
            .into_any_element()
    } else {
        titled(view, cx, k)
            .child(strip)
            .children(note)
            .child(page)
            .into_any_element()
    }
}

/// The reader's account name (Expo `index.tsx:61` `me.name`): none while
/// the session or the accounts do not say it yet.
fn me(view: &Explorer) -> Option<&str> {
    let number = view.acting.account?;
    Some(view.account(number)?.card.name.as_str())
}

// SDK gap: the root header's network caption, the SDK builds it later
/// The network the session is on, by its label (`<label>#<salt>`), and the
/// reader (Expo's root caption, `testkit · Dev`).
fn network_and_me(view: &Explorer) -> Option<String> {
    let network = view.network_label()?;
    Some(match me(view) {
        Some(me) => format!("{network} · {me}"),
        None => network.to_owned(),
    })
}

/// The title row: "Explorer", its caption, Search, and on the Programs
/// page Add ([`super::programs::add`]).
fn titled(view: &Explorer, cx: Cx, k: &Look) -> design::Page {
    let open = cx.listener(|view: &mut Explorer, _: &ClickEvent, window, cx| {
        view.searching = true;
        window.focus("explorer-search");
        cx.notify();
    });
    // the reader's avatar leads the row (Expo `index.tsx:80` `root`)
    let reader = me(view).unwrap_or_default().to_owned();
    let page = design::page(
        "explorer-root",
        "Explorer",
        design::Nav::root(reader),
        &k.ladder,
        &k.theme,
    )
    .trailing(design::icon_button(
        "explorer-search-open",
        design::icon("search", k.ladder.icon),
        "Search",
        &k.ladder,
        &k.theme,
        open,
    ));
    let page = match network_and_me(view) {
        Some(caption) => page.caption(caption),
        None => page,
    };
    match view.pages.first() {
        Route::Programs => super::programs::add(view, page, cx, k),
        _ => page,
    }
}

/// The root while searching (Expo `Header center`): the field in the
/// title's place and Close search, the page under it.
fn searching(view: &Explorer, cx: Cx, k: &Look) -> Stateful<Div> {
    let close = cx.listener(|view: &mut Explorer, _: &ClickEvent, _, cx| {
        view.searching = false;
        view.note = None;
        view.search.reset("");
        cx.notify();
    });
    let row = div()
        .id("explorer-root-title")
        .flex_none()
        .min_h(k.ladder.header)
        .flex()
        .items_center()
        .gap(space::XXS)
        .px(space::SM)
        .pr(CAPSULE_CLEARANCE)
        .bg(k.theme.background)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .px(space::XS)
                .child(field(view, cx, k)),
        )
        .child(design::icon_button(
            "explorer-search-close",
            design::icon("x", k.ladder.icon),
            "Close search",
            &k.ladder,
            &k.theme,
            close,
        ));
    div()
        .id("explorer-root")
        .size_full()
        .flex()
        .flex_col()
        .child(row)
}

// SDK gap: the header's centre slot that holds the field, the SDK builds
// it later
/// The search field (Expo `Field` with a magnifier lead): a height, a hash,
/// `#3` or a name; no capitals, no corrections, Return searches. Its edge
/// is the field's own hairline, which the host draws 2 wide in the ink
/// while it is focused, as Expo's `Field` does (`Field.tsx:38-39`); it
/// opens focused, and Return ends the edit, as Expo's field blurs on
/// submit: the keyboard goes to the root under it. A search that opened a
/// page took the field away with the root.
fn field(view: &Explorer, cx: Cx, k: &Look) -> impl IntoElement {
    let submit = cx.listener(|view: &mut Explorer, _: &(), window, cx| {
        view.search(cx);
        // SDK gap: the wire has no blur; the keyboard goes to the root's
        // container instead of nowhere, the SDK builds a blur later
        if view.searching {
            window.focus("explorer-root");
        }
    });
    let input = Input::new("explorer-search", &view.search, words::SEARCH)
        .autocorrect(false)
        .autocapitalize(Autocapitalize::None)
        .virtual_keyboard(VirtualKeyboard::Search)
        .input_action(TextInputAction::Search)
        .placeholder(words::SEARCH_HINT)
        .on_submit(submit);
    design::field(input, &k.ladder, &k.theme).lead("search")
}

/// The five pages on a strip that scrolls sideways (Expo `Strip`): one Tab
/// stop, ← → move through the pages, a press opens a page's list.
fn strip(view: &Explorer, cx: Cx, k: &Look) -> Stateful<Div> {
    let shown = view.pages.first().tab();
    let routes = || PAGES.map(|(_, _, route)| route);
    let (moved, pressed) = (routes(), routes());
    let pages: Vec<Stateful<Div>> = PAGES
        .into_iter()
        .map(|(key, label, route)| page_tab(key, label, route, shown, cx, k))
        .collect();
    let bar = design::composite("explorer-tabs", Role::TabList, "Pages")
        .orientation(design::Orientation::Horizontal)
        .wrap()
        .active(shown, PAGES.len())
        .on_move(
            cx.processor(move |view: &mut Explorer, index: usize, _, cx| {
                view.go(moved[index].clone(), cx)
            }),
        )
        .on_press(
            cx.processor(move |view: &mut Explorer, index: usize, _, cx| {
                view.go(pressed[index].clone(), cx)
            }),
        )
        .build();
    design::strip(bar, &k.ladder, &k.theme).children(pages)
}

/// One page on the strip (the Expo kit's `Strip` item, `design::tab`): its
/// press the strip's whole height.
fn page_tab(
    key: &str,
    label: &'static str,
    route: Route,
    shown: usize,
    cx: Cx,
    k: &Look,
) -> Stateful<Div> {
    let chosen = shown == route.tab();
    let go =
        cx.listener(move |view: &mut Explorer, _: &ClickEvent, _, cx| view.go(route.clone(), cx));
    let id = SharedString::from(format!("explorer-tab-{key}"));
    let tab = design::tab(id, label, chosen, &k.ladder, &k.theme, go).flex_none();
    design::item(tab, Role::Tab, chosen)
}
