//! The root screen, "Files": the places (the reader's home, or why there
//! is none; `/shared`; the whole tree) and the history. New design in the
//! kit's look; nearest Expo screen
//! `mobile/app/(tabs)/forge/[repo]/code/[...path].tsx`. Reads the reader
//! and the history; writes the route.
use view::{Div, Stateful};

use super::*;
use crate::state::{HISTORY_GLYPH, HOME_GLYPH, Place};

pub(super) fn places(view: &Files, cx: Cx, k: &Kit) -> AnyElement {
    let history = history_caption(view);
    let places = Place::ALL.map(|place| {
        let (title, caption) = words::place(place);
        place_row(
            place.key(),
            place.glyph(),
            title,
            place.path(),
            caption,
            cx,
            k,
        )
    });
    page("files-places", words::TITLE, view, cx, k)
        .child(
            scroller("files-page")
                .child(design::section_head(
                    "files/places-head",
                    words::PLACES,
                    &k.ladder,
                    &k.theme,
                ))
                .child(home(view, cx, k))
                .children(places)
                .child(design::section_head(
                    "files/history-head",
                    words::HISTORY,
                    &k.ladder,
                    &k.theme,
                ))
                .child(opening_row(
                    "files/place/history",
                    HISTORY_GLYPH,
                    words::HISTORY,
                    history,
                    k,
                    cx.listener(|view: &mut Files, _: &ClickEvent, _, cx| {
                        view.open(Route::History, cx)
                    }),
                )),
        )
        .into_any_element()
}

fn home(view: &Files, cx: Cx, k: &Kit) -> Stateful<Div> {
    match view.reader.home() {
        Ok(path) => place_row("home", HOME_GLYPH, words::HOME, &path, &path, cx, k),
        Err(why) => still_row(
            "files/place/home",
            HOME_GLYPH,
            words::HOME,
            words::no_home(&why),
            k,
        ),
    }
}

fn place_row(
    key: &str,
    glyph: &'static str,
    title: &'static str,
    path: &str,
    caption: &str,
    cx: Cx,
    k: &Kit,
) -> Stateful<Div> {
    let route = Route::Folder(path.to_owned());
    let open =
        cx.listener(move |view: &mut Files, _: &ClickEvent, _, cx| view.open(route.clone(), cx));
    opening_row(
        ElementId::Name(format!("files/place/{key}").into()),
        glyph,
        title,
        caption.to_owned(),
        k,
        open,
    )
}

fn history_caption(view: &Files) -> String {
    match &view.history {
        Loadable::Ready(history) | Loadable::Reloading(history, _) => {
            match history.snapshots.first() {
                Some(newest) => words::message_or_none(&newest.message).to_owned(),
                None => words::NO_SNAPSHOTS.to_owned(),
            }
        }
        Loadable::Failed(refusal) => design::sentence(refusal),
        Loadable::Idle | Loadable::Loading(_) => words::READING_HISTORY.to_owned(),
    }
}
