//! The loaders: the head and the window it pulls (`chain.status`, `chain.blocks`), the lists the
//! system programs answer, and what an op says it does (`program.describe`). A list is read with
//! `cx.load`, which keeps what is on screen while it reads again and draws only what changed.
use std::future::Future;

use view::Paged;
use view::host::Page;
use view::prelude::*;

use crate::chain::{self, BlockRow, TxRow};
use crate::state::{Listing, Route, blob_ref};
use crate::{Explorer, WINDOW, decode, queries};

/// The newest frames of an account a phone reads the transactions of
/// (Expo `ACTED`, `query/page.ts:12`): its first page.
const ACTED: usize = queries::PAGE as usize;
/// The payload a row's transaction is read with (Expo `ROW_PAYLOAD`,
/// `query/page.ts:10`): past it, the row is titled by its size.
const ROW_PAYLOAD: u32 = 4 << 10;

impl Explorer {
    /// Everything, again: the boot, a restore, a retry.
    pub(crate) fn read_all(&mut self, cx: &mut Context<Self>) {
        self.read_head(cx);
        self.read_accounts(cx);
        self.read_validators(cx);
        self.read_network(cx);
        self.read_used(cx);
        self.read_page(cx);
        self.read_acted(cx);
        self.rows_of = None;
        self.manifest_of = None;
        self.read_manifest(cx);
        self.pull(cx);
    }

    /// What the code behind the program page on screen declares, asked once
    /// per code id (`blob.manifest`): on every draw, so a page opened before
    /// the roster landed asks when it does, and a page that names other
    /// code blanks the one shown. Nothing to ask while no program page is
    /// open or the roster does not name it.
    pub(crate) fn read_manifest(&mut self, cx: &mut Context<Self>) {
        let Route::Program(name) = self.pages.top() else {
            return;
        };
        let Some(code) = self.code_of(name) else {
            return;
        };
        let id = blob_ref(&code);
        if self.manifest_of.as_deref() == Some(id.as_str()) {
            return;
        }
        self.manifest_of = Some(id.clone());
        self.manifest = Loadable::Idle;
        let ask = cx.host().ask::<BlobManifest>(id);
        cx.load(self, ask, |view| &mut view.manifest);
    }

    /// A head `chain.heads` pushed: the window follows it without a read,
    /// and the status moves to it when the page lands. A head is not drawn
    /// on its own: that redraw changed nothing (the page lands a tick later
    /// and draws once) and cost a full render of the page that was up, 74M
    /// fuel on Transactions, every block.
    pub(crate) fn at_head(&mut self, head: Head, cx: &mut Context<Self>) {
        // a change lands at its height without a write to the programs role, so
        // the programs role's live heads say nothing: the head that reaches it
        // reads the roster again
        if let Loadable::Ready(network) = &*self.network
            && network
                .changes
                .first()
                .is_some_and(|change| change.height <= head.height)
        {
            self.read_network(cx);
        }
        match &self.status {
            Loadable::Ready(_) | Loadable::Reloading(_, _) => {
                self.head = self.head.max(head.height);
                self.pull(cx);
            }
            _ => self.read_head(cx),
        }
    }

    /// The node's status, and the window it moves. A status on screen stays
    /// while it is read again; a refused re-read is logged and leaves it.
    pub(crate) fn read_head(&mut self, cx: &mut Context<Self>) {
        let ask = cx.host().ask::<ChainStatus>(());
        if self.status.ready().is_some() {
            self.rereading_status = Some(cx.land(ask, |view, status, cx| match status {
                Ok(status) => {
                    if view.status.ready() != Some(&status) {
                        view.status = Loadable::Ready(status);
                        cx.notify();
                    }
                    view.pull(cx);
                }
                Err(refusal) => cx.log_refused("the node's status", &refusal),
            }));
        } else if !self.status.is_loading() {
            cx.load(self, ask, |view| &mut view.status);
        }
        // a failed window is read again with the head, not in a loop
        if self.chain.failed.is_some() {
            self.pull(cx);
        }
    }

    // Each list is read again on its program's live heads, which on a busy network come nearly
    // every block: a head heard while the list is read lets that read land and reads once more
    // after it (`Reread`).
    pub(crate) fn read_accounts(&mut self, cx: &mut Context<Self>) {
        cx.reread(
            self,
            |view| &mut view.accounts,
            |_, cx| queries::accounts(cx.host()),
            Self::read_accounts,
        );
    }

    pub(crate) fn read_validators(&mut self, cx: &mut Context<Self>) {
        cx.reread(
            self,
            |view| &mut view.validators,
            |_, cx| queries::validators(cx.host()),
            Self::read_validators,
        );
    }

    pub(crate) fn read_network(&mut self, cx: &mut Context<Self>) {
        cx.reread(
            self,
            |view| &mut view.network,
            |_, cx| queries::network(cx.host()),
            Self::read_network,
        );
    }

