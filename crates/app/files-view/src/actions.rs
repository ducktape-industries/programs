//! Where the reader goes: a page opened over the one on screen, a page
//! opened straight over the places (a place, a crumb of the path, the
//! History tab, a link opened into Files), and Back, which closes the
//! sheet on screen before it leaves a page. Files' links are here too:
//! the one a row's menu copies ([`link`]) and how one opened is read back
//! ([`Files::open_link`]).
use files::Node;
use view::design::Pages;
use view::prelude::*;

use crate::state::{Note, Route, Sheet};
use crate::{Files, queries};

/// `duck://<chain>/files/<segment>/…`: the link to what is at `path`, its
/// tail the path's segments (`/home/ada/diary.txt` is
/// `duck://<chain>/files/home/ada/diary.txt`); `None` while the session
/// names no chain. A link names a path, not what is there: opened, it
/// shows what is there then ([`Files::open_link`]).
pub(crate) fn link(chain: &str, path: &str) -> Option<String> {
    let tail: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    ducklink::mint(chain, files::PROGRAM, &tail)
}

impl Files {
    /// Opens `route` over the page on screen. A file's or a snapshot's
    /// page is only ever opened from a list (a wide desktop shows each
    /// beside it), so a route opened from one takes its place: the stack
    /// does not grow with every row looked at, and Back leaves the list.
    pub(crate) fn open(&mut self, route: Route, cx: &mut Context<Self>) {
        match self.page() {
            Route::File(_) | Route::Snapshot(_) => self.pages.replace(route),
            Route::Places | Route::Folder(_) | Route::History => self.pages.push(route),
        }
        self.moved(cx);
    }

    /// Opens `route` with only the places under it.
    pub(crate) fn open_from_places(&mut self, route: Route, cx: &mut Context<Self>) {
        self.pages = Pages::root(Route::Places);
        if route != Route::Places {
            self.pages.push(route);
        }
        self.moved(cx);
    }

    /// A link opened into Files (`host.route`: its tail decoded, joined by
    /// `/`) names a path ([`link`]): what is there opens over the places,
    /// a folder's page or a file's (which says when nothing is there); a
    /// path `files` takes none of says why on the line.
    pub(crate) fn open_link(&mut self, route: &str, cx: &mut Context<Self>) {
        let path = format!("/{route}");
        if let Err(refusal) = files::paths::segments(&path) {
            self.note = Some(Note::Refused(refusal.message));
            cx.notify();
            return;
        }
        let read = queries::entry(cx.host(), path.clone());
        cx.land(read, move |view, entry, cx| match entry {
            Ok(entry) => {
                let route = match entry.map(|entry| entry.node) {
                    Some(Node::Dir { .. }) => Route::Folder(path),
                    Some(Node::File { .. } | Node::Symlink { .. }) | None => Route::File(path),
                };
                view.open_from_places(route, cx);
            }
            Err(refusal) => {
                view.note = Some(Note::Refused(design::sentence(&refusal)));
                cx.notify();
            }
        })
        .detach();
    }

    /// Back: the sheet on screen, else the page under this one; at the
    /// places with nothing open, nothing.
    pub(crate) fn back(&mut self, cx: &mut Context<Self>) {
        if self.sheet_shown().is_some() {
            self.sheet = None;
            cx.notify();
            return;
        }
        if self.pages.pop().is_some() {
            self.moved(cx);
        }
    }

    /// The page's writes, as a list over it.
    pub(crate) fn open_actions(&mut self, cx: &mut Context<Self>) {
        self.sheet = Some(Sheet::Actions);
        cx.notify();
    }

    pub(crate) fn close_sheet(&mut self, cx: &mut Context<Self>) {
        self.sheet = None;
        cx.notify();
    }

    /// A new page is on top: what the last one said and had open goes,
    /// and what this one shows is read.
    fn moved(&mut self, cx: &mut Context<Self>) {
        self.sheet = None;
        self.note = None;
        self.read_page(cx);
        cx.notify();
    }
}
