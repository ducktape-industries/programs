//! The menu a right press opens on a room in the sidebar, as the Menu key,
//! Shift+F10 or assistive tech's "show menu" opens it on the active room:
//! Open, Copy link and the room's details (`ui/room_menu.rs` draws it).
use view::Point;
use view::prelude::*;

use crate::{Chat, RoomMenu, links, words};

// The menu's element id, which takes the keys as it shows.
pub(crate) const ROOM_MENU: &str = "chat-room-menu";
// The room list, which takes the keys back as the menu closes.
const ROOMS_LIST: &str = "chat-sidebar-rooms-list";

// One thing a room's menu offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RoomItem {
    // the room, opened
    Open,
    // its `duck://` link, onto the clipboard
    CopyLink(String),
    // the room, opened with its details beside it
    Details,
}

impl Chat {
    // What the menu of `room` offers, in order: the room, its link once
    // the session names a chain, its details.
    pub(crate) fn room_items(&self, room: &str) -> Vec<RoomItem> {
        let link = links::channel_link(&self.session.chain_id, room, None);
        let mut items = vec![RoomItem::Open];
        items.extend(link.map(RoomItem::CopyLink));
        items.push(RoomItem::Details);
        items
    }

    // Opens the menu of `room` at `at`; from the keys (`None`), under its
    // row once the row says where it is. Shown at a point, it takes the
    // keys.
    pub(crate) fn open_room_menu(
        &mut self,
        room: String,
        at: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if at.is_some() {
            window.focus(ROOM_MENU);
        }
        self.room_menu = Some(RoomMenu {
            room,
            at,
            active: 0,
        });
        cx.notify();
    }

    // The row the keys opened the menu on is at `row`: the menu opens
    // under it, and takes the keys.
    pub(crate) fn place_room_menu(
        &mut self,
        row: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = &mut self.room_menu else {
            return;
        };
        if menu.at.is_none() {
            menu.at = Some(row.bottom_left());
            window.focus(ROOM_MENU);
            cx.notify();
        }
    }

    // An arrow put the menu on item `index`.
    pub(crate) fn move_in_room_menu(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(menu) = &mut self.room_menu {
            menu.active = index;
            cx.notify();
        }
    }

    // Escape, a press outside, or an item run: the menu closes, and the
    // keys go back to the room list.
    pub(crate) fn close_room_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.room_menu.take().is_some() {
            window.focus(ROOMS_LIST);
            cx.notify();
        }
    }

    // `item` of the menu of `room` pressed: the room opens (its details
    // beside it), or its link goes onto the clipboard.
    pub(crate) fn run_room_item(
        &mut self,
        room: String,
        item: RoomItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_room_menu(window, cx);
        match item {
            RoomItem::Open => self.choose(room, cx),
            RoomItem::CopyLink(link) => self.copy_text(link, words::ROOM_LINK, cx),
            RoomItem::Details => {
                self.choose(room, cx);
                if self.details.is_none() {
                    self.toggle_details();
                }
            }
        }
    }
}

impl RoomItem {
    // What its element is keyed by: `chat-room-menu/<slug>`.
    pub(crate) fn slug(&self) -> &'static str {
        match self {
            RoomItem::Open => "open",
            RoomItem::CopyLink(_) => "copy-link",
            RoomItem::Details => "details",
        }
    }

    // What it is called.
    pub(crate) fn label(&self) -> &'static str {
        match self {
            RoomItem::Open => words::OPEN,
            RoomItem::CopyLink(_) => words::COPY_LINK,
            RoomItem::Details => words::CHANNEL_DETAILS,
        }
    }

    // The glyph (a Lucide name) it leads with.
    pub(crate) fn glyph(&self) -> &'static str {
        match self {
            RoomItem::Open => "message-square",
            RoomItem::CopyLink(_) => "link",
            RoomItem::Details => "info",
        }
    }
}
