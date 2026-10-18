//! What chat hands the host's notification centre, decided here and
//! nowhere else: a message from someone else, landing in a room the reader
//! is not looking at, that is meant for them ([`Why`]). It is meant for
//! them when it is
//!
//! - in a direct room of theirs;
//! - a mention of them, in any room;
//! - a reply in a thread they are in: they wrote its root, replied in it,
//!   or were mentioned in it;
//! - in a room they are in: they wrote in its timeline (a reply in a thread
//!   puts them in that thread alone), opened it, or hold a seat in it, for
//!   a members-only room.
//!
//! Its title is the sender and why (`<sender> mentioned you`), its body the
//! message, its tag the room (`@peer`, `#channel`), its link the message.
//! The tab badge counts such messages in rooms the reader has not read.
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
//!
//! What the reader is in it keeps under `following/<signer>` ([`Following`]),
//! learned from the messages it reads: the new ones, and once a run, a
//! room's newest [`READ_BACK`] and the root and replies of a thread a new
//! reply lands in. A room on screen is never read here, so one that leaves
//! the screen is looked through again.
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

use borsh::{BorshDeserialize, BorshSerialize};
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

/// Where the notifier keeps the rooms and threads the reader is in.
fn following_key(signer: &str) -> String {
    format!("following/{signer}")
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
    /// The rooms and threads the reader is in; `None` until read off the
    /// store.
    following: Option<Following>,
    /// What this run looked through for the reader's part, room by room:
    /// a room listed was read back, and the roots listed under it were
    /// looked up.
    looked: BTreeMap<String, BTreeSet<u64>>,
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

/// The rooms and threads the reader is in, each with the seq they came in
/// at: what comes after it is meant for them.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Default, PartialEq, Eq)]
struct Following {
    /// Per room, their first message in its timeline (0 for a seat in it).
    rooms: BTreeMap<String, u64>,
    /// Per room and thread root, the first message of the thread they
    /// wrote or were mentioned in.
    threads: BTreeMap<String, BTreeMap<u64, u64>>,
}

impl Following {
    /// What `row` says of reader `me`: writing it in the timeline puts them
    /// in its room; writing it or being mentioned in it puts them in its
    /// thread, if it is one (a reply, or a root with replies). Whether
    /// anything moved.
    fn learn(&mut self, row: &MsgRow, me: u64) -> bool {
        let mine = row.author == Principal::Account(me);
        let mut moved = false;
        if mine && row.thread.is_none() {
            moved |= earliest(self.rooms.entry(row.channel_id.clone()), row.seq);
        }
        let thread = row.thread.or((row.reply_count > 0).then_some(row.seq));
        if let Some(root) = thread
            && (mine || mentions(&row.blocks, me))
        {
            let threads = self.threads.entry(row.channel_id.clone()).or_default();
            moved |= earliest(threads.entry(root), row.seq);
        }
        moved
    }

    /// Whether `row` comes after the reader came into its room.
    fn in_room(&self, row: &MsgRow) -> bool {
        self.rooms
            .get(&row.channel_id)
            .is_some_and(|since| row.seq > *since)
    }

    /// Whether `row` replies in a thread after the reader came into it.
    fn in_thread(&self, row: &MsgRow) -> bool {
        row.thread.is_some_and(|root| {
            self.threads
                .get(&row.channel_id)
                .and_then(|threads| threads.get(&root))
                .is_some_and(|since| row.seq > *since)
        })
    }

    /// The roots of `room`'s threads the reader is in.
    fn roots(&self, room: &str) -> impl Iterator<Item = u64> + '_ {
        self.threads
            .get(room)
            .into_iter()
            .flat_map(|threads| threads.keys().copied())
    }
}

/// `seq` into `entry`, unless an earlier one is there: whether it moved.
fn earliest<K: Ord>(entry: Entry<'_, K, u64>, seq: u64) -> bool {
    match entry {
        Entry::Vacant(vacant) => {
            vacant.insert(seq);
            true
        }
        Entry::Occupied(mut kept) if *kept.get() > seq => {
            kept.insert(seq);
            true
        }
        Entry::Occupied(_) => false,
    }
}

/// What one read of a room brings back.
struct Read {
    /// The room's newest messages.
    rows: Vec<MsgRow>,
    /// Each thread looked up: its root's seq, and its root and replies.
    threads: Vec<(u64, Vec<MsgRow>)>,
    /// Whether the reader sits in the room as a member.
    seated: bool,
}

