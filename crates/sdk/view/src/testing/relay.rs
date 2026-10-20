//! [`Relay`]: the node's live tracks for two or more views under test, each
//! in its own [`TestAppContext`], so what one publishes the others hear.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use program::live::Grant;

use crate::host::Error;
use crate::methods::{
    LiveAnnounce, LiveItem, LiveMode, LivePublish, LivePublishTrack, LiveSend, LiveSendObject,
    LiveStart, LiveSubscribe, LiveSubscribeTrack, LiveTrack, LiveTracks, Method, Publishing,
    Watching, refusal,
};
use crate::wire::{ElementIdWire, Event, Frame, Node, RemoteMedia, TrackState};

use super::{FakeHost, TestAppContext};

/// The node's relay for views under test: [`link`](Relay::link) each
/// view's [`TestAppContext`] and it answers their `live.*` requests as one
/// node would: a track a view publishes is announced to every
/// `live.tracks` of its room, an object one view sends reaches every view
/// subscribed to the track, the latest group is replayed to a late
/// subscriber, and a publisher hears its subscriber count. Its queues never
/// fill, so it never cuts a group (`Publishing::Cut`). The n-th view
/// linked acts as account n; the views opened in one [`TestAppContext`]
/// are windows of that one app, so a track one of them publishes another
/// is refused `duplicate_track`, as the app refuses it.
///
/// Grants are rows, as the room's program writes them
/// (`program::live::grant`), decided as the node decides them
/// ([`Grant::publishes`], [`Grant::subscribes`]): an account's own row,
/// else the room's `*` row, else nothing. Every room starts as if its
/// program wrote `*` = [`Grant::all`]; [`hold`](Relay::hold) takes every
/// row of a room away, [`grant`](Relay::grant) and
/// [`revoke`](Relay::revoke) write and remove one, each the block that does
/// it. An ask its row does not cover hears `Waiting` and waits for a block
/// that covers it; a request a block no longer covers ends `not_granted`,
/// and a room list whose row moved hears the tracks it gains and loses.
///
/// An item lands in a view's host as the frame that caused it is taken,
/// and the view reads it on its next tick: `a.simulate_click("send");
/// b.run_until_parked();` and `b` has it. The order is the frames' order,
/// so a run is the same every time.
///
/// A call's pieces ask as the app asks for them: a camera or microphone
/// piece that publishes is a `live.publish` of its track (a camera's in
/// stream mode, a microphone's in datagram mode) while its view's frames
/// place it, and a remote piece a `live.subscribe`; each hears where its
/// track stands as [`MediaState::track`](crate::MediaState::track) through
/// its `on_state`, when that changes, and no object crosses. A piece its
/// frame drops ends its track as a dropped `live.publish` does
/// (`track_ended` for its subscribers); a publishing piece whose track
/// ended publishes again on its view's next frame, and a remote piece
/// subscribes again when its track comes up, or, ended `not_granted`, on
/// its view's next frame, where it waits for a row as any ask does.
///
/// ```
/// # use view::testing::{Relay, TestAppContext};
/// let (a, b) = (TestAppContext::new(), TestAppContext::new());
/// let relay = Relay::new();
/// relay.link(&a).link(&b);
/// ```
#[derive(Clone, Default)]
pub struct Relay(Rc<RefCell<State>>);

#[derive(Default)]
struct State {
    hosts: Vec<FakeHost>,
    tracks: BTreeMap<(String, String), Published>,
    subscriptions: BTreeMap<(usize, u64), (String, String)>,
    watches: BTreeMap<(usize, u64), (String, Grant)>,
    rows: BTreeMap<String, BTreeMap<Option<u64>, Grant>>,
    waiting: Vec<Waiting>,
    pieces: Vec<Piece>,
    placed: u64,
}

const PIECES: u64 = 1 << 63;

struct Piece {
    link: usize,
    path: Vec<ElementIdWire>,
    id: u64,
    ask: PieceAsk,
}

#[derive(Clone, PartialEq)]
enum PieceAsk {
    Publish(LivePublish),
    Subscribe(LiveSubscribe),
}

impl PieceAsk {
    fn of(node: &Node) -> Option<Self> {
        let publish = |track: &LiveTrack, mode, priority| {
            PieceAsk::Publish(LivePublish {
                track: track.clone(),
                mode,
                priority,
            })
        };
        match node {
            Node::Camera {
                publish: Some(track),
                ..
            } => Some(publish(track, LiveMode::Stream, 128)),
            Node::Microphone {
                publish: Some(track),
                ..
            } => Some(publish(track, LiveMode::Datagram, 64)),
            Node::Remote { track, media, .. } => Some(PieceAsk::Subscribe(LiveSubscribe {
                track: track.clone(),
                from: match media {
                    RemoteMedia::Video { .. } => LiveStart::Latest,
                    RemoteMedia::Audio { .. } => LiveStart::Next,
                },
            })),
            _ => None,
        }
    }

    fn ask(&self) -> Ask {
        match self {
            PieceAsk::Publish(publish) => Ask::Publish(publish.clone(), Vec::new()),
            PieceAsk::Subscribe(subscribe) => Ask::Subscribe(subscribe.clone()),
        }
    }

    fn track(&self, item: &Result<Vec<u8>, Error>, done: bool) -> Option<TrackState> {
        let bytes = match item {
            Ok(bytes) => bytes,
            Err(refusal) => {
                return done.then(|| TrackState::Ended {
                    code: refusal.code.clone(),
                });
            }
        };
        match self {
            PieceAsk::Publish(_) => match LivePublishTrack::decode_reply(bytes).ok()? {
                Publishing::Waiting => Some(TrackState::Waiting),
                Publishing::Subscribers(subscribers) => Some(TrackState::Up { subscribers }),
                Publishing::Cut { .. } => None,
            },
            PieceAsk::Subscribe(_) => match LiveSubscribeTrack::decode_reply(bytes).ok()? {
                LiveItem::Waiting => Some(TrackState::Waiting),
                LiveItem::Opened { .. } => Some(TrackState::Up { subscribers: 0 }),
                LiveItem::Object { .. } | LiveItem::GroupEnd { .. } => None,
            },
        }
    }
}

struct Waiting {
    link: usize,
    id: u64,
    ask: Ask,
}

enum Ask {
    Publish(LivePublish, Vec<(u64, LiveSend)>),
    Subscribe(LiveSubscribe),
    Watch(String),
}

impl Ask {
    fn room(&self) -> &str {
        match self {
            Ask::Publish(publish, _) => &publish.track.room,
            Ask::Subscribe(subscribe) => &subscribe.track.room,
            Ask::Watch(room) => room,
        }
    }

    fn publishes(&self) -> bool {
        match self {
            Ask::Publish(..) => true,
            Ask::Subscribe(_) | Ask::Watch(_) => false,
        }
    }

    fn granted(&self, grant: &Grant, account: u64) -> bool {
        match self {
            Ask::Publish(publish, _) => grant.publishes(&publish.track.name, account),
            Ask::Subscribe(subscribe) => grant.subscribes(&subscribe.track.name, account),
            Ask::Watch(_) => grant.watches(),
        }
    }

    fn waits(&self, id: u64) -> Event {
        match self {
            Ask::Publish(..) => item::<LivePublishTrack>(id, Ok(Publishing::Waiting), false),
            Ask::Subscribe(_) => item::<LiveSubscribeTrack>(id, Ok(LiveItem::Waiting), false),
            Ask::Watch(_) => item::<LiveTracks>(id, Ok(Watching::Waiting), false),
        }
    }
}

struct Published {
    link: usize,
    request: u64,
    mode: LiveMode,
    group: Option<Group>,
    subscribers: Vec<Subscriber>,
}

impl Published {
    fn announced(&self, id: u64, name: &str, up: bool) -> Event {
        let announce = LiveAnnounce {
            name: name.to_owned(),
            by: State::account(self.link),
            mode: self.mode,
            up,
        };
        item::<LiveTracks>(id, Ok(Watching::Announce(announce)), false)
    }
}

struct Subscriber {
    link: usize,
    id: u64,
    passed: Option<u64>,
}

