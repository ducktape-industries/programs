//! Where the reader goes: a page opened over the one on screen, a page
//! opened straight over the places (a place, a crumb of the path, the
//! History tab, a link opened into Files), and Back, which closes the
//! sheet on screen before it leaves a page. Files' links are here too:
//! the one a row's menu copies ([`link`]) and how one opened is read back
//! ([`Files::open_link`]).
use files::Node;
use view::design::Pages;
use view::prelude::*;

use crate::state::{Note, Route, Sheet, name_of};
use crate::{Files, queries};

// What a markdown file at `path` hands a pressed link to: its folder's.
pub(crate) fn links(path: &str, cx: &mut Context<Files>) -> markdown::OnLink {
    let dir = path[..path.len() - name_of(path).len()].to_owned();
    std::rc::Rc::new(
        cx.listener(move |view, dest: &String, _, cx| view.follow_link(&dir, dest, cx)),
    )
}

pub(crate) fn link(chain: &str, path: &str) -> Option<String> {
    let tail: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    ducklink::mint(chain, files::PROGRAM, &tail)
}

impl Files {
    pub(crate) fn open(&mut self, route: Route, cx: &mut Context<Self>) {
        match self.page() {
            Route::File(_) | Route::Snapshot(_) => self.pages.replace(route),
            Route::Places | Route::Folder(_) | Route::History => self.pages.push(route),
        }
        self.moved(cx);
    }

    pub(crate) fn open_from_places(&mut self, route: Route, cx: &mut Context<Self>) {
        self.pages = Pages::root(Route::Places);
        if route != Route::Places {
            self.pages.push(route);
        }
        self.moved(cx);
    }

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

    // A link pressed in a markdown file in `dir`: the web through the host,
    // a path of the network's files opened as a link into Files is.
    pub(crate) fn follow_link(&mut self, dir: &str, dest: &str, cx: &mut Context<Self>) {
        match markdown::links::target(dir.as_bytes(), dest) {
            Some(markdown::links::Target::Web(url)) => cx.host().open_link(&url),
            Some(markdown::links::Target::Path(path)) => {
                self.open_link(&String::from_utf8_lossy(&path), cx)
            }
            None => {}
        }
    }

    pub(crate) fn back(&mut self, cx: &mut Context<Self>) {
        if self.policy_sheet_open() {
            return self.close_policy_sheet(cx);
        }
        if self.sheet_shown().is_some() {
            self.sheet = None;
            cx.notify();
            return;
        }
        if self.pages.pop().is_some() {
            self.moved(cx);
        }
    }

    pub(crate) fn open_actions(&mut self, cx: &mut Context<Self>) {
        self.sheet = Some(Sheet::Actions);
        cx.notify();
    }

    pub(crate) fn close_sheet(&mut self, cx: &mut Context<Self>) {
        self.sheet = None;
        cx.notify();
    }

    fn moved(&mut self, cx: &mut Context<Self>) {
        self.sheet = None;
        self.note = None;
        self.read_page(cx);
        cx.notify();
    }
}
