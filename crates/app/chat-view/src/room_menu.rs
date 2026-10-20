//! The menu a right press opens on a room in the sidebar, as the Menu key,
//! Shift+F10 or assistive tech's "show menu" opens it on the active room:
//! Open, Copy link and the room's details (`ui/room_menu.rs` draws it).
use view::Point;
use view::prelude::*;

use crate::{Chat, RoomMenu, links, words};

pub(crate) const ROOM_MENU: &str = "chat-room-menu";
const ROOMS_LIST: &str = "chat-sidebar-rooms-list";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RoomItem {
    Open,
    CopyLink(String),
    Details,
}

impl Chat {
    pub(crate) fn room_items(&self, room: &str) -> Vec<RoomItem> {
        let link = links::channel_link(&self.session.chain_id, room, None);
        let mut items = vec![RoomItem::Open];
        items.extend(link.map(RoomItem::CopyLink));
        items.push(RoomItem::Details);
        items
    }

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

    pub(crate) fn move_in_room_menu(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(menu) = &mut self.room_menu {
            menu.active = index;
            cx.notify();
        }
    }

    pub(crate) fn close_room_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.room_menu.take().is_some() {
            window.focus(ROOMS_LIST);
            cx.notify();
        }
    }

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
                    self.toggle_details(cx);
                }
            }
        }
    }
}

impl RoomItem {
    pub(crate) fn slug(&self) -> &'static str {
        match self {
            RoomItem::Open => "open",
            RoomItem::CopyLink(_) => "copy-link",
            RoomItem::Details => "details",
        }
    }

    pub(crate) fn label(&self) -> &'static str {
        match self {
            RoomItem::Open => words::OPEN,
            RoomItem::CopyLink(_) => words::COPY_LINK,
            RoomItem::Details => words::CHANNEL_DETAILS,
        }
    }

    pub(crate) fn glyph(&self) -> &'static str {
        match self {
            RoomItem::Open => "message-square",
            RoomItem::CopyLink(_) => "link",
            RoomItem::Details => "info",
        }
    }
}
