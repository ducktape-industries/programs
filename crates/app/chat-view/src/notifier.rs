//! What chat hands the host's notification centre, decided here and
//! nowhere else: a message that mentions the reader, or any message in a
//! direct room they are in, from someone else, landing in a room they are
//! not looking at. Its title is the sender (`<sender> mentioned you` for a
//! mention), its body the message, its tag the room (`@peer`, `#channel`),
//! its link the message. The tab badge counts such messages in rooms the
//! reader has not read.
//!
//! This is chat's NOTIFIER: the host runs it once per network, whether or
//! not a chat pane is open, and the panes post nothing. The read cursors
//! are the panes' (`reads/<signer>`, kept by `kept.rs`); a pane that reads
//! a room moves its cursor, the host tells the notifier (`store.changes`),
//! and the pane takes the room's notices down itself (`notify.seen`). The
//! room a pane shows on screen (`host.screen`) gets no notice and leaves
//! the badge.
//!
//! What it announced it keeps under `notified/<signer>`: the seq announced
//! up to in each room. A run that starts (a launch, a reconnect, a new
//! deployment) posts, once, what came for the reader past both that and
//! their cursor while nothing ran; a room it never saw is new, read from
//! its start. The very first run on a device has nothing kept: what is
//! there is history, not news.
use std::collections::{BTreeMap, BTreeSet};

use chat::view::Names;
use chat::{Block, ChannelInfo, Mark, MsgRow, PageRequest, Principal};
use identity::Identity;
use view::prelude::*;
use view::{Notifier, store};

use crate::links;
use crate::message::message_body;
use crate::names::dm_peer_of;

/// Where the panes keep the reader's read cursors, room by room.
pub(crate) fn reads_key(signer: &str) -> String {
    format!("reads/{signer}")
}

/// Where the notifier keeps what it announced, room by room.
fn notified_key(signer: &str) -> String {
    format!("notified/{signer}")
}

/// Rooms asked per page of the channel list.
const ROOMS: u64 = 64;

/// The most messages read back in one room at a time.
// ponytail: a room past this many unread since its cursor counts and
// announces only its newest READ_BACK; page back if a badge must count more.
const READ_BACK: u64 = 256;

/// The most rooms read at once. The host gives each turn of a notifier a
/// fixed fuel and ends one that runs out, and a turn lands every read
/// answered since the last: so a turn lands at most this many rooms of
/// READ_BACK messages, however many rooms a catch-up has.
const READS: usize = 4;

/// Chat's notifier: what it knows of the reader and the rooms, and what it
/// has counted and announced.
#[derive(Default)]
pub struct ChatNotices {
    session: Session,
    /// Moves with every new reader: what lands for an earlier one is dropped.
    run: u64,
    names: Option<Names>,
    /// The rooms and their heads, as the channel list last said.
    rooms: Option<Vec<ChannelInfo>>,
    /// The reader's cursors, as the panes keep them.
    reads: Option<BTreeMap<String, u64>>,
    /// The seq announced up to in each room; `None` until read off the
    /// store, and `Some(None)` when nothing was ever kept on this device.
    announced: Option<Option<BTreeMap<String, u64>>>,
    /// Per room, the messages meant for the reader past their cursor.
    counted: BTreeMap<String, Count>,
    /// The rooms a read of new messages is under way for.
    reading: BTreeSet<String>,
    /// The rooms a chat pane shows on screen now.
    screen: BTreeSet<String>,
    /// The tab badge last sent.
    badge: Option<i64>,
}

/// One room's count: the messages meant for the reader past `cursor`, up
/// to the seq `to`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Count {
    cursor: u64,
    to: u64,
    meant: i64,
}

