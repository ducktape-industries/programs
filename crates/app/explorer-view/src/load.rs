//! The loaders: the head and the window it pulls (`chain.status`, `chain.facts`), the lists the
//! system programs answer, and what an op says it does (`program.describe`). A list is read with
//! `cx.load`, which keeps what is on screen while it reads again and draws only what changed.
use std::future::Future;
use std::ops::Range;

use view::Paged;
use view::host::Page;
use view::prelude::*;

use crate::chain::{self, BlockRow, Shown, TxList, TxRow};
use crate::state::{Listing, Route, blob_ref};
use crate::{Explorer, WINDOW, decode, queries};

const ACTED: usize = queries::PAGE as usize;
const ROW_PAYLOAD: u32 = 4 << 10;

impl Explorer {
    pub(crate) fn read_all(&mut self, cx: &mut Context<Self>) {
        self.shown.reads.clear();
        self.shown.failed = None;
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

    pub(crate) fn start_over(&mut self, cx: &mut Context<Self>) {
        self.start_over_where(Listing::past_first_page, cx);
    }

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
        let limit = lacking as u32;
        self.pulling = true;
        let (host, top) = (cx.host(), self.chain.top());
        let read = async move {
            let mut page = host.ask::<ChainFacts>(FactsAsk { before, limit }).await?;
            let Some(top) = top.filter(|_| before.is_none()) else {
                return Ok(page);
            };
            let newest = page.blocks.first().map_or(top, |block| block.height);
            while let Some(oldest) = page.blocks.last().map(|block| block.height)
                && oldest > top + 1
                && newest.saturating_sub(top) <= WINDOW as u64
            {
                let below = host
                    .ask::<ChainFacts>(FactsAsk {
                        before: Some(oldest),
                        limit: u32::try_from(oldest - top - 1).unwrap_or(u32::MAX),
                    })
                    .await?;
                match below.blocks.last() {
                    None => break,
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
                    if let (Loadable::Ready(status) | Loadable::Reloading(status, _), Some(top)) =
                        (&mut view.status, view.chain.blocks.first())
                        && top.height > status.height
                    {
                        status.height = top.height;
                        status.tip = top.id;
                    }
                    if view.chain.top() != was.0 {
                        view.follow_shown(cx);
                    }
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
        for tx in self.held().filter(|tx| tx.hash == hash) {
            let _ = tx.op.set(op.clone());
        }
        cx.notify();
    }

    fn held(&self) -> impl Iterator<Item = &TxRow> {
        let located = self.located.ready().and_then(Option::as_ref);
        let acted = self.acted_rows.ready().map_or(&[][..], Vec::as_slice);
        self.shown.rows.iter().chain(located).chain(acted)
    }

    pub(crate) fn tx(&self, hash: &[u8; 32]) -> Option<&TxRow> {
        self.held().find(|tx| &tx.hash == hash)
    }

    pub(crate) fn tx_at(&self, height: u64, position: u32) -> Option<&TxRow> {
        let located = self.located.ready().and_then(Option::as_ref);
        let acted = self.acted_rows.ready().map_or(&[][..], Vec::as_slice);
        located
            .into_iter()
            .chain(acted)
            .find(|tx| (tx.height, tx.index) == (height, position))
    }

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
                if let Some(located) = host.ask::<ChainTx>(at).await? {
                    rows.push(chain::located(located));
                }
            }
            Ok::<_, Error>(rows)
        };
        self.acted_rows = Loadable::Idle;
        cx.load(self, read, |view| &mut view.acted_rows);
    }

    pub(crate) fn read_page(&mut self, cx: &mut Context<Self>) {
        match *self.pages.top() {
            Route::Block(height) => self.open_block(height, cx),
            Route::Tx { height, position } => self.locate(height, position, cx),
            _ => {}
        }
    }

