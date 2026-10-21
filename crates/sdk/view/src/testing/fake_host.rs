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

type Key = (String, Option<String>);
type Handler = Box<dyn FnMut(&Request) -> Option<Event>>;
type Stream = Rc<RefCell<StreamState>>;
type Hook = Box<dyn FnMut(&FakeHost, &Frame, Option<&Node>)>;

#[derive(Default)]
struct State {
    asks: HashMap<Key, Handler>,
    refusals: HashMap<Key, Handler>,
    streams: HashMap<Key, Stream>,
    requests: Vec<Request>,
    // the asks a handler left out (`never`), until `answer_held` answers them or the view
    // cancels them
    held: Vec<Request>,
    events: Vec<Event>,
    after_frame: Vec<Hook>,
    declared: Option<&'static [methods::Capability]>,
    pieces: Vec<Piece>,
    kept: BTreeMap<String, Vec<u8>>,
    secrets: BTreeMap<String, Vec<u8>>,
    standing: HashMap<String, Vec<Watcher>>,
}

struct Piece {
    path: Vec<ElementIdWire>,
    opens: Opens,
    handler: Option<u32>,
    state: Option<MediaState>,
}

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

    pub fn stand<P: methods::Program>(&self) -> Standings<P> {
        let mut state = self.0.borrow_mut();
        state.standing.entry(P::NAME.to_owned()).or_default();
        Standings {
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

    pub fn keep_secret<T: borsh::BorshSerialize>(&self, key: &str, value: &T) {
        let bytes = methods::encode(value);
        self.0.borrow_mut().secrets.insert(key.to_owned(), bytes);
    }

    pub fn secret<T: borsh::BorshDeserialize>(&self, key: &str) -> Option<T> {
        let bytes = self.0.borrow().secrets.get(key).cloned()?;
        Some(methods::decode(&bytes).expect("a secret reads as the type asked"))
    }
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

    pub(crate) fn after_frame(&self, hook: impl FnMut(&FakeHost, &Frame, Option<&Node>) + 'static) {
        self.0.borrow_mut().after_frame.push(Box::new(hook));
    }

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

    pub(crate) fn told_track(&self, path: &[ElementIdWire]) -> Option<TrackState> {
        let state = self.0.borrow();
        let piece = state.pieces.iter().find(|piece| piece.path == path)?;
        Some(piece.state.as_ref()?.track.clone())
    }

    pub(super) fn reset_connection(&self) {
        let mut state = self.0.borrow_mut();
        state.events.clear();
        for watchers in state.standing.values_mut() {
            watchers.clear();
        }
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
    pub(super) fn same(&self, other: &FakeHost) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
    pub(super) fn owe(&self, events: Vec<Event>) {
        self.0.borrow_mut().events.extend(events);
    }
    pub(crate) fn take_events(&self) -> Vec<Event> {
        std::mem::take(&mut self.0.borrow_mut().events)
    }
    pub(super) fn owes_events(&self) -> bool {
        !self.0.borrow().events.is_empty()
    }

    pub(crate) fn accept(&self, frame: &Frame, host: &crate::host::Host, tree: Option<&Node>) {
        let mut state = self.0.borrow_mut();
        if let Some(tree) = tree {
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
        for watchers in state.standing.values_mut() {
            watchers.retain(|watcher| !frame.cancels.contains(&watcher.id));
        }
        for request in &frame.requests {
            state.requests.push(request.clone());
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
                    None => kept(&mut state, request),
                },
                Shape::Ask => {
                    if let Some(answer) = find(&mut state.asks, kind, target) {
                        let answered = answer(request);
                        held = answered.is_none();
                        answered
                    } else if let Some(answer) = kept(&mut state, request) {
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
                    } else if let Some(answer) = stood(&mut state, request, target) {
                        Some(answer)
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

fn kept(state: &mut State, request: &Request) -> Option<Event> {
    use methods::{StoreGet, StoreList, StoreSecretGet, StoreSecretSet, StoreSet};
    let kept = &mut state.kept;
    let secrets = &mut state.secrets;
    let named = |key: &str| match key.is_empty() {
        true => Err(Error::new(
            methods::refusal::MALFORMED_REQUEST,
            "a store key is not empty",
        )),
        false => Ok(()),
    };
    let payload = &request.payload;
    let result = if request.kind == methods::HostMethods::KIND {
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
    } else if request.kind == StoreSecretGet::KIND {
        StoreSecretGet::decode_request(payload)
            .map_err(malformed)
            .and_then(|key| {
                named(&key)?;
                Ok(StoreSecretGet::encode_reply(&secrets.get(&key).cloned()))
            })
    } else if request.kind == StoreSecretSet::KIND {
        StoreSecretSet::decode_request(payload)
            .map_err(malformed)
            .and_then(|(key, value)| {
                named(&key)?;
                match value {
                    Some(value) => secrets.insert(key, value),
                    None => secrets.remove(&key),
                };
                Ok(StoreSecretSet::encode_reply(&()))
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

fn target_of(request: &Request) -> Option<String> {
    match request.kind.as_str() {
        "program.changes" => methods::decode::<String>(&request.payload).ok(),
        "program.query" | "op.submit" | WATCH => methods::decode::<methods::Call>(&request.payload)
            .ok()
            .map(|call| call.target),
        _ => None,
    }
}

fn matches<C: Method>(request: &Request) -> bool {
    request.kind == C::KIND
        && C::TARGET.is_none_or(|target| target_of(request).as_deref() == Some(target))
}

const WATCH: &str = "program.watch";

struct Watcher {
    id: u64,
    question: Request,
    last: Option<Vec<u8>>,
}

fn stood(state: &mut State, request: &Request, target: Option<&str>) -> Option<Event> {
    let fed = find(&mut state.streams, &request.kind, target).is_some();
    let stands_here = request.kind == WATCH && !fed;
    if !stands_here {
        return None;
    }
    let target = target.unwrap_or_default().to_owned();
    if !state.standing.contains_key(&target) {
        return Some(Event::Response {
            id: request.id,
            result: Err(Error::new(
                ::error::code::UNSUPPORTED,
                "this host stands no question: the view reads it",
            )),
            done: true,
        });
    }
    let question = Request {
        id: request.id,
        kind: "program.query".to_owned(),
        payload: request.payload.clone(),
    };
    Some(match answered(state, &target, &question) {
        Ok(answer) => {
            let watcher = Watcher {
                id: request.id,
                question,
                last: Some(answer.clone()),
            };
            state.standing.entry(target).or_default().push(watcher);
            Event::Response {
                id: request.id,
                result: Ok(pushed_answer(answer)),
                done: false,
            }
        }
        Err(refusal) => Event::Response {
            id: request.id,
            result: Err(refusal),
            done: true,
        },
    })
}

fn answered(state: &mut State, target: &str, question: &Request) -> Result<Vec<u8>, Error> {
    let answer = find(&mut state.asks, "program.query", Some(target)).expect(
        "a standing question is answered by the program's `Query` handler: give it one with `handle`",
    );
    match answer(question) {
        Some(Event::Response { result, .. }) => result,
        _ => panic!("a standing question is answered at once: `never` holds none"),
    }
}

fn pushed_answer(answer: Vec<u8>) -> Vec<u8> {
    let mut pushed = vec![1];
    pushed.extend(answer);
    pushed
}

/// The node's side of a program's standing questions in a test: each is answered at once by the program's `Query` handler, and again when the test says a block moved it.
#[must_use = "the node's side is how a test moves the answers: keep it"]
pub struct Standings<P> {
    host: FakeHost,
    marker: PhantomData<P>,
}

impl<P: methods::Program> Standings<P> {
    pub fn moved(&self) {
        let mut state = self.host.0.borrow_mut();
        let state = &mut *state;
        let mut watchers = state.standing.remove(P::NAME).unwrap_or_default();
        for watcher in &mut watchers {
            let (result, last) = match answered(state, P::NAME, &watcher.question) {
                Ok(answer) if watcher.last.as_ref() == Some(&answer) => continue,
                Ok(answer) => (Ok(pushed_answer(answer.clone())), Some(answer)),
                Err(refusal) => (Err(refusal), None),
            };
            watcher.last = last;
            state.events.push(Event::Response {
                id: watcher.id,
                result,
                done: false,
            });
        }
        state.standing.insert(P::NAME.to_owned(), watchers);
    }

    pub fn could_not_answer(&self) {
        let mut state = self.host.0.borrow_mut();
        let state = &mut *state;
        let watchers = state.standing.get(P::NAME).into_iter().flatten();
        let moved = watchers.map(|watcher| Event::Response {
            id: watcher.id,
            result: Ok(vec![0]),
            done: false,
        });
        state.events.extend(moved.collect::<Vec<_>>());
    }

    pub fn standing(&self) -> usize {
        self.host
            .0
            .borrow()
            .standing
            .get(P::NAME)
            .map_or(0, Vec::len)
    }
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

    fn answered(host: &FakeHost, channel: &crate::host::Host) {
        flush(host, channel);
        for event in host.take_events() {
            if let Event::Response { id, result, done } = event {
                channel.fulfill(id, result, done);
            }
        }
    }

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

    #[test]
    fn a_secret_nothing_handles_is_kept_apart_from_the_store() {
        use crate::store;
        use futures::executor::block_on;
        let host = FakeHost::default();
        let channel = crate::host::Host::default();
        host.keep("dc/2", &1u8);
        host.keep_secret("dc/4", &4u8);
        let kept = store::secret_set(&channel, "dc/2", Some(&7u8));
        let read = store::secret_get::<u8>(&channel, "dc/2");
        let stored = store::get::<u8>(&channel, "dc/2");
        let dropped = store::secret_set::<u8>(&channel, "dc/4", None);
        let unnamed = store::secret_get::<u8>(&channel, "");
        answered(&host, &channel);
        assert_eq!(block_on(kept), Ok(()));
        assert_eq!(block_on(read), Ok(Some(7)));
        assert_eq!(block_on(stored), Ok(Some(1)));
        assert_eq!(block_on(dropped), Ok(()));
        let refused = block_on(unnamed).unwrap_err();
        assert_eq!(refused.code, methods::refusal::MALFORMED_REQUEST);
        assert_eq!(host.secret::<u8>("dc/2"), Some(7));
        assert_eq!(host.secret::<u8>("dc/4"), None);
        assert_eq!(host.kept::<u8>("dc/2"), Some(1));
    }

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
