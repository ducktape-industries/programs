//! Driver-owned host requests and cancellable streams.
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

use crate::methods::{self, Method};
use futures::{Stream, StreamExt};
use std::rc::Rc;

use crate::wire::Request;

/// What one answer carries; the host's refusal is the `Err`.
///
/// A refusal arrives already split into a stable `code` token and the
/// refusing program's own `message` ([`Error`]), so no view parses a
/// transport envelope to find out what happened. A screen shows the
/// `message`.
pub type Answer = Result<Vec<u8>, Error>;

pub use error::Error;

/// The host answered and the bytes are not what this view expected — a decode
/// failure on OUR side, not a refusal anyone authored. One token in one place,
/// so `.map_err(host::malformed)` reads the same in every view.
pub fn malformed(error: String) -> Error {
    Error::new(::error::code::UNEXPECTED_REPLY, error)
}

/// A reply of another variant than the question asks for: the program
/// answered something else.
fn wrong_reply() -> Error {
    malformed("the program answered another question".into())
}

/// One page of a cursored listing: its rows and the cursor of the page after.
pub type Page<T> = (Vec<T>, Option<Vec<u8>>);

/// Reads a cursored listing whole: asks page after page from the first
/// (`ask(None)`), feeding each `next` back, until the listing ends. Nothing
/// here bounds how many pages that is: each page is a host round trip and
/// so a tick of its own, with its own fuel, and the rows gather in the
/// view's memory until the last one lands. Fit for a list that is whole by
/// nature (a roster, a settings list): one a screen searches, counts or
/// draws all of. A history is a [`Paged`](crate::Paged), which reads the
/// pages its list shows.
///
/// A program that rewrites its listing refuses a cursor it handed out
/// before the write as `stale` (`error::code::STALE`). The listing is then
/// read from its first page again, as a [`Paged`](crate::Paged) does; any
/// other refusal is the answer.
pub async fn all_pages<T, F: Future<Output = Result<Page<T>, Error>>>(
    ask: impl FnMut(Option<Vec<u8>>) -> F,
) -> Result<Vec<T>, Error> {
    let (Landed::Next((rows, _)) | Landed::Again((rows, _), _)) =
        walk(ask, usize::MAX, None).await?;
    Ok(rows)
}

/// What a read of a cursored listing brings: the page after a cursor, or
/// the listing from its start and how many pages that was.
pub(crate) enum Landed<T> {
    Next(Page<T>),
    Again(Page<T>, usize),
}

/// The one read of a cursored listing: the page after `after` alone, or
/// with no cursor the listing from its start, `pages` pages or as many as
/// it has. A cursor refused `stale` was handed out before the listing was
/// rewritten, whichever page it asks for: its program says to start the
/// listing over, so the walk does, from the first page, which carries no
/// cursor to refuse.
pub(crate) async fn walk<T, F: Future<Output = Result<Page<T>, Error>>>(
    mut ask: impl FnMut(Option<Vec<u8>>) -> F,
    pages: usize,
    mut after: Option<Vec<u8>>,
) -> Result<Landed<T>, Error> {
    let mut alone = after.is_some();
    let (mut rows, mut read) = (Vec::new(), 0);
    loop {
        let cursored = after.is_some();
        match ask(after).await {
            Err(refusal) if cursored && refusal.code == ::error::code::STALE => {
                (rows, read, after, alone) = (Vec::new(), 0, None, false);
            }
            Err(refusal) => return Err(refusal),
            Ok(page) if alone => return Ok(Landed::Next(page)),
            Ok((more, next)) => {
                rows.extend(more);
                read += 1;
                if read >= pages || next.is_none() {
                    return Ok(Landed::Again((rows, next), read));
                }
                after = next;
            }
        }
    }
}

#[derive(Default)]
struct Slot {
    stream: bool,
    answers: VecDeque<Answer>,
    closed: bool,
    waker: Option<Waker>,
}

