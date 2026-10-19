use crate::{
    host::{Error, Shape, malformed},
    methods::{self, Method},
    wire::{ElementIdWire, Event, Frame, MediaState, Node, Request, TrackState},
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    marker::PhantomData,
    rc::Rc,
};

/// A method kind and the program it addresses (`None` for the host's own).
type Key = (String, Option<String>);
type Handler = Box<dyn FnMut(&Request) -> Option<Event>>;
type Stream = Rc<RefCell<StreamState>>;
type Hook = Box<dyn FnMut(&FakeHost, &Frame, Option<&Node>)>;

#[derive(Default)]
struct State {
    /// What answers an ask (`handle`, `refuse`, `never`).
    asks: HashMap<Key, Handler>,
    /// What answers a subscription instead of a feed (`refuse`).
    refusals: HashMap<Key, Handler>,
    /// One feed per method: the subscriptions it reaches.
    streams: HashMap<Key, Stream>,
    requests: Vec<Request>,
    // the asks a handler left out (`never`), until `answer_held` answers them or the view
    // cancels them
    held: Vec<Request>,
    events: Vec<Event>,
    /// Run after each frame is taken, with no borrow held: what a backing
    /// feeds the view's subscriptions from the frame's effects.
    after_frame: Vec<Hook>,
    declared: Option<&'static [methods::Capability]>,
    /// The camera, microphone and remote pieces of the tree the host
    /// holds, as the app keeps them.
    pieces: Vec<Piece>,
    // The host's own store: it answers the `store.get`, `store.set` and
    // `store.list` no handler takes, as the app's file does.
    kept: BTreeMap<String, Vec<u8>>,
}

/// A piece as the host keeps it, by the path it is filed under: what it
/// opens, its state route and the state last told it (none yet), which the
/// host tells again only when it changes.
struct Piece {
    path: Vec<ElementIdWire>,
    opens: Opens,
    handler: Option<u32>,
    state: Option<MediaState>,
}

/// What the host opens for a piece: a camera or a microphone by its device,
/// a remote piece by its track. A piece that opens another under the same
/// path starts over, as the app opens it anew.
#[derive(PartialEq)]
enum Opens {
    Camera(Option<String>),
    Microphone(Option<String>),
    Remote(methods::LiveTrack),
}

/// A typed host, by the shape of each request:
/// - an ask waits for its answer, so the test says what comes back
///   ([`handle`](Self::handle), [`refuse`](Self::refuse) or
///   [`never`](Self::never)); an ask nothing answers fails the test;
/// - a subscription stays open, quiet until the test feeds it through
///   [`stream`](Self::stream), before or after the view subscribes;
/// - a notify waits for nothing.
///
/// Every request is recorded: [`requests`](Self::requests).
#[derive(Clone, Default)]
pub struct FakeHost(Rc<RefCell<State>>);

fn key<C: Method>() -> Key {
    (C::KIND.to_owned(), C::TARGET.map(str::to_owned))
}

/// The entry for `kind` addressed to `target`, else the one addressed to
/// no program in particular.
fn find<'a, T>(
    map: &'a mut HashMap<Key, T>,
    kind: &str,
    target: Option<&str>,
) -> Option<&'a mut T> {
    let exact = (kind.to_owned(), target.map(str::to_owned));
    if map.contains_key(&exact) {
        return map.get_mut(&exact);
    }
    map.get_mut(&(kind.to_owned(), None))
}

impl FakeHost {
    /// Answers each ask of `C` with what `handler` returns.
    pub fn handle<C: Method>(
        &self,
        mut handler: impl FnMut(C::Request) -> Result<C::Reply, Error> + 'static,
    ) {
        self.0.borrow_mut().asks.insert(
            key::<C>(),
            answer::<C>(move |request| Some(handler(request))),
        );
    }

