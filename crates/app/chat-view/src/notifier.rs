//! Chat's notifier: the notices it hands the host's centre (`chat::notice::why` says what is meant
//! for the reader), the tab badge as chat counts it (`Query::Unread`), and what it announced, kept
//! under `notified/<signer>` so a run posts what came while none ran, once (README, "The notifier").
use std::collections::{BTreeMap, BTreeSet};

use chat::notice::Why;
use chat::view::Names;
use chat::{ChannelInfo, Followed, MsgRow, Notices, PageRequest, Principal};
use identity::Identity;
use view::prelude::*;
use view::{Notifier, store};

use crate::links;
use crate::message::message_body;
use crate::names::dm_peer_of;

pub(crate) fn reads_key(signer: &str) -> String {
    format!("reads/{signer}")
}

fn notified_key(signer: &str) -> String {
    format!("notified/{signer}")
}

/// Chat's notifier: what it knows of the reader and the rooms, and what it
/// has counted and announced.
#[derive(Default)]
pub struct ChatNotices {
    session: Session,
    run: u64,
    names: Option<Names>,
    reading_names: bool,
    names_again: bool,
    rooms: Option<Vec<ChannelInfo>>,
    moved: Moved,
    reading_rooms: bool,
    reads: Option<BTreeMap<String, u64>>,
    announced: Option<Option<BTreeMap<String, u64>>>,
    choices: Option<BTreeMap<String, Choice>>,
    counted: BTreeMap<String, Count>,
    reading: BTreeSet<String>,
    counting: BTreeSet<String>,
    screen: BTreeSet<String>,
    badge: Option<i64>,
}

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

    fn new(cx: &mut Context<Self>) -> Self {
        cx.follow::<HostSession>((), |notices, session, cx| match session {
            Ok(session) => notices.session_changed(session, cx),
            Err(refusal) => log_refused(cx, "the session", &refusal),
        })
        .detach();
        cx.follow::<Changes<chat::Chat>>((), |notices, change, cx| match change {
            Ok(change) => {
                let change = change.as_ref();
                notices.moved.heard(change);
                notices.load_rooms(cx);
                if notices.names.is_none() {
                    notices.load_names(cx);
                }
                if let Some(me) = notices.session.account
                    && change.is_none_or(|change| choices_moved(change, me))
                {
                    notices.load_choices(cx);
                }
            }
            Err(refusal) => log_refused(cx, "chat's live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<Identity>>((), |notices, head, cx| match head {
            Ok(_) => notices.load_names(cx),
            Err(refusal) => log_refused(cx, "identity's live heads", &refusal),
        })
        .detach();
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
        self.load_choices(cx);
        self.load_rooms(cx);
    }

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
        if self.reading_names {
            self.names_again = true;
            return;
        }
        self.reading_names = true;
        let host = cx.host();
        let names = ::chat::view::roster(move |ask| host.query(ask));
        self.load(
            names,
            |notices, names, cx| {
                notices.reading_names = false;
                match names {
                    Ok(names) => {
                        notices.names = Some(names);
                        notices.settle(cx);
                    }
                    Err(refusal) => log_refused(cx, "the names", &refusal),
                }
                if std::mem::take(&mut notices.names_again) {
                    notices.load_names(cx);
                }
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

    fn load_choices(&mut self, cx: &mut Context<Self>) {
        let Some(me) = self.session.account else {
            return;
        };
        self.load(
            choices(cx.host(), me),
            |notices, choices, cx| {
                let choices = choices.unwrap_or_else(|refusal| {
                    log_refused(cx, "the reader's choices", &refusal);
                    BTreeMap::new()
                });
                let was = notices.choices.take().unwrap_or_default();
                for room in was.keys().chain(choices.keys()) {
                    if was.get(room) != choices.get(room)
                        && let Some(count) = notices.counted.get_mut(room)
                    {
                        *count = Count {
                            to: count.cursor,
                            meant: 0,
                            ..*count
                        };
                    }
                }
                notices.choices = Some(choices);
                notices.settle(cx);
            },
            cx,
        );
    }

    fn load_rooms(&mut self, cx: &mut Context<Self>) {
        if self.reading_rooms {
            return;
        }
        let Some(moved) = self.moved.take(self.rooms.is_some()) else {
            return;
        };
        self.reading_rooms = true;
        let read = read_list(cx.host(), moved.clone());
        self.load(
            read,
            move |notices, read, cx| {
                notices.reading_rooms = false;
                match read {
                    Ok(read) => {
                        notices.rooms = Some(landed(notices.rooms.take(), read));
                        notices.settle(cx);
                        notices.load_rooms(cx);
                    }
                    Err(refusal) => {
                        notices.moved.put_back(moved);
                        log_refused(cx, "the rooms", &refusal);
                    }
                }
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

    fn settle(&mut self, cx: &mut Context<Self>) {
        let Some(me) = self.session.account else {
            return self.settle_badge(cx);
        };
        let (Some(_), Some(rooms), Some(reads), Some(kept), Some(_)) = (
            &self.names,
            &self.rooms,
            &self.reads,
            &self.announced,
            &self.choices,
        ) else {
            return;
        };
        let first = kept.is_none();
        let mut announced = kept.clone().unwrap_or_default();
        let mut due = Vec::new();
        let mut uncounted = Vec::new();
        for info in rooms {
            let (room, head) = (info.channel.id.clone(), info.head_seq);
            if self.screen.contains(&room) {
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
            let was = *announced
                .entry(room.clone())
                .or_insert(if first { head } else { 0 });
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
                *count = Count {
                    cursor,
                    to: cursor,
                    meant: 0,
                };
            }
            if head > count.to && !self.counting.contains(&room) {
                uncounted.push((room.clone(), count.to));
            }
            let news = was.max(count.cursor);
            if head <= news || self.reading.contains(&room) {
                continue;
            }
            announced.insert(room.clone(), head);
            due.push((room, news, head));
        }
        let moved = Some(&announced) != kept.as_ref();
        self.announced = Some(Some(announced));
        if !uncounted.is_empty() {
            self.count(uncounted, me, cx);
        }
        for (room, news, head) in due {
            self.read(room, news, head, me, cx);
        }
        if moved {
            self.save_announced(cx);
        }
        self.settle_badge(cx);
    }

    fn count(&mut self, rooms: Vec<(String, u64)>, me: u64, cx: &mut Context<Self>) {
        self.counting
            .extend(rooms.iter().map(|(room, _)| room.clone()));
        let host = cx.host();
        let asked = rooms.clone();
        let counts = async move {
            host.query(chat::ask::Unread {
                account: me,
                rooms: asked,
            })
            .await
        };
        self.load(
            counts,
            move |notices, counts, cx| {
                for (room, _) in &rooms {
                    notices.counting.remove(room);
                }
                let counts = match counts {
                    Ok(counts) => counts,
                    Err(refusal) => {
                        log_refused(cx, "the unread counts", &refusal);
                        return notices.settle_badge(cx);
                    }
                };
                let mut moved = false;
                for counted in counts {
                    let since = rooms.iter().find(|(room, _)| *room == counted.channel_id);
                    if let Some(count) = notices.counted.get_mut(&counted.channel_id)
                        && since.is_some_and(|(_, since)| *since == count.to)
                        && counted.to > count.to
                    {
                        count.to = counted.to;
                        count.meant += counted.meant as i64;
                        moved = true;
                    }
                }
                match moved {
                    true => notices.settle(cx),
                    false => notices.settle_badge(cx),
                }
            },
            cx,
        );
    }

    fn read(&mut self, room: String, from: u64, head: u64, me: u64, cx: &mut Context<Self>) {
        let fresh = head - from;
        let host = cx.host();
        let channel_id = room.clone();
        let asked = async move {
            let rows = host
                .query(chat::ask::MessagesAround {
                    channel_id: channel_id.clone(),
                    seq: head,
                    viewer: Principal::writer(Some(me)).into_iter().collect(),
                    page: PageRequest {
                        after: None,
                        limit: Some(fresh.saturating_mul(2)),
                    },
                })
                .await?;
            let roots: BTreeSet<u64> = rows
                .iter()
                .filter(|row| row.seq > from && row.seq <= head)
                .filter_map(|row| row.thread)
                .collect();
            let followed = host
                .query(chat::ask::Followed {
                    channel_id,
                    account: me,
                    roots: roots.into_iter().collect(),
                })
                .await;
            Ok((rows, followed))
        };
        self.reading.insert(room.clone());
        self.load(
            asked,
            move |notices, read, cx| {
                notices.reading.remove(&room);
                let (rows, followed) = match read {
                    Ok(read) => read,
                    Err(refusal) => {
                        log_refused(cx, "the new messages", &refusal);
                        if let Some(Some(announced)) = &mut notices.announced {
                            announced.insert(room.clone(), from);
                        }
                        return;
                    }
                };
                let followed = followed.unwrap_or_else(|refusal| {
                    log_refused(cx, "where the reader is", &refusal);
                    Followed::default()
                });
                let choice = notices
                    .choices
                    .iter()
                    .flatten()
                    .find_map(|(chosen, choice)| (*chosen == room).then_some(*choice))
                    .unwrap_or_default();
                let empty = Names::default();
                let names = notices.names.as_ref().unwrap_or(&empty);
                let name = notices
                    .rooms
                    .iter()
                    .flatten()
                    .find(|info| info.channel.id == room)
                    .map(|info| info.channel.name.clone())
                    .unwrap_or_default();
                let meant: Vec<(&MsgRow, Why)> = rows
                    .iter()
                    .filter(|row| row.seq > from && row.seq <= head)
                    .filter_map(|row| Some((row, notification_reason(row, me, &followed, choice)?)))
                    .collect();
                let cursor = notices
                    .counted
                    .get(&room)
                    .map_or(from, |count| count.cursor);
                if !notices.screen.contains(&room) {
                    for (row, why) in meant
                        .into_iter()
                        .filter(|(row, _)| row.seq > from && row.seq > cursor)
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

    fn save_announced(&self, cx: &mut Context<Self>) {
        if let Some(Some(announced)) = &self.announced {
            store::set(
                &cx.host(),
                &notified_key(&self.session.signer),
                Some(announced),
            );
        }
    }

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

fn notification_reason(row: &MsgRow, me: u64, followed: &Followed, choice: Choice) -> Option<Why> {
    let (listening, cut) = match choice {
        Choice::Every => (true, None),
        Choice::Default => (false, None),
        Choice::Cut(notices) => (false, Some(notices)),
    };
    chat::notice::why(row, me, followed, listening, cut)
}

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
        link: links::channel_link(chain, &row.channel_id, Some(row.seq)).unwrap_or_default(),
        ..Notification::default()
    }
}

pub(crate) fn tag(me: u64, room: &str, name: &str, names: &Names) -> String {
    match dm_peer_of(me, room) {
        Some(peer) => format!("@{}", names.author(&Principal::Account(peer))),
        None => format!("#{name}"),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Choice {
    Every,
    #[default]
    Default,
    Cut(Notices),
}

impl Choice {
    pub(crate) fn op(self, channel_id: String) -> chat::Op {
        match self {
            Choice::Every => chat::Op::Listen {
                channel_id,
                listening: true,
            },
            Choice::Default => chat::Op::SetNotices {
                channel_id,
                notices: None,
            },
            Choice::Cut(notices) => chat::Op::SetNotices {
                channel_id,
                notices: Some(notices),
            },
        }
    }

    pub(crate) fn op_for(self, accounts: Vec<u64>, channel_id: String) -> chat::Op {
        let (listening, notices) = match self {
            Choice::Every => (true, None),
            Choice::Default => (false, None),
            Choice::Cut(notices) => (false, Some(notices)),
        };
        chat::Op::ChooseForMany {
            accounts,
            channel_id,
            listening,
            notices,
        }
    }

    pub(crate) fn of(listening: bool, notices: Option<Notices>) -> Choice {
        match (listening, notices) {
            (true, _) => Choice::Every,
            (false, None) => Choice::Default,
            (false, Some(notices)) => Choice::Cut(notices),
        }
    }
}

pub(crate) async fn choices(host: Host, me: u64) -> Result<BTreeMap<String, Choice>, Error> {
    let page = |after| PageRequest { after, limit: None };
    let every = host
        .query_all(|after| chat::ask::Listening {
            account: me,
            page: page(after),
        })
        .await?;
    let cut = host
        .query_all(|after| chat::ask::Notices {
            account: me,
            page: page(after),
        })
        .await?;
    let every = every.into_iter().map(|room| (room, Choice::Every));
    let cut = cut
        .into_iter()
        .map(|(room, notices)| (room, Choice::Cut(notices)));
    Ok(every.chain(cut).collect())
}

pub(crate) fn choices_moved(change: &Change, me: u64) -> bool {
    let mine = [
        chat::tables::LISTENING.key(&me),
        chat::tables::NOTICES.key(&me),
    ];
    change
        .keys
        .iter()
        .any(|key| mine.iter().any(|head| key.starts_with(head)))
}

#[cfg(test)]
mod tests;

#[derive(Clone, Default)]
pub(crate) struct Moved {
    every: bool,
    rooms: BTreeSet<String>,
}

pub(crate) enum ListRead {
    Every(Vec<ChannelInfo>),
    Rooms(Vec<(String, Option<ChannelInfo>)>),
}

impl Moved {
    pub(crate) fn heard(&mut self, change: Option<&view::methods::Change>) {
        match change {
            Some(change) => self
                .rooms
                .extend(chat::view::channels_written(&change.keys)),
            None => self.every = true,
        }
    }

    pub(crate) fn every(&mut self) {
        self.every = true;
    }

    pub(crate) fn take(&mut self, held: bool) -> Option<Moved> {
        let moved = std::mem::take(self);
        let every = moved.every || !held;
        (every || !moved.rooms.is_empty()).then_some(Moved {
            every,
            rooms: moved.rooms,
        })
    }

    pub(crate) fn put_back(&mut self, moved: Moved) {
        self.every |= moved.every;
        self.rooms.extend(moved.rooms);
    }
}

pub(crate) async fn read_list(host: Host, moved: Moved) -> Result<ListRead, Error> {
    if moved.every {
        let every = host
            .query_all(|after| chat::ask::Channels {
                page: PageRequest { after, limit: None },
            })
            .await?;
        return Ok(ListRead::Every(every));
    }
    let mut rooms = Vec::new();
    for room in moved.rooms {
        let asked = chat::ask::Channel {
            channel_id: room.clone(),
        };
        let info = host.query(asked).await?;
        rooms.push((room, info));
    }
    Ok(ListRead::Rooms(rooms))
}

pub(crate) fn landed(held: Option<Vec<ChannelInfo>>, read: ListRead) -> Vec<ChannelInfo> {
    let rooms = match read {
        ListRead::Every(every) => return every,
        ListRead::Rooms(rooms) => rooms,
    };
    let mut list = held.unwrap_or_default();
    for (room, info) in rooms {
        let at = list.iter().position(|held| held.channel.id == room);
        match (at, info) {
            (Some(at), Some(info)) => list[at] = info,
            (Some(at), None) => {
                list.remove(at);
            }
            (None, Some(info)) => {
                let after = list.iter().position(|held| held.channel.id > room);
                list.insert(after.unwrap_or(list.len()), info);
            }
            (None, None) => {}
        }
    }
    list
}
