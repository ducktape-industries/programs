//! The open room: opening, landing, paging history, the thread beside it and
//! what the reader has read.
use chat::{ChannelInfo, MsgRow, PageRequest, Principal, ask};
use view::prelude::*;

use crate::composer::Target;
use crate::queries::{self, Side};
use crate::words;
use crate::{Chat, Drawn, PAGE, Pane, Room, SPAN, Thread, WINDOW};

const EDGE_ROWS: usize = 4;

enum Reread {
    Tail,
    Span(u64, u64),
}

impl Chat {
    /// The composer's placeholder: who or where a message goes
    /// (`Message #general`, `Message Priya Nair`).
    pub(crate) fn composer_hint(&self, room: &Room) -> String {
        format!("Message {}", self.room_name(&room.id))
    }

    /// A pane's refused read, asked again: the open thread, or the room
    /// from its newest message.
    pub(crate) fn retry_read(&mut self, pane: Pane, cx: &mut Context<Self>) {
        let Some(room) = &self.room else { return };
        match (pane, room.thread.as_ref().map(|thread| thread.root)) {
            (Pane::Thread, Some(root)) => self.open_thread(root, cx),
            _ => self.open(room.id.clone(), cx),
        }
    }

    /// A room the reader chose: opened here, and only here. Named to the
    /// host as a link, its route would reach another chat window too.
    pub(crate) fn choose(&mut self, id: String, cx: &mut Context<Self>) {
        self.create = None;
        self.search_clear();
        self.open(id, cx);
        // read to the head this view knows now, not at the next list: a
        // reader who opens a room and quits has read it
        if let Some(list) = self.channels.ready().cloned() {
            self.channels_arrived(list, cx);
            self.save_reads(cx);
        }
    }

    pub(crate) fn open(&mut self, id: String, cx: &mut Context<Self>) {
        self.open_at(id, 0, cx);
    }

    /// The room at its live tail (`land` 0) or around a landing seq.
    pub(crate) fn open_at(&mut self, id: String, land: u64, cx: &mut Context<Self>) {
        let viewer = self.viewer();
        let same = self.room.as_ref().is_some_and(|room| room.id == id);
        if !same {
            self.reads.boundary = 0;
            self.reads.entering = self.reads.visible;
            self.menu = None;
            self.copy = None;
            self.details = None;
            // the next room's row at the same seq is not the one under
            // the pointer
            self.hovered = None;
        }
        let room = self.room.get_or_insert_default();
        if !same {
            *room = Room {
                id: id.clone(),
                at_tail: true,
                reaches_head: true,
                ..Room::default()
            };
        }
        room.landed = land > 0;
        room.target = land;
        room.at_tail = land == 0;
        room.older_loading = false;
        room.newer_loading = false;
        room.read_to = 0;
        // a re-read still out is the old window's: wherever it lands, its
        // rows go over the new window or the read for it
        room.rereading = None;
        // the room is entered, or entered again at another row: the rows
        // and the roster shown go, and both read as loading
        room.messages = Loadable::Idle;
        room.members = Loadable::Idle;
        *self.timeline_drawn.borrow_mut() = Drawn::default();
        let members_id = room.id.clone();
        if land > 0 {
            let rows = queries::around(cx.host(), id, land, viewer);
            room.messages = Loadable::Loading(cx.land(rows, |chat, result, cx| {
                cx.notify();
                let room = room_of(chat);
                if let Ok(rows) = &result {
                    room.has_older = rows.first().is_some_and(|row| row.seq > 1);
                }
                room.messages = result.into();
                room.settle();
                room.read_reactors(cx);
                if chat
                    .channels
                    .ready()
                    .is_some_and(|list| chat.landed_on_head(list))
                {
                    chat.reached_head(cx);
                }
            }));
        } else {
            let rows = queries::roots(cx.host(), id, viewer, None, WINDOW);
            room.messages = Loadable::Loading(cx.land(rows, |chat, result, cx| {
                cx.notify();
                chat.rows_arrived(result);
                room_of(chat).read_reactors(cx);
            }));
        }
        cx.load(self, queries::members(cx.host(), members_id), |chat| {
            &mut room_of(chat).members
        });
    }

