//! State stored by the root view and its panes.
use chat::{ChannelInfo, MemberRow, MsgRow, Principal};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use view::prelude::*;

use crate::composer::{Draft, Place};
use crate::message::ChatMessage;
use chat::view::Names;
use view::design::Pages;

// Each account's picture by its card's blob (identity's), and the pictures
// read so far: a face asks `of` for the one it draws.
#[derive(Clone, Default)]
pub(crate) struct Faces {
    pub(crate) avatars: Rc<BTreeMap<u64, abi::BlobId>>,
    pub(crate) pictures: Option<Entity<view::pictures::Pictures>>,
}

impl Faces {
    // Account `account`'s picture, once read; its first ask reads it.
    pub(crate) fn of(
        &self,
        account: u64,
        cx: &mut impl view::AppContext,
    ) -> Option<std::sync::Arc<view::Image>> {
        let blob = self.avatars.get(&account)?;
        let pictures = self.pictures.as_ref()?;
        pictures.update(cx, |pictures, cx| pictures.get(blob, cx))
    }

    // What a kept row's face reads: the account's blob, and whether its
    // picture is read yet; a row whose deps hold it draws again when either
    // moves.
    pub(crate) fn dep(
        &self,
        account: u64,
        cx: &mut impl view::AppContext,
    ) -> (Option<abi::BlobId>, bool) {
        let blob = self.avatars.get(&account).cloned();
        (blob, self.of(account, cx).is_some())
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct Chat {
    pub(crate) session: Session,
    // whether the host has handed a session yet: until it has, the panes draw their
    // placeholders, never "Not connected"
    #[serde(skip)]
    pub(crate) session_heard: bool,
    #[serde(skip)]
    pub(crate) names: Loadable<Names>,
    /// whom each agent of the roster answers besides its manager, by
    /// number, as identity keeps it
    #[serde(skip)]
    pub(crate) ownerships: Loadable<BTreeMap<u64, identity::Ownership>>,
    // each account's picture, read again on identity's heads
    #[serde(skip)]
    pub(crate) faces: Faces,
    pub(crate) channels: Loadable<Vec<ChannelInfo>>,
    /// what the reader chose for each room's notices, read while the
    /// details are open; a room not named is at its default
    #[serde(skip)]
    pub(crate) choices: Loadable<BTreeMap<String, crate::notifier::Choice>>,
    #[serde(skip)]
    pub(crate) managed_choices: Loadable<Vec<(u64, crate::notifier::Choice)>>,
    pub(crate) room: Option<Room>,
    pub(crate) drafts: BTreeMap<String, Draft>,
    /// the banner over the room: the last refusal, until the reader moves on
    pub(crate) notice: String,
    pub(crate) search: Search,
    pub(crate) create: Option<ChannelCreate>,
    pub(crate) dm_pick: Option<DmPick>,
    pub(crate) details: Option<Details>,
    pub(crate) layout: Layout,
    pub(crate) reads: Reads,
    pub(crate) menu: Option<Menu>,
    // the reaction whose choosers `Mode::Reactors` names
    #[serde(default)]
    pub(crate) reactors_of: String,
    /// The reaction picker's search and tab while it is open.
    #[serde(skip)]
    pub(crate) picker: Picker,
    /// The reader's reactions, newest first: the picker's frequent row.
    #[serde(default)]
    pub(crate) recent_emoji: Vec<String>,
    // the rooms the reader opened on this device (`kept.rs`) and the ones
    // they listen to: the phone lists both as theirs
    #[serde(skip)]
    pub(crate) opened: BTreeSet<String>,
    #[serde(skip)]
    pub(crate) opened_kept: Option<String>,
    #[serde(skip)]
    pub(crate) listening: BTreeSet<String>,
    // asked this session: both lists and `#` read them
    #[serde(skip)]
    pub(crate) listening_asked: bool,
    // the rooms the reader wrote in, and the order they put their rooms and folders in: chat's,
    // asked with `listening`
    #[serde(skip)]
    pub(crate) posted: BTreeSet<String>,
    #[serde(skip)]
    pub(crate) order: Vec<chat::Entry>,
    // the sidebar's channel rows where layout last put them, and the row held down to drag
    #[serde(skip)]
    pub(crate) drawn_rows: crate::room_list::Drawn,
    #[serde(skip)]
    pub(crate) drag: Option<crate::room_list::Drag>,
    // the block height each room's newest message last moved at, as this
    // session heard it: what `#` offers first
    #[serde(skip)]
    pub(crate) active: BTreeMap<String, u64>,
    // the room list's folders the reader closed, by path; kept on the device
    #[serde(default)]
    pub(crate) closed_folders: BTreeSet<String>,
    // the channels the reader is not in, listed under the room list's last row
    #[serde(skip)]
    pub(crate) others_open: bool,
    #[serde(skip)]
    pub(crate) more_open: BTreeSet<String>,
    /// Where the pointer is among the message rows: the row that carries
    /// the action strip, beside a chosen one. One slot whatever the pointer
    /// entered took last, so a strip that vanished under the pointer (its
    /// row deleted or scrolled away) leaves nothing once it moves on.
    #[serde(skip)]
    pub(crate) hovered: Option<Hovered>,
    /// the reader's day the chain's newest head falls on (days since the
    /// epoch, in the reader's zone): a phone's separator says "Today"
    #[serde(skip)]
    pub(crate) today: Option<u64>,
    /// The room the arrows are on in the sidebar's list; the open room
    /// until they move.
    #[serde(skip)]
    pub(crate) rooms_cursor: Option<crate::room_list::Spot>,
    #[serde(skip)]
    pub(crate) dm_heads: BTreeMap<String, DmHead>,
    /// The message the arrows are on in the room, and in the thread.
    #[serde(skip)]
    pub(crate) timeline_cursor: Cursor,
    #[serde(skip)]
    pub(crate) thread_cursor: Cursor,
    /// The item the arrows are on in the open message menu.
    #[serde(skip)]
    pub(crate) menu_cursor: usize,
    // the open message menu came from a right press: it hangs from the
    // press, not under the strip's ⋯
    #[serde(skip)]
    pub(crate) menu_at_pointer: bool,
    // the menu of a room in the sidebar, while one is open
    #[serde(skip)]
    pub(crate) room_menu: Option<RoomMenu>,
    #[serde(skip)]
    pub(crate) notices_menu: Option<NoticesMenu>,
    /// The line over the room saying a copy landed: not a refusal, so not
    /// the `notice` banner.
    #[serde(skip)]
    pub(crate) confirmation: String,
    pub(crate) copy: Option<CopyRange>,
    #[serde(skip)]
    pub(crate) timeline_drawn: RefCell<Drawn>,
    #[serde(skip)]
    pub(crate) thread_drawn: RefCell<Drawn>,
    /// the window's title last sent
    #[serde(skip)]
    pub(crate) title: Option<String>,
    /// the room last named to the host as the one shown (`host.showing`)
    #[serde(skip)]
    pub(crate) showing: Option<String>,
    /// the channel list read again while the one on screen stays
    #[serde(skip)]
    pub(crate) rereading_channels: Option<view::Task<()>>,
    #[serde(skip)]
    pub(crate) catch_ups_asked: u64,
    // what chat blocks moved of the channel list since that read (`notifier::Moved`)
    #[serde(skip)]
    pub(crate) moved_channels: crate::notifier::Moved,
    // each draft file's put, by its draft and id: dropped, it stops
    #[serde(skip)]
    pub(crate) uploads: BTreeMap<(String, u64), view::Task<()>>,
    // what each file a message carries is, once read, by path and snapshot
    #[serde(skip)]
    pub(crate) shown_files: BTreeMap<(String, [u8; 32]), crate::attach::Shown>,
    #[serde(skip)]
    pub(crate) unread_files:
        BTreeMap<crate::attach::FileKey, (chat::message::Attachment, crate::attach::Unread)>,
    #[serde(skip)]
    pub(crate) files_moved: Option<view::Task<()>>,
    #[serde(skip)]
    pub(crate) files_seen: Option<files::BlobId>,
    #[serde(skip)]
    pub(crate) files_diffing: bool,
    #[serde(skip)]
    pub(crate) files_diff_again: bool,
    // the picture a click opened whole over the room
    #[serde(skip)]
    pub(crate) enlarged: Option<chat::message::Attachment>,
    // the rooms opened this session and left, by id
    #[serde(skip)]
    pub(crate) visited: BTreeMap<String, Visited>,
    // the widgets the open room names, what they show and what they read
    #[serde(skip)]
    pub(crate) widgets: crate::widget::Widgets,
    // who can edit the open room, and the message whose menu is open (`policies.rs`)
    #[serde(skip)]
    pub(crate) room_policy: policy_view::Editing,
    #[serde(skip)]
    pub(crate) message_policy: policy_view::Editing,
    // the open room's rules as its details read them (`rules.rs`)
    #[serde(skip)]
    pub(crate) rules: crate::rules::Rules,
    #[serde(skip)]
    pub(crate) typing: crate::typing::Typing,
}

/// A page of the view's [`Pages`]: the room list, search, the new-channel
/// form, the open room, its thread, its details. Each is a marker; its
/// data lives on [`Chat`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Rooms,
    /// message search, over the rooms (the phone's own page)
    Search,
    /// the create-channel form (the phone's own page; the desktop's
    /// dialog)
    New,
    // New message, whom a direct message goes to (the phone's own page;
    // the desktop's dialog)
    NewMessage,
    Room,
    Thread,
    Details,
}

impl Chat {
    /// Where the reader is, as the phone's screens stack: read off the
    /// data that is open (`search`, `create`, `room`, its `thread`,
    /// `details`), so the stack and the data cannot disagree. The phone
    /// shows the top page alone and Back closes it ([`Self::back`]); the
    /// desktop docks the same data as panes.
    pub(crate) fn pages(&self) -> Pages<Page> {
        let mut pages = Pages::root(Page::Rooms);
        if self.search.shown {
            pages.push(Page::Search);
        }
        if self.create.is_some() {
            pages.push(Page::New);
        }
        if self.dm_pick.is_some() {
            pages.push(Page::NewMessage);
        }
        if let Some(room) = &self.room {
            pages.push(Page::Room);
            if room.thread.is_some() {
                pages.push(Page::Thread);
            }
            if self.details.is_some() {
                pages.push(Page::Details);
            }
        }
        pages
    }