    fn open_block(&mut self, height: u64, cx: &mut Context<Self>) {
        let held = self.chain.block(height).is_some();
        let opened = matches!(self.opened.ready(), Some(Some(row)) if row.height == height);
        if held || opened {
            return;
        }
        let ask = cx.host().ask::<ChainFacts>(FactsAsk {
            before: Some(height.saturating_add(1)),
            limit: 1,
        });
        self.opened = Loadable::Idle;
        cx.load(
            self,
            async move {
                let page = ask.await?;
                let found = page.blocks.into_iter().find(|facts| facts.height == height);
                Ok::<_, Error>(found.map(chain::facts_row))
            },
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

    pub(crate) fn block(&self, height: u64) -> Option<BlockRow> {
        if let Some(row) = self.chain.block(height) {
            return Some(row.clone());
        }
        match self.opened.ready() {
            Some(Some(row)) if row.height == height => Some(row.clone()),
            _ => None,
        }
    }

    pub(crate) fn place_at(&self, list: &TxList, index: usize) -> Option<TxPlace> {
        match list {
            TxList::Block { height, count } => {
                let position = count.checked_sub(index + 1)?;
                Some(TxPlace {
                    height: *height,
                    position: position as u32,
                })
            }
            TxList::Window => self.chain.place(index),
            TxList::To(_) => self.shown_at(list, index).map(TxRow::place),
        }
    }

    pub(crate) fn shown_at(&self, list: &TxList, index: usize) -> Option<&TxRow> {
        if !self.shown.of(list) {
            return None;
        }
        match list {
            TxList::To(_) => self.shown.rows.get(index),
            TxList::Block { .. } | TxList::Window => self.shown.at(self.place_at(list, index)?),
        }
    }

    pub(crate) fn shown_read(&self, list: &TxList) -> usize {
        match self.shown.of(list) {
            true => self.shown.rows.len(),
            false => 0,
        }
    }

    pub(crate) fn shown_ended(&self, list: &TxList) -> bool {
        self.shown.of(list) && self.shown.ended()
    }

    pub(crate) fn shown_count(&self, list: &TxList) -> usize {
        match list {
            TxList::Block { count, .. } => *count,
            TxList::Window => self.chain.transactions(),
            TxList::To(_) => {
                let more = match self.shown_ended(list) {
                    true => 0,
                    false => self.rows_fit.max(1),
                };
                self.shown_read(list) + more
            }
        }
    }

    pub(crate) fn shown_failed(&self, list: &TxList) -> Option<&str> {
        self.shown.failed.as_deref().filter(|_| self.shown.of(list))
    }

    pub(crate) fn read_latest(&mut self, cx: &mut Context<Self>) {
        let on_overview = matches!(self.pages.top(), Route::Overview);
        if !on_overview {
            return;
        }
        let latest = crate::ui::LATEST.min(self.chain.transactions());
        self.want(TxList::Window, 0..latest, cx);
    }

    pub(crate) fn want(&mut self, list: TxList, range: Range<usize>, cx: &mut Context<Self>) {
        if !self.shown.of(&list) {
            self.shown = Shown {
                list: Some(list.clone()),
                ..Shown::default()
            };
        }
        self.shown.seen = range.clone();
        if !self.shown.wanted.contains(&range) {
            self.shown.wanted.push(range.clone());
        }
        let appends = matches!(list, TxList::To(_));
        let paging = appends && !self.shown.reads.is_empty();
        let held_back = paging || self.shown.failed.is_some();
        if held_back {
            return;
        }
        let Some(ask) = self.range_ask(&list, range) else {
            return;
        };
        if self.shown.reads.contains(&ask) {
            return;
        }
        self.read_shown(list, ask, appends, cx);
    }

    fn wanted_spans(&self, list: &TxList) -> Vec<(TxPlace, TxPlace)> {
        let span = |range: &Range<usize>| {
            let last = range.end.checked_sub(1)?;
            Some((
                self.place_at(list, range.start)?,
                self.place_at(list, last)?,
            ))
        };
        self.shown.wanted.iter().filter_map(span).collect()
    }

    fn range_ask(&self, list: &TxList, range: Range<usize>) -> Option<TxRange> {
        if range.is_empty() {
            return None;
        }
        if let TxList::To(program) = list {
            let wanted = range.end.saturating_sub(self.shown.rows.len());
            let nothing_more = wanted == 0 || self.shown.ended();
            if nothing_more {
                return None;
            }
            return Some(TxRange {
                before: self.shown.next.filter(|_| self.shown.read),
                oldest: self.chain.bottom()?,
                target: Some(program.clone()),
                limit: u32::try_from(wanted.max(self.rows_fit)).unwrap_or(u32::MAX),
            });
        }
        let held = range
            .clone()
            .all(|index| self.shown_at(list, index).is_some());
        if held {
            return None;
        }
        let newest = self.place_at(list, range.start)?;
        let oldest = self.place_at(list, range.end - 1)?;
        Some(TxRange {
            before: Some(TxPlace {
                height: newest.height,
                position: newest.position + 1,
            }),
            oldest: oldest.height,
            target: None,
            limit: u32::try_from(range.len()).unwrap_or(u32::MAX),
        })
    }

    pub(crate) fn follow_shown(&mut self, cx: &mut Context<Self>) {
        let Some(TxList::To(program)) = self.shown.list.clone() else {
            return;
        };
        let at_the_top = self.shown.seen.start == 0;
        if !at_the_top {
            return;
        }
        let Some(oldest) = self.chain.bottom() else {
            return;
        };
        let limit = self.shown.seen.len().max(self.rows_fit).max(1);
        let ask = TxRange {
            before: None,
            oldest,
            target: Some(program.clone()),
            limit: u32::try_from(limit).unwrap_or(u32::MAX),
        };
        self.shown.reads.clear();
        self.read_shown(TxList::To(program), ask, false, cx);
    }

    fn read_shown(&mut self, list: TxList, ask: TxRange, appends: bool, cx: &mut Context<Self>) {
        self.shown.reads.push(ask.clone());
        self.shown.failed = None;
        let asked = ask.clone();
        let host = cx.host();
        let read = async move {
            let wanted = ask.limit as usize;
            let mut range = ask;
            let mut rows = Vec::new();
            loop {
                let page = host.ask::<ChainTxs>(range.clone()).await?;
                rows.extend(page.txs);
                let unfilled = rows.len() < wanted;
                match page.next {
                    Some(next) if unfilled => {
                        range.before = Some(next);
                        range.limit = u32::try_from(wanted - rows.len()).unwrap_or(u32::MAX);
                    }
                    next => return Ok::<_, Error>((rows, next)),
                }
            }
        };
        cx.land(read, move |view, read, cx| {
            let current = view.shown.of(&list) && view.shown.landed(&asked);
            if !current {
                return;
            }
            match read {
                Ok((rows, next)) => {
                    let rows = rows.into_iter().map(chain::summary_row);
                    match &list {
                        TxList::To(_) if appends => view.shown.rows.extend(rows),
                        TxList::To(_) => view.shown.rows = rows.collect(),
                        TxList::Block { .. } | TxList::Window => {
                            let spans = view.wanted_spans(&list);
                            view.shown.merge(rows, &spans);
                        }
                    }
                    view.shown.next = next;
                    view.shown.read = true;
                }
                Err(refusal) => view.shown.failed = Some(design::sentence(&refusal)),
            }
            cx.notify();
        })
        .detach();
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
