//! The side of a wide pane, in place of the tabs, in the sidebar's look
//! chat's has: the places (the reader's home, `/shared`, the whole tree),
//! then the history and every pin; the one the page on screen is under
//! picked. One Tab stop: ↑ ↓ open the next. Reads the reader, the route
//! and the pins; writes the route.
use files::PinRow;
use view::{Div, Stateful};

use super::*;
use crate::menu::On;
use crate::state::{HISTORY_GLYPH, HOME_GLYPH, PIN_GLYPH, Place, SHARED_PATH};

#[derive(Clone)]
enum Item<'a> {
    Home(String),
    Place(Place),
    History,
    Pin(&'a PinRow),
}

impl Item<'_> {
    fn route(&self) -> Route {
        match self {
            Item::Home(path) => Route::Folder(path.clone()),
            Item::Place(place) => Route::Folder(place.path().to_owned()),
            Item::History => Route::History,
            Item::Pin(pin) => Route::Snapshot(pin.snapshot),
        }
    }

    fn key(&self) -> String {
        match self {
            Item::Home(_) => "home".into(),
            Item::Place(place) => place.key().into(),
            Item::History => "history".into(),
            Item::Pin(pin) => format!("pin/{}", pin.name),
        }
    }

    fn on(&self) -> Option<On> {
        let path = match self {
            Item::Home(path) => path.clone(),
            Item::Place(place) => place.path().to_owned(),
            Item::History | Item::Pin(_) => return None,
        };
        let back = "files-side-list".to_owned();
        Some(On::Place { path, back })
    }

    fn glyph(&self) -> &'static str {
        match self {
            Item::Home(_) => HOME_GLYPH,
            Item::Place(place) => place.glyph(),
            Item::History => HISTORY_GLYPH,
            Item::Pin(_) => PIN_GLYPH,
        }
    }

    fn label(&self) -> String {
        match self {
            Item::Home(_) => words::HOME.into(),
            Item::Place(place) => words::place(*place).0.into(),
            Item::History => words::HISTORY.into(),
            Item::Pin(pin) => pin.name.clone(),
        }
    }
}

fn items(view: &Files) -> Vec<Item<'_>> {
    let home = view.reader.home().ok().map(Item::Home);
    let places = Place::ALL.map(Item::Place);
    let pins = view
        .history
        .ready()
        .into_iter()
        .flat_map(|history| &history.pins)
        .map(Item::Pin);
    home.into_iter()
        .chain(places)
        .chain([Item::History])
        .chain(pins)
        .collect()
}

fn here(page: &Route, items: &[Item]) -> Option<usize> {
    let under =
        |path: &str, folder: &str| path == folder || path.starts_with(&format!("{folder}/"));
    let opens = |item: &Item| match (page, item) {
        (Route::Folder(path) | Route::File(path), Item::Home(home)) => under(path, home),
        (Route::Folder(path) | Route::File(path), Item::Place(Place::Shared)) => {
            under(path, SHARED_PATH)
        }
        (Route::Folder(_) | Route::File(_), Item::Place(Place::Everything)) => true,
        (Route::History, Item::History) => true,
        (Route::Snapshot(id), Item::Pin(pin)) => pin.snapshot == *id,
        _ => false,
    };
    let history = |item: &Item| matches!(item, Item::History);
    items.iter().position(opens).or_else(|| match page {
        Route::Snapshot(_) => items.iter().position(history),
        _ => None,
    })
}

pub(super) fn side(view: &Files, cx: Cx, k: &Kit) -> Stateful<Div> {
    let items = items(view);
    let here = here(view.page(), &items);
    let routes: Vec<Route> = items.iter().map(Item::route).collect();
    let count = routes.len();
    let open = move |view: &mut Files, index: usize, _: &mut Window, cx: &mut Context<Files>| {
        view.open_from_places(routes[index].clone(), cx)
    };
    let moved = open.clone();
    let history_at = items
        .iter()
        .position(|item| matches!(item, Item::History))
        .unwrap_or(count);
    let mut rows = items.iter().enumerate().map(|(index, item)| {
        row(
            view,
            item,
            here == Some(index),
            index == here.unwrap_or(0),
            cx,
            k,
        )
    });
    let places =
        group("files-side-places", words::PLACES, k).children(rows.by_ref().take(history_at));
    let history = group("files-side-history", words::HISTORY, k)
        .mt_3()
        .children(rows);
    let asked = items.get(here.unwrap_or(0)).and_then(Item::on);
    let keys = cx.listener(move |view: &mut Files, event: &KeyDownEvent, window, cx| {
        if let Some(on) = asked
            .clone()
            .filter(|_| design::asks_for_menu(&event.keystroke))
        {
            view.open_menu(on, None, window, cx)
        }
    });
    let list = design::composite("files-side-list", Role::ListBox, words::SIDE)
        .wrap()
        .active(here.unwrap_or(0), count)
        .on_move(cx.processor(moved))
        .on_press(cx.processor(open))
        .build()
        .on_key_down(keys)
        .flex()
        .flex_col()
        .child(places)
        .child(history);
    div()
        .id("files-side")
        .flex_none()
        .w(px(SIDE_W))
        .h_full()
        .overflow_y_scroll()
        .p_2()
        .bg(k.theme.sidebar)
        .text_color(k.theme.sidebar_foreground)
        .child(list)
}

fn group(id: &'static str, label: &'static str, k: &Kit) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap_1()
        .role(Role::Group)
        .aria_label(label)
        .child(
            div()
                .px_2()
                .py_1()
                .text_size(design::text::CAPTION)
                .text_color(k.theme.sidebar_muted)
                .child(label),
        )
}

fn row(view: &Files, item: &Item, picked: bool, active: bool, cx: Cx, k: &Kit) -> Stateful<Div> {
    let route = item.route();
    let open = cx.listener(move |view: &mut Files, _: &ClickEvent, _, cx| {
        view.open_from_places(route.clone(), cx)
    });
    let theme = k.theme;
    let row = div()
        .id(ElementId::Name(format!("files/side/{}", item.key()).into()))
        .min_h(design::size::CONTROL)
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .cursor_pointer()
        .bg(match picked {
            true => theme.sidebar_raised,
            false => theme.sidebar,
        })
        .hover(move |style| style.bg(theme.sidebar_raised))
        .aria_selected(picked)
        .on_click(open)
        .child(design::icon(item.glyph(), k.ladder.icon_row).text_color(theme.sidebar_muted))
        .child(div().flex_1().min_w_0().truncate().child(item.label()));
    let row = match item.on() {
        Some(on) => menu::opens(row, on, view, cx),
        None => row,
    };
    design::item(row, Role::ListBoxOption, active)
}
