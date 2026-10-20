//! The recent window: the last [`WINDOW`] finalized blocks and the
//! transactions they carry, folded in a page at a time.
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::cell::{Cell, OnceCell};
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
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for TxRow {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let stored = StoredTx::deserialize(d)?;
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
            receipt: stored.receipt.into_owned(),
        })
    }
}

pub(crate) fn rows(block: Block) -> (BlockRow, Vec<TxRow>) {
    let row = BlockRow {
        height: block.height,
        id: block.id,
        parent: block.parent,
        time: block.time,
        epoch: block.epoch,
        proposer: block.proposer,
        txs: block.txs.len(),
    };
    let txs = block
        .txs
        .into_iter()
        .enumerate()
        .rev()
        .map(|(index, tx)| tx_row(block.height, index as u32, block.time, tx))
        .collect();
    (row, txs)
}

pub(crate) fn located(located: Located) -> TxRow {
    tx_row(located.height, located.position, located.time, located.tx)
}

fn tx_row(height: u64, index: u32, time: u64, tx: Tx) -> TxRow {
    TxRow {
        hash: tx.hash,
        height,
        index,
        time,
        signer: tx.signer,
        anchor: tx.anchor.height,
        target: tx.target,
        size: tx.size,
        payload: tx.payload,
        op: Default::default(),
        asked: Default::default(),
        receipt: tx.receipt,
    }
}

/// The recent window, newest first.
#[derive(Default, Serialize, Deserialize)]
pub struct Chain {
    pub blocks: Vec<BlockRow>,
    pub txs: Vec<TxRow>,
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

    pub(crate) fn land(&mut self, before: Option<u64>, page: BlockList) {
        self.failed = None;
        self.listed_from = page.listed_from;
        let Some(oldest) = page.blocks.last().map(|block| block.height) else {
            self.complete |= before.is_some();
            return;
        };
        let reached_bottom = page.listed_from.is_none_or(|from| oldest <= from);
        let (blocks, txs): (Vec<_>, Vec<_>) = page.blocks.into_iter().map(rows).unzip();
        let txs = txs.into_iter().flatten();
        match (before, self.top()) {
            (Some(_), _) => {
                self.blocks.extend(blocks);
                self.txs.extend(txs);
                self.complete = reached_bottom;
            }
            (None, Some(top)) if blocks.last().is_some_and(|b| b.height <= top + 1) => {
                let fresh = blocks.iter().take_while(|b| b.height > top).count();
                let mut merged: Vec<_> = blocks.into_iter().take(fresh).collect();
                merged.append(&mut self.blocks);
                self.blocks = merged;
                let mut merged: Vec<_> = txs.filter(|tx| tx.height > top).collect();
                merged.append(&mut self.txs);
                self.txs = merged;
            }
            (None, _) => {
                self.blocks = blocks;
                self.txs = txs.collect();
                self.complete = reached_bottom;
            }
        }
        self.blocks.truncate(WINDOW);
        let oldest = self.blocks.last().map_or(0, |block| block.height);
        self.txs.retain(|tx| tx.height >= oldest);
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