impl Subscriber {
    fn hears(&self, group: u64) -> bool {
        self.passed.is_none_or(|passed| group > passed)
    }
}

struct Group {
    number: u64,
    objects: Vec<(u32, Vec<u8>)>,
    closed: bool,
}

const REVOKED: &str = "the room's program took the grant back";

fn revoked() -> Error {
    Error::new(refusal::NOT_GRANTED, REVOKED)
}

fn item<M: Method>(id: u64, reply: Result<M::Reply, Error>, done: bool) -> Event {
    Event::Response {
        id,
        result: reply.map(|reply| M::encode_reply(&reply)),
        done,
    }
}

impl Relay {
    pub fn new() -> Self {
        Self::default()
    }

    /// Links `cx`'s view to the relay, as the account after the last one
    /// linked (the first is account 1).
    pub fn link(&self, cx: &TestAppContext) -> &Self {
        let host = cx.host();
        let link = {
            let mut state = self.0.borrow_mut();
            state.hosts.push(host.clone());
            state.hosts.len() - 1
        };
        let relay = self.clone();
        host.after_frame(move |_, frame, tree| relay.take(link, frame, tree));
        self
    }

    /// The block that takes every row of `room` away, as a room whose
    /// program has written none yet: each `live.publish`, `live.subscribe`
    /// and `live.tracks` asked in it hears `Waiting`, and waits for a
    /// [`grant`](Relay::grant) that covers it; what a view sends onto a
    /// waiting track goes out once the track is up. What the rows allowed
    /// before ends as [`revoke`](Relay::revoke) ends it.
    pub fn hold(&self, room: &str) -> &Self {
        self.0.borrow_mut().block(room, |rows| rows.clear());
        self
    }

    /// The block that writes `who`'s row in `room` (`None`: the `*` row),
    /// replacing the one it had. What the room's rows no longer cover ends
    /// as [`revoke`](Relay::revoke) ends it; then each ask waiting in the
    /// room that they now cover is answered as if just asked, the
    /// publications first, so a waiting subscription to one of them finds
    /// it up.
    pub fn grant(&self, room: &str, who: Option<u64>, grant: Grant) {
        self.0.borrow_mut().block(room, |rows| {
            rows.insert(who, grant);
        });
    }

    /// The block that removes `who`'s row in `room` (`None`: the `*` row), as
    /// `program::live::revoke`. What no row covers now ends with a final
    /// `not_granted`, in the node's order: publications (their subscribers
    /// hear `publisher_gone`, and each room list that heard the track hears
    /// it go down), then subscriptions (each publisher hears the lower
    /// count), then room lists. A room list the rows still let watch is
    /// re-aimed instead: it hears the tracks still up that its new row adds
    /// come up and the ones it drops go down, as a fresh `live.tracks` would
    /// list them. An ask waiting for `who` that the `*` row covers is
    /// answered.
    pub fn revoke(&self, room: &str, who: Option<u64>) {
        self.0.borrow_mut().block(room, |rows| {
            rows.remove(&who);
        });
    }

    /// The view linked as `cx` goes away with its tracks up, as an app
    /// that quit: each one ends `publisher_gone` for its subscribers and
    /// goes down in every `live.tracks`; each of its subscriptions is
    /// dropped, and the track's publisher hears the lower count.
    pub fn gone(&self, cx: &TestAppContext) {
        let host = cx.host();
        let mut state = self.0.borrow_mut();
        let Some(link) = state.hosts.iter().position(|linked| linked.same(&host)) else {
            panic!("this view is not linked to the relay");
        };
        let up: Vec<_> = state
            .tracks
            .iter()
            .filter(|(_, published)| published.link == link)
            .map(|(track, _)| track.clone())
            .collect();
        for track in up {
            state.end(&track, refusal::PUBLISHER_GONE, "the publisher went away");
        }
        let subscribed: Vec<u64> = state
            .subscriptions
            .keys()
            .filter(|(at, _)| *at == link)
            .map(|(_, id)| *id)
            .collect();
        for id in subscribed {
            state.unsubscribe(link, id);
        }
        state.watches.retain(|(at, _), _| *at != link);
        state.waiting.retain(|waiting| waiting.link != link);
        state.pieces.retain(|piece| piece.link != link);
    }

    fn take(&self, link: usize, frame: &Frame, tree: Option<&Node>) {
        let mut state = self.0.borrow_mut();
        for id in &frame.cancels {
            state.cancel(link, *id);
        }
        for request in &frame.requests {
            let payload = &request.payload;
            match request.kind.as_str() {
                LivePublishTrack::KIND => {
                    if let Ok(publish) = LivePublishTrack::decode_request(payload) {
                        state.ask(link, request.id, Ask::Publish(publish, Vec::new()));
                    }
                }
                LiveSendObject::KIND => {
                    if let Ok(send) = LiveSendObject::decode_request(payload) {
                        state.send(link, request.id, send);
                    }
                }
                LiveSubscribeTrack::KIND => {
                    if let Ok(subscribe) = LiveSubscribeTrack::decode_request(payload) {
                        state.ask(link, request.id, Ask::Subscribe(subscribe));
                    }
                }
                LiveTracks::KIND => {
                    if let Ok(room) = LiveTracks::decode_request(payload) {
                        state.ask(link, request.id, Ask::Watch(room));
                    }
                }
                _ => {}
            }
        }
        if let Some(tree) = tree {
            let placed = super::pieces(tree)
                .into_iter()
                .filter_map(|(path, node)| Some((path, PieceAsk::of(node)?)))
                .collect();
            state.place(link, placed);
        }
    }
}

impl State {
    fn tell(&self, link: usize, event: Event) {
        if let Event::Response { id, result, done } = &event
            && *id >= PIECES
        {
            let piece = self
                .pieces
                .iter()
                .find(|piece| piece.link == link && piece.id == *id);
            if let Some(piece) = piece
                && let Some(track) = piece.ask.track(result, *done)
            {
                self.hosts[link].media(&piece.path, |state| state.track = track);
            }
            return;
        }
        self.hosts[link].owe(vec![event]);
    }

