//! A folder's screen: its name over its path, its writes behind the
//! title row's control, then its entries, each opening its folder or file.
//! New design in the kit's look; nearest Expo screen
//! `mobile/app/(tabs)/forge/[repo]/code/[...path].tsx` (a folder's rows).
//! Reads the folder; writes the route and opens the page's writes.
use files::EntryInfo;
use view::{Div, Stateful};

use super::*;
use crate::state::{entry_glyph, name_of};

pub(super) fn folder(view: &Files, path: &str, cx: Cx, k: &Kit) -> AnyElement {
    let page = page("files-folder", name_of(path).to_owned(), view, cx, k).caption(path.to_owned());
    let body = match view.folder_at(path) {
        Some(folder) => {
            let writable = view.reader.writes(&folder.children());
            entries(&folder.entries, writable, cx, k)
        }
        None => match view.folder.failed() {
            Some(refusal) => refused("files-folder", refusal, cx, k),
            None => quiet("files-folder-reading", words::READING_FOLDER, k),
        },
    };
    let policy = crate::policies::row(view, cx, &k.ladder, &k.theme);
    page.child(scroller("files-page").children(policy).child(body))
        .into_any_element()
}

fn entries(entries: &[EntryInfo], writable: bool, cx: Cx, k: &Kit) -> AnyElement {
    if entries.is_empty() {
        let detail = words::empty_folder(writable);
        return design::empty_state(
            "files-empty",
            "folder-open",
            words::EMPTY_FOLDER,
            detail,
            &k.ladder,
            &k.theme,
        )
        .into_any_element();
    }
    div()
        .id("files-entries")
        .flex()
        .flex_col()
        .children(entries.iter().map(|entry| entry_row(entry, cx, k)))
        .into_any_element()
}

fn entry_row(entry: &EntryInfo, cx: Cx, k: &Kit) -> Stateful<Div> {
    let route = Route::of(entry);
    let open =
        cx.listener(move |view: &mut Files, _: &ClickEvent, _, cx| view.open(route.clone(), cx));
    let name = name_of(&entry.path).to_owned();
    opening_row(
        ElementId::Name(format!("files/entry/{name}").into()),
        entry_glyph(entry),
        name,
        words::entry_caption(&entry.node),
        k,
        open,
    )
}