    /// The newest window landed: the rows, and whether older ones remain.
    fn rows_arrived(&mut self, result: Result<(Vec<MsgRow>, bool), Error>) {
        let Some(room) = &mut self.room else { return };
        match result {
            Ok((rows, has_older)) => {
                room.has_older = has_older;
                room.reaches_head = true;
                room.messages = Loadable::Ready(rows);
                room.settle();
            }
            Err(refusal) => room.messages = Loadable::Failed(refusal),
        }
    }

    /// Everything chat shows, read again: the channel list and the open
    /// room, after this reader's own op or a session change.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        self.changed(None, cx);
    }

    /// A chat block landed: the channel list and what the open room shows
    /// are read again where the block wrote to what they read, as the
    /// program declares it; `None`, a reopened node link, reads everything
    /// again.
    pub(crate) fn changed(&mut self, change: Option<&Change>, cx: &mut Context<Self>) {
        let list = ask::Channels {
            page: PageRequest::default(),
        };
        if change.is_none_or(|change| change.touches::<chat::Chat, _>(&list)) {
            self.reread_channels(cx);
        }
        self.refresh_room(change, cx);
    }

    pub(crate) fn reread_channels(&mut self, cx: &mut Context<Self>) {
        let list = queries::channels(cx.host());
        self.rereading_channels = Some(cx.land(list, |chat, rooms, cx| match rooms {
            Ok(rooms) => chat.channels_landed(rooms, cx),
            Err(refusal) => cx.log_refused("the channel list", &refusal),
        }));
    }

    /// Re-read what the room shows that `change` wrote to (`None`: all of
    /// it), keeping the rows there until fresh land; rows that land as they
    /// were draw nothing. Each read is held by what it reads, so leaving the
    /// room or the thread drops it.
    fn refresh_room(&mut self, change: Option<&Change>, cx: &mut Context<Self>) {
        let touched = |read: &dyn program::Reads| {
            change.is_none_or(|change| change.touches::<chat::Chat, _>(read))
        };
        let viewer = self.viewer();
        let Some(room) = &mut self.room else { return };
        let id = room.id.clone();
        let page = PageRequest::default();
        let reread = match room.landed {
            true => room
                .messages
                .ready()
                .and_then(|rows| Some((rows.first()?.seq, rows.last()?.seq)))
                .filter(|(first, last)| {
                    touched(&ask::MessagesAround {
                        channel_id: id.clone(),
                        seq: first.midpoint(*last),
                        viewer: viewer.clone(),
                        page: page.clone(),
                    })
                })
                .map(|(first, last)| Reread::Span(first, last)),
            false => touched(&ask::Roots {
                channel_id: id.clone(),
                viewer: viewer.clone(),
                page: page.clone(),
            })
            .then_some(Reread::Tail),
        };
        match reread {
            Some(Reread::Tail) => {
                let shown = room
                    .messages
                    .ready()
                    .map_or(WINDOW, |rows| rows.len().max(WINDOW));
                let rows = queries::roots(cx.host(), id.clone(), viewer.clone(), None, shown);
                room.rereading = Some(cx.land(rows, |chat, rows, cx| {
                    let room = room_of(chat);
                    match rows {
                        Ok((rows, has_older)) => {
                            let moved =
                                room.messages.ready() != Some(&rows) || room.has_older != has_older;
                            if moved {
                                room.has_older = has_older;
                                room.messages = Loadable::Ready(rows);
                            }
                            room.landed_rows(moved, cx);
                        }
                        Err(refusal) => cx.log_refused("the room", &refusal),
                    }
                }));
            }
            Some(Reread::Span(first, last)) => {
                let rows = queries::span(cx.host(), id.clone(), (first, last), viewer.clone());
                room.rereading = Some(cx.land(rows, |chat, rows, cx| {
                    let room = room_of(chat);
                    match rows {
                        Ok(rows) => {
                            let moved = room.messages.ready() != Some(&rows);
                            if moved {
                                room.messages = Loadable::Ready(rows);
                            }
                            room.landed_rows(moved, cx);
                        }
                        Err(refusal) => cx.log_refused("the room", &refusal),
                    }
                }));
            }
            None => {}
        }
        if touched(&ask::Members {
            channel_id: id.clone(),
            page: page.clone(),
        }) {
            let roster = queries::members(cx.host(), id.clone());
            cx.load(self, roster, |chat| &mut room_of(chat).members);
        }
        if let Some(thread) = room_of(self).thread.as_mut()
            && touched(&ask::Thread {
                channel_id: id.clone(),
                root_seq: thread.root,
                viewer: viewer.clone(),
                page,
            })
        {
            let root = thread.root;
            let replies = queries::thread(cx.host(), id, root, viewer, None);
            thread.rereading = Some(cx.land(replies, move |chat, replies, cx| {
                let room = room_of(chat);
                let Some(thread) = room.thread.as_mut().filter(|thread| thread.root == root) else {
                    return;
                };
                match replies {
                    Ok((replies, next)) => {
                        let moved = thread.replies.ready() != Some(&replies) || thread.next != next;
                        if moved {
                            thread.replies = Loadable::Ready(replies);
                            thread.has_more = next.is_some();
                            thread.next = next;
                        }
                        room.landed_rows(moved, cx);
                    }
                    Err(refusal) => cx.log_refused("the thread", &refusal),
                }
            }));
        }
    }

    /// One older page before the oldest row on screen.
    pub(crate) fn load_older(&mut self, cx: &mut Context<Self>) {
        match self.room.as_ref().is_some_and(|room| room.landed) {
            true => self.page_landed(Side::Older, cx),
            false => self.page_roots(cx),
        }
    }

    fn page_roots(&mut self, cx: &mut Context<Self>) {
        let viewer = self.viewer();
        let Some(room) = &mut self.room else { return };
        let Some(oldest) = room
            .messages
            .ready()
            .and_then(|rows| rows.first())
            .map(|r| r.seq)
        else {
            return;
        };
        if !room.has_older || room.older_loading {
            return;
        }
        room.older_loading = true;
        let id = room.id.clone();
        let below = chat::roots_below(&id, oldest);
        let older = queries::roots(cx.host(), id.clone(), viewer, Some(below), PAGE);
        cx.land(older, move |chat, result, cx| {
            cx.notify();
            let Some(room) = &mut chat.room else { return };
            let beside = room.id == id && !room.landed && edge(room, Side::Older) == Some(oldest);
            if !beside {
                return;
            }
            room.older_loading = false;
            match result {
                Ok((older, has_older)) => {
                    room.has_older = has_older;
                    room.older_failed = None;
                    if let Some(rows) = room.messages.ready_mut() {
                        let mut all = older;
                        all.append(rows);
                        *rows = all;
                    }
                    room.settle();
                    room.read_reactors(cx);
                }
                Err(refusal) => room.older_failed = Some(design::sentence(&refusal)),
            }
        })
        .detach();
    }

    fn page_landed(&mut self, side: Side, cx: &mut Context<Self>) {
        let viewer = self.viewer();
        let Some(room) = &mut self.room else { return };
        let Some(at) = edge(room, side) else { return };
        let busy = match side {
            Side::Older => room.older_loading || !room.has_older,
            Side::Newer => room.newer_loading,
        };
        if busy {
            return;
        }
        match side {
            Side::Older => room.older_loading = true,
            Side::Newer => room.newer_loading = true,
        }
        let id = room.id.clone();
        let page = queries::beside(cx.host(), id.clone(), at, side, viewer);
        cx.land(page, move |chat, result, cx| {
            cx.notify();
            let Some(room) = chat.room.as_mut() else {
                return;
            };
            let beside = room.id == id && room.landed && edge(room, side) == Some(at);
            if !beside {
                return;
            }
            match side {
                Side::Older => room.older_loading = false,
                Side::Newer => room.newer_loading = false,
            }
            let page = match result {
                Ok(page) => page,
                Err(refusal) if side == Side::Older => {
                    room.older_failed = Some(design::sentence(&refusal));
                    return;
                }
                Err(refusal) => {
                    chat.notice = words::unreadable_room(&design::sentence(&refusal));
                    return;
                }
            };
            if side == Side::Older {
                room.older_failed = None;
            }
            let at_head = side == Side::Newer && page.is_empty();
            if at_head {
                chat.reached_head(cx);
                return;
            }
            let last = page.last().map_or(at, |row| row.seq);
            if side == Side::Newer {
                // replies alone draw no row: the list does not move, so the
                // host reports nothing new and the reader is still at the
                // foot. The window keeps them out and reads on.
                if page.iter().all(|row| row.thread.is_some()) {
                    room.read_to = last;
                    chat.page_landed(side, cx);
                    return;
                }
                // roots past a run of replies longer than the window: it
                // cannot hold them beside its newest root, the rows the
                // reader is on, so the room stays a landing
                let newest_root = room
                    .messages
                    .ready()
                    .and_then(|rows| rows.iter().rev().find(|row| row.thread.is_none()));
                if newest_root.is_some_and(|root| last - root.seq >= SPAN as u64) {
                    return;
                }
            }
            room.extend(side, page);
            room.read_reactors(cx);
        })
        .detach();
    }

    fn reached_head(&mut self, cx: &mut Context<Self>) {
        let Some(room) = &mut self.room else { return };
        room.live();
        if let Some(list) = self.channels.ready().cloned() {
            self.channels_arrived(list, cx);
            self.save_reads(cx);
        }
    }

    /// Settled native list geometry drives paging and tail state. Wheel deltas
    /// are intentionally not used: remeasurement and programmatic scrolling
    /// can move the viewport without one.
    pub(crate) fn list_scrolled(
        &mut self,
        pane: Pane,
        event: &view::ListScrollEvent,
        cx: &mut Context<Self>,
    ) {
        let at_top = event.visible_range.start <= EDGE_ROWS;
        let at_foot = event.visible_range.end.saturating_add(EDGE_ROWS) >= event.count;
        match pane {
            Pane::Timeline => {
                let Some(room) = &mut self.room else { return };
                let at_tail = event.is_following_tail || event.visible_range.end >= event.count;
                if room.at_tail != at_tail {
                    room.at_tail = at_tail;
                    cx.notify();
                }
                let landed = room.landed;
                if at_top {
                    self.load_older(cx);
                }
                if landed && at_foot {
                    self.page_landed(Side::Newer, cx);
                }
            }
            Pane::Thread => {
                if at_foot {
                    self.load_more_replies(cx);
                }
            }
        }
    }

    /// The thread under `root`, read and shown; the keys stay where they
    /// are.
    pub(crate) fn open_thread(&mut self, root: u64, cx: &mut Context<Self>) {
        self.load_thread(root, false, cx);
    }

    /// The thread under `root`, its reply field taking the keys once it
    /// can be typed in.
    pub(crate) fn reply_in_thread(&mut self, root: u64, cx: &mut Context<Self>) {
        self.load_thread(root, true, cx);
    }

    fn load_thread(&mut self, root: u64, reply: bool, cx: &mut Context<Self>) {
        let viewer = self.viewer();
        self.details = None;
        self.menu = None;
        self.copy = None;
        let Some(room) = &mut self.room else { return };
        let id = room.id.clone();
        let thread = room.thread.get_or_insert_default();
        *thread = Thread {
            root,
            ..Thread::default()
        };
        let handle = cx.spawn(async move |this, cx| {
            let host = cx.host();
            let result = queries::thread(host, id.clone(), root, viewer, None).await;
            let _ = this.update_in(cx, |chat, window, cx| {
                cx.notify();
                let Some(thread) = chat.room.as_mut().and_then(|r| r.thread.as_mut()) else {
                    return;
                };
                if thread.root != root {
                    return;
                }
                match result {
                    Ok((replies, next)) => {
                        thread.replies = Loadable::Ready(replies);
                        thread.has_more = next.is_some();
                        thread.next = next;
                        room_of(chat).settle();
                        room_of(chat).read_reactors(cx);
                        if reply {
                            let key = Target::Post {
                                channel: id,
                                thread: Some(root),
                            }
                            .key();
                            crate::composer::focus(window, &key);
                        }
                    }
                    Err(refusal) => thread.replies = Loadable::Failed(refusal),
                }
            });
        });
        let Some(thread) = self.room.as_mut().and_then(|r| r.thread.as_mut()) else {
            return;
        };
        thread.replies = Loadable::Loading(handle);
    }

    pub(crate) fn load_more_replies(&mut self, cx: &mut Context<Self>) {
        let viewer = self.viewer();
        let Some(room) = &mut self.room else { return };
        let id = room.id.clone();
        let Some(thread) = &mut room.thread else {
            return;
        };
        if !thread.has_more || thread.more_loading {
            return;
        }
        thread.more_loading = true;
        let (root, after) = (thread.root, thread.next.clone());
        let more = queries::thread(cx.host(), id, root, viewer, after);
        cx.land(more, move |chat, result, cx| {
            cx.notify();
            let Some(thread) = chat.room.as_mut().and_then(|r| r.thread.as_mut()) else {
                return;
            };
            thread.more_loading = false;
            match result {
                Ok(_) if thread.root != root => {}
                Ok((more, next)) => {
                    thread.has_more = next.is_some();
                    thread.next = next;
                    if let Some(rows) = thread.replies.ready_mut() {
                        rows.extend(more);
                    }
                    room_of(chat).settle();
                    room_of(chat).read_reactors(cx);
                }
                Err(refusal) => {
                    chat.notice =
                        format!("Couldn’t read this thread: {}", design::sentence(&refusal))
                }
            }
        })
        .detach();
    }

    pub(crate) fn close_thread(&mut self) {
        if let Some(room) = &mut self.room {
            room.thread = None;
        }
        if self
            .menu
            .as_ref()
            .is_some_and(|menu| menu.pane == Pane::Thread)
        {
            self.menu = None;
        }
        if self.copy.is_some_and(|copy| copy.pane == Pane::Thread) {
            self.copy = None;
        }
    }

    /// The channel list landed: the rooms, and what each one's head says
    /// about what the reader has read. A room landed on its newest root is
    /// the room at its head ([`Self::landed_on_head`]). The room on screen
    /// is read to its head, and the host's notices for it with it.
    pub(crate) fn channels_arrived(&mut self, channels: Vec<ChannelInfo>, cx: &mut Context<Self>) {
        if self.landed_on_head(&channels)
            && let Some(room) = &mut self.room
        {
            room.live();
            cx.notify();
        }
        let reading = self
            .room
            .as_ref()
            .filter(|room| self.reads.visible && !room.landed)
            .map(|room| room.id.clone());
        let mut read = None;
        // what the list, its unread marks and the divider draw
        let mut moved = self.channels.ready() != Some(&channels);
        for info in &channels {
            let cursor = match self.reads.cursors.entry(info.channel.id.clone()) {
                std::collections::btree_map::Entry::Occupied(cursor) => cursor.into_mut(),
                std::collections::btree_map::Entry::Vacant(cursor) => {
                    moved = true;
                    cursor.insert(info.head_seq)
                }
            };
            if reading.as_deref() == Some(info.channel.id.as_str()) {
                if self.reads.entering && info.head_seq > *cursor {
                    self.reads.boundary = *cursor;
                }
                self.reads.entering = false;
                if info.head_seq > *cursor {
                    *cursor = info.head_seq;
                    read = Some(info.channel.id.clone());
                    moved = true;
                }
            }
        }
        // a room gone from the list takes its cursor with it: the kept map
        // is written whole, and must not grow with every room ever seen.
        let listed: std::collections::HashSet<&str> = channels
            .iter()
            .map(|info| info.channel.id.as_str())
            .collect();
        let kept = self.reads.cursors.len();
        self.reads
            .cursors
            .retain(|room, _| listed.contains(room.as_str()));
        if moved || self.reads.cursors.len() != kept {
            self.channels = Loadable::Ready(channels);
            cx.notify();
        }
        if let Some(room) = read {
            self.read_notices(&room, cx);
        }
    }

    /// Whether the room landed on its newest root, its window holding the
    /// room's head as `channels` names it. The host draws the last row at
    /// the list's foot either way, so such a room is the room at its head:
    /// nothing newer to page to, nothing to jump to. A landing with newer
    /// roots below it stays one, since only the host knows whether they fit
    /// on the screen.
    fn landed_on_head(&self, channels: &[ChannelInfo]) -> bool {
        let Some(room) = self.room.as_ref().filter(|room| room.landed) else {
            return false;
        };
        let Some(rows) = room.messages.ready() else {
            return false;
        };
        let head = channels
            .iter()
            .find(|info| info.channel.id == room.id)
            .map(|info| info.head_seq);
        let holds_head = rows
            .last()
            .zip(head)
            .is_some_and(|(row, head)| row.seq >= head);
        // the row the timeline lands on: the target, or a reply's root
        let landing = rows
            .iter()
            .find(|row| row.seq == room.target)
            .map(|row| row.thread.unwrap_or(row.seq));
        let newest_root = rows
            .iter()
            .rev()
            .find(|row| row.thread.is_none())
            .map(|row| row.seq);
        holds_head && landing.is_some() && landing == newest_root
    }

    pub(crate) fn unread(&self, info: &ChannelInfo) -> bool {
        self.reads
            .cursors
            .get(&info.channel.id)
            .is_some_and(|cursor| info.head_seq > *cursor)
    }

    pub(crate) fn room_id(&self) -> String {
        self.room
            .as_ref()
            .map(|room| room.id.clone())
            .unwrap_or_default()
    }

    pub(crate) fn info(&self, id: &str) -> Option<&ChannelInfo> {
        self.channels
            .ready()?
            .iter()
            .find(|info| info.channel.id == id)
    }

    /// The window's title: the open room as its header names it, `#name`
    /// for a channel, the peer for a direct room; empty while the name is
    /// still out, never the room's id. Sent when it changes.
    pub(crate) fn settle_title(&mut self, cx: &mut Context<Self>) {
        let title = match &self.room {
            Some(room) if chat::dm_peers(&room.id).is_some() => {
                self.dm_peer().map(|(peer, _)| peer)
            }
            Some(room) => self
                .info(&room.id)
                .map(|info| format!("#{}", info.channel.name)),
            None => None,
        }
        .unwrap_or_default();
        if self.title.as_deref() != Some(title.as_str()) {
            cx.host().notify::<HostTitle>(title.clone());
            self.title = Some(title);
        }
    }

    pub(crate) fn room_info(&self) -> Option<&ChannelInfo> {
        self.info(&self.room.as_ref()?.id)
    }

    /// The open room's members, as the roster names them.
    pub(crate) fn roster(&self) -> Vec<(Principal, String)> {
        let (Some(names), Some(members)) = (
            self.names.ready(),
            self.room.as_ref().and_then(|room| room.members.ready()),
        ) else {
            return Vec::new();
        };
        members
            .iter()
            .map(|row| (row.principal.clone(), names.member(&row.principal)))
            .collect()
    }
}