    pub fn ends<C: Method>(
        &self,
        mut handler: impl FnMut(C::Request) -> Result<C::Reply, Error> + 'static,
    ) {
        self.0.borrow_mut().refusals.insert(
            key::<C>(),
            answer::<C>(move |request| Some(handler(request))),
        );
    }

    /// The host never answers an ask of `C`: it stays pending.
    pub fn never<C: Method>(&self) {
        self.0
            .borrow_mut()
            .asks
            .insert(key::<C>(), answer::<C>(|_| None));
    }

    // Answers now, with what answers `C` by then, every ask of `C` the host held out (`never`)
    // and the view still waits for: a host slow to answer, not one that never does. Each lands
    // on the view's next tick.
    pub fn answer_held<C: Method>(&self) {
        let mut state = self.0.borrow_mut();
        let state = &mut *state;
        let (kind, target) = key::<C>();
        let (asked, kept): (Vec<Request>, Vec<Request>) = std::mem::take(&mut state.held)
            .into_iter()
            .partition(|request| matches::<C>(request));
        state.held = kept;
        for request in asked {
            let answer = find(&mut state.asks, &kind, target.as_deref())
                .expect("answer_held answers with the handler `C` has: give it one first");
            match answer(&request) {
                Some(event) => state.events.push(event),
                None => state.held.push(request),
            }
        }
    }

    /// Refuses every ask of and subscription to `C`.
    pub fn refuse<C: Method>(&self, code: &str, message: &str) {
        let refusal = Error::new(code, message);
        let mut state = self.0.borrow_mut();
        let ask = refusal.clone();
        state
            .asks
            .insert(key::<C>(), answer::<C>(move |_| Some(Err(ask.clone()))));
        state
            .refusals
            .insert(key::<C>(), answer::<C>(move |_| Some(Err(refusal.clone()))));
    }

    /// The feed of `C`'s subscriptions: every one the view opens, before or
    /// after this call, hears what it sends. One feed per method: a second
    /// call hands out the same one.
    pub fn stream<C: Method>(&self) -> StreamSender<C> {
        let state = self
            .0
            .borrow_mut()
            .streams
            .entry(key::<C>())
            .or_default()
            .clone();
        StreamSender {
            state,
            host: self.clone(),
            marker: PhantomData,
        }
    }

    /// Every `C` the view asked, subscribed to or notified, in order.
    pub fn requests<C: Method>(&self) -> Vec<C::Request> {
        self.0
            .borrow()
            .requests
            .iter()
            .filter(|r| matches::<C>(r))
            .map(|r| C::decode_request(&r.payload).expect("valid capability request"))
            .collect()
    }

    // Seeds the host's own store: `value` kept under `key`, borsh-encoded
    // as a view's `store.set` keeps it.
    pub fn keep<T: borsh::BorshSerialize>(&self, key: &str, value: &T) {
        let bytes = methods::encode(value);
        self.0.borrow_mut().kept.insert(key.to_owned(), bytes);
    }

    // What the host's own store keeps under `key`, read as `T`; `None`
    // where it keeps nothing.
    pub fn kept<T: borsh::BorshDeserialize>(&self, key: &str) -> Option<T> {
        let bytes = self.0.borrow().kept.get(key).cloned()?;
        Some(methods::decode(&bytes).expect("a kept value reads as the type asked"))
    }
    /// Answers each ask of `kind` addressed to `target` (`None`: to any
    /// program, where no typed handler names one) with the bytes
    /// `handler` gives for the request's: the fallback a chain-backed host
    /// installs under a test's own typed handlers.
    #[cfg(feature = "rig")]
    pub(crate) fn answer_with(
        &self,
        kind: &str,
        target: Option<&str>,
        mut handler: impl FnMut(&[u8]) -> Result<Vec<u8>, Error> + 'static,
    ) {
        let key = (kind.to_owned(), target.map(str::to_owned));
        let handler = Rc::new(RefCell::new(move |request: &Request| {
            Some(Event::Response {
                id: request.id,
                result: handler(&request.payload),
                done: true,
            })
        }));
        let ends = handler.clone();
        let mut state = self.0.borrow_mut();
        state.asks.insert(
            key.clone(),
            Box::new(move |request| (handler.borrow_mut())(request)),
        );
        state
            .refusals
            .insert(key, Box::new(move |request| (ends.borrow_mut())(request)));
    }