/// What one read of a room asks the node.
struct ReadPlan {
    room: String,
    /// The new messages are past `from`, up to `head`.
    from: u64,
    head: u64,
    /// How many messages before `head` are read.
    back: u64,
    /// Whether to ask if the reader sits in the room.
    seat: bool,
    /// The roots of the room's threads already known or looked up.
    known: BTreeSet<u64>,
    reader: Principal,
}

impl ReadPlan {
    /// The room's newest messages, then each thread a new reply lands in
    /// that is not known (its root and replies say whether the reader is in
    /// it), then the reader's seat, if asked.
    async fn run(self, host: Host) -> Result<Read, Error> {
        let ReadPlan {
            room,
            from,
            head,
            back,
            seat,
            known,
            reader,
        } = self;
        let rows = host
            .query(chat::ask::MessagesAround {
                channel_id: room.clone(),
                seq: head,
                viewer: vec![reader.clone()],
                page: PageRequest {
                    after: None,
                    limit: Some(back.saturating_mul(2)),
                },
            })
            .await?;
        let roots: BTreeSet<u64> = rows
            .iter()
            .filter(|row| row.seq > from && row.seq <= head)
            .filter_map(|row| row.thread)
            .filter(|root| !known.contains(root))
            .collect();
        let mut threads = Vec::new();
        for root_seq in roots {
            // ponytail: a thread's first READ_BACK replies only; page on if
            // the reader's part sits past them
            let (root, replies) = host
                .query(chat::ask::Thread {
                    channel_id: room.clone(),
                    root_seq,
                    viewer: Vec::new(),
                    page: PageRequest {
                        after: None,
                        limit: Some(READ_BACK),
                    },
                })
                .await?;
            threads.push((root_seq, root.into_iter().chain(replies.items).collect()));
        }
        let seated = seat
            && host
                .query_all(|after| chat::ask::Members {
                    channel_id: room.clone(),
                    page: PageRequest { after, limit: None },
                })
                .await?
                .iter()
                .any(|member| member.principal == reader);
        Ok(Read {
            rows,
            threads,
            seated,
        })
    }
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
        self.load_following(cx);
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