impl Notifier for ChatNotices {
    const NAME: &'static str = "Chat";
    const CAPABILITIES: &'static [Capability] = &[
        Capability::Program,
        Capability::Host,
        Capability::Notify,
        Capability::Store,
    ];
    const TARGETS: &'static [&'static str] = &[chat::PROGRAM, identity::PROGRAM];

    fn new(cx: &mut Context<Self>) -> Self {
        cx.follow::<HostSession>((), |notices, session, cx| match session {
            Ok(session) => notices.session_changed(session, cx),
            Err(refusal) => log_refused(cx, "the session", &refusal),
        })
        .detach();
        // a block re-reads the rooms where it wrote to the list
        cx.follow::<Changes<chat::Chat>>((), |notices, change, cx| match change {
            Ok(change) => {
                let list = chat::ask::Channels {
                    page: PageRequest::default(),
                };
                if change.is_none_or(|change| change.touches::<chat::Chat, _>(&list)) {
                    notices.load_rooms(cx);
                }
            }
            Err(refusal) => log_refused(cx, "chat's live heads", &refusal),
        })
        .detach();
        // a name another signer claims replaces its "account N"
        cx.follow::<Changes<Identity>>((), |notices, head, cx| match head {
            Ok(_) => notices.load_names(cx),
            Err(refusal) => log_refused(cx, "identity's live heads", &refusal),
        })
        .detach();
        // a pane read a room: its cursor moved
        cx.follow::<StoreChanges>((), |notices, key, cx| match key {
            Ok(key) if Some(&key) == notices.reads_key().as_ref() => notices.load_reads(cx),
            Ok(_) => {}
            Err(refusal) => log_refused(cx, "the store's changes", &refusal),
        })
        .detach();
        cx.follow::<HostScreen>((), |notices, routes, cx| match routes {
            Ok(routes) => notices.screen_changed(routes, cx),
            Err(refusal) => log_refused(cx, "the screen", &refusal),
        })
        .detach();
        Self::default()
    }
}

fn log_refused(cx: &mut Context<ChatNotices>, what: &str, refusal: &Error) {
    cx.host()
        .log(format!("{}: {what} refused: {refusal}", ChatNotices::NAME));
}

impl ChatNotices {
    fn reads_key(&self) -> Option<String> {
        (!self.session.signer.is_empty()).then(|| reads_key(&self.session.signer))
    }

    fn session_changed(&mut self, next: Session, cx: &mut Context<Self>) {
        let reader = (&next.signer, &next.chain_id, next.account, next.connected);
        let was = (
            &self.session.signer,
            &self.session.chain_id,
            self.session.account,
            self.session.connected,
        );
        if reader == was {
            self.session = next;
            return;
        }
        // another reader, network or account: nothing known carries over
        *self = Self {
            session: next,
            run: self.run + 1,
            screen: std::mem::take(&mut self.screen),
            badge: self.badge,
            ..Self::default()
        };
        if !self.session.connected || self.session.signer.is_empty() {
            return self.settle_badge(cx);
        }
        self.load_names(cx);
        self.load_reads(cx);
        self.load_announced(cx);
        self.load_rooms(cx);
    }

    /// `land` once `work` is answered, unless the reader changed meanwhile.
    fn load<T: 'static>(
        &self,
        work: impl Future<Output = Result<T, Error>> + 'static,
        land: impl FnOnce(&mut Self, Result<T, Error>, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let run = self.run;
        cx.land(work, move |notices, answer, cx| {
            if notices.run == run {
                land(notices, answer, cx);
            }
        })
        .detach();
    }

    fn load_names(&mut self, cx: &mut Context<Self>) {
        let host = cx.host();
        let names = ::chat::view::roster(move |ask| host.query(ask));
        self.load(
            names,
            |notices, names, cx| {
                // a roster that will not load names everyone "account N":
                // the notices still go out
                let names = names.unwrap_or_else(|refusal| {
                    log_refused(cx, "the names", &refusal);
                    Names::default()
                });
                notices.names = Some(names);
                notices.settle(cx);
            },
            cx,
        );
    }

    fn load_reads(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.reads_key() else { return };
        let reads = store::get::<BTreeMap<String, u64>>(&cx.host(), &key);
        self.load(
            reads,
            |notices, reads, cx| {
                let reads = reads.unwrap_or_else(|refusal| {
                    log_refused(cx, "the read cursors", &refusal);
                    None
                });
                notices.reads = Some(reads.unwrap_or_default());
                notices.settle(cx);
            },
            cx,
        );
    }

    fn load_announced(&mut self, cx: &mut Context<Self>) {
        let kept =
            store::get::<BTreeMap<String, u64>>(&cx.host(), &notified_key(&self.session.signer));
        self.load(
            kept,
            |notices, kept, cx| {
                let kept = kept.unwrap_or_else(|refusal| {
                    log_refused(cx, "what was announced", &refusal);
                    None
                });
                notices.announced = Some(kept);
                notices.settle(cx);
            },
            cx,
        );
    }