    /// One item, as bytes, to every open subscription to `kind` addressed
    /// to `target`; none open, it reaches nobody and is dropped.
    #[cfg(feature = "rig")]
    pub(crate) fn send_raw(&self, kind: &str, target: Option<&str>, payload: Vec<u8>) {
        let mut state = self.0.borrow_mut();
        let Some(stream) = find(&mut state.streams, kind, target).cloned() else {
            return;
        };
        let stream = stream.borrow();
        if stream.closed {
            return;
        }
        state
            .events
            .extend(stream.ids.iter().map(|id| Event::Response {
                id: *id,
                result: Ok(payload.clone()),
                done: false,
            }));
    }

    /// Runs `hook` on every frame the host takes, with the tree it holds
    /// after it (none for a notifier's), once its requests are answered and
    /// no borrow is held.
    pub(crate) fn after_frame(&self, hook: impl FnMut(&FakeHost, &Frame, Option<&Node>) + 'static) {
        self.0.borrow_mut().after_frame.push(Box::new(hook));
    }

    /// The piece filed under `path` reports the state `update` leaves:
    /// told to its `on_state` on the view's next tick when it is the
    /// piece's first or changed, as the app tells it. A piece the tree does
    /// not hold is not told.
    pub(crate) fn media(&self, path: &[ElementIdWire], update: impl FnOnce(&mut MediaState)) {
        let mut state = self.0.borrow_mut();
        let Some(piece) = state.pieces.iter_mut().find(|piece| piece.path == path) else {
            return;
        };
        let mut next = piece.state.clone().unwrap_or_default();
        update(&mut next);
        if piece.state.as_ref() == Some(&next) {
            return;
        }
        piece.state = Some(next.clone());
        let told = piece.handler.map(|handler| Event::Media {
            handler,
            state: next,
        });
        state.events.extend(told);
    }

    /// The track the piece filed under `path` was last told; `None` before
    /// it was told any, or with no such piece.
    pub(crate) fn told_track(&self, path: &[ElementIdWire]) -> Option<TrackState> {
        let state = self.0.borrow();
        let piece = state.pieces.iter().find(|piece| piece.path == path)?;
        Some(piece.state.as_ref()?.track.clone())
    }

    pub(super) fn reset_connection(&self) {
        let mut state = self.0.borrow_mut();
        state.events.clear();
        for stream in state.streams.values() {
            let mut stream = stream.borrow_mut();
            if let Some(host) = stream.host.take() {
                for id in stream.ids.drain(..) {
                    host.close_stream(id);
                }
            }
        }
    }
    pub(super) fn declare(&self, capabilities: &'static [methods::Capability]) {
        self.0.borrow_mut().declared = Some(capabilities);
    }
    /// Whether `other` is this host, not a copy of it.
    pub(super) fn same(&self, other: &FakeHost) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
    /// Events the host owes the view's next tick, after whatever it owes already.
    pub(super) fn owe(&self, events: Vec<Event>) {
        self.0.borrow_mut().events.extend(events);
    }
    pub(crate) fn take_events(&self) -> Vec<Event> {
        std::mem::take(&mut self.0.borrow_mut().events)
    }
    /// Answers or feed items wait for the view's next tick.
    pub(super) fn owes_events(&self) -> bool {
        !self.0.borrow().events.is_empty()
    }