#[derive(Default)]
struct Registry {
    next_id: u64,
    outbox: Vec<Request>,
    pending: HashMap<u64, Rc<RefCell<Slot>>>,
    cancels: Vec<u64>,
    diagnostics: HashMap<u64, String>,
}

impl Registry {
    fn ask(&mut self, kind: &str, payload: &[u8]) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.outbox.push(Request {
            id,
            kind: kind.to_string(),
            payload: payload.to_vec(),
        });
        id
    }
}

/// The request channel owned by one driver; clones share only that driver.
#[derive(Clone, Default)]
pub struct Host(Rc<RefCell<Registry>>);

impl Host {
    fn open(&self, kind: &str, payload: &[u8]) -> (u64, Rc<RefCell<Slot>>) {
        let slot = Rc::new(RefCell::new(Slot::default()));
        let mut registry = self.0.borrow_mut();
        let id = registry.ask(kind, payload);
        registry.pending.insert(id, slot.clone());
        (id, slot)
    }
    /// A request still in the outbox never reaches the host: it leaves
    /// the outbox, and no cancel follows it for an id the host never saw.
    fn close(&self, id: u64) {
        let mut registry = self.0.borrow_mut();
        let open = registry.pending.remove(&id).is_some();
        let unsent = registry.outbox.iter().position(|request| request.id == id);
        match unsent {
            Some(at) => {
                registry.outbox.remove(at);
            }
            None if open => registry.cancels.push(id),
            None => {}
        }
        registry.diagnostics.remove(&id);
    }
    pub(crate) fn request(&self, kind: &str, payload: &[u8]) -> Response {
        let (id, slot) = self.open(kind, payload);
        Response {
            id,
            slot,
            host: self.clone(),
        }
    }
    pub(crate) fn raw_subscribe(&self, kind: &str, payload: &[u8]) -> Subscription {
        let (id, slot) = self.open(kind, payload);
        slot.borrow_mut().stream = true;
        Subscription {
            id,
            slot,
            host: self.clone(),
        }
    }
    /// One request, answered once. The method names the kind and both codecs;
    /// there is no other way to ask, so there is no other codec.
    pub fn ask<D: Method>(
        &self,
        request: D::Request,
    ) -> impl Future<Output = Result<D::Reply, Error>> + 'static + use<D> {
        let response = self.request(D::KIND, &D::encode_request(&request));
        self.remember(response.id, &request);
        async move { D::decode_reply(&response.await?).map_err(malformed) }
    }
    /// One question to a program, typed alone ([`program::Ask`]): the
    /// program's `program.query`, with the same bytes, answered with the
    /// reply that answers it, or refused as an unexpected reply when the
    /// program answered another question.
    /// `host.query(identity::ask::List { page })` is a page of accounts.
    pub fn query<A: program::Ask + 'static>(
        &self,
        ask: A,
    ) -> impl Future<Output = Result<A::Reply, Error>> + 'static + use<A> {
        let reply = self.ask::<methods::Query<A::Program>>(ask.into());
        async move { A::answer(reply.await?).ok_or_else(wrong_reply) }
    }
    /// A cursored listing of a program, read whole: `ask` is given the
    /// cursor of each page (`None` first, then each page's `next`) and
    /// says where it goes in the question; the pages are asked one after
    /// another until the listing ends, and their rows come back as one
    /// list. `host.query` reads one page, `query_all` all of them:
    ///
    /// ```ignore
    /// let page = |after| PageRequest { after, limit: None };
    /// let accounts = host.query_all(|after| identity::ask::List { page: page(after) }).await?;
    /// ```
    ///
    /// It is [`all_pages`] over `host.query`, with the same rule for a
    /// cursor refused `stale` (the read starts over from the first page)
    /// and the same cost: every page is a round trip and a tick of its
    /// own. For a listing that is whole by nature; a history is a
    /// [`Paged`](crate::Paged).
    pub fn query_all<A, R>(
        &self,
        mut ask: impl FnMut(Option<Vec<u8>>) -> A,
    ) -> impl Future<Output = Result<Vec<R>, Error>>
    where
        A: program::Ask<Reply = ::store::PageResponse<R>> + 'static,
    {
        all_pages(move |after| {
            let reply = self.query(ask(after));
            async move { reply.await.map(|page| (page.items, page.next)) }
        })
    }
    /// A subscription: an item per answer until the stream is dropped.
    pub fn subscribe<D: Method>(
        &self,
        request: D::Request,
    ) -> impl Stream<Item = Result<D::Reply, Error>> + Unpin + 'static + use<D> {
        let subscription = self.raw_subscribe(D::KIND, &D::encode_request(&request));
        self.remember(subscription.id, &request);
        subscription
            .map(|answer| answer.and_then(|bytes| D::decode_reply(&bytes).map_err(malformed)))
    }
    pub fn put(
        &self,
        blobs: methods::PutBlobs,
    ) -> impl Future<Output = Result<Vec<[u8; 32]>, Error>> + 'static + use<> {
        let mut putting = self.subscribe::<methods::OpPut>(blobs);
        async move {
            while let Some(item) = putting.next().await {
                if let methods::Putting::Held(digests) = item? {
                    return Ok(digests);
                }
            }
            Err(Error::new(
                methods::refusal::REQUEST_CLOSED,
                "the host closed the put before the network held it",
            ))
        }
    }
    /// A request whose answer nobody waits for.
    pub fn notify<D: Method>(&self, request: D::Request) {
        let id = self
            .0
            .borrow_mut()
            .ask(D::KIND, &D::encode_request(&request));
        self.remember(id, &request);
    }
    /// The request as text, for a test's "unhandled request" panic. Views
    /// run as wasm, where nothing reads it, so they skip the formatting.
    fn remember(&self, id: u64, request: &impl std::fmt::Debug) {
        if cfg!(not(target_arch = "wasm32")) {
            self.0
                .borrow_mut()
                .diagnostics
                .insert(id, format!("{request:?}"));
        }
    }
    pub fn log(&self, message: impl AsRef<str>) {
        self.notify::<methods::HostLog>(message.as_ref().to_owned());
    }
    pub fn open_link(&self, link: &str) {
        self.notify::<methods::LinkOpen>(link.to_owned());
    }
    pub(crate) fn diagnostic(&self, id: u64) -> Option<String> {
        self.0.borrow().diagnostics.get(&id).cloned()
    }
    pub(crate) fn pending_requests(&self) -> bool {
        self.0
            .borrow()
            .pending
            .values()
            .any(|slot| !slot.borrow().stream)
    }
    pub(crate) fn waiting_stream(&self, waker: &Waker) -> bool {
        self.0.borrow().pending.values().any(|slot| {
            let slot = slot.borrow();
            slot.stream
                && !slot.closed
                && slot.answers.is_empty()
                && slot
                    .waker
                    .as_ref()
                    .is_some_and(|waiting| waiting.will_wake(waker))
        })
    }
    /// What the view waits for on request `id`.
    pub(crate) fn shape(&self, id: u64) -> Shape {
        match self.0.borrow().pending.get(&id) {
            Some(slot) if slot.borrow().stream => Shape::Subscription,
            Some(_) => Shape::Ask,
            None => Shape::Notify,
        }
    }
}