    fn place(&mut self, link: usize, placed: Vec<(Vec<ElementIdWire>, PieceAsk)>) {
        let (kept, gone): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pieces)
            .into_iter()
            .partition(|piece| {
                piece.link != link
                    || placed
                        .iter()
                        .any(|(path, ask)| *path == piece.path && *ask == piece.ask)
            });
        self.pieces = kept;
        for piece in gone {
            self.cancel(link, piece.id);
        }
        for (path, ask) in placed {
            let held = self
                .pieces
                .iter()
                .find(|piece| piece.link == link && piece.path == path)
                .map(|piece| piece.id);
            match held {
                Some(id) => {
                    let again = match ask {
                        PieceAsk::Publish(_) => true,
                        PieceAsk::Subscribe(_) => {
                            let not_granted = TrackState::Ended {
                                code: refusal::NOT_GRANTED.into(),
                            };
                            self.hosts[link].told_track(&path) == Some(not_granted)
                        }
                    };
                    if again && !self.open(link, id) {
                        self.ask(link, id, ask.ask());
                    }
                }
                None => {
                    let id = PIECES + self.placed;
                    self.placed += 1;
                    let asked = ask.ask();
                    self.pieces.push(Piece {
                        link,
                        path,
                        id,
                        ask,
                    });
                    self.ask(link, id, asked);
                }
            }
        }
    }

    fn open(&self, link: usize, id: u64) -> bool {
        self.tracks
            .values()
            .any(|published| published.link == link && published.request == id)
            || self.subscriptions.contains_key(&(link, id))
            || self
                .waiting
                .iter()
                .any(|waiting| waiting.link == link && waiting.id == id)
    }

    fn account(link: usize) -> u64 {
        link as u64 + 1
    }

    fn grant_of(&self, room: &str, account: u64) -> Option<Grant> {
        let Some(rows) = self.rows.get(room) else {
            return Some(Grant::all());
        };
        rows.get(&Some(account))
            .or_else(|| rows.get(&None))
            .cloned()
    }

    fn covers(&self, link: usize, ask: &Ask) -> bool {
        let account = Self::account(link);
        self.grant_of(ask.room(), account)
            .is_some_and(|grant| ask.granted(&grant, account))
    }

    fn subscribes(&self, link: usize, room: &str, name: &str) -> bool {
        let account = Self::account(link);
        self.grant_of(room, account)
            .is_some_and(|grant| grant.subscribes(name, account))
    }

    fn publishes(&self, link: usize, track: &LiveTrack) -> bool {
        let up = self
            .tracks
            .get(&(track.room.clone(), track.name.clone()))
            .is_some_and(|held| held.link == link);
        up || self.waiting.iter().any(|waiting| {
            waiting.link == link
                && matches!(&waiting.ask, Ask::Publish(publish, _) if publish.track == *track)
        })
    }

    fn rows_of(&mut self, room: &str) -> &mut BTreeMap<Option<u64>, Grant> {
        self.rows
            .entry(room.to_owned())
            .or_insert_with(|| BTreeMap::from([(None, Grant::all())]))
    }

    fn announce(&self, (room, name): &(String, String), published: &Published, up: bool) {
        for ((at, id), (watched, grant)) in &self.watches {
            if watched == room && grant.subscribes(name, Self::account(*at)) {
                self.tell(*at, published.announced(*id, name, up));
            }
        }
    }

    fn rewatch(&mut self, link: usize, id: u64, grant: Grant) {
        let Some((room, was)) = self.watches.get(&(link, id)).cloned() else {
            return;
        };
        let account = Self::account(link);
        for ((at, name), published) in &self.tracks {
            let now = grant.subscribes(name, account);
            if *at == room && was.subscribes(name, account) != now {
                self.tell(link, published.announced(id, name, now));
            }
        }
        self.watches.insert((link, id), (room, grant));
    }

    fn count(&self, published: &Published) {
        let count = Publishing::Subscribers(published.subscribers.len() as u32);
        self.tell(
            published.link,
            item::<LivePublishTrack>(published.request, Ok(count), false),
        );
    }

    fn ask(&mut self, link: usize, id: u64, ask: Ask) {
        if let Ask::Publish(publish, _) = &ask
            && self.publishes(link, &publish.track)
        {
            let refusal = Error::new(
                refusal::DUPLICATE_TRACK,
                "another window of this app publishes this track",
            );
            self.tell(link, item::<LivePublishTrack>(id, Err(refusal), true));
            return;
        }
        if !self.covers(link, &ask) {
            self.tell(link, ask.waits(id));
            self.waiting.push(Waiting { link, id, ask });
            return;
        }
        self.admit(link, id, ask);
    }

    fn admit(&mut self, link: usize, id: u64, ask: Ask) {
        match ask {
            Ask::Publish(publish, sent) => {
                self.publish(link, id, publish);
                for (send_id, send) in sent {
                    self.send(link, send_id, send);
                }
            }
            Ask::Subscribe(subscribe) => self.subscribe(link, id, subscribe),
            Ask::Watch(room) => self.watch(link, id, room),
        }
    }

    fn block(&mut self, room: &str, write: impl FnOnce(&mut BTreeMap<Option<u64>, Grant>)) {
        write(self.rows_of(room));
        self.recheck(room);
        let (in_room, elsewhere): (Vec<_>, Vec<_>) = std::mem::take(&mut self.waiting)
            .into_iter()
            .partition(|waiting| waiting.ask.room() == room);
        self.waiting = elsewhere;
        let (publications, others): (Vec<_>, Vec<_>) = in_room
            .into_iter()
            .partition(|waiting| waiting.ask.publishes());
        for waiting in publications.into_iter().chain(others) {
            if !self.covers(waiting.link, &waiting.ask) {
                self.waiting.push(waiting);
                continue;
            }
            self.admit(waiting.link, waiting.id, waiting.ask);
        }
    }

    fn recheck(&mut self, room: &str) {
        let unpublished: Vec<(String, String)> = self
            .tracks
            .iter()
            .filter(|((at, name), published)| {
                let account = Self::account(published.link);
                let publishes = self
                    .grant_of(at, account)
                    .is_some_and(|grant| grant.publishes(name, account));
                at == room && !publishes
            })
            .map(|(track, _)| track.clone())
            .collect();
        for track in unpublished {
            let gone = "the publisher's grant was taken back";
            let Some(published) = self.end(&track, refusal::PUBLISHER_GONE, gone) else {
                continue;
            };
            let ended = item::<LivePublishTrack>(published.request, Err(revoked()), true);
            self.tell(published.link, ended);
        }
        let unsubscribed: Vec<(usize, u64)> = self
            .subscriptions
            .iter()
            .filter(|((link, _), (at, name))| at == room && !self.subscribes(*link, at, name))
            .map(|(subscription, _)| *subscription)
            .collect();
        for (link, id) in unsubscribed {
            self.unsubscribe(link, id);
            self.tell(link, item::<LiveSubscribeTrack>(id, Err(revoked()), true));
        }
        let watching: Vec<(usize, u64)> = self
            .watches
            .iter()
            .filter(|(_, (watched, _))| watched == room)
            .map(|(watch, _)| *watch)
            .collect();
        for (link, id) in watching {
            match self
                .grant_of(room, Self::account(link))
                .filter(Grant::watches)
            {
                Some(grant) => self.rewatch(link, id, grant),
                None => {
                    self.watches.remove(&(link, id));
                    self.tell(link, item::<LiveTracks>(id, Err(revoked()), true));
                }
            }
        }
    }

    fn held_objects(
        &mut self,
        link: usize,
        track: &LiveTrack,
    ) -> Option<&mut Vec<(u64, LiveSend)>> {
        self.waiting
            .iter_mut()
            .find_map(|waiting| match &mut waiting.ask {
                Ask::Publish(publish, sent) if waiting.link == link && publish.track == *track => {
                    Some(sent)
                }
                Ask::Publish(..) | Ask::Subscribe(_) | Ask::Watch(_) => None,
            })
    }

    fn publish(&mut self, link: usize, id: u64, publish: LivePublish) {
        let track = (publish.track.room, publish.track.name);
        if self.tracks.contains_key(&track) {
            let refusal = Error::new(refusal::DUPLICATE_TRACK, "another account publishes it");
            self.tell(link, item::<LivePublishTrack>(id, Err(refusal), true));
            return;
        }
        let published = Published {
            link,
            request: id,
            mode: publish.mode,
            group: None,
            subscribers: Vec::new(),
        };
        if id >= PIECES {
            let up = item::<LivePublishTrack>(id, Ok(Publishing::Subscribers(0)), false);
            self.tell(link, up);
        }
        self.announce(&track, &published, true);
        let again: Vec<(usize, u64, Ask)> = self
            .pieces
            .iter()
            .filter(|piece| {
                matches!(&piece.ask, PieceAsk::Subscribe(subscribe)
                    if (&subscribe.track.room, &subscribe.track.name) == (&track.0, &track.1))
            })
            .map(|piece| (piece.link, piece.id, piece.ask.ask()))
            .collect();
        self.tracks.insert(track, published);
        for (at, id, ask) in again {
            if !self.open(at, id) {
                self.ask(at, id, ask);
            }
        }
    }

    fn end(&mut self, track: &(String, String), code: &str, message: &str) -> Option<Published> {
        let published = self.tracks.remove(track)?;
        for subscriber in &published.subscribers {
            self.subscriptions.remove(&(subscriber.link, subscriber.id));
            let refusal = Error::new(code, message);
            let ended = item::<LiveSubscribeTrack>(subscriber.id, Err(refusal), true);
            self.tell(subscriber.link, ended);
        }
        self.announce(track, &published, false);
        Some(published)
    }

    fn cancel(&mut self, link: usize, id: u64) {
        let publication = self
            .tracks
            .iter()
            .find(|(_, published)| published.link == link && published.request == id)
            .map(|(track, _)| track.clone());
        if let Some(track) = publication {
            self.end(
                &track,
                refusal::TRACK_ENDED,
                "the publisher ended the track",
            );
        }
        self.unsubscribe(link, id);
        self.watches.remove(&(link, id));
        self.waiting
            .retain(|waiting| (waiting.link, waiting.id) != (link, id));
    }

    fn unsubscribe(&mut self, link: usize, id: u64) {
        let Some(track) = self.subscriptions.remove(&(link, id)) else {
            return;
        };
        let Some(published) = self.tracks.get_mut(&track) else {
            return;
        };
        published
            .subscribers
            .retain(|subscriber| (subscriber.link, subscriber.id) != (link, id));
        self.count(&self.tracks[&track]);
    }

    fn send(&mut self, link: usize, id: u64, send: LiveSend) {
        if let Some(held) = self.held_objects(link, &send.track) {
            held.push((id, send));
            return;
        }
        let track = (send.track.room, send.track.name);
        let Some(published) = self.tracks.get_mut(&track).filter(|held| held.link == link) else {
            let refusal = Error::new(refusal::LIVE_NOT_PUBLISHED, "no such track is published");
            self.tell(link, item::<LiveSendObject>(id, Err(refusal), true));
            return;
        };
        let mut items = Vec::new();
        match published.mode {
            LiveMode::Datagram => {
                let newest = published
                    .group
                    .as_ref()
                    .and_then(|group| Some((group.number, group.objects.last()?.0)));
                if newest.is_some_and(|newest| (send.group, send.object) <= newest) {
                    return;
                }
                published.group = Some(Group {
                    number: send.group,
                    objects: vec![(send.object, send.body.clone())],
                    closed: false,
                });
                let object = LiveItem::Object {
                    group: send.group,
                    object: send.object,
                    body: send.body,
                };
                items.push((send.group, object));
            }
            LiveMode::Stream => {
                let open = published
                    .group
                    .as_ref()
                    .is_some_and(|group| group.number == send.group && !group.closed);
                if !open {
                    if let Some(group) = published.group.as_ref().filter(|group| !group.closed) {
                        let end = LiveItem::GroupEnd {
                            group: group.number,
                            complete: true,
                        };
                        items.push((group.number, end));
                    }
                    published.group = Some(Group {
                        number: send.group,
                        objects: Vec::new(),
                        closed: false,
                    });
                }
                let group = published.group.as_mut().expect("the group in hand");
                if !send.body.is_empty() || !send.end {
                    let object = group.objects.len() as u32;
                    group.objects.push((object, send.body.clone()));
                    let object = LiveItem::Object {
                        group: send.group,
                        object,
                        body: send.body,
                    };
                    items.push((send.group, object));
                }
                if send.end {
                    group.closed = true;
                    let end = LiveItem::GroupEnd {
                        group: send.group,
                        complete: true,
                    };
                    items.push((send.group, end));
                }
            }
        }
        let mut told = Vec::new();
        for subscriber in &published.subscribers {
            for (group, live) in &items {
                if !subscriber.hears(*group) {
                    continue;
                }
                let event = item::<LiveSubscribeTrack>(subscriber.id, Ok(live.clone()), false);
                told.push((subscriber.link, event));
            }
        }
        for (at, event) in told {
            self.tell(at, event);
        }
    }

    fn subscribe(&mut self, link: usize, id: u64, subscribe: LiveSubscribe) {
        let track = (subscribe.track.room, subscribe.track.name);
        let Some(published) = self.tracks.get_mut(&track) else {
            let refusal = Error::new(refusal::UNKNOWN_TRACK, "no one publishes this track");
            self.tell(link, item::<LiveSubscribeTrack>(id, Err(refusal), true));
            return;
        };
        let in_hand = published.group.as_ref();
        let in_flight = in_hand
            .filter(|group| published.mode == LiveMode::Stream && !group.closed)
            .map(|group| group.number);
        let (replay, passed) = match subscribe.from {
            LiveStart::Latest => (in_hand, None),
            LiveStart::Next => (None, in_flight),
        };
        published.subscribers.push(Subscriber { link, id, passed });
        let mut items = vec![LiveItem::Opened {
            by: Self::account(published.link),
            mode: published.mode,
            latest: replay.map(|group| group.number),
        }];
        if let Some(group) = replay {
            items.extend(group.objects.iter().map(|(object, body)| LiveItem::Object {
                group: group.number,
                object: *object,
                body: body.clone(),
            }));
            if group.closed {
                items.push(LiveItem::GroupEnd {
                    group: group.number,
                    complete: true,
                });
            }
        }
        for live in items {
            self.tell(link, item::<LiveSubscribeTrack>(id, Ok(live), false));
        }
        self.subscriptions.insert((link, id), track.clone());
        self.count(&self.tracks[&track]);
    }

    fn watch(&mut self, link: usize, id: u64, room: String) {
        let account = Self::account(link);
        let grant = self
            .grant_of(&room, account)
            .expect("an admitted room list has a row");
        for ((at, name), published) in &self.tracks {
            if *at == room && grant.subscribes(name, account) {
                self.tell(link, published.announced(id, name, true));
            }
        }
        self.watches.insert((link, id), (room, grant));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::{DEFAULT_PRIORITY, Received, Track, subscribe};
    use crate::methods::{Capability, LiveTrack};
    use crate::prelude::*;
    use crate::wire::TrackState;
    use crate::{Task, View};
    use serde::{Deserialize, Serialize};

    const KINDS: [&str; 4] = [
        LivePublishTrack::KIND,
        LiveSendObject::KIND,
        LiveSubscribeTrack::KIND,
        LiveTracks::KIND,
    ];

    #[test]
    fn the_relay_answers_every_live_kind() {
        let live: Vec<&str> = crate::methods::ALL
            .iter()
            .copied()
            .filter(|kind| kind.starts_with("live."))
            .collect();
        assert_eq!(live, KINDS);
    }

    fn track(name: &str) -> LiveTrack {
        LiveTrack {
            room: "lobby".into(),
            name: name.into(),
        }
    }

    fn latest(name: &str) -> LiveSubscribe {
        LiveSubscribe {
            track: track(name),
            from: LiveStart::Latest,
        }
    }

    #[derive(Default, Serialize, Deserialize)]
    struct Echo {
        me: u64,
        heard: Vec<String>,
        roster: Vec<(String, u64, bool)>,
        subscribers: Option<u32>,
        waits: u32,
        refused: Vec<String>,
        #[serde(skip)]
        mine: Option<Track<String>>,
        #[serde(skip)]
        hearing: BTreeMap<String, Task<()>>,
        #[serde(skip)]
        watch: Option<Task<()>>,
    }

    impl View for Echo {
        const NAME: &'static str = "Echo";
        const CAPABILITIES: &'static [Capability] = &[Capability::Live];
    }

    impl Render for Echo {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().children(self.heard.iter().map(|line| div().child(line.clone())))
        }
    }

    impl Echo {
        fn start(&mut self, me: u64, cx: &mut Context<Self>) {
            self.me = me;
            cx.notify();
            let mine = LivePublish {
                track: track(&format!("echo/{me}")),
                mode: LiveMode::Stream,
                priority: DEFAULT_PRIORITY,
            };
            self.mine = Some(Track::publish(cx, mine, |view: &mut Echo, item, cx| {
                match item {
                    Ok(Publishing::Waiting) => view.waits += 1,
                    Ok(Publishing::Subscribers(count)) => view.subscribers = Some(count),
                    Ok(Publishing::Cut { .. }) => unreachable!("the relay's queue never fills"),
                    Err(refusal) => view.refused.push(refusal.code),
                }
                cx.notify();
            }));
            self.watch = Some(cx.follow::<LiveTracks>("lobby".into(), |view, item, cx| {
                match item {
                    Ok(Watching::Waiting) => view.waits += 1,
                    Ok(Watching::Announce(LiveAnnounce { name, by, up, .. })) => {
                        view.roster.push((name.clone(), by, up));
                        if up && by != view.me {
                            view.hear(&name, cx);
                        }
                    }
                    Err(refusal) => view.refused.push(refusal.code),
                }
                cx.notify();
            }));
        }

        fn hear(&mut self, name: &str, cx: &mut Context<Self>) {
            let task = subscribe(cx, latest(name), |view: &mut Echo, item, cx| {
                match item {
                    Ok(Received::Object { value, .. }) => view.heard.push(value),
                    Ok(Received::Waiting) => view.waits += 1,
                    Ok(Received::Opened { .. } | Received::GroupEnd { .. }) => return,
                    Err(refusal) => view.refused.push(refusal.code),
                }
                cx.notify();
            });
            self.hearing.insert(name.to_owned(), task);
        }

        fn say(&mut self, line: &str, cx: &mut Context<Self>) {
            let mine = self.mine.as_mut().unwrap();
            mine.send(&cx.host(), &line.to_owned());
            mine.next_group(&cx.host());
        }
    }

    fn echo(cx: &mut TestAppContext, me: u64) -> crate::Entity<Echo> {
        let view = cx.open::<Echo>();
        cx.update(&view, |echo, _, cx| echo.start(me, cx));
        cx.run_until_parked();
        view
    }

    #[test]
    fn a_line_one_view_sends_reaches_the_other_and_a_late_one_hears_the_last() {
        let (mut a, mut b, mut c) = (
            TestAppContext::new(),
            TestAppContext::new(),
            TestAppContext::new(),
        );
        let relay = Relay::new();
        relay.link(&a).link(&b).link(&c);
        let one = echo(&mut a, 1);
        let two = echo(&mut b, 2);
        a.run_until_parked();
        assert!(a.update(&one, |echo, _, _| echo.roster.contains(&(
            "echo/2".into(),
            2,
            true
        ))));
        a.update(&one, |echo, _, cx| echo.say("hello", cx));
        a.run_until_parked();
        b.run_until_parked();
        assert!(b.has_text("hello"), "{:?}", b.texts());
        assert_eq!(a.update(&one, |echo, _, _| echo.subscribers), Some(1));
        let _three = echo(&mut c, 3);
        assert!(c.has_text("hello"), "{:?}", c.texts());
        a.run_until_parked();
        assert_eq!(a.update(&one, |echo, _, _| echo.subscribers), Some(2));
        b.update(&two, |echo, _, cx| echo.say("hi", cx));
        b.run_until_parked();
        a.run_until_parked();
        assert!(a.has_text("hi"), "{:?}", a.texts());
    }

    #[test]
    fn a_publisher_hears_its_count_only_as_it_moves_and_a_subscriber_gone_lowers_it() {
        let (mut a, mut b) = (TestAppContext::new(), TestAppContext::new());
        let relay = Relay::new();
        relay.link(&a).link(&b);
        let one = echo(&mut a, 1);
        assert_eq!(a.update(&one, |echo, _, _| echo.subscribers), None);
        let _two = echo(&mut b, 2);
        a.run_until_parked();
        assert_eq!(a.update(&one, |echo, _, _| echo.subscribers), Some(1));
        relay.gone(&b);
        a.run_until_parked();
        assert_eq!(a.update(&one, |echo, _, _| echo.subscribers), Some(0));
    }

    #[test]
    fn a_publisher_gone_ends_each_subscription_and_takes_its_track_down() {
        let (mut a, mut b) = (TestAppContext::new(), TestAppContext::new());
        let relay = Relay::new();
        relay.link(&a).link(&b);
        let _one = echo(&mut a, 1);
        let two = echo(&mut b, 2);
        a.run_until_parked();
        relay.gone(&a);
        b.run_until_parked();
        let (refused, roster) = b.update(&two, |echo, _, _| {
            (echo.refused.clone(), echo.roster.clone())
        });
        assert_eq!(refused, [refusal::PUBLISHER_GONE]);
        assert!(roster.contains(&("echo/1".into(), 1, false)), "{roster:?}");
    }

    #[test]
    fn a_track_another_account_holds_is_refused_and_an_unknown_one_too() {
        let (mut a, mut b) = (TestAppContext::new(), TestAppContext::new());
        let relay = Relay::new();
        relay.link(&a).link(&b);
        let _one = echo(&mut a, 1);
        let two = echo(&mut b, 1);
        b.run_until_parked();
        let refused = b.update(&two, |echo, _, _| echo.refused.clone());
        assert_eq!(refused, [refusal::DUPLICATE_TRACK]);
        b.update(&two, |echo, _, cx| echo.hear("nobody", cx));
        b.run_until_parked();
        let refused = b.update(&two, |echo, _, _| echo.refused.clone());
        assert_eq!(refused, [refusal::DUPLICATE_TRACK, refusal::UNKNOWN_TRACK]);
    }

    #[test]
    fn an_ask_in_a_held_room_waits_and_its_grant_answers_it() {
        let (mut a, mut b) = (TestAppContext::new(), TestAppContext::new());
        let relay = Relay::new();
        relay.hold("lobby").link(&a).link(&b);
        let two = b.open::<Echo>();
        b.update(&two, |echo, _, cx| {
            echo.hear("echo/1", cx);
            echo.hear("nobody", cx);
        });
        b.run_until_parked();
        let one = echo(&mut a, 1);
        a.update(&one, |echo, _, cx| echo.say("early", cx));
        a.run_until_parked();
        let waited = a.update(&one, |echo, _, _| {
            (
                echo.waits,
                echo.refused.clone(),
                echo.subscribers,
                echo.roster.clone(),
            )
        });
        assert_eq!(waited, (2, vec![], None, vec![]));
        let waited = b.update(&two, |echo, _, _| (echo.waits, echo.refused.clone()));
        assert_eq!(waited, (2, vec![]));
        relay.grant("lobby", None, Grant::all());
        a.run_until_parked();
        b.run_until_parked();
        assert!(b.has_text("early"), "{:?}", b.texts());
        assert_eq!(
            b.update(&two, |echo, _, _| echo.refused.clone()),
            [refusal::UNKNOWN_TRACK]
        );
        let admitted = a.update(&one, |echo, _, _| (echo.subscribers, echo.roster.clone()));
        assert_eq!(admitted, (Some(1), vec![("echo/1".into(), 1, true)]));
    }

    fn listens() -> Grant {
        Grant {
            publish: Vec::new(),
            subscribe: vec!["*".into()],
        }
    }

    #[test]
    fn an_accounts_own_row_replaces_the_star_row_and_account_names_the_asker() {
        let (mut a, mut b) = (TestAppContext::new(), TestAppContext::new());
        let relay = Relay::new();
        relay.link(&a).link(&b);
        let open = Grant {
            publish: vec!["echo/{account}".into()],
            subscribe: vec!["*".into()],
        };
        relay.grant("lobby", None, open);
        relay.grant("lobby", Some(2), listens());
        let one = echo(&mut a, 1);
        let two = echo(&mut b, 2);
        a.run_until_parked();
        let heard = b.update(&two, |echo, _, _| (echo.waits, echo.roster.clone()));
        assert_eq!(heard, (1, vec![("echo/1".into(), 1, true)]));
        assert_eq!(a.update(&one, |echo, _, _| echo.subscribers), Some(1));
        let theirs = LivePublish {
            track: track("echo/2"),
            mode: LiveMode::Stream,
            priority: DEFAULT_PRIORITY,
        };
        let _theirs = a.update(&one, |_, _, cx| {
            Track::<String>::publish(cx, theirs, |echo: &mut Echo, item, cx| {
                if let Ok(Publishing::Waiting) = item {
                    echo.waits += 1;
                }
                cx.notify();
            })
        });
        a.run_until_parked();
        let asked = a.update(&one, |echo, _, _| (echo.waits, echo.refused.clone()));
        assert_eq!(asked, (1, vec![]));
    }

    #[test]
    fn a_row_taken_back_ends_what_it_allowed_with_a_final_not_granted() {
        let (mut a, mut b) = (TestAppContext::new(), TestAppContext::new());
        let relay = Relay::new();
        relay.link(&a).link(&b);
        let one = echo(&mut a, 1);
        let two = echo(&mut b, 2);
        a.run_until_parked();
        relay.grant("lobby", Some(1), listens());
        a.run_until_parked();
        b.run_until_parked();
        assert_eq!(
            a.update(&one, |echo, _, _| echo.refused.clone()),
            [refusal::NOT_GRANTED]
        );
        let (refused, roster) = b.update(&two, |echo, _, _| {
            (echo.refused.clone(), echo.roster.clone())
        });
        assert_eq!(refused, [refusal::PUBLISHER_GONE]);
        assert!(roster.contains(&("echo/1".into(), 1, false)), "{roster:?}");
        b.update(&two, |echo, _, cx| echo.say("still here", cx));
        b.run_until_parked();
        a.run_until_parked();
        assert!(a.has_text("still here"), "{:?}", a.texts());
        relay.revoke("lobby", None);
        a.run_until_parked();
        b.run_until_parked();
        assert_eq!(
            b.update(&two, |echo, _, _| echo.refused.clone()),
            [
                refusal::PUBLISHER_GONE,
                refusal::NOT_GRANTED,
                refusal::NOT_GRANTED
            ]
        );
        let (refused, roster) = a.update(&one, |echo, _, _| {
            (echo.refused.clone(), echo.roster.clone())
        });
        assert_eq!(refused, [refusal::NOT_GRANTED, refusal::PUBLISHER_GONE]);
        assert!(roster.contains(&("echo/2".into(), 2, false)), "{roster:?}");
    }

    #[test]
    fn an_own_row_removed_leaves_the_star_row_to_answer_what_waited() {
        let (mut a, mut b) = (TestAppContext::new(), TestAppContext::new());
        let relay = Relay::new();
        relay.link(&a).link(&b);
        relay.grant("lobby", Some(2), Grant::default());
        let one = echo(&mut a, 1);
        let two = echo(&mut b, 2);
        a.run_until_parked();
        assert_eq!(b.update(&two, |echo, _, _| echo.waits), 2);
        relay.revoke("lobby", Some(2));
        a.run_until_parked();
        b.run_until_parked();
        a.run_until_parked();
        let heard_two = a.update(&one, |echo, _, _| echo.roster.clone());
        assert!(
            heard_two.contains(&("echo/2".into(), 2, true)),
            "{heard_two:?}"
        );
        let heard_one = b.update(&two, |echo, _, _| echo.roster.clone());
        assert!(
            heard_one.contains(&("echo/1".into(), 1, true)),
            "{heard_one:?}"
        );
        assert_eq!(a.update(&one, |echo, _, _| echo.subscribers), Some(1));
        let speaks = Grant {
            publish: vec!["*".into()],
            subscribe: Vec::new(),
        };
        relay.grant("lobby", Some(2), speaks);
        a.run_until_parked();
        b.run_until_parked();
        assert_eq!(a.update(&one, |echo, _, _| echo.subscribers), Some(0));
        assert_eq!(
            b.update(&two, |echo, _, _| echo.refused.clone()),
            [refusal::NOT_GRANTED; 2]
        );
    }

    #[test]
    fn a_waiting_ask_withdrawn_or_gone_is_forgotten() {
        let (mut a, mut b, mut c) = (
            TestAppContext::new(),
            TestAppContext::new(),
            TestAppContext::new(),
        );
        let relay = Relay::new();
        relay.hold("lobby").link(&a).link(&b).link(&c);
        let one = echo(&mut a, 1);
        let _two = echo(&mut b, 2);
        a.update(&one, |echo, _, _| echo.mine = None);
        a.run_until_parked();
        relay.gone(&b);
        relay.grant("lobby", None, Grant::all());
        let three = echo(&mut c, 3);
        let roster = c.update(&three, |echo, _, _| echo.roster.clone());
        assert_eq!(roster, [("echo/3".into(), 3, true)]);
    }

    #[test]
    fn a_track_asks_the_node_for_the_priority_it_names() {
        let mut a = TestAppContext::new();
        let one = a.open::<Echo>();
        let top = LivePublish {
            track: track("cursor/1"),
            mode: LiveMode::Datagram,
            priority: 0,
        };
        let _top = a.update(&one, |_, _, cx| {
            Track::<u32>::publish(cx, top, |_: &mut Echo, _, _| {})
        });
        a.run_until_parked();
        let asked = a.host().requests::<LivePublishTrack>();
        assert_eq!(
            asked.iter().map(|ask| ask.priority).collect::<Vec<_>>(),
            [0]
        );
    }

    #[test]
    fn a_next_subscription_passes_over_the_group_in_hand() {
        let (mut a, mut b) = (TestAppContext::new(), TestAppContext::new());
        let relay = Relay::new();
        relay.link(&a).link(&b);
        let one = a.open::<Echo>();
        let mut doc: Option<Track<u32>> = None;
        let request = LivePublish {
            track: track("doc"),
            mode: LiveMode::Stream,
            priority: DEFAULT_PRIORITY,
        };
        a.update(&one, |_, _, cx| {
            doc = Some(Track::publish(cx, request, |_: &mut Echo, _, _| {}));
        });
        a.run_until_parked();
        let doc = doc.as_mut().unwrap();
        a.update(&one, |_, _, cx| doc.send(&cx.host(), &1));
        a.run_until_parked();
        let two = b.open::<Echo>();
        let heard = Rc::new(RefCell::new(Vec::new()));
        let sink = heard.clone();
        let next = LiveSubscribe {
            track: track("doc"),
            from: LiveStart::Next,
        };
        let _task = b.update(&two, |_, _, cx| {
            subscribe(
                cx,
                next,
                move |_: &mut Echo, item: Result<Received<u32>, _>, _| {
                    sink.borrow_mut().push(item.unwrap());
                },
            )
        });
        b.run_until_parked();
        a.update(&one, |_, _, cx| {
            doc.send(&cx.host(), &2);
            doc.next_group(&cx.host());
            doc.send(&cx.host(), &3);
        });
        a.run_until_parked();
        b.run_until_parked();
        assert_eq!(
            *heard.borrow(),
            [
                Received::Opened {
                    by: 1,
                    mode: LiveMode::Stream,
                    latest: None
                },
                Received::Object {
                    group: 1,
                    object: 0,
                    value: 3
                }
            ]
        );
    }

    #[test]
    fn a_datagram_track_keeps_the_newest_and_drops_an_older_object() {
        let (mut a, mut b) = (TestAppContext::new(), TestAppContext::new());
        let relay = Relay::new();
        relay.link(&a).link(&b);
        let one = a.open::<Echo>();
        let mut cursor: Option<Track<u32>> = None;
        let request = LivePublish {
            track: track("cursor/1"),
            mode: LiveMode::Datagram,
            priority: DEFAULT_PRIORITY,
        };
        a.update(&one, |_, _, cx| {
            cursor = Some(Track::publish(cx, request, |_: &mut Echo, _, _| {}));
        });
        a.run_until_parked();
        let cursor = cursor.as_mut().unwrap();
        a.update(&one, |_, _, cx| {
            cursor.send_latest(&cx.host(), 5, &50);
            cursor.send_latest(&cx.host(), 5, &51);
        });
        a.run_until_parked();
        let two = b.open::<Echo>();
        let heard = Rc::new(RefCell::new(Vec::new()));
        let sink = heard.clone();
        let _task = b.update(&two, |_, _, cx| {
            subscribe(
                cx,
                latest("cursor/1"),
                move |_: &mut Echo, item: Result<Received<u32>, _>, _| {
                    sink.borrow_mut().push(item.unwrap());
                },
            )
        });
        b.run_until_parked();
        assert_eq!(
            *heard.borrow(),
            [
                Received::Opened {
                    by: 1,
                    mode: LiveMode::Datagram,
                    latest: Some(5)
                },
                Received::Object {
                    group: 5,
                    object: 1,
                    value: 51
                }
            ]
        );
        a.update(&one, |_, _, cx| {
            cursor.send_latest(&cx.host(), 4, &40);
            cursor.send_latest(&cx.host(), 6, &60);
        });
        a.run_until_parked();
        b.run_until_parked();
        assert_eq!(
            heard.borrow()[2..],
            [Received::Object {
                group: 6,
                object: 0,
                value: 60
            }]
        );
    }

    #[test]
    fn a_next_datagram_subscription_hears_every_datagram_from_now_on() {
        let (mut a, mut b) = (TestAppContext::new(), TestAppContext::new());
        let relay = Relay::new();
        relay.link(&a).link(&b);
        let one = a.open::<Echo>();
        let mut cursor: Option<Track<u32>> = None;
        let request = LivePublish {
            track: track("cursor/1"),
            mode: LiveMode::Datagram,
            priority: DEFAULT_PRIORITY,
        };
        a.update(&one, |_, _, cx| {
            cursor = Some(Track::publish(cx, request, |_: &mut Echo, _, _| {}));
        });
        a.run_until_parked();
        let cursor = cursor.as_mut().unwrap();
        a.update(&one, |_, _, cx| cursor.send_latest(&cx.host(), 0, &1));
        a.run_until_parked();
        let two = b.open::<Echo>();
        let heard = Rc::new(RefCell::new(Vec::new()));
        let sink = heard.clone();
        let next = LiveSubscribe {
            track: track("cursor/1"),
            from: LiveStart::Next,
        };
        let _task = b.update(&two, |_, _, cx| {
            subscribe(
                cx,
                next,
                move |_: &mut Echo, item: Result<Received<u32>, _>, _| {
                    sink.borrow_mut().push(item.unwrap());
                },
            )
        });
        b.run_until_parked();
        a.update(&one, |_, _, cx| {
            cursor.send_latest(&cx.host(), 0, &2);
            cursor.send_latest(&cx.host(), 0, &3);
        });
        a.run_until_parked();
        b.run_until_parked();
        assert_eq!(
            *heard.borrow(),
            [
                Received::Opened {
                    by: 1,
                    mode: LiveMode::Datagram,
                    latest: None
                },
                Received::Object {
                    group: 0,
                    object: 1,
                    value: 2
                },
                Received::Object {
                    group: 0,
                    object: 2,
                    value: 3
                }
            ]
        );
    }

    fn watcher(cx: &mut TestAppContext) -> crate::Entity<Echo> {
        let view = cx.open::<Echo>();
        cx.update(&view, |echo, _, cx| {
            echo.watch = Some(cx.follow::<LiveTracks>("lobby".into(), |echo, item, cx| {
                match item {
                    Ok(Watching::Announce(LiveAnnounce { name, by, up, .. })) => {
                        echo.roster.push((name, by, up))
                    }
                    Ok(Watching::Waiting) => echo.waits += 1,
                    Err(refusal) => echo.refused.push(refusal.code),
                }
                cx.notify();
            }));
        });
        cx.run_until_parked();
        view
    }

    fn stream(name: &str) -> LivePublish {
        LivePublish {
            track: track(name),
            mode: LiveMode::Stream,
            priority: DEFAULT_PRIORITY,
        }
    }

    #[test]
    fn a_room_list_hears_the_tracks_its_moved_row_adds_and_drops() {
        let (mut a, mut b) = (TestAppContext::new(), TestAppContext::new());
        let relay = Relay::new();
        relay.link(&a).link(&b);
        let presence = Grant {
            publish: vec!["*".into()],
            subscribe: vec!["presence/*".into()],
        };
        relay.grant("lobby", None, presence.clone());
        let one = a.open::<Echo>();
        let mut doc: Option<Track<u32>> = None;
        let _presence = a.update(&one, |_, _, cx| {
            doc = Some(Track::publish(cx, stream("doc"), |_: &mut Echo, _, _| {}));
            Track::<u32>::publish(cx, stream("presence/1"), |_: &mut Echo, _, _| {})
        });
        a.run_until_parked();
        let two = watcher(&mut b);
        let roster = |b: &mut TestAppContext| b.update(&two, |echo, _, _| echo.roster.clone());
        assert_eq!(roster(&mut b), [("presence/1".into(), 1, true)]);
        relay.grant("lobby", None, Grant::all());
        b.run_until_parked();
        assert_eq!(
            roster(&mut b),
            [("presence/1".into(), 1, true), ("doc".into(), 1, true)]
        );
        relay.grant("lobby", None, presence);
        b.run_until_parked();
        a.update(&one, |_, _, _| doc = None);
        a.run_until_parked();
        b.run_until_parked();
        assert_eq!(
            roster(&mut b),
            [
                ("presence/1".into(), 1, true),
                ("doc".into(), 1, true),
                ("doc".into(), 1, false)
            ]
        );
    }

    #[test]
    fn a_block_that_narrows_a_room_list_and_ends_a_track_tells_its_down_once() {
        let (mut a, mut b) = (TestAppContext::new(), TestAppContext::new());
        let relay = Relay::new();
        relay.link(&a).link(&b);
        let one = a.open::<Echo>();
        let _doc = a.update(&one, |_, _, cx| {
            Track::<u32>::publish(cx, stream("doc"), |_: &mut Echo, _, _| {})
        });
        a.run_until_parked();
        let two = watcher(&mut b);
        let presence = Grant {
            publish: vec!["presence/*".into()],
            subscribe: vec!["presence/*".into()],
        };
        relay.grant("lobby", None, presence);
        b.run_until_parked();
        let (roster, refused) = b.update(&two, |echo, _, _| {
            (echo.roster.clone(), echo.refused.clone())
        });
        assert_eq!(roster, [("doc".into(), 1, true), ("doc".into(), 1, false)]);
        assert!(refused.is_empty(), "{refused:?}");
    }

    fn tagged(
        cx: &mut TestAppContext,
        view: &crate::Entity<Echo>,
        tag: &'static str,
        name: &str,
    ) -> Track<String> {
        let request = stream(name);
        cx.update(view, |_, _, cx| {
            Track::publish(cx, request, move |echo: &mut Echo, item, cx| {
                if let Err(refusal) = item {
                    echo.refused.push(format!("{tag}:{}", refusal.code));
                }
                cx.notify();
            })
        })
    }

    #[test]
    fn a_track_this_app_publishes_already_is_refused_and_the_first_goes_on() {
        let (mut a, mut b) = (TestAppContext::new(), TestAppContext::new());
        let relay = Relay::new();
        relay.link(&a).link(&b);
        let one = a.open::<Echo>();
        let mut first = tagged(&mut a, &one, "first", "echo/1");
        a.run_until_parked();
        let _two = echo(&mut b, 2);
        a.run_until_parked();
        let _second = tagged(&mut a, &one, "second", "echo/1");
        a.run_until_parked();
        assert_eq!(
            a.update(&one, |echo, _, _| echo.refused.clone()),
            ["second:duplicate_track"]
        );
        a.update(&one, |_, _, cx| {
            first.send(&cx.host(), &"still mine".into())
        });
        a.run_until_parked();
        b.run_until_parked();
        assert!(b.has_text("still mine"), "{:?}", b.texts());
    }

    #[test]
    fn a_track_this_app_waits_to_publish_is_refused_a_second_time() {
        let mut a = TestAppContext::new();
        let relay = Relay::new();
        relay.hold("lobby").link(&a);
        let one = a.open::<Echo>();
        let _first = tagged(&mut a, &one, "first", "echo/1");
        let _second = tagged(&mut a, &one, "second", "echo/1");
        a.run_until_parked();
        assert_eq!(
            a.update(&one, |echo, _, _| echo.refused.clone()),
            ["second:duplicate_track"]
        );
    }

    #[test]
    fn a_block_that_widens_a_room_list_and_ends_the_track_tells_it_nothing() {
        let (mut a, mut b) = (TestAppContext::new(), TestAppContext::new());
        let relay = Relay::new();
        relay.link(&a).link(&b);
        let presence = Grant {
            publish: vec!["*".into()],
            subscribe: vec!["presence/*".into()],
        };
        relay.grant("lobby", None, presence);
        let one = a.open::<Echo>();
        let _doc = tagged(&mut a, &one, "doc", "doc");
        a.run_until_parked();
        let two = watcher(&mut b);
        let widened = Grant {
            publish: vec!["presence/*".into()],
            subscribe: vec!["*".into()],
        };
        relay.grant("lobby", None, widened);
        a.run_until_parked();
        b.run_until_parked();
        assert_eq!(b.update(&two, |echo, _, _| echo.roster.clone()), []);
        assert_eq!(
            a.update(&one, |echo, _, _| echo.refused.clone()),
            ["doc:not_granted"]
        );
    }

    #[test]
    fn a_block_that_ends_a_room_list_and_its_track_tells_the_track_down_first() {
        let (mut a, mut b) = (TestAppContext::new(), TestAppContext::new());
        let relay = Relay::new();
        relay.link(&a).link(&b);
        let one = a.open::<Echo>();
        let _doc = tagged(&mut a, &one, "doc", "doc");
        a.run_until_parked();
        let two = watcher(&mut b);
        relay.revoke("lobby", None);
        b.run_until_parked();
        let (roster, refused) = b.update(&two, |echo, _, _| {
            (echo.roster.clone(), echo.refused.clone())
        });
        assert_eq!(roster, [("doc".into(), 1, true), ("doc".into(), 1, false)]);
        assert_eq!(refused, [refusal::NOT_GRANTED]);
    }

    #[test]
    fn a_hold_after_the_asks_ends_what_the_rows_allowed() {
        let mut a = TestAppContext::new();
        let relay = Relay::new();
        relay.link(&a);
        let one = echo(&mut a, 1);
        relay.hold("lobby");
        a.run_until_parked();
        assert_eq!(
            a.update(&one, |echo, _, _| echo.refused.clone()),
            [refusal::NOT_GRANTED; 2]
        );
    }

    #[derive(Default, Serialize, Deserialize)]
    struct Call {
        me: u64,
        speaking: bool,
        hearing: Vec<u64>,
        mine: Vec<TrackState>,
        theirs: Vec<(u64, TrackState)>,
        roster: Vec<(String, u64, bool)>,
        #[serde(skip)]
        watch: Option<Task<()>>,
    }

    impl View for Call {
        const NAME: &'static str = "Call";
        const CAPABILITIES: &'static [Capability] = &[Capability::Microphone, Capability::Live];
    }

    fn on_call(name: String) -> LiveTrack {
        LiveTrack {
            room: "call".into(),
            name,
        }
    }

    impl Render for Call {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let me = self.me;
            let mine = cx.listener(|call: &mut Call, state: &MediaState, _, cx| {
                call.mine.push(state.track.clone());
                cx.notify();
            });
            div()
                .when(self.speaking, |div| {
                    div.child(
                        microphone("me")
                            .publish(on_call(format!("mic/{me}")))
                            .on_state(mine),
                    )
                })
                .children(self.hearing.iter().map(|&them| {
                    let heard = cx.listener(move |call: &mut Call, state: &MediaState, _, cx| {
                        call.theirs.push((them, state.track.clone()));
                        cx.notify();
                    });
                    remote_audio(("voice", them), on_call(format!("mic/{them}"))).on_state(heard)
                }))
        }
    }

    fn call(
        cx: &mut TestAppContext,
        me: u64,
        speaking: bool,
        hearing: Vec<u64>,
    ) -> crate::Entity<Call> {
        let view = cx.open::<Call>();
        cx.update(&view, |call, _, cx| {
            (call.me, call.speaking, call.hearing) = (me, speaking, hearing);
            call.watch = Some(cx.follow::<LiveTracks>("call".into(), |call, item, cx| {
                if let Ok(Watching::Announce(LiveAnnounce { name, by, up, .. })) = item {
                    call.roster.push((name, by, up));
                    cx.notify();
                }
            }));
            cx.notify();
        });
        cx.run_until_parked();
        view
    }

    fn in_call() -> Grant {
        Grant {
            publish: vec!["mic/{account}".into()],
            subscribe: vec!["*".into()],
        }
    }

    fn up(subscribers: u32) -> TrackState {
        TrackState::Up { subscribers }
    }

    fn ended(code: &str) -> TrackState {
        TrackState::Ended { code: code.into() }
    }

    #[test]
    fn a_published_microphone_is_heard_and_a_voice_waits_for_its_row() {
        let (mut a, mut b) = (TestAppContext::new(), TestAppContext::new());
        let relay = Relay::new();
        relay.hold("call").link(&a).link(&b);
        relay.grant("call", Some(1), in_call());
        let one = call(&mut a, 1, true, Vec::new());
        assert_eq!(a.update(&one, |call, _, _| call.mine.clone()), [up(0)]);
        let two = call(&mut b, 2, false, vec![1]);
        let heard = b.update(&two, |call, _, _| {
            (call.theirs.clone(), call.roster.clone())
        });
        assert_eq!(heard, (vec![(1, TrackState::Waiting)], vec![]));
        relay.grant("call", Some(2), in_call());
        b.run_until_parked();
        a.run_until_parked();
        let heard = b.update(&two, |call, _, _| {
            (call.theirs.clone(), call.roster.clone())
        });
        assert_eq!(
            heard,
            (
                vec![(1, TrackState::Waiting), (1, up(0))],
                vec![("mic/1".into(), 1, true)]
            )
        );
        assert_eq!(
            a.update(&one, |call, _, _| call.mine.clone()),
            [up(0), up(1)]
        );
        a.update(&one, |call, _, cx| {
            call.speaking = false;
            cx.notify();
        });
        a.run_until_parked();
        b.run_until_parked();
        let heard = b.update(&two, |call, _, _| {
            (call.theirs.clone(), call.roster.clone())
        });
        assert_eq!(heard.0.last(), Some(&(1, ended(refusal::TRACK_ENDED))));
        assert_eq!(heard.1.last(), Some(&("mic/1".into(), 1, false)));
        a.update(&one, |call, _, cx| {
            call.speaking = true;
            cx.notify();
        });
        a.run_until_parked();
        b.run_until_parked();
        let mine = a.update(&one, |call, _, _| call.mine.clone());
        assert_eq!(mine, [up(0), up(1), up(0), up(1)]);
        let theirs = b.update(&two, |call, _, _| call.theirs.clone());
        assert_eq!(theirs.last(), Some(&(1, up(0))));
        relay.gone(&a);
        b.run_until_parked();
        let theirs = b.update(&two, |call, _, _| call.theirs.clone());
        assert_eq!(theirs.last(), Some(&(1, ended(refusal::PUBLISHER_GONE))));
    }

    #[test]
    fn a_publishing_piece_whose_row_is_taken_back_asks_again_and_waits() {
        let (mut a, mut b) = (TestAppContext::new(), TestAppContext::new());
        let relay = Relay::new();
        relay.hold("call").link(&a).link(&b);
        relay.grant("call", Some(1), in_call());
        relay.grant("call", Some(2), in_call());
        let one = call(&mut a, 1, true, Vec::new());
        let two = call(&mut b, 2, false, vec![1]);
        a.run_until_parked();
        relay.revoke("call", Some(1));
        a.run_until_parked();
        b.run_until_parked();
        let mine = a.update(&one, |call, _, _| call.mine.clone());
        assert_eq!(
            mine,
            [
                up(0),
                up(1),
                ended(refusal::NOT_GRANTED),
                TrackState::Waiting
            ]
        );
        let theirs = b.update(&two, |call, _, _| call.theirs.clone());
        assert_eq!(theirs, [(1, up(0)), (1, ended(refusal::PUBLISHER_GONE))]);
        relay.grant("call", Some(1), in_call());
        a.run_until_parked();
        b.run_until_parked();
        a.run_until_parked();
        let mine = a.update(&one, |call, _, _| call.mine.clone());
        assert_eq!(mine[4..], [up(0), up(1)]);
        let theirs = b.update(&two, |call, _, _| call.theirs.clone());
        assert_eq!(theirs.last(), Some(&(1, up(0))));
    }

    #[test]
    fn a_voice_whose_row_is_taken_back_asks_again_and_waits_for_it() {
        let (mut a, mut b) = (TestAppContext::new(), TestAppContext::new());
        let relay = Relay::new();
        relay.hold("call").link(&a).link(&b);
        relay.grant("call", Some(1), in_call());
        relay.grant("call", Some(2), in_call());
        let _one = call(&mut a, 1, true, Vec::new());
        let two = call(&mut b, 2, false, vec![1]);
        relay.revoke("call", Some(2));
        b.run_until_parked();
        let theirs = b.update(&two, |call, _, _| call.theirs.clone());
        assert_eq!(
            theirs,
            [
                (1, up(0)),
                (1, ended(refusal::NOT_GRANTED)),
                (1, TrackState::Waiting)
            ]
        );
        relay.grant("call", Some(2), in_call());
        b.run_until_parked();
        let theirs = b.update(&two, |call, _, _| call.theirs.clone());
        assert_eq!(theirs.last(), Some(&(1, up(0))));
    }
}
