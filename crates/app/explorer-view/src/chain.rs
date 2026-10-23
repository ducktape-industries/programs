//! The recent window: the facts of the last [`WINDOW`] finalized blocks, folded in a page at a
//! time, and the transaction rows the list on screen shows, read for the range it shows.
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::cell::{Cell, OnceCell};
use std::ops::Range;
use view::prelude::*;

use crate::{WINDOW, decode};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockRow {
    pub height: u64,
    pub id: [u8; 32],
    pub parent: [u8; 32],
    pub time: u64,
    pub epoch: u64,
    pub proposer: Option<Vec<u8>>,
    pub txs: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TxRow {
    pub hash: [u8; 32],
    pub height: u64,
    /// its place in the block's transactions: with `height`, where it sits
    /// on the chain, which no two rows share
    pub index: u32,
    pub time: u64,
    pub signer: Vec<u8>,
    pub anchor: u64,
    pub target: String,
    pub size: u32,
    pub payload: Option<Vec<u8>>,
    pub(crate) op: OnceCell<Description>,
    pub(crate) asked: Cell<bool>,
    pub applied: Option<bool>,
    /// how its run ended, where the node kept its receipt
    pub receipt: Option<Receipt>,
}

impl TxRow {
    /// The op as its program described it, once the host has answered.
    pub fn op(&self) -> Option<&Description> {
        self.op.get()
    }

    pub fn title(&self) -> String {
        match (self.op.get(), &self.payload) {
            (Some(op), _) => op.title.clone(),
            (None, Some(_)) => String::new(),
            (None, None) => decode::sized(&self.target, u64::from(self.size)),
        }
    }

    pub(crate) fn place(&self) -> TxPlace {
        TxPlace {
            height: self.height,
            position: self.index,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct StoredTx<'a> {
    hash: [u8; 32],
    height: u64,
    index: u32,
    time: u64,
    signer: Cow<'a, [u8]>,
    anchor: u64,
    target: Cow<'a, str>,
    size: u32,
    op: Option<Description>,
    payload: Option<Cow<'a, [u8]>>,
    receipt: Cow<'a, Option<Receipt>>,
    #[serde(default)]
    applied: Option<bool>,
}

impl Serialize for TxRow {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let (op, payload) = match self.op.get() {
            Some(op) => (Some(op.clone()), None),
            None => (None, self.payload.as_deref().map(Cow::Borrowed)),
        };
        StoredTx {
            hash: self.hash,
            height: self.height,
            index: self.index,
            time: self.time,
            signer: Cow::Borrowed(&self.signer),
            anchor: self.anchor,
            target: Cow::Borrowed(&self.target),
            size: self.size,
            op,
            payload,
            receipt: Cow::Borrowed(&self.receipt),
            applied: self.applied,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for TxRow {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let stored = StoredTx::deserialize(d)?;
        let applied = stored
            .applied
            .or_else(|| applied(stored.receipt.as_ref().as_ref()));
        Ok(TxRow {
            hash: stored.hash,
            height: stored.height,
            index: stored.index,
            time: stored.time,
            signer: stored.signer.into_owned(),
            anchor: stored.anchor,
            target: stored.target.into_owned(),
            size: stored.size,
            payload: stored.payload.map(Cow::into_owned),
            op: stored.op.map(Into::into).unwrap_or_default(),
            asked: Default::default(),
            applied,
            receipt: stored.receipt.into_owned(),
        })
    }
}

fn applied(receipt: Option<&Receipt>) -> Option<bool> {
    receipt.map(|run| matches!(run.outcome, Outcome::Applied { .. }))
}

pub(crate) fn facts_row(facts: Facts) -> BlockRow {
    BlockRow {
        height: facts.height,
        id: facts.id,
        parent: facts.parent,
        time: facts.time,
        epoch: facts.epoch,
        proposer: facts.proposer,
        txs: facts.txs as usize,
    }
}

pub(crate) fn block_row(block: Block) -> BlockRow {
    BlockRow {
        height: block.height,
        id: block.id,
        parent: block.parent,
        time: block.time,
        epoch: block.epoch,
        proposer: block.proposer,
        txs: block.txs.len(),
    }
}

pub(crate) fn located(located: Located) -> TxRow {
    let tx = located.tx;
    TxRow {
        hash: tx.hash,
        height: located.height,
        index: located.position,
        time: located.time,
        signer: tx.signer,
        anchor: tx.anchor.height,
        target: tx.target,
        size: tx.size,
        payload: tx.payload,
        op: Default::default(),
        asked: Default::default(),
        applied: applied(tx.receipt.as_ref()),
        receipt: tx.receipt,
    }
}

pub(crate) fn summary_row(summary: TxSummary) -> TxRow {
    TxRow {
        hash: summary.hash,
        height: summary.height,
        index: summary.position,
        time: summary.time,
        signer: summary.signer,
        anchor: summary.anchor.height,
        target: summary.target,
        size: summary.size,
        payload: None,
        op: Default::default(),
        asked: Default::default(),
        applied: summary.applied,
        receipt: None,
    }
}

/// The recent window, newest first.
#[derive(Default, Serialize, Deserialize)]
pub struct Chain {
    pub blocks: Vec<BlockRow>,
    /// the archive ends inside the window: nothing older to read
    pub complete: bool,
    pub listed_from: Option<u64>,
    /// the last page was refused, in the host's words
    pub failed: Option<String>,
}

impl Chain {
    pub(crate) fn top(&self) -> Option<u64> {
        self.blocks.first().map(|block| block.height)
    }

    pub(crate) fn bottom(&self) -> Option<u64> {
        self.blocks.last().map(|block| block.height)
    }

    /// Now, as the chain tells it: the newest block's time.
    pub fn now(&self) -> u64 {
        self.blocks.first().map_or(0, |block| block.time)
    }

    pub fn block(&self, height: u64) -> Option<&BlockRow> {
        let index = self.top()?.checked_sub(height)? as usize;
        self.blocks
            .get(index)
            .filter(|block| block.height == height)
    }

    pub fn unlisted_below(&self) -> Option<u64> {
        self.listed_from.filter(|from| *from > 0)
    }

    pub fn transactions(&self) -> usize {
        self.blocks.iter().map(|block| block.txs).sum()
    }

    pub(crate) fn place(&self, index: usize) -> Option<TxPlace> {
        let mut above = 0;
        for block in &self.blocks {
            let within = index - above < block.txs;
            if within {
                let position = block.txs - 1 - (index - above);
                return Some(TxPlace {
                    height: block.height,
                    position: position as u32,
                });
            }
            above += block.txs;
        }
        None
    }

    pub(crate) fn land(&mut self, before: Option<u64>, page: FactsPage) {
        self.failed = None;
        self.listed_from = page.listed_from;
        let Some(oldest) = page.blocks.last().map(|block| block.height) else {
            self.complete |= before.is_some();
            return;
        };
        let reached_bottom = page.listed_from.is_none_or(|from| oldest <= from);
        let blocks: Vec<BlockRow> = page.blocks.into_iter().map(facts_row).collect();
        match (before, self.top()) {
            (Some(_), _) => {
                self.blocks.extend(blocks);
                self.complete = reached_bottom;
            }
            (None, Some(top)) if blocks.last().is_some_and(|b| b.height <= top + 1) => {
                let fresh = blocks.iter().take_while(|b| b.height > top).count();
                let mut merged: Vec<_> = blocks.into_iter().take(fresh).collect();
                merged.append(&mut self.blocks);
                self.blocks = merged;
            }
            (None, _) => {
                self.blocks = blocks;
                self.complete = reached_bottom;
            }
        }
        self.blocks.truncate(WINDOW);
    }
}

/// A transaction list a page shows, newest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TxList {
    /// One block's transactions, `count` of them.
    Block { height: u64, count: usize },
    /// Every transaction in the window.
    Window,
    /// The window's transactions sent to one program, read on a page at a time.
    To(String),
}

/// The rows of the list on screen: those of the ranges its last frame lowered, or for a list
/// read on by cursor, every page read so far.
#[derive(Default)]
pub struct Shown {
    pub(crate) list: Option<TxList>,
    pub(crate) rows: Vec<TxRow>,
    pub(crate) read: bool,
    pub(crate) next: Option<TxPlace>,
    pub(crate) seen: Range<usize>,
    pub(crate) wanted: Vec<Range<usize>>,
    pub(crate) reads: Vec<TxRange>,
    pub(crate) failed: Option<String>,
}

impl Shown {
    pub(crate) fn of(&self, list: &TxList) -> bool {
        self.list.as_ref() == Some(list)
    }

    pub(crate) fn ended(&self) -> bool {
        self.read && self.next.is_none()
    }

    pub(crate) fn at(&self, place: TxPlace) -> Option<&TxRow> {
        let found = self
            .rows
            .binary_search_by(|row| place.cmp(&row.place()))
            .ok()?;
        self.rows.get(found)
    }

    pub(crate) fn landed(&mut self, ask: &TxRange) -> bool {
        let reading = self.reads.len();
        self.reads.retain(|asked| asked != ask);
        self.reads.len() < reading
    }

    pub(crate) fn merge(
        &mut self,
        rows: impl Iterator<Item = TxRow>,
        spans: &[(TxPlace, TxPlace)],
    ) {
        for row in rows {
            let place = row.place();
            if let Err(at) = self.rows.binary_search_by(|held| place.cmp(&held.place())) {
                self.rows.insert(at, row);
            }
        }
        self.rows.retain(|row| {
            let place = row.place();
            spans
                .iter()
                .any(|(newest, oldest)| *oldest <= place && place <= *newest)
        });
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Line<'a> {
    Block(&'a BlockRow),
    Empty { newest: u64, oldest: u64 },
}

pub(crate) fn lines(blocks: &[BlockRow], rows: usize) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    let mut at = 0;
    while at < blocks.len() && lines.len() < rows {
        let run = blocks[at..]
            .iter()
            .take_while(|block| block.txs == 0)
            .count();
        if run >= 2 {
            let (newest, oldest) = (blocks[at].height, blocks[at + run - 1].height);
            lines.push(Line::Empty { newest, oldest });
            at += run;
        } else {
            lines.push(Line::Block(&blocks[at]));
            at += 1;
        }
    }
    lines
}