    /// Takes `frame`, after which the host holds `tree` (none for a
    /// notifier, which has no tree).
    pub(crate) fn accept(&self, frame: &Frame, host: &crate::host::Host, tree: Option<&Node>) {
        let mut state = self.0.borrow_mut();
        if let Some(tree) = tree {
            // a piece keeps its state while its path is in the tree and it
            // opens what it opened; one placed anew, or opening another,
            // starts over, as the app keeps them
            let mut held = std::mem::take(&mut state.pieces);
            state.pieces = super::pieces(tree)
                .into_iter()
                .map(|(path, node)| {
                    let (opens, handler) = match node {
                        Node::Camera {
                            device, on_state, ..
                        } => (Opens::Camera(device.clone()), *on_state),
                        Node::Microphone {
                            device, on_state, ..
                        } => (Opens::Microphone(device.clone()), *on_state),
                        Node::Remote {
                            track, on_state, ..
                        } => (Opens::Remote(track.clone()), *on_state),
                        _ => unreachable!("a piece"),
                    };
                    let state = held
                        .iter()
                        .position(|piece| piece.path == path && piece.opens == opens)
                        .and_then(|at| held.swap_remove(at).state);
                    Piece {
                        path,
                        opens,
                        handler,
                        state,
                    }
                })
                .collect();
        }
        // The app refuses a web, camera, microphone or remote piece from a
        // view whose manifest leaves out what it needs (and ends the view);
        // a test fails on it instead.
        if let Some(capability) = state
            .declared
            .and_then(|declared| frame.undeclared(declared))
        {
            let name = capability.as_str();
            panic!(
                "{}: a {name} piece needs the `{name}` capability, \
                 which this view's CAPABILITIES does not declare",
                methods::refusal::UNDECLARED_CAPABILITY,
            );
        }
        for stream in state.streams.values() {
            let mut stream = stream.borrow_mut();
            stream.host = Some(host.clone());
            stream.ids.retain(|id| !frame.cancels.contains(id));
        }
        state
            .held
            .retain(|request| !frame.cancels.contains(&request.id));
        for request in &frame.requests {
            state.requests.push(request.clone());
            // The app refuses a method the manifest leaves out
            // (`undeclared_capability`); a test fails on it instead.
            // `host.methods` is answered to every view, as the app answers it
            if let (Some(declared), Some((capability, _)), false) = (
                state.declared,
                methods::Capability::of_kind(&request.kind),
                request.kind == methods::HostMethods::KIND,
            ) {
                assert!(
                    declared.contains(&capability),
                    "{}: `{}` needs the `{}` capability, \
                     which this view's CAPABILITIES does not declare",
                    methods::refusal::UNDECLARED_CAPABILITY,
                    request.kind,
                    capability.as_str()
                );
            }
            // a node method may name any program, as the app lets it
            let target = target_of(request);
            let (kind, target) = (request.kind.as_str(), target.as_deref());
            let shown = || {
                host.diagnostic(request.id)
                    .unwrap_or_else(|| String::from_utf8_lossy(&request.payload).into_owned())
            };
            let mut held = false;
            let event = match host.shape(request.id) {
                Shape::Notify => match find(&mut state.asks, kind, target) {
                    Some(answer) => answer(request),
                    None => kept(&mut state.kept, request),
                },
                Shape::Ask => {
                    if let Some(answer) = find(&mut state.asks, kind, target) {
                        let answered = answer(request);
                        held = answered.is_none();
                        answered
                    } else if let Some(answer) = kept(&mut state.kept, request) {
                        Some(answer)
                    } else if find(&mut state.streams, kind, target).is_some() {
                        panic!(
                            "the view asks `{kind}` {}, and `stream` feeds only subscriptions: \
                             answer the ask with `handle`, `refuse` or `never`",
                            shown()
                        )
                    } else {
                        panic!(
                            "unhandled ask `{kind}` {}: the view waits for its answer, so say \
                             what comes back with `handle`, `refuse` or `never`",
                            shown()
                        )
                    }
                }
                Shape::Subscription => {
                    if let Some(refuse) = find(&mut state.refusals, kind, target) {
                        refuse(request)
                    } else {
                        if find(&mut state.streams, kind, target).is_none() {
                            assert!(
                                find(&mut state.asks, kind, target).is_none(),
                                "the view subscribes to `{kind}` {}, and `handle` and `never` \
                                 answer only asks: a subscription stays open by itself, and \
                                 `stream` feeds it",
                                shown()
                            );
                        }
                        let stream = match find(&mut state.streams, kind, target) {
                            Some(stream) => stream.clone(),
                            None => state
                                .streams
                                .entry((kind.to_owned(), target.map(str::to_owned)))
                                .or_default()
                                .clone(),
                        };
                        let mut stream = stream.borrow_mut();
                        stream.host = Some(host.clone());
                        stream.ids.push(request.id);
                        if stream.closed {
                            host.close_stream(request.id);
                        }
                        None
                    }
                }
            };
            if held {
                state.held.push(request.clone());
            }
            state.events.extend(event);
        }
        drop(state);
        let mut hooks = std::mem::take(&mut self.0.borrow_mut().after_frame);
        for hook in &mut hooks {
            hook(self, frame, tree);
        }
        let mut state = self.0.borrow_mut();
        hooks.append(&mut state.after_frame);
        state.after_frame = hooks;
    }
}

