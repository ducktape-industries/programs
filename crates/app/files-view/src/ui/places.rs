//! Places: where to start. The reader's home (or why there is none),
//! `/shared`, the whole tree, then the newest snapshots. Reads the reader
//! and the history; writes the route.
use view::{Div, Stateful};

use super::*;
use crate::menu::On;
use crate::state::{HOME_GLYPH, Place};

const LATEST: usize = 5;

pub(super) fn places(view: &Files, cx: Cx, k: &Kit) -> AnyElement {
    let places = Place::ALL.map(|place| {
        let (title, caption) = words::place(place);
        let row = place_row(
            place.key(),
            place.glyph(),
            title,
            place.path(),
            caption,
            cx,
            k,
        );
        place_menu(view, row, place.key(), place.path(), cx)
    });
    div()
        .id("files-places")
        .flex()
        .flex_col()
        .child(heading("files/places-head", words::PLACES, None, k))
        .child(home(view, cx, k))
        .children(places)
        .child(heading("files/latest-head", words::LATEST, None, k))
        .child(latest(view, cx, k))
        .into_any_element()
}

fn home(view: &Files, cx: Cx, k: &Kit) -> Stateful<Div> {
    match view.reader.home() {
        Ok(path) => {
            let row = place_row("home", HOME_GLYPH, words::HOME, &path, &path, cx, k);
            place_menu(view, row, "home", &path, cx)
        }
        Err(why) => row_look(
            "files/place/home".into(),
            HOME_GLYPH,
            words::HOME.into(),
            words::no_home(&why),
            k,
        ),
    }
}

fn place_row(
    key: &str,
    glyph: &'static str,
    title: &str,
    path: &str,
    caption: &str,
    cx: Cx,
    k: &Kit,
) -> Stateful<Div> {
    let route = Route::Folder(path.to_owned());
    let open =
        cx.listener(move |view: &mut Files, _: &ClickEvent, _, cx| view.open(route.clone(), cx));
    row(
        format!("files/place/{key}"),
        glyph,
        title.to_owned(),
        caption.to_owned(),
        k,
        open,
    )
}

fn place_menu(view: &Files, row: Stateful<Div>, key: &str, path: &str, cx: Cx) -> Stateful<Div> {
    let on = On::Place {
        path: path.to_owned(),
        back: format!("files/place/{key}"),
    };
    menu::opens(row, on, view, cx)
}

fn latest(view: &Files, cx: Cx, k: &Kit) -> AnyElement {
    let history = match &view.history {
        Loadable::Ready(history) | Loadable::Reloading(history, _) => history,
        Loadable::Failed(refusal) => return refused("files-history", refusal, cx, k),
        Loadable::Idle | Loadable::Loading(_) => {
            return reading("files-latest-reading", words::READING_HISTORY, k);
        }
    };
    if history.snapshots.is_empty() {
        return design::empty_state(
            "files-no-snapshots",
            "history",
            words::NO_SNAPSHOTS,
            words::NO_SNAPSHOTS_DETAIL,
            &k.ladder,
            &k.theme,
        )
        .into_any_element();
    }
    let rows = history
        .snapshots
        .iter()
        .take(LATEST)
        .map(|snapshot| history::snapshot_row(view, snapshot, cx, k));
    div().flex().flex_col().children(rows).into_any_element()
}