    /// The Search page, over the rooms.
    pub(crate) fn open_search(&mut self) {
        self.search.shown = true;
    }

    /// The new-channel form, empty unless one is already being written.
    pub(crate) fn begin_create(&mut self) {
        self.create.get_or_insert_default();
    }

    /// `sheet` over the channel's details, its field fresh: the member's
    /// number empty, the name the channel's own.
    pub(crate) fn open_details_sheet(&mut self, sheet: DetailsSheet) {
        let name = self
            .room_info()
            .map(|info| info.channel.name.clone())
            .unwrap_or_default();
        if let Some(details) = &mut self.details {
            details.member_draft.reset("");
            details.name_draft.reset(name);
            details.sheet = Some(sheet);
        }
    }

    /// The sheet over the channel's details closed; the page stays.
    pub(crate) fn close_details_sheet(&mut self) {
        if let Some(details) = &mut self.details {
            details.sheet = None;
        }
    }

    /// The phone's Back: a dialog closes first, then the top page.
    pub(crate) fn back(&mut self) {
        if self.menu.take().is_some()
            || self.create.take().is_some()
            || self.dm_pick.take().is_some()
        {
            return;
        }
        if self.message_policy.sheet.take().is_some() || self.room_policy.sheet.take().is_some() {
            return;
        }
        if let Some(details) = &mut self.details
            && details.sheet.take().is_some()
        {
            return;
        }
        match self.pages().top() {
            Page::Details => self.details = None,
            Page::Thread => self.close_thread(),
            Page::Room => self.close_room(),
            Page::Search => self.search_clear(),
            // a dialog above: closed first
            Page::New | Page::NewMessage | Page::Rooms => {}
        }
    }

