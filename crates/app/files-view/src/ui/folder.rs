//! A folder: the path as crumbs, the writes the reader may make here, then
//! its entries, each opening its folder or file; with room, in columns (a
//! name, what it holds, how big). A right press on a row opens its menu,
//! on the room the rows leave the folder's (`menu.rs`). Split, the folder
//! a file is in is listed beside the file, the file's row marked. Reads
//! the folder; writes the route and opens a write's form or a menu.
use files::EntryInfo;
use view::{Div, Stateful};

use super::*;
use crate::menu::{On, entry_key};
use crate::state::{Folder, entry_glyph, name_of, parent_of};

/// A column's width in a wide list: what an entry holds, how big it is.
const KIND_W: Pixels = px(96.);
const SIZE_W: Pixels = px(96.);
/// The columns' names' height, over the rows.
const HEADS_H: Pixels = px(32.);

/// How a folder is listed.
struct Listing<'a> {
    /// the path the crumbs run to
    crumbs: &'a str,
    /// the row of the file open beside the list
    marked: Option<&'a str>,
    /// whether the page's writes show over the entries
    writes: bool,
    /// whether the entries run in columns
    columns: bool,
}

/// A folder's page.
pub(super) fn folder(view: &Files, path: &str, columns: bool, cx: Cx, k: &Kit) -> AnyElement {
    let listing = Listing {
        crumbs: path,
        marked: None,
        writes: true,
        columns,
    };
    listed(view, path, listing, cx, k)
}

/// The folder the file at `path` is in, listed beside it: the file's
/// crumbs, its row marked. Its writes are the file page's, beside.
pub(super) fn beside(view: &Files, path: &str, cx: Cx, k: &Kit) -> AnyElement {
    let listing = Listing {
        crumbs: path,
        marked: Some(path),
        writes: false,
        columns: true,
    };
    listed(view, parent_of(path), listing, cx, k)
}

fn listed(view: &Files, path: &str, listing: Listing, cx: Cx, k: &Kit) -> AnyElement {
    // as tall as the pane at least: the entries' empty room reaches its foot
    let page = div()
        .id("files-folder")
        .min_h_full()
        .flex()
        .flex_col()
        .child(crumbs(listing.crumbs, cx, k));
    if let Some(folder) = view.folder_at(path) {
        let writes = listing.writes.then(|| writes(view, cx, k)).flatten();
        return page
            .children(writes)
            .child(entries(view, folder, &listing, cx, k))
            .into_any_element();
    }
    let waiting = match view.folder.failed() {
        Some(refusal) => refused("files-folder", refusal, cx, k),
        None => reading("files-folder-reading", words::READING_FOLDER, k),
    };
    page.child(waiting).into_any_element()
}

/// The folder's entries, down to the pane's foot: a right press on the
/// room under them opens the folder's menu, what is made in it.
fn entries(view: &Files, folder: &Folder, listing: &Listing, cx: Cx, k: &Kit) -> Stateful<Div> {
    let list = div()
        .id("files-entries")
        .flex_1()
        .flex()
        .flex_col()
        .role(Role::Group)
        .aria_label(words::ENTRIES);
    let list = menu::pointed(list, On::Folder(folder.path.clone()), cx);
    if folder.entries.is_empty() {
        let detail = words::empty_folder(view.reader.writes(&folder.children()));
        return list.child(design::empty_state(
            "files-empty",
            "folder-open",
            words::EMPTY_FOLDER,
            detail,
            &k.ladder,
            &k.theme,
        ));
    }
    // ponytail: every entry is drawn; a uniform_list once folders hold
    // more than a screenful of hundreds
    let rows = folder.entries.iter().map(|entry| {
        let marked = listing.marked == Some(entry.path.as_str());
        entry_row(view, entry, listing.columns, marked, cx, k)
    });
    list.mt_2()
        .border_t_1()
        .border_color(k.theme.border)
        .children(listing.columns.then(|| heads(k)))
        .children(rows)
}

/// The columns' names, over the rows.
fn heads(k: &Kit) -> Div {
    div()
        .h(HEADS_H)
        .flex()
        .items_center()
        .gap_3()
        .px_5()
        .border_b_1()
        .border_color(k.theme.border)
        .text_size(design::text::CAPTION)
        .text_color(k.theme.muted)
        // the glyph's room
        .child(div().flex_none().w(k.ladder.icon_row))
        .child(div().flex_1().child(words::NAME))
        .child(cells(words::KIND, words::SIZE.to_owned()))
}

/// What an entry holds and how big it is, each in its column.
fn cells(kind: &'static str, size: String) -> Div {
    div()
        .flex()
        .gap_3()
        .child(div().w(KIND_W).truncate().child(kind))
        .child(div().w(SIZE_W).truncate().child(size))
}

fn entry_row(
    view: &Files,
    entry: &EntryInfo,
    columns: bool,
    marked: bool,
    cx: Cx,
    k: &Kit,
) -> Stateful<Div> {
    let route = Route::of(entry);
    let open =
        cx.listener(move |view: &mut Files, _: &ClickEvent, _, cx| view.open(route.clone(), cx));
    let name = name_of(&entry.path).to_owned();
    let aside = match columns {
        true => cells(words::entry_kind(entry), words::entry_size(&entry.node)).into_any_element(),
        false => words::entry_caption(&entry.node).into_any_element(),
    };
    let row = row(
        entry_key(&entry.path),
        entry_glyph(entry),
        name,
        aside,
        k,
        open,
    );
    mark(menu::row(row, &entry.path, view, cx), marked, k)
}