impl Room {
    /// The room at its head: no longer a window around a landing.
    fn live(&mut self) {
        self.landed = false;
        self.target = 0;
        self.at_tail = true;
    }

    /// Fresh rows landed or a send was held: a pending send the index
    /// serves leaves, so the room never lists a message twice.
    pub(crate) fn settle(&mut self) {
        let shown = |id: &str, rows: Option<&Vec<MsgRow>>| {
            rows.is_some_and(|rows| rows.iter().any(|row| row.message_id == id))
        };
        let replies = self.thread.as_ref().and_then(|t| t.replies.ready());
        let messages = self.messages.ready();
        self.pending
            .retain(|p| !shown(&p.message_id, messages) && !shown(&p.message_id, replies));
    }

    fn extend(&mut self, side: Side, page: Vec<MsgRow>) {
        let Some(rows) = self.messages.ready_mut() else {
            return;
        };
        match side {
            Side::Older => {
                self.has_older = page.first().is_some_and(|row| row.seq > 1);
                rows.splice(0..0, page);
                let Some(first) = rows.first().map(|row| row.seq) else {
                    return;
                };
                let held = rows.len();
                rows.retain(|row| row.seq - first < SPAN as u64);
                // the newer end let go: newer pages read it again
                if rows.len() < held {
                    self.read_to = 0;
                }
            }
            Side::Newer => {
                rows.extend(page);
                let Some(last) = rows.last().map(|row| row.seq) else {
                    return;
                };
                let held = rows.len();
                rows.retain(|row| last - row.seq < SPAN as u64);
                self.has_older |= rows.len() < held;
            }
        }
        self.settle();
    }