/// An answer to one request of `C`: the reply `reply` gives, if any.
fn answer<C: Method>(
    mut reply: impl FnMut(C::Request) -> Option<Result<C::Reply, Error>> + 'static,
) -> Handler {
    Box::new(move |request| {
        let reply = match C::decode_request(&request.payload) {
            Ok(decoded) => reply(decoded)?,
            Err(error) => Err(malformed(error)),
        };
        Some(Event::Response {
            id: request.id,
            result: reply.map(|reply| C::encode_reply(&reply)),
            done: true,
        })
    })
}

// The host's own store's answer to `request`, as the app's file answers it;
// `None` for a kind it does not keep. A key is not empty: the app refuses
// one `malformed_request`, and so does this.
fn kept(kept: &mut BTreeMap<String, Vec<u8>>, request: &Request) -> Option<Event> {
    use methods::{StoreGet, StoreList, StoreSet};
    let named = |key: &str| match key.is_empty() {
        true => Err(Error::new(
            methods::refusal::MALFORMED_REQUEST,
            "a store key is not empty",
        )),
        false => Ok(()),
    };
    let payload = &request.payload;
    let result = if request.kind == methods::HostMethods::KIND {
        // what a host of this build answers: every kind
        Ok(methods::HostMethods::encode_reply(&methods::Hosted {
            level: crate::wire::WIRE_LEVEL,
            kinds: methods::ALL.iter().map(|kind| (*kind).to_owned()).collect(),
        }))
    } else if request.kind == StoreGet::KIND {
        StoreGet::decode_request(payload)
            .map_err(malformed)
            .and_then(|key| {
                named(&key)?;
                Ok(StoreGet::encode_reply(&kept.get(&key).cloned()))
            })
    } else if request.kind == StoreSet::KIND {
        StoreSet::decode_request(payload)
            .map_err(malformed)
            .and_then(|(key, value)| {
                named(&key)?;
                match value {
                    Some(value) => kept.insert(key, value),
                    None => kept.remove(&key),
                };
                Ok(StoreSet::encode_reply(&()))
            })
    } else if request.kind == StoreList::KIND {
        StoreList::decode_request(payload)
            .map_err(malformed)
            .map(|prefix| {
                let keys: Vec<String> = kept
                    .keys()
                    .filter(|key| key.starts_with(&prefix))
                    .cloned()
                    .collect();
                StoreList::encode_reply(&keys)
            })
    } else {
        return None;
    };
    Some(Event::Response {
        id: request.id,
        result,
        done: true,
    })
}