    fn load_following(&mut self, cx: &mut Context<Self>) {
        let kept = store::get::<Following>(&cx.host(), &following_key(&self.session.signer));
        self.load(
            kept,
            |notices, kept, cx| {
                // nothing kept, or nothing readable: learned again from the
                // rooms as they are read
                let kept = kept.unwrap_or_else(|refusal| {
                    log_refused(cx, "what the reader is in", &refusal);
                    None
                });
                notices.following = Some(kept.unwrap_or_default());
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
        let screen: BTreeSet<String> = routes
            .iter()
            .filter_map(|route| links::route_target(route).map(|(room, _)| room))
            .collect();
        // what the reader wrote in a room on screen was never read here: one
        // that leaves the screen is looked through again
        for room in self.screen.difference(&screen) {
            self.looked.remove(room);
        }
        self.screen = screen;
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
        let (Some(_), Some(rooms), Some(reads), Some(kept), Some(_)) = (
            &self.names,
            &self.rooms,
            &self.reads,
            &self.announced,
            &self.following,
        ) else {
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

    /// The messages in `room` past `from` up to `head`: what they say of the
    /// reader is learned, those meant for them are counted, and those past
    /// `news` posted.
    fn read(
        &mut self,
        room: String,
        from: u64,
        head: u64,
        news: u64,
        me: u64,
        cx: &mut Context<Self>,
    ) {
        let following = self.following.as_ref();
        // once a run a room is read back: the reader's own messages in it
        // say whether they are in it
        // ponytail: what they wrote before the newest READ_BACK is not
        // seen; keep a per-author index in chat if that misses rooms
        let look = !self.looked.contains_key(&room);
        let reader = Principal::Account(me);
        let plan = ReadPlan {
            room: room.clone(),
            from,
            head,
            back: if look {
                READ_BACK
            } else {
                (head - from).min(READ_BACK)
            },
            // a members-only room they did not open asks whether they sit
            // in it
            seat: look
                && !following.is_some_and(|following| following.rooms.contains_key(&room))
                && self.rooms.iter().flatten().any(|info| {
                    info.channel.id == room
                        && info.channel.members_only()
                        && info.channel.owner != reader
                }),
            known: self
                .looked
                .entry(room.clone())
                .or_default()
                .iter()
                .copied()
                .chain(
                    following
                        .into_iter()
                        .flat_map(|following| following.roots(&room)),
                )
                .collect(),
            reader,
        };
        let asked = plan.run(cx.host());
        self.reading.insert(room.clone());
        self.load(
            asked,
            move |notices, read, cx| {
                notices.reading.remove(&room);
                let read = match read {
                    Ok(read) => read,
                    Err(refusal) => {
                        log_refused(cx, "the new messages", &refusal);
                        // announced only as far as before: read again, and
                        // looked through again, at the next list
                        if let Some(Some(announced)) = &mut notices.announced {
                            announced.insert(room.clone(), news);
                        }
                        if look {
                            notices.looked.remove(&room);
                        }
                        return;
                    }
                };
                notices.learn(&room, &read, me, cx);
                let empty = Names::default();
                let names = notices.names.as_ref().unwrap_or(&empty);
                let info = notices
                    .rooms
                    .iter()
                    .flatten()
                    .find(|info| info.channel.id == room);
                let name = info
                    .map(|info| info.channel.name.clone())
                    .unwrap_or_default();
                let opened = info.is_some_and(|info| info.channel.owner == Principal::Account(me));
                let empty = Following::default();
                let following = notices.following.as_ref().unwrap_or(&empty);
                let meant: Vec<(&MsgRow, Why)> = read
                    .rows
                    .iter()
                    .filter(|row| row.seq > from && row.seq <= head)
                    .filter_map(|row| Some((row, meant_for(row, me, opened, following)?)))
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
                    for (row, why) in meant
                        .into_iter()
                        .filter(|(row, _)| row.seq > news && row.seq > cursor)
                    {
                        let post = notice(row, why, me, &name, &notices.session.chain_id, names);
                        cx.host().notify::<NotifyPost>(post);
                    }
                }
                notices.settle(cx);
            },
            cx,
        );
    }

    /// What `read` of `room` says of the reader is kept: the rooms and
    /// threads they are in, and the threads it looked up.
    fn learn(&mut self, room: &str, read: &Read, me: u64, cx: &mut Context<Self>) {
        // a room that left the screen meanwhile is looked through again
        if let Some(looked) = self.looked.get_mut(room) {
            looked.extend(read.threads.iter().map(|(root, _)| *root));
        }
        let following = self.following.get_or_insert_default();
        let mut moved = read.seated && earliest(following.rooms.entry(room.to_owned()), 0);
        let threads = read.threads.iter().flat_map(|(_, rows)| rows);
        for row in read.rows.iter().chain(threads) {
            moved |= following.learn(row, me);
        }
        if moved {
            let key = following_key(&self.session.signer);
            store::set(&cx.host(), &key, Some(&*following));
        }
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

/// Why a message is meant for the reader, the first that holds of these:
/// what its notice's title says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Why {
    /// It is in a direct room of theirs.
    Direct,
    /// It mentions them.
    Mention,
    /// It replies in a thread they are in.
    Thread,
    /// It is in a room they are in.
    Room,
}

/// Why `row` is meant for account `me`, if it is; `opened` says whether
/// they opened its room. Their own never is.
fn meant_for(row: &MsgRow, me: u64, opened: bool, following: &Following) -> Option<Why> {
    if row.deleted || row.author == Principal::Account(me) {
        None
    } else if dm_peer_of(me, &row.channel_id).is_some() {
        Some(Why::Direct)
    } else if mentions(&row.blocks, me) {
        Some(Why::Mention)
    } else if following.in_thread(row) {
        Some(Why::Thread)
    } else if opened || following.in_room(row) {
        Some(Why::Room)
    } else {
        None
    }
}

/// The notice a row meant for account `me`, for `why`, makes.
fn notice(row: &MsgRow, why: Why, me: u64, name: &str, chain: &str, names: &Names) -> Notification {
    let sender = names.author(&row.author);
    let tag = tag(me, &row.channel_id, name, names);
    Notification {
        title: match why {
            Why::Direct => sender,
            Why::Mention => format!("{sender} mentioned you"),
            Why::Thread => format!("{sender} replied in a thread"),
            Why::Room => format!("{sender} in {tag}"),
        },
        body: message_body(&row.blocks, names),
        tag,
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
