//! The menu a right press opens at the pointer, as the Menu key, Shift+F10
//! or assistive tech's "show menu" opens it at a focused row: on a
//! folder's or a file's row, Open, Copy link and the row's own writes; on
//! the room a folder's list leaves empty, what is made in the folder. What
//! a menu holds is [`Files::items`], read again as it is drawn; it opens
//! with [`Files::open_menu`], and an item runs with [`Files::run`]. Where
//! the menu's Copy link leads, and how a link opened into Files is read
//! back, is `actions.rs` ([`link`], [`Files::open_link`]).
use view::Point;
use view::methods::ClipboardWrite;
use view::prelude::*;

use crate::actions::link;
use crate::state::{Note, Route, Sheet, name_of, parent_of};
use crate::write::Write;
use crate::{Files, words};

pub(crate) const MENU: &str = "files-menu";

pub(crate) fn entry_key(path: &str) -> String {
    format!("files/entry/{}", name_of(path))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum On {
    Entry(String),
    Folder(String),
    Place { path: String, back: String },
}

pub(crate) struct Menu {
    pub(crate) on: On,
    pub(crate) at: Option<Point<Pixels>>,
    pub(crate) active: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Item {
    Open(Route),
    CopyLink(String),
    Write(Write),
    Place(String),
}

impl Files {
    pub(crate) fn items(&self, on: &On) -> Vec<Item> {
        let path = match on {
            On::Folder(folder) => {
                return self.made_in(folder).into_iter().map(Item::Write).collect();
            }
            On::Place { path, .. } => {
                let mut items = vec![Item::Place(path.clone())];
                items.extend(link(&self.chain, path).map(Item::CopyLink));
                return items;
            }
            On::Entry(path) => path,
        };
        let listed = self.folder_at(parent_of(path));
        let Some(entry) = listed.and_then(|folder| folder.entries.iter().find(|e| e.path == *path))
        else {
            return Vec::new();
        };
        let mut items = vec![Item::Open(Route::of(entry))];
        items.extend(link(&self.chain, path).map(Item::CopyLink));
        items.extend(self.own(path).into_iter().map(Item::Write));
        items
    }

    pub(crate) fn open_menu(
        &mut self,
        on: On,
        at: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.items(&on).is_empty() {
            return;
        }
        if at.is_some() {
            window.focus(MENU);
        }
        self.sheet = Some(Sheet::Menu(Menu { on, at, active: 0 }));
        cx.notify();
    }

    pub(crate) fn place_menu(
        &mut self,
        row: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Sheet::Menu(menu)) = &mut self.sheet else {
            return;
        };
        if menu.at.is_none() {
            menu.at = Some(row.bottom_left());
            window.focus(MENU);
            cx.notify();
        }
    }

    pub(crate) fn move_in_menu(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(Sheet::Menu(menu)) = &mut self.sheet {
            menu.active = index;
            cx.notify();
        }
    }

    pub(crate) fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Sheet::Menu(menu)) = &self.sheet else {
            return;
        };
        let back = match &menu.on {
            On::Entry(path) => Some(entry_key(path)),
            On::Place { back, .. } => Some(back.clone()),
            On::Folder(_) => None,
        };
        if let Some(back) = back {
            window.focus(back);
        }
        self.sheet = None;
        cx.notify();
    }

    pub(crate) fn run(&mut self, item: Item, window: &mut Window, cx: &mut Context<Self>) {
        match item {
            Item::Open(route) => self.open(route, cx),
            Item::Place(path) => self.open_from_places(Route::Folder(path), cx),
            Item::CopyLink(link) => {
                self.close_menu(window, cx);
                self.copy(link, cx);
            }
            Item::Write(write) => {
                self.sheet = None;
                self.begin(write, cx);
            }
        }
    }

    fn copy(&mut self, link: String, cx: &mut Context<Self>) {
        let ask = cx.host().ask::<ClipboardWrite>(link);
        cx.land(ask, |view, copied, cx| {
            view.note = Some(match copied {
                Ok(()) => Note::Copied,
                Err(refusal) => Note::Refused(design::sentence(&refusal)),
            });
            cx.notify();
        })
        .detach();
    }
}

impl Item {
    pub(crate) fn slug(&self) -> &'static str {
        match self {
            Item::Open(_) | Item::Place(_) => "open",
            Item::CopyLink(_) => "copy-link",
            Item::Write(write) => write.slug(),
        }
    }

    pub(crate) fn label(&self) -> &'static str {
        match self {
            Item::Open(_) | Item::Place(_) => words::OPEN,
            Item::CopyLink(_) => words::COPY_LINK,
            Item::Write(write) => words::verb(write),
        }
    }

    pub(crate) fn glyph(&self) -> &'static str {
        match self {
            Item::Open(Route::Folder(_)) | Item::Place(_) => "folder-open",
            Item::Open(_) => "file",
            Item::CopyLink(_) => "link",
            Item::Write(write) => write.glyph(),
        }
    }
}