    fn load_rooms(&mut self, cx: &mut Context<Self>) {
        let host = cx.host();
        let rooms = async move {
            host.query_all(|after| chat::ask::Channels {
                page: PageRequest {
                    after,
                    limit: Some(ROOMS),
                },
            })
            .await
        };
        self.load(
            rooms,
            |notices, rooms, cx| match rooms {
                Ok(rooms) => {
                    notices.rooms = Some(rooms);
                    notices.settle(cx);
                }
                Err(refusal) => log_refused(cx, "the rooms", &refusal),
            },
            cx,
        );
    }

    fn screen_changed(&mut self, routes: Vec<String>, cx: &mut Context<Self>) {
        self.screen = routes
            .iter()
            .filter_map(|route| links::route_target(route).map(|(room, _)| room))
            .collect();
        self.settle(cx);
    }

    /// Everything known: each room not on screen is read past what was
    /// counted, what is new for the reader is announced, and the badge is
    /// counted again.
    fn settle(&mut self, cx: &mut Context<Self>) {
        // no account, nothing is meant for the reader; the rest still
        // loading, the badge stays as it was until it is counted
        let Some(me) = self.session.account else {
            return self.settle_badge(cx);
        };
        let (Some(_), Some(rooms), Some(reads), Some(kept)) =
            (&self.names, &self.rooms, &self.reads, &self.announced)
        else {
            return;
        };
        // the first run on this device: every room as it stands is history
        let first = kept.is_none();
        let mut announced = kept.clone().unwrap_or_default();
        let mut due = Vec::new();
        for info in rooms {
            let (room, head) = (info.channel.id.clone(), info.head_seq);
            if self.screen.contains(&room) {
                // in front of the reader: read, and nothing in it is news
                self.counted.insert(
                    room.clone(),
                    Count {
                        cursor: head,
                        to: head,
                        meant: 0,
                    },
                );
                announced.insert(room, head);
                continue;
            }
            // a room never announced is new since the last run, read from
            // its start; on the first run, history
            let was = *announced
                .entry(room.clone())
                .or_insert(if first { head } else { 0 });
            // a room no pane keeps a cursor for is counted from where this
            // run first saw it
            let kept = reads.get(&room).copied();
            let start = kept.unwrap_or(was);
            let count = self.counted.entry(room.clone()).or_insert(Count {
                cursor: start,
                to: start,
                meant: 0,
            });
            if let Some(cursor) = kept
                && count.cursor != cursor
            {
                // the reader read the room: counted again from their cursor
                *count = Count {
                    cursor,
                    to: cursor,
                    meant: 0,
                };
            }
            let cursor = count.cursor;
            if head <= count.to || self.reading.contains(&room) {
                continue;
            }
            if self.reading.len() + due.len() >= READS {
                // read, and announced, once a read under way lands
                continue;
            }
            // announced up to the head now: a list landing before this
            // read reads past it, never this range again
            announced.insert(room.clone(), head);
            due.push((room, count.to, head, was.max(cursor)));
        }
        let moved = Some(&announced) != kept.as_ref();
        self.announced = Some(Some(announced));
        for (room, from, head, news) in due {
            self.read(room, from, head, news, me, cx);
        }
        if moved {
            self.save_announced(cx);
        }
        self.settle_badge(cx);
    }

