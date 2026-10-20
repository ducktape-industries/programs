//! The open room: opening, landing, paging history, the thread beside it and
//! what the reader has read.
use chat::view::Written;
use chat::{ChannelInfo, MsgRow, PageRequest, Principal, ask};
use std::collections::BTreeSet;
use view::prelude::*;

use crate::composer::Target;
use crate::queries::{self, Side};
use crate::words;
use crate::{Anchor, Chat, Drawn, KeptThread, Pane, Room, Thread, Visited};

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

    // who is answering in `pane`, drawn at the foot of its list: the room's drafts while it shows
    // its newest rows, a thread's once every reply is in
    pub(crate) fn answering(&self, pane: Pane) -> Vec<chat::Draft> {
        let Some(room) = &self.room else {
            return Vec::new();
        };
        let under = match (pane, &room.thread) {
            (Pane::Timeline, _) if !room.landed => None,
            (Pane::Thread, Some(thread)) if !thread.has_more => Some(thread.root),
            (Pane::Timeline | Pane::Thread, _) => return Vec::new(),
        };
        let drafts = room.answering.ready().into_iter().flatten();
        drafts
            .filter(|draft| draft.thread == under)
            .filter(|draft| !self.wrote_it(draft))
            .cloned()
            .collect()
    }

    // who is answering under the root `seq` of the open room, by name: its reply row in the
    // channel says so, while the draft itself is drawn in the thread
    pub(crate) fn answering_under(&self, seq: u64) -> Vec<String> {
        let Some(room) = &self.room else {
            return Vec::new();
        };
        let drafts = room.answering.ready().into_iter().flatten();
        drafts
            .filter(|draft| draft.thread == Some(seq))
            .filter(|draft| !self.wrote_it(draft))
            .map(|draft| crate::ui::answering::name(self, &draft.author))
            .collect()
    }

    fn wrote_it(&self, draft: &chat::Draft) -> bool {
        self.me().as_ref() == Some(&draft.author)
    }

    pub(crate) fn busy(&self, pane: Pane) -> Option<chat::Holder> {
        let room = self.room.as_ref()?;
        let held = match (pane, &room.thread) {
            (Pane::Timeline, _) if !room.landed => &room.busy,
            (Pane::Thread, Some(thread)) if !thread.has_more => &thread.busy,
            (Pane::Timeline | Pane::Thread, _) => return None,
        };
        held.ready().cloned().flatten()
    }

    pub(crate) fn settle_busy(&mut self, cx: &mut Context<Self>) {
        let Some(room) = &self.room else { return };
        let thread = room.thread.as_ref();
        let (foot, under) = (
            room.busy.is_idle(),
            thread.is_some_and(|t| t.busy.is_idle()),
        );
        if foot {
            self.read_foot_busy(cx);
        }
        if under {
            self.read_thread_busy(cx);
        }
    }

    fn read_foot_busy(&mut self, cx: &mut Context<Self>) {
        let (Some(me), Some(room)) = (self.my_account(), &self.room) else {
            return;
        };
        let channel = room.id.clone();
        cx.reread(
            self,
            |chat| &mut room_of(chat).busy,
            move |_, cx| queries::busy(cx.host(), channel, None, me),
            Self::read_foot_busy,
        );
    }

    fn read_thread_busy(&mut self, cx: &mut Context<Self>) {
        let (Some(me), Some(room)) = (self.my_account(), &self.room) else {
            return;
        };
        let Some(root) = room.thread.as_ref().map(|thread| thread.root) else {
            return;
        };
        let channel = room.id.clone();
        cx.reread(
            self,
            |chat| &mut room_of(chat).thread.get_or_insert_default().busy,
            move |_, cx| queries::busy(cx.host(), channel, Some(root), me),
            Self::read_thread_busy,
        );
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
        let first = self.screenful();
        let same = self.room.as_ref().is_some_and(|room| room.id == id);
        if !same {
            self.keep_room();
            self.reads.boundary = 0;
            self.reads.entering = self.reads.visible;
            self.menu = None;
            self.copy = None;
            self.details = None;
            // the next room's row at the same seq is not the one under
            // the pointer
            self.hovered = None;
            // a refusal is said where it happened: another room lets it go
            for draft in self.drafts.values_mut() {
                draft.hush();
            }
        }
        // a link names one message: a thread or the details over the room
        // are not it, and a reply's own thread opens once its rows land
        if land > 0 {
            self.close_thread();
            self.details = None;
        }
        // a room opened this session shows what it showed at once; a link
        // lands afresh
        let kept = match (same, land) {
            (false, 0) => self.visited.remove(&id),
            _ => None,
        };
        if let Some(seq) = kept.as_ref().and_then(Visited::landing) {
            let open = kept.as_ref().is_some_and(|kept| kept.thread_open);
            let thread = kept.and_then(|kept| kept.thread).filter(|_| open);
            self.open_at(id, seq, cx);
            if let Some(thread) = thread {
                self.restore_thread(thread, true, cx);
            }
            return;
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
        if !same {
            self.read_foot_busy(cx);
        }
        let room = self.room.get_or_insert_default();
        room.landed = land > 0;
        room.target = land;
        room.at_tail = land == 0;
        room.older_loading = false;
        room.newer_loading = false;
        room.read_to = 0;
        // a re-read still out is the old window's: wherever it lands, its
        // rows go over the new window or the read for it
        room.rereading = None;
        room.rereading_all = false;
        // the room is entered, or entered again at another row: the rows
        // and the roster shown go, and both read as loading
        room.messages = Loadable::Idle;
        room.members = Loadable::Idle;
        *self.timeline_drawn.borrow_mut() = Drawn::default();
        let members_id = room.id.clone();
        let answering = queries::answering(cx.host(), members_id.clone());
        room.answering = Loadable::Loading(cx.land(answering, |chat, answering, cx| {
            cx.notify();
            room_of(chat).answering = answering.into();
        }));
        if land > 0 {
            let rows = queries::around(cx.host(), id.clone(), land, viewer, first);
            room.messages = Loadable::Loading(cx.land(rows, move |chat, result, cx| {
                cx.notify();
                // a reply opens in its thread, on it, and the room lands on
                // its root: around the root when the reply's rows lack it
                let reply = result.as_ref().ok().and_then(|rows| reply_root(rows, land));
                match reply {
                    Some((root, false)) => {
                        chat.open_at(id, root, cx);
                        chat.load_thread(root, false, land, cx);
                        return;
                    }
                    Some((root, true)) => chat.load_thread(root, false, land, cx),
                    None => {}
                }
                let room = room_of(chat);
                if let Ok(rows) = &result {
                    room.has_older = rows.first().is_some_and(|row| row.seq > 1);
                }
                room.messages = result.into();
                room.settle();
                room.read_reactors(cx);
                // the thread's root is its list's first row
                chat.seek(cx);
                if chat
                    .channels
                    .ready()
                    .is_some_and(|list| chat.landed_on_head(list))
                {
                    chat.reached_head(cx);
                }
            }));
        } else if let Some(kept) = kept {
            room.has_older = kept.has_older;
            room.messages = Loadable::Ready(kept.rows);
            room.reactors = kept.reactors;
            room.settle();
            let members = (!kept.members_moved).then_some(kept.members);
            let held = members.is_some();
            if let Some(members) = members {
                room.members = Loadable::Ready(members);
            }
            let replied = |root: u64| kept.written.replies.iter().any(|(at, _)| *at == root);
            let open = match kept.thread {
                Some(thread) if kept.thread_open => {
                    let newer = replied(thread.root);
                    Some((thread, newer))
                }
                thread => {
                    room.kept_thread = thread;
                    None
                }
            };
            self.timeline_drawn.borrow_mut().restore =
                kept.anchor.map(|anchor| (anchor.id, anchor.offset));
            // what blocks wrote to it since it was left, and nothing more
            self.catch_up(kept.written, cx);
            if let Some((thread, newer)) = open {
                self.restore_thread(thread, newer, cx);
            }
            if held {
                return;
            }
        } else {
            let rows = queries::roots(cx.host(), id, viewer, None, first);
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
        // the rooms it wrote the row, head or meta of, each read alone; `None`, every room
        self.moved_channels.heard(change);
        self.visited_heard(change);
        self.active_heard(change);
        self.reread_moved(cx);
        // the reader's choices, and those of the accounts it manages, show in the details alone
        let managed = self.managed_choices.ready().into_iter().flatten();
        let managed: Vec<u64> = managed.map(|(account, _)| *account).collect();
        if let Some(me) = self.my_account()
            && self.details.is_some()
            && change.is_none_or(|change| {
                let mut whose = std::iter::once(me).chain(managed.iter().copied());
                whose.any(|account| crate::notifier::choices_moved(change, account))
            })
        {
            self.load_choices(cx);
        }
        self.refresh_room(change, cx);
        // the rooms the reader listens to and wrote in, which both lists hold as theirs, and
        // their order
        if let Some(me) = self.my_account().filter(|_| self.listening_asked) {
            let listening = ask::Listening {
                account: me,
                page: PageRequest::default(),
            };
            let posted = ask::PostedIn {
                account: me,
                page: PageRequest::default(),
            };
            let order = ask::RoomOrder { account: me };
            if change.is_none_or(|change| {
                change.touches::<chat::Chat, _>(&listening)
                    || change.touches::<chat::Chat, _>(&posted)
                    || change.touches::<chat::Chat, _>(&order)
            }) {
                self.load_listening(cx);
            }
        }
    }

    // every room read again, in place of a read under way
    pub(crate) fn reread_channels(&mut self, cx: &mut Context<Self>) {
        self.rereading_channels = None;
        self.moved_channels.every();
        self.reread_moved(cx);
    }

    // the rooms that moved, read one read at a time: what moves during a read is the next one's
    fn reread_moved(&mut self, cx: &mut Context<Self>) {
        if self.rereading_channels.is_some() {
            return;
        }
        let held = self.channels.ready().is_some();
        let Some(moved) = self.moved_channels.take(held) else {
            return;
        };
        let read = crate::notifier::read_list(cx.host(), moved.clone());
        self.rereading_channels = Some(cx.land(read, move |chat, read, cx| {
            chat.rereading_channels = None;
            match read {
                Ok(read) => {
                    let held = chat.channels.ready().cloned();
                    chat.channels_landed(crate::notifier::landed(held, read), cx);
                    chat.reread_moved(cx);
                }
                Err(refusal) => {
                    chat.moved_channels.put_back(moved);
                    cx.log_refused("the channel list", &refusal);
                }
            }
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
        let first = self.screenful();
        let Some(room) = &mut self.room else { return };
        let id = room.id.clone();
        let landed = room.landed;
        let written = change.map(|change| chat::view::messages_written(&change.keys, &id));
        let mut catch_up = None;
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
            // a block: what it wrote, read again alone
            Some(Reread::Tail) if written.is_some() => catch_up = written.clone(),
            Some(Reread::Tail) => {
                let shown = room
                    .messages
                    .ready()
                    .map_or(first, |rows| rows.len().max(first));
                let rows = queries::roots(cx.host(), id.clone(), viewer.clone(), None, shown);
                room.rereading_all = true;
                room.rereading = Some(cx.land(rows, |chat, rows, cx| {
                    let room = room_of(chat);
                    room.rereading_all = false;
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
        if touched(&ask::Drafts {
            channel_id: id.clone(),
        }) {
            let answering = queries::answering(cx.host(), id.clone());
            cx.load(self, answering, |chat| &mut room_of(chat).answering);
        }
        let busy_moved = self.my_account().is_some_and(|me| {
            touched(&ask::Busy {
                channel_id: id.clone(),
                thread: None,
                for_account: me,
            })
        });
        if busy_moved {
            self.read_foot_busy(cx);
            self.read_thread_busy(cx);
        }
        if let Some(written) = catch_up {
            self.catch_up(written, cx);
        }
        let Some(thread) = self.room.as_ref().and_then(|room| room.thread.as_ref()) else {
            return;
        };
        let root = thread.root;
        // a reply added, a landing's block or a reopened link: the replies;
        // a block's other rows of it are the catch-up's
        let replied = match (&written, landed) {
            (Some(written), false) => written.replies.iter().any(|(at, _)| *at == root),
            _ => true,
        };
        let asked = ask::Thread {
            channel_id: id,
            root_seq: root,
            viewer,
            page,
        };
        if replied && touched(&asked) {
            self.reread_thread(written.is_some(), cx);
        }
    }

    fn reread_thread(&mut self, newer: bool, cx: &mut Context<Self>) {
        let viewer = self.viewer();
        let Some(room) = &mut self.room else { return };
        let id = room.id.clone();
        let Some(thread) = &mut room.thread else {
            return;
        };
        let root = thread.root;
        let newest = thread.replies.ready().and_then(|replies| replies.last());
        let newest = newest.map(|reply| reply.seq).filter(|_| newer);
        let after = newest.map(|seq| chat::replies_after(&id, root, seq));
        let replies = queries::thread(cx.host(), id, root, viewer, after);
        thread.rereading = Some(cx.land(replies, move |chat, replies, cx| {
            let room = room_of(chat);
            let Some(thread) = room.thread.as_mut().filter(|thread| thread.root == root) else {
                return;
            };
            match replies {
                Ok((fresh, next)) => {
                    let seqs = reacted(&fresh);
                    let replies: Vec<MsgRow> = match newest {
                        Some(newest) => {
                            let held = thread.replies.ready().into_iter().flatten().cloned();
                            let since = fresh.into_iter().filter(|reply| reply.seq > newest);
                            held.chain(since).collect()
                        }
                        None => {
                            thread.earlier = false;
                            fresh
                        }
                    };
                    let moved = thread.replies.ready() != Some(&replies) || thread.next != next;
                    if moved {
                        thread.replies = Loadable::Ready(replies);
                        thread.has_more = next.is_some();
                        thread.next = next;
                    }
                    room.landed_some(moved, seqs, cx);
                }
                Err(refusal) => cx.log_refused("the thread", &refusal),
            }
        }));
    }

    // The rows of the shortest height one pane shows: the tallest pane placed this session, and
    // before any is placed the window's own height, which no pane outgrows. Every read of rows
    // is one; a list short of its pane reads the page before (`listing::edged`).
    pub(crate) fn screenful(&self) -> usize {
        let row = design::height::ROW as f32;
        let placed = self.layout.pane.as_ref().map_or(0., |pane| pane.height);
        let height = match placed.max(self.layout.tallest) {
            height if height >= row => height,
            _ => self.layout.viewport.1,
        };
        (height / row).ceil() as usize + 1
    }

    // What a block wrote of the open room, read again and nothing more: each
    // row it holds, in the room or its thread, that `written` names (a span
    // at a time), and the newest page when a root came past its newest row.
    // A read still out when the room is left is the kept room's to make.
    fn catch_up(&mut self, mut written: Written, cx: &mut Context<Self>) {
        let first = self.screenful();
        let viewer = self.viewer();
        let Some(room) = &mut self.room else { return };
        let wrote_here = written != Written::default();
        if wrote_here {
            written.merge(std::mem::take(&mut room.unread));
        }
        let replies = room
            .thread
            .as_ref()
            .and_then(|thread| thread.replies.ready());
        let held: BTreeSet<u64> = (room.messages.ready().into_iter().chain(replies))
            .flatten()
            .map(|row| row.seq)
            .collect();
        let newest = room.messages.ready().and_then(|rows| rows.last());
        let newest = newest.map_or(0, |row| row.seq);
        let added: BTreeSet<u64> = written.replies.iter().map(|(_, reply)| *reply).collect();
        let past = |seq: &u64| *seq > newest && !added.contains(seq);
        let newer = written.roots || written.seqs.iter().any(past);
        let again: Vec<u64> = written.seqs.intersection(&held).copied().collect();
        room.unsettled.merge(written);
        let id = room.id.clone();
        if newer {
            room.reading += 1;
            let rows = queries::roots(cx.host(), id.clone(), viewer.clone(), None, first);
            let room_id = id.clone();
            cx.land(rows, move |chat, rows, cx| {
                let Some(room) = chat.room.as_mut().filter(|room| room.id == room_id) else {
                    return;
                };
                room.settled();
                match rows {
                    Ok((fresh, has_older)) => room.tail_landed(fresh, has_older, cx),
                    Err(refusal) => {
                        room.unread.roots = true;
                        cx.log_refused("the room", &refusal)
                    }
                }
            })
            .detach();
        }
        for span in spans(&again) {
            room.reading += 1;
            let rows = queries::span(cx.host(), id.clone(), span, viewer.clone());
            let room_id = id.clone();
            cx.land(rows, move |chat, rows, cx| {
                let Some(room) = chat.room.as_mut().filter(|room| room.id == room_id) else {
                    return;
                };
                room.settled();
                match rows {
                    Ok(fresh) => room.rows_landed(fresh, cx),
                    Err(refusal) => {
                        room.unread.seqs.extend(span.0..=span.1);
                        cx.log_refused("the room", &refusal)
                    }
                }
            })
            .detach();
        }
    }

    // The rooms a block posted in, at its height: what `#` offers first.
    fn active_heard(&mut self, change: Option<&Change>) {
        let Some(change) = change else { return };
        for info in self.channels.ready().into_iter().flatten() {
            let head = chat::tables::HEADS.key(&info.channel.id);
            if change.keys.contains(&head) {
                self.active.insert(info.channel.id.clone(), change.height);
            }
        }
    }

    // What a block wrote to the rooms kept this session, kept with them for
    // when they open; a reopened link (`None`) lets them all go.
    fn visited_heard(&mut self, change: Option<&Change>) {
        let Some(change) = change else {
            self.visited.clear();
            return;
        };
        for (id, visited) in &mut self.visited {
            let written = chat::view::messages_written(&change.keys, id);
            visited.written.merge(written);
            let roster = chat::tables::MEMBERS.key(id);
            visited.members_moved |= change.keys.iter().any(|key| key.starts_with(&roster));
        }
    }

    pub(crate) fn keep_room(&mut self) {
        let thread_open = self.keep_thread();
        let page = self.screenful();
        let anchor = self.anchor(Pane::Timeline);
        let Some(room) = self.room.take() else { return };
        let mut rows = match room.messages {
            _ if room.rereading_all => return,
            Loadable::Ready(_) if room.landed && anchor.is_none() => return,
            Loadable::Ready(_) if room.landed => Vec::new(),
            Loadable::Ready(rows) => rows,
            _ => return,
        };
        let over = rows.len().saturating_sub(page);
        rows.drain(..over);
        let thread = room.kept_thread;
        let replies = thread.iter().flat_map(|thread| &thread.replies);
        let kept: BTreeSet<u64> = rows.iter().chain(replies).map(|row| row.seq).collect();
        let mut reactors = room.reactors;
        reactors.retain(|seq, _| kept.contains(seq));
        let (members, members_moved) = match room.members {
            Loadable::Ready(members) => (members, false),
            _ => (Vec::new(), true),
        };
        let mut written = match room.reading {
            0 => Written::default(),
            _ => room.unsettled,
        };
        written.merge(room.unread);
        let visited = Visited {
            rows,
            has_older: room.has_older || over > 0,
            members,
            members_moved,
            reactors,
            thread,
            thread_open,
            anchor,
            written,
        };
        self.visited.insert(room.id, visited);
    }

    fn keep_thread(&mut self) -> bool {
        let page = self.screenful();
        let anchor = self.anchor(Pane::Thread);
        let Some(room) = &mut self.room else {
            return false;
        };
        let Some(thread) = room.thread.take() else {
            return false;
        };
        let Loadable::Ready(mut replies) = thread.replies else {
            return false;
        };
        let over = replies.len().saturating_sub(page);
        replies.drain(..over);
        room.kept_thread = Some(KeptThread {
            root: thread.root,
            replies,
            earlier: thread.earlier || over > 0,
            has_more: thread.has_more,
            next: thread.next,
            anchor,
        });
        true
    }

    fn anchor(&self, pane: Pane) -> Option<Anchor> {
        let drawn = self.drawn(pane).borrow();
        let (_, state) = drawn.list.as_ref()?;
        if state.is_following_tail() {
            return None;
        }
        let top = state.logical_scroll_top();
        let messages = &drawn.folded.as_ref()?.messages;
        let mut below = drawn.keys.get(top.item_ix..)?.iter().enumerate();
        let (past, message) = below.find_map(|(past, key)| {
            let message = messages.iter().find(|message| &message.id == key)?;
            Some((past, message))
        })?;
        let offset = match past {
            0 => f32::from(top.offset_in_item),
            _ => 0.,
        };
        Some(Anchor {
            id: message.id.clone(),
            seq: message.seq,
            offset,
        })
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
        let page = self.screenful();
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
        let older = queries::roots(cx.host(), id.clone(), viewer, Some(below), page);
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
                    let seqs = reacted(&older);
                    room.has_older = has_older;
                    room.older_failed = None;
                    if let Some(rows) = room.messages.ready_mut() {
                        let mut all = older;
                        all.append(rows);
                        *rows = all;
                    }
                    room.settle();
                    room.read_reactors_of(seqs, cx);
                }
                Err(refusal) => room.older_failed = Some(design::sentence(&refusal)),
            }
        })
        .detach();
    }

    fn page_landed(&mut self, side: Side, cx: &mut Context<Self>) {
        let viewer = self.viewer();
        let screenful = self.screenful();
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
        let page = queries::beside(cx.host(), id.clone(), at, side, viewer, screenful);
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
                // foot. The room keeps them out and reads on.
                if page.iter().all(|row| row.thread.is_some()) {
                    room.read_to = last;
                    chat.page_landed(side, cx);
                    return;
                }
            }
            let seqs = reacted(&page);
            room.extend(side, page);
            room.read_reactors_of(seqs, cx);
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
        let at_foot = event.visible_range.end >= event.count;
        match pane {
            Pane::Timeline => {
                let Some(room) = &mut self.room else { return };
                let at_tail = event.is_following_tail || event.visible_range.end >= event.count;
                if room.at_tail != at_tail {
                    room.at_tail = at_tail;
                    cx.notify();
                }
                if room.older_failed.take().is_some() {
                    cx.notify();
                }
                if room.landed && at_foot {
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
        self.load_thread(root, false, 0, cx);
    }

    /// The thread under `root`, its reply field taking the keys once it
    /// can be typed in.
    pub(crate) fn reply_in_thread(&mut self, root: u64, cx: &mut Context<Self>) {
        self.load_thread(root, true, 0, cx);
    }

    // `target`: the reply a link opened the thread on, 0 for none
    fn load_thread(&mut self, root: u64, reply: bool, target: u64, cx: &mut Context<Self>) {
        let viewer = self.viewer();
        self.details = None;
        self.menu = None;
        self.copy = None;
        self.keep_thread();
        let fresh = !reply && target == 0;
        let kept = self
            .room
            .as_mut()
            .and_then(|room| room.kept_thread.take_if(|kept| fresh && kept.root == root));
        if let Some(kept) = kept {
            self.restore_thread(kept, true, cx);
            return;
        }
        let Some(room) = &mut self.room else { return };
        let id = room.id.clone();
        let thread = room.thread.get_or_insert_default();
        *thread = Thread {
            root,
            target,
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
                        let seqs = reacted(&replies);
                        thread.replies = Loadable::Ready(replies);
                        thread.has_more = next.is_some();
                        thread.next = next;
                        room_of(chat).settle();
                        room_of(chat).read_reactors_of(seqs, cx);
                        chat.thread_drawn.borrow_mut().follow |= target == 0;
                        chat.seek(cx);
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

    fn restore_thread(&mut self, kept: KeptThread, newer: bool, cx: &mut Context<Self>) {
        let Some(room) = &mut self.room else { return };
        let root = kept.root;
        let above = kept.anchor.as_ref().is_some_and(|anchor| {
            anchor.seq != root
                && !kept
                    .replies
                    .iter()
                    .any(|reply| reply.message_id == anchor.id)
        });
        room.thread = Some(Thread {
            root,
            replies: Loadable::Ready(kept.replies),
            earlier: kept.earlier,
            has_more: kept.has_more,
            next: kept.next,
            ..Thread::default()
        });
        room.settle();
        *self.thread_drawn.borrow_mut() = Drawn {
            follow: kept.anchor.is_none(),
            restore: kept.anchor.map(|anchor| (anchor.id, anchor.offset)),
            ..Drawn::default()
        };
        if above {
            self.load_earlier_replies(cx);
        } else if newer {
            self.reread_thread(true, cx);
        }
    }

    pub(crate) fn before_first(&self, pane: Pane, at: usize) -> bool {
        let Some(room) = &self.room else { return false };
        match pane {
            Pane::Timeline => {
                at == 0 && room.has_older && !room.older_loading && room.older_failed.is_none()
            }
            Pane::Thread => {
                let thread = room.thread.as_ref();
                at == 1 && thread.is_some_and(|thread| thread.earlier && thread.paging.is_none())
            }
        }
    }

    pub(crate) fn reached_first(&mut self, pane: Pane, cx: &mut Context<Self>) {
        match pane {
            Pane::Timeline => self.load_older(cx),
            Pane::Thread => self.load_earlier_replies(cx),
        }
    }

    fn load_earlier_replies(&mut self, cx: &mut Context<Self>) {
        let viewer = self.viewer();
        let Some(room) = &mut self.room else { return };
        let id = room.id.clone();
        let Some(thread) = &mut room.thread else {
            return;
        };
        let readable = thread.earlier && thread.paging.is_none();
        if !readable {
            return;
        }
        let every = queries::thread(cx.host(), id, thread.root, viewer, None);
        thread.paging = Some(cx.land(every, move |chat, result, cx| {
            cx.notify();
            let Some(thread) = chat.room.as_mut().and_then(|r| r.thread.as_mut()) else {
                return;
            };
            thread.paging = None;
            match result {
                Ok((mut every, next)) => {
                    let seqs = reacted(&every);
                    let newest = every.last().map_or(0, |reply| reply.seq);
                    if next.is_none() {
                        let shown = thread.replies.ready().into_iter().flatten();
                        every.extend(shown.filter(|reply| reply.seq > newest).cloned());
                    }
                    thread.replies = Loadable::Ready(every);
                    thread.earlier = false;
                    thread.has_more = next.is_some();
                    thread.next = next;
                    room_of(chat).settle();
                    room_of(chat).read_reactors_of(seqs, cx);
                }
                Err(refusal) => {
                    chat.notice =
                        format!("Couldn’t read this thread: {}", design::sentence(&refusal))
                }
            }
        }));
    }

    pub(crate) fn load_more_replies(&mut self, cx: &mut Context<Self>) {
        let viewer = self.viewer();
        let Some(room) = &mut self.room else { return };
        let id = room.id.clone();
        let Some(thread) = &mut room.thread else {
            return;
        };
        let readable = thread.has_more && thread.paging.is_none();
        if !readable {
            return;
        }
        let after = thread.next.clone();
        let more = queries::thread(cx.host(), id, thread.root, viewer, after);
        thread.paging = Some(cx.land(more, move |chat, result, cx| {
            cx.notify();
            let Some(thread) = chat.room.as_mut().and_then(|r| r.thread.as_mut()) else {
                return;
            };
            thread.paging = None;
            match result {
                Ok((more, next)) => {
                    let seeking = !thread.reached();
                    let seqs = reacted(&more);
                    thread.has_more = next.is_some();
                    thread.next = next;
                    if let Some(rows) = thread.replies.ready_mut() {
                        rows.extend(more);
                    }
                    room_of(chat).settle();
                    room_of(chat).read_reactors_of(seqs, cx);
                    if seeking {
                        chat.seek(cx);
                    }
                }
                Err(refusal) => {
                    chat.notice =
                        format!("Couldn’t read this thread: {}", design::sentence(&refusal))
                }
            }
        }));
    }

    // a thread opened on a reply reads its pages until the reply is in,
    // then its list opens afresh, on the reply
    fn seek(&mut self, cx: &mut Context<Self>) {
        let Some(thread) = self.room.as_ref().and_then(|room| room.thread.as_ref()) else {
            return;
        };
        if thread.target == 0 {
            return;
        }
        if thread.reached() {
            *self.thread_drawn.borrow_mut() = Drawn::default();
        } else if thread.has_more {
            self.load_more_replies(cx);
        }
    }

    pub(crate) fn close_thread(&mut self) {
        self.keep_thread();
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
        let mut own = Vec::new();
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
            if let Some(&was) = self.reads.posted.get(&info.channel.id)
                && info.head_seq > was
            {
                self.reads.posted.remove(&info.channel.id);
                if info.head_seq > *cursor {
                    own.push((info.channel.id.clone(), *cursor, info.head_seq));
                }
            }
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
        for (room, from, head) in own {
            self.read_own(room, from, head, cx);
        }
    }

    fn read_own(&mut self, room: String, from: u64, head: u64, cx: &mut Context<Self>) {
        let Some(me) = self.me() else { return };
        let rows = queries::span(cx.host(), room.clone(), (from + 1, head), self.viewer());
        cx.land(rows, move |chat, rows, cx| {
            let rows = match rows {
                Ok(rows) => rows,
                Err(refusal) => return cx.log_refused("the reader's own posts", &refusal),
            };
            let Some(cursor) = chat.reads.cursors.get_mut(&room) else {
                return;
            };
            let was = *cursor;
            let mine = |seq: u64| rows.iter().any(|row| row.seq == seq && row.author == me);
            while *cursor < head && mine(*cursor + 1) {
                *cursor += 1;
            }
            if *cursor != was {
                chat.save_reads(cx);
                cx.notify();
            }
        })
        .detach();
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

// The root of the reply at `seq` among `rows`, and whether `rows` hold it.
fn reply_root(rows: &[MsgRow], seq: u64) -> Option<(u64, bool)> {
    let root = rows.iter().find(|row| row.seq == seq)?.thread?;
    Some((root, rows.iter().any(|row| row.seq == root)))
}

impl Thread {
    // the reply a link opened the thread on is read, or none was named
    fn reached(&self) -> bool {
        let target = self.target;
        target == 0
            || self
                .replies
                .ready()
                .is_some_and(|rows| rows.iter().any(|row| row.seq >= target))
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
            }
            Side::Newer => rows.extend(page),
        }
        self.settle();
    }

    // A catch-up's read landed: once none is out, nothing is left to read.
    fn settled(&mut self) {
        self.reading = self.reading.saturating_sub(1);
        if self.reading == 0 {
            self.unsettled = Written::default();
        }
    }

    // The newest page landed over the rows held: the rows from its oldest on
    // are its, the older ones stay. A page that does not reach the rows held
    // starts the room over from it; what lies between pages in on scroll.
    fn tail_landed(&mut self, fresh: Vec<MsgRow>, has_older: bool, cx: &mut Context<Chat>) {
        let Some(rows) = self.messages.ready_mut() else {
            return;
        };
        let seqs = reacted(&fresh);
        let from = fresh.first().map_or(u64::MAX, |row| row.seq);
        let reaches = !has_older || rows.last().is_some_and(|row| row.seq >= from);
        let mut merged: Vec<MsgRow> = match reaches {
            true => rows.iter().filter(|row| row.seq < from).cloned().collect(),
            false => Vec::new(),
        };
        merged.extend(fresh);
        let moved = *rows != merged;
        if moved {
            *rows = merged;
        }
        let older = match (reaches, has_older) {
            (true, true) => self.has_older,
            (true, false) => false,
            (false, older) => older,
        };
        self.has_older = older;
        self.landed_some(moved, seqs, cx);
    }

    // Rows read again by seq: each one held, in the room or its thread,
    // takes its fresh copy.
    fn rows_landed(&mut self, fresh: Vec<MsgRow>, cx: &mut Context<Chat>) {
        let mut moved = false;
        let replies = self
            .thread
            .as_mut()
            .and_then(|thread| thread.replies.ready_mut());
        for rows in self.messages.ready_mut().into_iter().chain(replies) {
            for row in rows.iter_mut() {
                if let Some(new) = fresh.iter().find(|new| new.seq == row.seq)
                    && *row != *new
                {
                    *row = new.clone();
                    moved = true;
                }
            }
        }
        self.landed_some(moved, reacted(&fresh), cx);
    }

    // Some rows landed: drops the sends they show, draws if they `moved` or
    // a send left, and reads who reacted to `seqs` alone.
    fn landed_some(&mut self, moved: bool, seqs: Vec<u64>, cx: &mut Context<Chat>) {
        let pending = self.pending.len();
        self.settle();
        if moved || self.pending.len() != pending {
            cx.notify();
        }
        self.read_reactors_of(seqs, cx);
    }

    // Who chose the reactions on `seqs` alone, put over what the room holds
    // of them: the rows that landed, not the whole window again.
    pub(crate) fn read_reactors_of(&mut self, seqs: Vec<u64>, cx: &mut Context<Chat>) {
        if seqs.is_empty() {
            return;
        }
        let id = self.id.clone();
        let read = queries::reactors(cx.host(), id.clone(), seqs.clone());
        cx.land(read, move |chat, read, cx| {
            let Some(room) = chat.room.as_mut().filter(|room| room.id == id) else {
                return;
            };
            match read {
                Ok(read) => {
                    let mut reactors = room.reactors.clone();
                    for seq in &seqs {
                        reactors.remove(seq);
                    }
                    reactors.extend(read);
                    if reactors != room.reactors {
                        room.reactors = reactors;
                        cx.notify();
                    }
                }
                Err(refusal) => cx.log_refused("who reacted", &refusal),
            }
        })
        .detach();
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

    // Who chose each reaction on the rows shown, room and thread: read
    // again each time rows land, kept until the next read lands.
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

// The seqs of `rows` that carry a reaction: who chose them is read.
fn reacted(rows: &[MsgRow]) -> Vec<u64> {
    let reacted = rows.iter().filter(|row| !row.reactions.is_empty());
    reacted.map(|row| row.seq).collect()
}

// `seqs`, ascending, in runs of neighbours: one read each, of those rows
// alone, so a block that wrote one row reads one row.
fn spans(seqs: &[u64]) -> Vec<(u64, u64)> {
    let mut spans: Vec<(u64, u64)> = Vec::new();
    for &seq in seqs {
        match spans.last_mut() {
            Some((_, last)) if seq == *last + 1 => *last = seq,
            _ => spans.push((seq, seq)),
        }
    }
    spans
}

pub(crate) fn room_of(chat: &mut Chat) -> &mut Room {
    chat.room.get_or_insert_default()
}