    /// The open room left: the list alone shows. Its draft stays in
    /// `drafts`; its reads were saved when it was chosen.
    pub(crate) fn close_room(&mut self) {
        self.keep_room();
        self.details = None;
        self.menu = None;
        self.copy = None;
        self.hovered = None;
        self.room_policy = Default::default();
        self.message_policy = Default::default();
    }
}

#[derive(Default)]
pub struct Drawn {
    pub(crate) list: Option<(Shape, view::ListState)>,
    // the thread's every reply is in: its list follows its end
    pub(crate) whole: bool,
    pub(crate) follow: bool,
    pub(crate) restore: Option<(String, f32)>,
    pub(crate) keys: Vec<String>,
    pub(crate) folded: Option<Folded>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Tail,
    Landed,
    Thread,
}

pub struct Folded {
    pub(crate) rows: Vec<MsgRow>,
    pub(crate) names: Names,
    pub(crate) rooms: crate::links::Rooms,
    pub(crate) boundary: Option<u64>,
    pub(crate) zone: u64,
    pub(crate) messages: Rc<[ChatMessage]>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Room {
    pub(crate) id: String,
    pub(crate) messages: Loadable<Vec<MsgRow>>,
    pub(crate) members: Loadable<Vec<MemberRow>>,
    pub(crate) thread: Option<Thread>,
    #[serde(skip)]
    pub(crate) pending: Vec<MsgRow>,
    pub(crate) has_older: bool,
    #[serde(skip)]
    pub(crate) older_loading: bool,
    /// why the last read of older messages failed, until one lands: the
    /// phone says it at the list's end (Expo's), the desktop as its notice
    #[serde(skip)]
    pub(crate) older_failed: Option<String>,
    #[serde(skip)]
    pub(crate) newer_loading: bool,
    /// the newest seq a landed window has read: past its last row when the
    /// pages after it held replies alone, which it keeps out
    #[serde(skip)]
    pub(crate) read_to: u64,
    /// opened around a landing seq: the window may not reach the head
    pub(crate) landed: bool,
    #[serde(skip)]
    pub(crate) target: u64,
    pub(crate) at_tail: bool,
    /// the rows read again while the ones on screen stay
    #[serde(skip)]
    pub(crate) rereading: Option<view::Task<()>>,
    // who chose each reaction on the rows shown, room and thread, by seq
    // and emoji: what a chip's tooltip names
    #[serde(skip)]
    pub(crate) reactors: Reactors,
    // the read of who chose them, until it lands
    #[serde(skip)]
    pub(crate) reading_reactors: Option<view::Task<()>>,
    #[serde(skip)]
    pub(crate) kept_thread: Option<KeptThread>,
    // the reads of what blocks wrote still out, and what they read again
    #[serde(skip)]
    pub(crate) reading: usize,
    #[serde(skip)]
    pub(crate) unsettled: chat::view::Written,
    #[serde(skip)]
    pub(crate) unread: chat::view::Written,
    #[serde(skip)]
    pub(crate) row_read_at: BTreeMap<u64, u64>,
    #[serde(skip)]
    pub(crate) tail_read_at: u64,
    // the whole window read again, until it lands
    #[serde(skip)]
    pub(crate) rereading_all: bool,
    // who is answering in the room right now (chat's `Drafts`), each drawn at the foot of the
    // list it answers in
    #[serde(skip)]
    pub(crate) answering: Loadable<Vec<chat::Draft>>,
    #[serde(skip)]
    pub(crate) busy: Reread<Option<chat::Holder>>,
}

#[derive(Default)]
pub struct KeptThread {
    pub(crate) root: u64,
    pub(crate) replies: Vec<MsgRow>,
    pub(crate) earlier: bool,
    pub(crate) has_more: bool,
    pub(crate) next: Option<Vec<u8>>,
    pub(crate) anchor: Option<Anchor>,
}

#[derive(Default)]
pub struct Visited {
    pub(crate) rows: Vec<MsgRow>,
    pub(crate) has_older: bool,
    pub(crate) members: Vec<MemberRow>,
    pub(crate) members_moved: bool,
    pub(crate) reactors: Reactors,
    pub(crate) thread: Option<KeptThread>,
    pub(crate) thread_open: bool,
    pub(crate) anchor: Option<Anchor>,
    pub(crate) written: chat::view::Written,
}

impl Visited {
    pub(crate) fn landing(&self) -> Option<u64> {
        let anchor = self.anchor.as_ref()?;
        let held = self.rows.iter().any(|row| row.message_id == anchor.id);
        (!held).then_some(anchor.seq)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Anchor {
    pub(crate) id: String,
    pub(crate) seq: u64,
    pub(crate) offset: f32,
}

// Who chose each reaction, by message seq and emoji.
pub(crate) type Reactors = BTreeMap<u64, BTreeMap<String, Vec<Principal>>>;

#[derive(Serialize, Deserialize, Default)]
pub struct Thread {
    pub(crate) root: u64,
    // the reply a link opened the thread on (0: none): its pages are read
    // until it is in, and the list opens on it
    #[serde(skip)]
    pub(crate) target: u64,
    pub(crate) replies: Loadable<Vec<MsgRow>>,
    pub(crate) has_more: bool,
    pub(crate) next: Option<Vec<u8>>,
    #[serde(skip)]
    pub(crate) earlier: bool,
    #[serde(skip)]
    pub(crate) paging: Option<view::Task<()>>,
    /// the replies read again while the ones on screen stay
    #[serde(skip)]
    pub(crate) rereading: Option<view::Task<()>>,
    #[serde(skip)]
    pub(crate) busy: Reread<Option<chat::Holder>>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Search {
    /// The phone's Search page is open (the desktop's field is always
    /// beside the rooms).
    pub(crate) shown: bool,
    pub(crate) draft: TextField,
    /// the query the hits answer; "" while no search stands
    pub(crate) query: String,
    pub(crate) hits: Loadable<Hits>,
    #[serde(skip)]
    pub(crate) paging: Option<view::Task<()>>,
}

#[derive(Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct Hits {
    pub(crate) rows: Vec<MsgRow>,
    pub(crate) has_more: bool,
    pub(crate) next_after: Option<Vec<u8>>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct ChannelCreate {
    pub(crate) name: TextField,
    pub(crate) members_only: bool,
    pub(crate) error: String,
    #[serde(skip)]
    pub(crate) busy: bool,
    // the name's folder chip as the field last held it (`room_list::chip`)
    #[serde(skip)]
    pub(crate) chip: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct DmPick {
    pub(crate) query: TextField,
    pub(crate) cursor: Option<u64>,
}

#[derive(Default)]
pub struct DmHead {
    pub(crate) seq: u64,
    pub(crate) time: Option<u64>,
    pub(crate) reading: Option<view::Task<()>>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Details {
    pub(crate) name_draft: TextField,
    #[serde(default)]
    pub(crate) rules_draft: TextField,
    pub(crate) member_draft: TextField,
    /// the notices choice the arrows are on, until one is chosen
    #[serde(default)]
    pub(crate) notices_at: Option<usize>,
    /// the sheet open over the phone's details page
    pub(crate) sheet: Option<DetailsSheet>,
}

/// A sheet over the details page (the Expo details screen's): add a
/// member, rename the channel, or remove the member with this account.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum DetailsSheet {
    Add,
    Rename,
    Remove(u64),
    Notices(Option<u64>),
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct Layout {
    /// the pane's size, read from the window each render
    #[serde(skip)]
    pub(crate) viewport: (f32, f32),
    pub(crate) sidebar: f32,
    pub(crate) details: f32,
    pub(crate) thread: f32,
    /// where the pointer last pressed: a menu opens there
    pub(crate) press: (f32, f32),
    #[serde(default)]
    pub(crate) pane: Option<Place>,
    #[serde(skip)]
    pub(crate) tallest: f32,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            viewport: (0., 0.),
            sidebar: 236.,
            details: 320.,
            thread: 330.,
            press: (0., 0.),
            pane: None,
            tallest: 0.,
        }
    }
}

/// The sidebar's width bounds; it never takes more than half the window.
const SIDEBAR_W: (f32, f32) = (180., 420.);
/// The details pane's width bounds.
const DETAILS_W: (f32, f32) = (260., 520.);
/// The thread pane's width bounds.
const THREAD_W: (f32, f32) = (280., 640.);
/// What a side pane leaves the room: its narrowest column and the dividers.
const ROOM_KEEPS_W: f32 = 320. + 20.;

impl Layout {
    /// The panes' widths as this viewport allows them: the person's
    /// settings clamped at read, never written back, so a narrow window
    /// leaves no mark on a wide one after it (rule 12).
    pub(crate) fn clamped(&self) -> Layout {
        let (w, _) = self.viewport;
        let (lo, hi) = SIDEBAR_W;
        let sidebar = self.sidebar.clamp(lo, (w * 0.5).clamp(lo, hi));
        let side = w - sidebar - ROOM_KEEPS_W;
        let (lo, hi) = DETAILS_W;
        let details = self.details.clamp(lo, side.clamp(lo, hi));
        let (lo, hi) = THREAD_W;
        let thread = self.thread.clamp(lo, side.clamp(lo, hi));
        Layout {
            sidebar,
            details,
            thread,
            ..*self
        }
    }

    pub(crate) fn placed(&mut self, pane: Place) {
        self.tallest = self.tallest.max(pane.height);
        self.pane = Some(pane);
    }

    /// A drag's new width, kept inside the pane's own bounds: what the
    /// viewport allows is settled at read ([`Self::clamped`]).
    pub(crate) fn drag_sidebar(&mut self, dx: f32) {
        self.sidebar = (self.sidebar + dx).clamp(SIDEBAR_W.0, SIDEBAR_W.1);
    }
    pub(crate) fn drag_details(&mut self, dx: f32) {
        self.details = (self.details - dx).clamp(DETAILS_W.0, DETAILS_W.1);
    }
    pub(crate) fn drag_thread(&mut self, dx: f32) {
        self.thread = (self.thread - dx).clamp(THREAD_W.0, THREAD_W.1);
    }

    /// Whether a side pane `side` wide fits beside the sidebar and the
    /// room's narrowest; else it floats over the room.
    pub(crate) fn docks(&self, side: f32) -> bool {
        view::design::docks(self.viewport.0, self.sidebar + ROOM_KEEPS_W, side)
    }
}

/// What the reader has read: per room, the head seq when they last had it on
/// screen. `boundary` is the cursor the open room was entered with — the
/// unread divider's row is the first message past it.
#[derive(Serialize, Deserialize, Default)]
pub struct Reads {
    pub(crate) cursors: BTreeMap<String, u64>,
    #[serde(skip)]
    pub(crate) visible: bool,
    #[serde(skip)]
    pub(crate) entering: bool,
    pub(crate) boundary: u64,
    /// The store key the cursors were loaded from; nothing is written back
    /// until they have been.
    #[serde(default)]
    pub(crate) kept: Option<String>,
    /// The cursors last written to the store.
    #[serde(skip)]
    pub(crate) written: BTreeMap<String, u64>,
    #[serde(skip)]
    pub(crate) posted: BTreeMap<String, u64>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pane {
    Timeline,
    Thread,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// chosen: the floating actions stay open
    Toolbar,
    More,
    Reactions,
    Editing,
    Delete,
    /// an agent author's card, opened from its avatar
    Profile,
    // who chose one reaction (`Chat::reactors_of`), from a phone chip's
    // long press
    Reactors,
}

/// The delete confirmation's Cancel, the button that takes the keys as it
/// opens (a stray Enter keeps the message), and its Delete: one id each
/// in both designs' dialogs.
pub(crate) const CANCEL_DELETE: &str = "chat-menu-cancel-delete";
pub(crate) const CONFIRM_DELETE: &str = "chat-menu-confirm-delete";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Menu {
    pub(crate) pane: Pane,
    pub(crate) seq: u64,
    pub(crate) rev: u32,
    pub(crate) mode: Mode,
    pub(crate) at: (f32, f32),
}

// A sidebar room's menu: the room, where it opens (the press; from the
// keys, under the room's row once the row says where it is) and the item
// the arrows are on.
#[derive(Clone, Debug)]
pub struct RoomMenu {
    pub(crate) room: String,
    pub(crate) at: Option<view::Point<Pixels>>,
    pub(crate) active: usize,
}

#[derive(Clone, Debug)]
pub struct NoticesMenu {
    pub(crate) whose: Option<u64>,
    pub(crate) at: Option<view::Point<Pixels>>,
    pub(crate) active: usize,
}

/// The message the arrows are on in a pane's grid, and which of its cells:
/// its content (0) or one of its controls, recorded in paint order as the
/// row is drawn so Enter knows what the cell does. The newest message
/// until the arrows move.
#[derive(Default, Debug)]
pub struct Cursor {
    pub(crate) id: Option<String>,
    pub(crate) cell: usize,
    /// the controls of the message `controls_of`, the active row when it
    /// was last drawn; another message's are nobody's
    pub(crate) controls: Vec<Control>,
    pub(crate) controls_of: Option<String>,
}

impl Cursor {
    /// The controls of message `id`, if they are the ones recorded.
    pub(crate) fn controls_of(&self, id: &str) -> &[Control] {
        match self.controls_of.as_deref() == Some(id) {
            true => &self.controls,
            false => &[],
        }
    }
}

/// The message row under the pointer, or its action strip (`strip`): the
/// strip hangs half over the row above and occludes it, so the row itself
/// is not hovered while the pointer is on the strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Hovered {
    pub(crate) key: (Pane, u64),
    pub(crate) strip: bool,
}

/// A control on a message card: what pressing it does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Control {
    /// a run's avatar: its author's identity page
    Profile(u64),
    /// the block link in the header: `link`
    Height(String),
    /// a program post's "Open in …": `link`
    ProgramOpen(String),
    Reaction {
        emoji: String,
        add: bool,
    },
    AddReaction,
    Replies,
    Thread,
    ThumbsUp,
    React,
    More,
}

impl Chat {
    /// Whether the pointer is on the message row `key`: on the row or on
    /// its action strip.
    pub(crate) fn pointer_on(&self, key: (Pane, u64)) -> bool {
        self.hovered.is_some_and(|hovered| hovered.key == key)
    }

    pub(crate) fn cursor(&self, pane: Pane) -> &Cursor {
        match pane {
            Pane::Timeline => &self.timeline_cursor,
            Pane::Thread => &self.thread_cursor,
        }
    }
    pub(crate) fn cursor_mut(&mut self, pane: Pane) -> &mut Cursor {
        match pane {
            Pane::Timeline => &mut self.timeline_cursor,
            Pane::Thread => &mut self.thread_cursor,
        }
    }
    pub(crate) fn drawn(&self, pane: Pane) -> &RefCell<Drawn> {
        match pane {
            Pane::Timeline => &self.timeline_drawn,
            Pane::Thread => &self.thread_drawn,
        }
    }

    /// Where a popup the keys open sits: a key has no pointer position, so
    /// it takes the top-right of the pane's message list, inside the window.
    pub(crate) fn key_spot(&self, pane: Pane) -> (f32, f32) {
        let layout = self.layout.clamped();
        let (width, _) = layout.viewport;
        let side = match pane {
            Pane::Thread => 0.,
            Pane::Timeline if self.details.is_some() && self.room.is_some() => layout.details,
            Pane::Timeline if self.room.as_ref().is_some_and(|room| room.thread.is_some()) => {
                layout.thread
            }
            Pane::Timeline if self.widget_side_shown() => layout.details,
            Pane::Timeline => 0.,
        };
        let side = if side > 0. && layout.docks(side) {
            side
        } else {
            0.
        };
        (width - side - 16., 96.)
    }
}

/// The reaction picker: what the search holds, which tab is open, and the
/// cell the arrows are on in one of its grids (the grid's id, the cell).
#[derive(Default, Debug)]
pub struct Picker {
    pub(crate) query: TextField,
    pub(crate) tab: usize,
    pub(crate) cursor: Option<(&'static str, usize)>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct CopyRange {
    pub(crate) pane: Pane,
    pub(crate) anchor: u64,
    pub(crate) head: u64,
}

impl CopyRange {
    pub(crate) fn holds(&self, pane: Pane, seq: u64) -> bool {
        self.pane == pane && seq >= self.anchor.min(self.head) && seq <= self.anchor.max(self.head)
    }
}