    /// A re-read landed: drops the sends it shows, and draws if the rows
    /// `moved` or a send left.
    fn landed_rows(&mut self, moved: bool, cx: &mut Context<Chat>) {
        let pending = self.pending.len();
        self.settle();
        if moved || self.pending.len() != pending {
            cx.notify();
        }
        // a reaction one dropped and another chose in one block moves no
        // count: the rows land as they were, and who chose them is read
        // again all the same
        self.read_reactors(cx);
    }

    /// Who chose each reaction on the rows shown, room and thread: read
    /// again each time rows land, kept until the next read lands.
    pub(crate) fn read_reactors(&mut self, cx: &mut Context<Chat>) {
        let replies = self
            .thread
            .as_ref()
            .and_then(|thread| thread.replies.ready());
        let seqs: Vec<u64> = (self.messages.ready().into_iter().chain(replies))
            .flatten()
            .filter(|row| !row.reactions.is_empty())
            .map(|row| row.seq)
            .collect();
        if seqs.is_empty() {
            self.reactors.clear();
            self.reading_reactors = None;
            return;
        }
        let read = queries::reactors(cx.host(), self.id.clone(), seqs);
        self.reading_reactors = Some(cx.land(read, |chat, read, cx| {
            let room = room_of(chat);
            match read {
                Ok(reactors) if reactors != room.reactors => {
                    room.reactors = reactors;
                    cx.notify();
                }
                Ok(_) => {}
                Err(refusal) => cx.log_refused("who reacted", &refusal),
            }
        }));
    }
}

fn edge(room: &Room, side: Side) -> Option<u64> {
    let rows = room.messages.ready()?;
    match side {
        Side::Older => rows.first().map(|row| row.seq),
        Side::Newer => rows.last().map(|row| row.seq.max(room.read_to)),
    }
}

pub(crate) fn room_of(chat: &mut Chat) -> &mut Room {
    chat.room.get_or_insert_default()
}
