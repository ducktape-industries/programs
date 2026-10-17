//! The menu a right press opens at the pointer, as the Menu key, Shift+F10
//! or assistive tech's "show menu" opens it at a focused row: on a
//! folder's or a file's row, Open, Copy link and the row's own writes; on
//! the room a folder's list leaves empty, what is made in the folder. What
//! a menu holds is [`Files::items`], read again as it is drawn; it opens
//! with [`Files::open_menu`], and an item runs with [`Files::run`]. Where
//! the menu's Copy link leads, and how a link opened into Files is read
//! back, is `actions.rs` ([`link`], [`Files::open_link`]).
use view::methods::ClipboardWrite;
use view::prelude::*;
use view::{Keystroke, Point};

use crate::actions::link;
use crate::state::{Note, Route, Sheet, name_of, parent_of};
use crate::write::Write;
use crate::{Files, words};

/// The menu's element id, which takes the keys as it shows.
pub(crate) const MENU: &str = "files-menu";

/// The element id of the row of the entry at `path`, which the keys go
/// back to as its menu closes.
pub(crate) fn entry_key(path: &str) -> String {
    format!("files/entry/{}", name_of(path))
}

/// What a menu is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum On {
    /// an entry of the folder on screen, by its path
    Entry(String),
    /// the room the folder's list leaves empty, by the folder's path
    Folder(String),
}

/// A menu open over the page.
pub(crate) struct Menu {
    pub(crate) on: On,
    /// where it opens, in the window's pixels: the press; from the keys,
    /// under its row, once the row says where it is (`None` until then)
    pub(crate) at: Option<Point<Pixels>>,
    /// the item the arrows are on
    pub(crate) active: usize,
}

/// One thing a menu offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Item {
    /// the entry's page
    Open(Route),
    /// the entry's `duck://` link, onto the clipboard
    CopyLink(String),
    Write(Write),
}

impl Files {
    /// What `on` offers, in order: an entry's page, its link (once the
    /// session names a chain), then its own writes; the empty room of a
    /// folder, what is made in it. An entry the folder on screen no longer
    /// lists offers nothing.
    pub(crate) fn items(&self, on: &On) -> Vec<Item> {
        let path = match on {
            On::Folder(folder) => {
                return self.made_in(folder).into_iter().map(Item::Write).collect();
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

    /// Opens the menu of `on` at `at`; from the keys (`None`), under its
    /// row once the row says where it is ([`Files::place_menu`]). A menu
    /// that would offer nothing does not open. Shown at a point, it takes
    /// the keys.
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

    /// The row the keys opened the menu on is at `row`: the menu opens
    /// under it, and takes the keys.
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

    /// An arrow put the menu on item `index`.
    pub(crate) fn move_in_menu(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(Sheet::Menu(menu)) = &mut self.sheet {
            menu.active = index;
            cx.notify();
        }
    }

    /// Escape, a press outside, or Copy link: the menu closes, and the keys
    /// go back to the row it was on.
    pub(crate) fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Sheet::Menu(menu)) = &self.sheet else {
            return;
        };
        if let On::Entry(path) = &menu.on {
            window.focus(entry_key(path));
        }
        self.sheet = None;
        cx.notify();
    }

    /// `item` pressed: the entry's page opens, its link goes onto the
    /// clipboard, or the write's form opens (an upload's picker first) in
    /// the menu's place.
    pub(crate) fn run(&mut self, item: Item, window: &mut Window, cx: &mut Context<Self>) {
        match item {
            Item::Open(route) => self.open(route, cx),
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

    /// `link` onto the clipboard; the line says it is there, or why not.
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

/// Whether `keystroke` asks for the focused row's menu: the Menu key, or
/// Shift+F10.
pub(crate) fn asks_for_menu(keystroke: &Keystroke) -> bool {
    let held = keystroke.modifiers;
    match keystroke.key.as_str() {
        "menu" => !held.modified(),
        "f10" => held.shift && !(held.control || held.alt || held.platform || held.function),
        _ => false,
    }
}

impl Item {
    /// What its element is keyed by: `files/menu/<slug>`.
    pub(crate) fn slug(&self) -> &'static str {
        match self {
            Item::Open(_) => "open",
            Item::CopyLink(_) => "copy-link",
            Item::Write(write) => write.slug(),
        }
    }

    /// What it is called.
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Item::Open(_) => words::OPEN,
            Item::CopyLink(_) => words::COPY_LINK,
            Item::Write(write) => words::verb(write),
        }
    }

    /// The glyph (a Lucide name) it leads with.
    pub(crate) fn glyph(&self) -> &'static str {
        match self {
            Item::Open(Route::Folder(_)) => "folder-open",
            Item::Open(_) => "file",
            Item::CopyLink(_) => "link",
            Item::Write(write) => write.glyph(),
        }
    }
}