/// The program a node method addresses: `program.changes` names it outright,
/// `program.query` and `op.submit` carry it on their [`methods::Call`]
/// envelope. `None` for every other kind (`program.describe` names a
/// program too, but reads its describe module: no target of the view's).
fn target_of(request: &Request) -> Option<String> {
    match request.kind.as_str() {
        "program.changes" => methods::decode::<String>(&request.payload).ok(),
        "program.query" | "op.submit" => methods::decode::<methods::Call>(&request.payload)
            .ok()
            .map(|call| call.target),
        _ => None,
    }
}

fn matches<C: Method>(request: &Request) -> bool {
    request.kind == C::KIND
        && C::TARGET.is_none_or(|target| target_of(request).as_deref() == Some(target))
}

#[derive(Default)]
struct StreamState {
    ids: Vec<u64>,
    closed: bool,
    host: Option<crate::host::Host>,
}

/// What a test sends down a method's subscriptions.
#[must_use = "the feed is how a test speaks for the host: keep it to send"]
pub struct StreamSender<C: Method> {
    state: Stream,
    host: FakeHost,
    marker: PhantomData<C>,
}
impl<C: Method> StreamSender<C> {
    /// One item to every open subscription; with none open, the item would
    /// reach nobody, and the test fails instead.
    pub fn send(&self, item: C::Reply) {
        let state = self.state.borrow();
        assert!(!state.closed, "cannot send to a closed stream");
        assert!(
            !state.ids.is_empty(),
            "no subscription to `{}` is open: the view has not subscribed (open it \
             first) or it dropped the stream, and the item would reach nobody",
            C::KIND
        );
        let payload = C::encode_reply(&item);
        self.host
            .0
            .borrow_mut()
            .events
            .extend(state.ids.iter().map(|id| Event::Response {
                id: *id,
                result: Ok(payload.clone()),
                done: false,
            }));
    }
    /// Whether the view holds a subscription this feed reaches.
    pub fn subscribed(&self) -> bool {
        !self.state.borrow().ids.is_empty()
    }
    pub fn close(&self) {
        let mut state = self.state.borrow_mut();
        state.closed = true;
        let host = state.host.clone();
        for id in state.ids.drain(..) {
            host.as_ref().expect("active stream host").close_stream(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::methods::{Changes, Program, Query};

    struct First;
    struct Second;
    macro_rules! program {
        ($name:ident, $target:literal) => {
            impl Program for $name {
                const NAME: &'static str = $target;
                type Op = ();
                type Query = String;
                type Reply = String;

                fn execute(_: &program::ExecCtx, _: Self::Op) -> Result<(), program::Error> {
                    unreachable!()
                }

                fn query(
                    _: &program::QueryCtx,
                    _: Self::Query,
                ) -> Result<Self::Reply, program::Error> {
                    unreachable!()
                }
            }
        };
    }
    program!(First, "first");
    program!(Second, "second");

    fn request<C: Method>(id: u64, value: C::Request) -> Request {
        Request {
            id,
            kind: C::KIND.into(),
            payload: C::encode_request(&value),
        }
    }

    #[test]
    fn handlers_and_request_history_distinguish_envelope_targets() {
        let host = FakeHost::default();
        host.handle::<Query<First>>(|query| Ok(format!("first:{query}")));
        host.handle::<Query<Second>>(|query| Ok(format!("second:{query}")));
        host.accept(
            &Frame {
                requests: vec![
                    request::<Query<First>>(1, "one".into()),
                    request::<Query<Second>>(2, "two".into()),
                ],
                ..Frame::default()
            },
            &crate::host::Host::default(),
            None,
        );
        assert_eq!(host.requests::<Query<First>>(), ["one"]);
        assert_eq!(host.requests::<Query<Second>>(), ["two"]);
        let events = host.take_events();
        assert!(
            matches!(&events[0], Event::Response { id: 1, result: Ok(bytes), done: true } if Query::<First>::decode_reply(bytes).unwrap() == "first:one")
        );
        assert!(
            matches!(&events[1], Event::Response { id: 2, result: Ok(bytes), done: true } if Query::<Second>::decode_reply(bytes).unwrap() == "second:two")
        );
    }

    /// Sends what `channel` asked since the last frame to `host`.
    fn flush(host: &FakeHost, channel: &crate::host::Host) {
        host.accept(
            &Frame {
                requests: channel.drain_outbox(),
                cancels: channel.drain_cancels(),
                ..Frame::default()
            },
            channel,
            None,
        );
    }

    #[test]
    fn streams_stop_delivering_to_cancelled_subscriptions() {
        let host = FakeHost::default();
        let feed = host.stream::<Changes<First>>();
        let channel = crate::host::Host::default();
        let stream = channel.subscribe::<Changes<First>>(());
        flush(&host, &channel);
        feed.send(None);
        assert_eq!(host.take_events().len(), 1);
        drop(stream);
        flush(&host, &channel);
        assert!(!feed.subscribed());
    }

    #[test]
    fn a_subscription_nothing_feeds_stays_open_and_a_later_feed_reaches_it() {
        use futures::StreamExt;
        let host = FakeHost::default();
        let channel = crate::host::Host::default();
        let mut stream = channel.subscribe::<Changes<First>>(());
        flush(&host, &channel);
        assert_eq!(host.requests::<Changes<First>>().len(), 1);
        assert!(host.take_events().is_empty());
        let feed = host.stream::<Changes<First>>();
        feed.send(None);
        for event in host.take_events() {
            let crate::wire::Event::Response { id, result, done } = event else {
                panic!("an answer")
            };
            channel.fulfill(id, result, done);
        }
        assert!(futures::executor::block_on(stream.next()).is_some());
    }

    #[test]
    fn a_second_feed_of_one_method_is_the_first() {
        let host = FakeHost::default();
        let first = host.stream::<Changes<First>>();
        let channel = crate::host::Host::default();
        let _stream = channel.subscribe::<Changes<First>>(());
        flush(&host, &channel);
        let second = host.stream::<Changes<First>>();
        second.send(None);
        assert_eq!(host.take_events().len(), 1);
        first.send(None);
        assert_eq!(host.take_events().len(), 1);
    }

    #[test]
    #[should_panic(expected = "no subscription to `program.changes` is open")]
    fn a_feed_nobody_hears_fails_the_test() {
        FakeHost::default().stream::<Changes<First>>().send(None);
    }

    #[test]
    #[should_panic(expected = "the view asks `program.query`")]
    fn a_feed_for_an_ask_names_the_mismatch() {
        let host = FakeHost::default();
        let _feed = host.stream::<Query<First>>();
        let channel = crate::host::Host::default();
        let _ask = channel.ask::<Query<First>>("one".into());
        flush(&host, &channel);
    }

    #[test]
    #[should_panic(expected = "the view subscribes to `program.changes`")]
    fn an_answer_for_a_subscription_names_the_mismatch() {
        let host = FakeHost::default();
        host.never::<Changes<First>>();
        let channel = crate::host::Host::default();
        let _stream = channel.subscribe::<Changes<First>>(());
        flush(&host, &channel);
    }

    #[test]
    fn a_notify_is_recorded() {
        let host = FakeHost::default();
        let channel = crate::host::Host::default();
        channel.log("hello");
        flush(&host, &channel);
        assert_eq!(host.requests::<crate::methods::HostLog>(), ["hello"]);
        assert!(host.take_events().is_empty());
    }

    #[test]
    fn one_capability_can_handle_asks_and_subscriptions_independently() {
        let host = FakeHost::default();
        host.handle::<Query<First>>(|query| Ok(format!("answer:{query}")));
        let feed = host.stream::<Query<First>>();
        let channel = crate::host::Host::default();
        let _ask = channel.ask::<Query<First>>("ask".into());
        let _stream = channel.subscribe::<Query<First>>("subscribe".into());
        flush(&host, &channel);
        let replies = host.take_events();
        assert!(matches!(
            &replies[..],
            [Event::Response {
                id: 0,
                done: true,
                ..
            }]
        ));
        feed.send("item".into());
        assert!(matches!(
            &host.take_events()[..],
            [Event::Response {
                id: 1,
                done: false,
                ..
            }]
        ));
    }

    #[test]
    fn closing_a_feed_finishes_without_fabricating_an_item_or_refusal() {
        use futures::StreamExt;
        let host = FakeHost::default();
        let feed = host.stream::<Changes<First>>();
        let channel = crate::host::Host::default();
        let mut stream = channel.subscribe::<Changes<First>>(());
        flush(&host, &channel);
        feed.close();
        assert!(futures::executor::block_on(stream.next()).is_none());
    }

    // Sends what `channel` asked to `host`, and hands its answers back.
    fn answered(host: &FakeHost, channel: &crate::host::Host) {
        flush(host, channel);
        for event in host.take_events() {
            if let Event::Response { id, result, done } = event {
                channel.fulfill(id, result, done);
            }
        }
    }

    // A store no handler takes is the host's own: what the view sets it
    // reads back and lists, a removed key is gone, an empty key is
    // refused as the app refuses it, and the test reads and seeds it.
    #[test]
    fn a_store_nothing_handles_is_the_hosts_own() {
        use crate::store;
        use futures::executor::block_on;
        let host = FakeHost::default();
        let channel = crate::host::Host::default();
        host.keep("rooms/seen", &3u64);
        store::set(&channel, "rooms/collapsed", Some(&vec!["work".to_owned()]));
        store::set(&channel, "emoji", Some(&1u8));
        store::remove(&channel, "emoji");
        let collapsed = store::get::<Vec<String>>(&channel, "rooms/collapsed");
        let rooms = store::list(&channel, "rooms/");
        let every = store::list(&channel, "");
        let unnamed = store::get::<u8>(&channel, "");
        answered(&host, &channel);
        assert_eq!(block_on(collapsed), Ok(Some(vec!["work".to_owned()])));
        let listed = ["rooms/collapsed", "rooms/seen"].map(str::to_owned);
        assert_eq!(block_on(rooms), Ok(listed.to_vec()));
        assert_eq!(block_on(every), Ok(listed.to_vec()));
        let refused = block_on(unnamed).unwrap_err();
        assert_eq!(refused.code, methods::refusal::MALFORMED_REQUEST);
        assert_eq!(host.kept::<u8>("emoji"), None);
        assert_eq!(host.kept::<u64>("rooms/seen"), Some(3));
        assert_eq!(host.requests::<methods::StoreSet>().len(), 3);
    }

    // A test's own store handler answers in the host's own store's place.
    #[test]
    fn a_store_handler_wins_over_the_hosts_own() {
        use futures::executor::block_on;
        let host = FakeHost::default();
        host.keep("a", &1u8);
        host.handle::<methods::StoreGet>(|_| Ok(None));
        let channel = crate::host::Host::default();
        let read = crate::store::get::<u8>(&channel, "a");
        answered(&host, &channel);
        assert_eq!(block_on(read), Ok(None));
    }

    // A host older than `store.list` refuses it, and the view hears the
    // refusal it falls back on.
    #[test]
    fn an_old_host_refuses_a_list() {
        use futures::executor::block_on;
        let host = FakeHost::default();
        host.refuse::<methods::StoreList>(methods::refusal::UNKNOWN_REQUEST, "no such method");
        let channel = crate::host::Host::default();
        let listed = crate::store::list(&channel, "");
        answered(&host, &channel);
        let refused = block_on(listed).unwrap_err();
        assert_eq!(refused.code, methods::refusal::UNKNOWN_REQUEST);
    }

    #[test]
    #[should_panic(expected = "unhandled ask `program.query`")]
    fn an_ask_nothing_answers_fails_at_the_host_boundary() {
        let host = FakeHost::default();
        let channel = crate::host::Host::default();
        let _ask = channel.ask::<Query<First>>("unexpected".into());
        flush(&host, &channel);
    }
}