/// How a request waits: for one answer, for many, or for none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    Ask,
    Subscription,
    Notify,
}

/// The host's eventual answer to a [`Host::ask`].
pub(crate) struct Response {
    id: u64,
    slot: Rc<RefCell<Slot>>,
    host: Host,
}

impl Drop for Response {
    fn drop(&mut self) {
        self.host.close(self.id);
    }
}

impl Future for Response {
    type Output = Answer;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Answer> {
        let mut slot = self.slot.borrow_mut();
        match slot.answers.pop_front() {
            Some(answer) => Poll::Ready(answer),
            None if slot.closed => Poll::Ready(Err(Error::new(
                methods::refusal::REQUEST_CLOSED,
                "the host closed the request",
            ))),
            None => {
                slot.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// Every answer the host sends to a [`Host::subscribe`], until it closes.
pub(crate) struct Subscription {
    id: u64,
    slot: Rc<RefCell<Slot>>,
    host: Host,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.host.close(self.id);
    }
}

impl Stream for Subscription {
    type Item = Answer;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Answer>> {
        let mut slot = self.slot.borrow_mut();
        match slot.answers.pop_front() {
            Some(answer) => {
                slot.waker = None;
                Poll::Ready(Some(answer))
            }
            None if slot.closed => Poll::Ready(None),
            None => {
                slot.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl Host {
    pub(crate) fn drain_outbox(&self) -> Vec<Request> {
        let mut registry = self.0.borrow_mut();
        let keep: std::collections::HashSet<_> = registry
            .pending
            .keys()
            .copied()
            .chain(registry.outbox.iter().map(|request| request.id))
            .collect();
        registry.diagnostics.retain(|id, _| keep.contains(id));
        registry.outbox.drain(..).collect()
    }

    pub(crate) fn drain_cancels(&self) -> Vec<u64> {
        let mut registry = self.0.borrow_mut();
        registry.cancels.drain(..).collect()
    }

    /// Ends a stream: its subscriber reads `None` next; an id nobody waits
    /// for is ignored.
    pub(crate) fn close_stream(&self, id: u64) {
        let slot = self.0.borrow_mut().pending.remove(&id);
        if let Some(slot) = slot {
            let mut slot = slot.borrow_mut();
            slot.closed = true;
            if let Some(waker) = slot.waker.take() {
                waker.wake();
            }
        }
    }

    /// Delivers one answer; an id nobody waits for is dropped.
    pub(crate) fn fulfill(&self, id: u64, answer: Answer, done: bool) {
        let slot = {
            let mut registry = self.0.borrow_mut();
            if done {
                registry.pending.remove(&id)
            } else {
                registry.pending.get(&id).cloned()
            }
        };
        if let Some(slot) = slot {
            let mut slot = slot.borrow_mut();
            slot.answers.push_back(answer);
            slot.closed |= done;
            if let Some(waker) = slot.waker.take() {
                waker.wake();
            }
        }
    }
}

#[cfg(test)]
mod ask_tests {
    use super::Host;
    use crate::methods::{Method, Query};
    use borsh::{BorshDeserialize, BorshSerialize};

    pub struct Shop;
    impl program::Program for Shop {
        const NAME: &'static str = "shop";
        type Op = ();
        type Query = Asked;
        type Reply = Said;

        fn execute(_: &program::ExecCtx, _: Self::Op) -> Result<(), program::Error> {
            unreachable!()
        }

        fn query(_: &program::QueryCtx, _: Self::Query) -> Result<Self::Reply, program::Error> {
            unreachable!()
        }
    }

    /// The program the shop asks what is owed: a question of the shop's
    /// is answered partly from its tables.
    pub struct Ledger;
    impl program::Program for Ledger {
        const NAME: &'static str = "ledger";
        type Op = ();
        type Query = ();
        type Reply = ();

        fn execute(_: &program::ExecCtx, _: Self::Op) -> Result<(), program::Error> {
            unreachable!()
        }

        fn query(_: &program::QueryCtx, _: Self::Query) -> Result<Self::Reply, program::Error> {
            unreachable!()
        }
    }

    /// A table as a program declares one: `#[reads]` asks it `owns` alone.
    struct Table(&'static str);
    impl Table {
        fn owns(&self, key: &[u8]) -> bool {
            key.starts_with(self.0.as_bytes())
        }
    }
    const PRICES: Table = Table("price/");
    const STOCK: Table = Table("stock/");
    const BALANCES: Table = Table("balance/");

    #[derive(Clone, Debug, BorshSerialize, BorshDeserialize, ::program::Ask)]
    #[ask(Shop)]
    pub enum Asked {
        #[ask(Said::Price(u64))]
        #[reads(PRICES)]
        Price { item: String },
        #[ask(Said::Stock { count: u32, at: u64 })]
        #[reads(STOCK, PRICES)]
        Stock(String),
        #[ask(Said::Open(bool))]
        Open,
        /// The stock less what the ledger says is owed.
        #[ask(Said::Free(u32))]
        #[reads(STOCK)]
        #[reads(Ledger: BALANCES)]
        Free(String),
        /// Every item's number, a page at a time.
        #[ask(Said::Items(::store::PageResponse<u8>))]
        Items { after: Option<Vec<u8>> },
    }

    /// A block's change touches the questions that read a table it wrote
    /// to, asked of the query or of its ask type alike, as a block of the
    /// program the follower names: the same key from another program's
    /// block touches nothing, and a question answered partly from that
    /// program's tables is touched by its blocks to those. A question that
    /// declares nothing is touched by every block.
    #[test]
    fn a_change_touches_the_questions_that_read_what_it_wrote() {
        let change = crate::methods::Change {
            height: 3,
            keys: vec![b"stock/tea".to_vec()],
        };
        assert!(!change.touches::<Shop, _>(&ask::Price { item: "tea".into() }));
        assert!(change.touches::<Shop, _>(&ask::Stock("tea".into())));
        assert!(change.touches::<Shop, _>(&ask::Open));
        assert!(change.touches::<Shop, _>(&Asked::Stock("tea".into())));
        assert!(!change.touches::<Shop, _>(&Asked::Price { item: "tea".into() }));
        assert!(change.touches::<Shop, _>(&ask::Free("tea".into())));
        assert!(!change.touches::<Ledger, _>(&Asked::Stock("tea".into())));
        assert!(!change.touches::<Ledger, _>(&ask::Free("tea".into())));
        let elsewhere = crate::methods::Change {
            height: 4,
            keys: vec![b"shelf/1".to_vec()],
        };
        assert!(!elsewhere.touches::<Shop, _>(&ask::Stock("tea".into())));
        assert!(elsewhere.touches::<Shop, _>(&Asked::Open));
        let owed = crate::methods::Change {
            height: 5,
            keys: vec![b"balance/tea".to_vec()],
        };
        assert!(owed.touches::<Ledger, _>(&ask::Free("tea".into())));
        assert!(owed.touches::<Ledger, _>(&Asked::Free("tea".into())));
        assert!(!owed.touches::<Shop, _>(&ask::Free("tea".into())));
        assert!(!owed.touches::<Ledger, _>(&ask::Stock("tea".into())));
        assert!(owed.touches::<Ledger, _>(&ask::Open));
    }

    #[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
    pub enum Said {
        Price(u64),
        Stock { count: u32, at: u64 },
        Open(bool),
        Free(u32),
        Items(::store::PageResponse<u8>),
    }

    /// Reads the shop's items whole with `query_all`, the host answering
    /// each page with what `page` says for its cursor: the rows, and the
    /// cursor of every page asked, in order.
    fn items(
        mut page: impl FnMut(Option<Vec<u8>>) -> Result<::store::PageResponse<u8>, super::Error>,
    ) -> (Result<Vec<u8>, super::Error>, Vec<Option<u8>>) {
        use std::task::{Context, Poll};
        let host = Host::default();
        let mut asked = Vec::new();
        let mut all = Box::pin(host.query_all(|after| ask::Items { after }));
        let mut cx = Context::from_waker(futures::task::noop_waker_ref());
        for _ in 0..32 {
            if let Poll::Ready(rows) = all.as_mut().poll(&mut cx) {
                return (rows, asked);
            }
            for request in host.drain_outbox() {
                let Ok(Asked::Items { after }) = Query::<Shop>::decode_request(&request.payload)
                else {
                    panic!("query_all asks the listing and nothing else");
                };
                asked.push(after.as_ref().map(|cursor| cursor[0]));
                let said = page(after).map(|page| borsh::to_vec(&Said::Items(page)).unwrap());
                host.fulfill(request.id, said, true);
            }
        }
        panic!("the listing never ended: asked {asked:?}");
    }

    /// Items 0..10, three a page.
    fn listing(after: Option<Vec<u8>>) -> Result<::store::PageResponse<u8>, super::Error> {
        let start = after.map_or(0, |cursor| cursor[0]);
        let end = (start + 3).min(10);
        Ok(::store::PageResponse {
            height: 1,
            items: (start..end).collect(),
            next: (end < 10).then(|| vec![end]),
        })
    }

    /// The closure is handed `None`, then each page's `next`, and the rows
    /// of every page come back as one list.
    #[test]
    fn query_all_follows_the_cursor_to_the_end() {
        let (all, asked) = items(listing);
        assert_eq!(all, Ok((0..10).collect::<Vec<_>>()));
        assert_eq!(asked, [None, Some(3), Some(6), Some(9)]);
    }

    /// `query_all` is `all_pages`' walk: a cursor the program refuses
    /// `stale` starts the listing over, and the refusal is never the answer.
    #[test]
    fn a_write_between_two_pages_starts_query_all_over() {
        let mut written = false;
        let (all, asked) = items(|after| {
            // the write lands once, after the first page is answered
            if after.is_some() && !std::mem::replace(&mut written, true) {
                return Err(super::Error::new(
                    ::error::code::STALE,
                    "the listing changed; restart it",
                ));
            }
            listing(after)
        });
        assert_eq!(all, Ok((0..10).collect::<Vec<_>>()));
        assert_eq!(asked, [None, Some(3), None, Some(3), Some(6), Some(9)]);
    }

    /// Asks `ask` and answers it `said`: the bytes it sent, and what the
    /// asker got.
    fn asked<A: program::Ask + 'static>(
        ask: A,
        said: Said,
    ) -> (Vec<u8>, Result<A::Reply, super::Error>) {
        let host = Host::default();
        let answer = host.query(ask);
        let [request] = host.drain_outbox().try_into().unwrap();
        assert_eq!(request.kind, "program.query");
        host.fulfill(request.id, Ok(borsh::to_vec(&said).unwrap()), true);
        (request.payload, futures::executor::block_on(answer))
    }

    /// A question asked alone is the program's own query on the wire, and
    /// gets the reply that answers it; another reply is refused as such.
    #[test]
    fn a_typed_ask_sends_the_query_and_reads_its_own_reply() {
        let (sent, price) = asked(ask::Price { item: "tea".into() }, Said::Price(3));
        let query = Asked::Price { item: "tea".into() };
        assert_eq!(sent, Query::<Shop>::encode_request(&query));
        assert_eq!(price.unwrap(), 3);
        let stock = asked(ask::Stock("tea".into()), Said::Stock { count: 2, at: 9 });
        assert_eq!(stock.1.unwrap(), (2, 9));
        assert!(asked(ask::Open, Said::Open(true)).1.unwrap());
        let wrong = asked(ask::Open, Said::Price(3)).1.unwrap_err();
        assert_eq!(wrong.code, ::error::code::UNEXPECTED_REPLY);
    }

    #[test]
    fn a_put_waits_past_its_progress_for_held() {
        use crate::methods::{OpPut, PutBlobs, Putting};
        let item = |putting: &Putting| Ok(OpPut::encode_reply(putting));
        let host = Host::default();
        let held = host.put(PutBlobs { blobs: Vec::new() });
        let [request] = host.drain_outbox().try_into().unwrap();
        assert_eq!(request.kind, "op.put");
        host.fulfill(request.id, item(&Putting::Sent(3)), false);
        host.fulfill(request.id, item(&Putting::Held(vec![[7; 32]])), true);
        assert_eq!(futures::executor::block_on(held), Ok(vec![[7; 32]]));
        let closed = host.put(PutBlobs { blobs: Vec::new() });
        let [request] = host.drain_outbox().try_into().unwrap();
        host.fulfill(request.id, item(&Putting::Sent(3)), false);
        host.close_stream(request.id);
        let refused = futures::executor::block_on(closed).unwrap_err();
        assert_eq!(refused.code, crate::methods::refusal::REQUEST_CLOSED);
    }
}