    pub(crate) fn read_used(&mut self, cx: &mut Context<Self>) {
        cx.reread(
            self,
            |view| &mut view.used,
            |_, cx| queries::used(cx.host()),
            Self::read_used,
        );
    }

    /// What the account page on screen did, newest first, a page at a time;
    /// its first page again when it is asked again, as
    /// [`reread_first`](Self::reread_first) says.
    pub(crate) fn read_acted(&mut self, cx: &mut Context<Self>) {
        let Route::Account(number) = *self.pages.top() else {
            return;
        };
        let host = cx.host();
        if self.acted(number).is_some() {
            let first = move || queries::acted(host, number, None);
            return self.reread_first(
                first,
                move |view| {
                    let (account, acted) = view.acted.as_mut()?;
                    (*account == number).then_some(acted)
                },
                cx,
            );
        }
        let page = move |after| queries::acted(host.clone(), number, after);
        self.acted = Some((number, Listing::new(page, cx)));
    }

    /// A write to activity, for the listing `listing` picks: its first page
    /// again. While that is all it holds, it is read again in place
    /// ([`Paged::reread`]), the rows on screen staying until it lands. A
    /// listing read on past it is not: `reread` reads every page held from
    /// the first, and with a write nearly every block (garage's bots) a
    /// listing read far enough never lands that read before the next write
    /// starts it again, nor reads on while it is out. Its first page is
    /// asked alone instead (`first`), and one that starts with a row it
    /// does not hold marks it [`behind`](Listing::behind) until it starts
    /// over ([`show_newest`](Self::show_newest), or arriving on a page), or
    /// is back to one page (a page refused empties it). A listing whose
    /// first page is still on its way is left to land.
    fn reread_first<F>(
        &mut self,
        first: impl FnOnce() -> F,
        listing: impl Fn(&mut Explorer) -> Option<&mut Listing> + 'static,
        cx: &mut Context<Self>,
    ) where
        F: Future<Output = Result<Page<activity::Frame>, Error>> + 'static,
    {
        let Some(held) = listing(self) else {
            return;
        };
        if held.rows.read(Paged::is_loading) {
            return;
        }
        // a listing back to one page (a refused page empties it) is read
        // again whatever it was told before
        if !held.past_first_page() {
            held.behind = false;
            return held.rows.update(cx, |rows, cx| rows.reread(cx));
        }
        if held.behind {
            return;
        }
        let newest = held.rows.read(|rows| rows.rows().first().cloned());
        let checked = cx.land(first(), move |view, page, cx| match page {
            Ok((rows, _)) => {
                if rows.first() != newest.as_ref()
                    && let Some(listing) = listing(view)
                    && listing.past_first_page()
                {
                    listing.behind = true;
                    cx.notify();
                }
            }
            Err(refusal) => cx.log_refused("activity's newest frames", &refusal),
        });
        held.checking = Some(checked);
    }

    /// A listing read on past its first page starts over at it: the page
    /// the reader arrives on opens at its top.
    pub(crate) fn start_over(&mut self, cx: &mut Context<Self>) {
        self.start_over_where(Listing::past_first_page, cx);
    }