    /// The messages in `room` past `from` up to `head`: those meant for the
    /// reader are counted, and those past `news` posted.
    fn read(
        &mut self,
        room: String,
        from: u64,
        head: u64,
        news: u64,
        me: u64,
        cx: &mut Context<Self>,
    ) {
        let fresh = (head - from).min(READ_BACK);
        let asked = cx.host().query(chat::ask::MessagesAround {
            channel_id: room.clone(),
            seq: head,
            viewer: Principal::writer(Some(me)).into_iter().collect(),
            page: PageRequest {
                after: None,
                limit: Some(fresh.saturating_mul(2)),
            },
        });
        self.reading.insert(room.clone());
        self.load(
            asked,
            move |notices, rows, cx| {
                notices.reading.remove(&room);
                let rows = match rows {
                    Ok(rows) => rows,
                    Err(refusal) => {
                        log_refused(cx, "the new messages", &refusal);
                        // announced only as far as before: read again at the
                        // next list
                        if let Some(Some(announced)) = &mut notices.announced {
                            announced.insert(room.clone(), news);
                        }
                        return;
                    }
                };
                let empty = Names::default();
                let names = notices.names.as_ref().unwrap_or(&empty);
                let name = notices
                    .rooms
                    .iter()
                    .flatten()
                    .find(|info| info.channel.id == room)
                    .map(|info| info.channel.name.clone())
                    .unwrap_or_default();
                let meant: Vec<&MsgRow> = rows
                    .iter()
                    .filter(|row| row.seq > from && row.seq <= head && meant_for(row, me))
                    .collect();
                // counted only onto the count it read past: one the reader
                // reset meanwhile is counted again at the next settle
                if let Some(count) = notices.counted.get_mut(&room)
                    && count.to == from
                {
                    count.to = head;
                    count.meant += meant.len() as i64;
                }
                let cursor = notices
                    .counted
                    .get(&room)
                    .map_or(news, |count| count.cursor);
                if !notices.screen.contains(&room) {
                    // a notice is made only for what is posted; the rest is
                    // only counted
                    for row in meant
                        .into_iter()
                        .filter(|row| row.seq > news && row.seq > cursor)
                    {
                        let post = notice(row, me, &name, &notices.session.chain_id, names);
                        cx.host().notify::<NotifyPost>(post);
                    }
                }
                notices.settle(cx);
            },
            cx,
        );
    }

    fn save_announced(&self, cx: &mut Context<Self>) {
        if let Some(Some(announced)) = &self.announced {
            store::set(
                &cx.host(),
                &notified_key(&self.session.signer),
                Some(announced),
            );
        }
    }

    /// The tab badge: messages meant for the reader in rooms still unread
    /// and not on screen.
    fn settle_badge(&mut self, cx: &mut Context<Self>) {
        let count: i64 = self
            .rooms
            .iter()
            .flatten()
            .filter(|info| !self.screen.contains(&info.channel.id))
            .filter_map(|info| self.counted.get(&info.channel.id))
            .map(|count| count.meant)
            .sum();
        if self.badge != Some(count) {
            self.badge = Some(count);
            cx.host().notify::<HostBadge>(count);
        }
    }
}

/// Whether `row` is meant for account `me`: it mentions them, or it is in
/// a direct room they are in. Their own never is.
fn meant_for(row: &MsgRow, me: u64) -> bool {
    !row.deleted
        && row.author != Principal::Account(me)
        && (dm_peer_of(me, &row.channel_id).is_some() || mentions(&row.blocks, me))
}

/// The notice a row [`meant_for`] account `me` makes.
fn notice(row: &MsgRow, me: u64, name: &str, chain: &str, names: &Names) -> Notification {
    let sender = names.author(&row.author);
    Notification {
        title: match dm_peer_of(me, &row.channel_id) {
            Some(_) => sender,
            None => format!("{sender} mentioned you"),
        },
        body: message_body(&row.blocks, names),
        tag: tag(me, &row.channel_id, name, names),
        // a notice without a link is one the centre cannot open, not a bad one
        link: links::channel_link(chain, &row.channel_id, Some(row.seq)).unwrap_or_default(),
        ..Notification::default()
    }
}

/// The tag a room's notices go under: `@peer` for a direct room, the
/// channel's `#name` otherwise.
pub(crate) fn tag(me: u64, room: &str, name: &str, names: &Names) -> String {
    match dm_peer_of(me, room) {
        Some(peer) => format!("@{}", names.author(&Principal::Account(peer))),
        None => format!("#{name}"),
    }
}

fn mentions(blocks: &[Block], me: u64) -> bool {
    blocks.iter().any(|block| match block {
        Block::Paragraph(spans) | Block::Quote(spans) => spans
            .iter()
            .any(|span| span.marks.contains(&Mark::Mention(Principal::Account(me)))),
        Block::Code { .. } | Block::Divider | Block::Attachment(_) => false,
    })
}

#[cfg(test)]
mod tests;