    /// Show newest: a listing behind the rows written above it starts over
    /// at its first page, its list at the top.
    pub(crate) fn show_newest(&mut self, cx: &mut Context<Self>) {
        self.start_over_where(|listing| listing.behind, cx);
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    /// The listings `stale` picks, read anew from their first page.
    fn start_over_where(&mut self, stale: impl Fn(&Listing) -> bool, cx: &mut Context<Self>) {
        if self.acted.as_ref().is_some_and(|(_, acted)| stale(acted)) {
            self.acted = None;
            self.read_acted(cx);
        }
    }

    pub(crate) fn pull(&mut self, cx: &mut Context<Self>) {
        if self.pulling {
            return;
        }
        let head = self
            .status
            .ready()
            .map(|status| status.height.max(self.head));
        // a page asks for the blocks the window lacks, never more: the node
        // lists every block it keeps to a page that asks for them all
        // (garage's 56k, 14.6 MB decoded in one call to keep the newest
        // 1,000, ran the view out of fuel)
        let (before, lacking) = match (self.chain.top(), head) {
            (None, _) => (None, WINDOW),
            (Some(top), Some(head)) if head > top => (
                None,
                usize::try_from(head - top).map_or(WINDOW, |new| new.min(WINDOW)),
            ),
            _ if !self.chain.complete && self.chain.blocks.len() < WINDOW => (
                self.chain.blocks.last().map(|block| block.height),
                WINDOW - self.chain.blocks.len(),
            ),
            _ => return,
        };
        // at most the window
        let limit = lacking as u32;
        self.pulling = true;
        let (host, top) = (cx.host(), self.chain.top());
        let read = async move {
            let mut page = host
                .ask::<ChainBlocks>(BlockPage {
                    before,
                    limit,
                    most: 0,
                })
                .await?;
            // the node's tip moves on while the ask travels, so a page read
            // down from it can stop short of the window: read on below it
            // until it joins (Expo `advance`), and the window, the rows on
            // screen, stays whole until it lands. An answer a whole window
            // past the top starts it again.
            let Some(top) = top.filter(|_| before.is_none()) else {
                return Ok(page);
            };
            let newest = page.blocks.first().map_or(top, |block| block.height);
            while let Some(oldest) = page.blocks.last().map(|block| block.height)
                && oldest > top + 1
                && newest.saturating_sub(top) <= WINDOW as u64
            {
                let below = host
                    .ask::<ChainBlocks>(BlockPage {
                        before: Some(oldest),
                        limit: u32::try_from(oldest - top - 1).unwrap_or(u32::MAX),
                        most: 0,
                    })
                    .await?;
                match below.blocks.last() {
                    // the archive moved past the window: it starts again
                    None => break,
                    // an answer that reaches no lower moves nothing: the
                    // window stays, and the next head reads again
                    Some(block) if block.height >= oldest => {
                        page.blocks.clear();
                        break;
                    }
                    Some(_) => {}
                }
                page.listed_from = below.listed_from;
                page.blocks.extend(below.blocks);
            }
            Ok::<_, Error>(page)
        };
        cx.land(read, move |view, page, cx| {
            view.pulling = false;
            match page {
                Ok(page) => {
                    let was = (view.chain.top(), view.chain.blocks.len());
                    view.chain.land(before, page);
                    // the status moves with the page that reached the head
                    if let (Loadable::Ready(status) | Loadable::Reloading(status, _), Some(top)) =
                        (&mut view.status, view.chain.blocks.first())
                        && top.height > status.height
                    {
                        status.height = top.height;
                        status.tip = top.id;
                    }
                    // a page that moved nothing is not asked for again
                    // until the head moves
                    if (view.chain.top(), view.chain.blocks.len()) != was {
                        view.pull(cx);
                    }
                }
                Err(refusal) => view.chain.failed = Some(design::sentence(&refusal)),
            }
            cx.notify();
        })
        .detach();
    }

    /// Asks the host what `tx` does, once, the first time it is drawn; a
    /// program that says nothing, or a refusal, leaves its bytes.
    pub(crate) fn describe(&self, tx: &TxRow, cx: &mut Context<Self>) {
        let Some(payload) = &tx.payload else {
            return;
        };
        if tx.op.get().is_some() || tx.asked.replace(true) {
            return;
        }
        let ask = cx
            .host()
            .ask::<ProgramDescribe>((tx.target.clone(), payload.clone()));
        let (hash, target, payload) = (tx.hash, tx.target.clone(), payload.clone());
        cx.land(ask, move |view, described, cx| {
            let op = worded(&target, &payload, described, cx);
            view.described(hash, op, cx);
        })
        .detach();
    }

    pub(crate) fn open(&self, tx: &TxRow, cx: &mut Context<Self>) {
        if tx.payload.is_some() {
            return self.describe(tx, cx);
        }
        if tx.op.get().is_some() || tx.asked.replace(true) {
            return;
        }
        let host = cx.host();
        let whole = TxAsk {
            tx: TxRef::At {
                height: tx.height,
                position: tx.index,
            },
            most: u32::MAX,
        };
        let target = tx.target.clone();
        let read = async move {
            let located = host.ask::<ChainTx>(whole).await?;
            let Some(payload) = located.and_then(|located| located.tx.payload) else {
                return Ok(None);
            };
            let described = host.ask::<ProgramDescribe>((target, payload.clone())).await;
            Ok::<_, Error>(Some((payload, described)))
        };
        let (hash, target, size) = (tx.hash, tx.target.clone(), u64::from(tx.size));
        cx.land(read, move |view, read, cx| {
            let op = match read {
                Ok(Some((payload, described))) => worded(&target, &payload, described, cx),
                Ok(None) => unread(&target, size),
                Err(refusal) => {
                    cx.log_refused("a transaction's payload", &refusal);
                    unread(&target, size)
                }
            };
            view.described(hash, op, cx);
        })
        .detach();
    }

    fn described(&mut self, hash: [u8; 32], op: Description, cx: &mut Context<Self>) {
        // every row of the hash: a frame the node landed again is
        // the same op, drawn once per landing
        for tx in self.held().filter(|tx| tx.hash == hash) {
            let _ = tx.op.set(op.clone());
        }
        cx.notify();
    }

    fn held(&self) -> impl Iterator<Item = &TxRow> {
        let opened = match self.opened.ready() {
            Some(Some((_, txs))) => txs.as_slice(),
            _ => &[],
        };
        let located = self.located.ready().and_then(Option::as_ref);
        let acted = self.acted_rows.ready().map_or(&[][..], Vec::as_slice);
        self.chain
            .txs
            .iter()
            .chain(opened)
            .chain(located)
            .chain(acted)
    }

    pub(crate) fn tx(&self, hash: &[u8; 32]) -> Option<&TxRow> {
        self.held().find(|tx| &tx.hash == hash)
    }

    pub(crate) fn tx_at(&self, height: u64, position: u32) -> Option<&TxRow> {
        self.held()
            .find(|tx| (tx.height, tx.index) == (height, position))
    }

    /// A phone's Activity rows: the transactions behind the account page's
    /// newest frames, each read by its place, one after another (Expo
    /// `query/activity.ts:26-31`), so a row says what its transaction did.
    /// Asked on every draw under a finger, once per frames read; the
    /// desktop lists the frames and reads no transaction.
    pub(crate) fn read_rows(&mut self, cx: &mut Context<Self>) {
        let Route::Account(number) = *self.pages.top() else {
            return;
        };
        let Some(acted) = self.acted(number) else {
            return;
        };
        let places = acted.rows.read(|acted| {
            let read = !acted.is_loading() && acted.failed().is_none();
            let newest = acted.rows().iter().take(ACTED);
            read.then(|| {
                let places = newest.map(|frame| (frame.height, frame.position));
                places.collect::<Vec<_>>()
            })
        });
        let Some(places) = places else {
            return;
        };
        let of = Some((number, places.clone()));
        if self.rows_of == of {
            return;
        }
        self.rows_of = of;
        let host = cx.host();
        let read = async move {
            let mut rows = Vec::with_capacity(places.len());
            for (height, position) in places {
                let at = TxAsk {
                    tx: TxRef::At { height, position },
                    most: ROW_PAYLOAD,
                };
                // a frame whose transaction the node no longer holds is
                // left out, as Expo leaves it
                if let Some(located) = host.ask::<ChainTx>(at).await? {
                    rows.push(chain::located(located));
                }
            }
            Ok::<_, Error>(rows)
        };
        self.acted_rows = Loadable::Idle;
        cx.load(self, read, |view| &mut view.acted_rows);
    }

    /// What the page on screen reads past the window: a block or a
    /// transaction the window does not hold. On arriving, and on a retry.
    pub(crate) fn read_page(&mut self, cx: &mut Context<Self>) {
        match *self.pages.top() {
            Route::Block(height) => self.open_block(height, cx),
            Route::Tx { height, position } => self.locate(height, position, cx),
            _ => {}
        }
    }

    fn open_block(&mut self, height: u64, cx: &mut Context<Self>) {
        let held = self.chain.block(height).is_some();
        let opened = matches!(self.opened.ready(), Some(Some((row, _))) if row.height == height);
        if held || opened {
            return;
        }
        let ask = cx.host().ask::<ChainBlock>(BlockAsk {
            by: BlockRef::Height(height),
            most: 0,
        });
        // another block: the one opened before is not shown under it
        self.opened = Loadable::Idle;
        cx.load(
            self,
            async move { ask.await.map(|block| block.map(chain::rows)) },
            |view| &mut view.opened,
        );
    }

    fn locate(&mut self, height: u64, position: u32, cx: &mut Context<Self>) {
        let held = self.tx_at(height, position).is_some();
        if held {
            return;
        }
        let whole = TxAsk {
            tx: TxRef::At { height, position },
            most: u32::MAX,
        };
        let ask = cx.host().ask::<ChainTx>(whole);
        self.located = Loadable::Idle;
        cx.load(
            self,
            async move { ask.await.map(|found| found.map(chain::located)) },
            |view| &mut view.located,
        );
    }

    /// A block and its transactions, in the window or opened outside it.
    pub(crate) fn block(&self, height: u64) -> Option<(BlockRow, Vec<&TxRow>)> {
        if let Some(row) = self.chain.block(height) {
            let txs = self.chain.txs.iter().filter(|tx| tx.height == height);
            return Some((row.clone(), txs.collect()));
        }
        match self.opened.ready() {
            Some(Some((row, txs))) if row.height == height => {
                Some((row.clone(), txs.iter().collect()))
            }
            _ => None,
        }
    }
}

fn worded(
    target: &str,
    payload: &[u8],
    described: Result<Option<Description>, Error>,
    cx: &mut Context<Explorer>,
) -> Description {
    match described {
        Ok(Some(op)) => op,
        Ok(None) => decode::bytes(target, payload),
        Err(refusal) => {
            cx.log_refused("a description", &refusal);
            decode::bytes(target, payload)
        }
    }
}

fn unread(target: &str, size: u64) -> Description {
    Description {
        title: decode::sized(target, size),
        fields: Vec::new(),
    }
}
