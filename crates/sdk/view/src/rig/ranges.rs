//! The node's block facts and transaction summary pages, answered from a chain held oldest first,
//! as core's noded answers `/v1/blocks/facts` and `/v1/txs` from its archive.
use view_wire::methods::{
    Block, Facts, FactsAsk, FactsPage, Outcome, Tx, TxPage, TxPlace, TxRange, TxSummary,
};

pub fn facts_of(block: &Block) -> Facts {
    Facts {
        height: block.height,
        id: block.id,
        parent: block.parent,
        time: block.time,
        epoch: block.epoch,
        proposer: block.proposer.clone(),
        txs: block.txs.len() as u32,
    }
}

pub fn summary_of(block: &Block, position: u32, tx: &Tx) -> TxSummary {
    let applied = tx
        .receipt
        .as_ref()
        .map(|receipt| matches!(receipt.outcome, Outcome::Applied { .. }));
    TxSummary {
        height: block.height,
        position,
        time: block.time,
        hash: tx.hash,
        signer: tx.signer.clone(),
        anchor: tx.anchor,
        target: tx.target.clone(),
        size: tx.size,
        applied,
    }
}

fn span(blocks: &[Block]) -> Option<(u64, u64)> {
    Some((blocks.first()?.height, blocks.last()?.height))
}

fn at(blocks: &[Block], lowest: u64, height: u64) -> Option<&Block> {
    let index = usize::try_from(height.checked_sub(lowest)?).ok()?;
    blocks.get(index).filter(|block| block.height == height)
}

pub fn facts_page(blocks: &[Block], ask: &FactsAsk) -> FactsPage {
    let Some((lowest, newest)) = span(blocks) else {
        return FactsPage {
            blocks: Vec::new(),
            listed_from: None,
        };
    };
    let top = match ask.before {
        None => Some(newest),
        Some(before) => before.checked_sub(1).map(|top| top.min(newest)),
    };
    let listed = top
        .filter(|top| *top >= lowest)
        .map_or_else(Vec::new, |top| {
            (lowest..=top)
                .rev()
                .take(ask.limit as usize)
                .map_while(|height| at(blocks, lowest, height))
                .map(facts_of)
                .collect()
        });
    FactsPage {
        blocks: listed,
        listed_from: Some(lowest),
    }
}

fn below(place: TxPlace, newest: u64) -> Option<(u64, Option<u32>)> {
    let past_the_tip = place.height > newest;
    if past_the_tip {
        return Some((newest, None));
    }
    let before_the_block = place.position == 0;
    if before_the_block {
        return place.height.checked_sub(1).map(|height| (height, None));
    }
    Some((place.height, Some(place.position)))
}

pub fn tx_page(blocks: &[Block], range: &TxRange) -> TxPage {
    let Some((lowest, newest)) = span(blocks) else {
        return TxPage {
            txs: Vec::new(),
            next: None,
        };
    };
    let oldest = range.oldest.max(lowest);
    let limit = range.limit as usize;
    let mut next = Some(range.before.unwrap_or(TxPlace {
        height: newest.saturating_add(1),
        position: 0,
    }));
    let reach =
        |next: Option<TxPlace>| below(next?, newest).filter(|(height, _)| *height >= oldest);
    let mut txs = Vec::new();
    let mut read = 0;
    'blocks: while let Some((height, end)) = reach(next) {
        let page_spent = txs.len() >= limit || read >= limit;
        if page_spent {
            break;
        }
        let Some(block) = at(blocks, lowest, height) else {
            next = None;
            break;
        };
        read += 1;
        let end = end.map_or(block.txs.len(), |end| block.txs.len().min(end as usize));
        for position in (0..end).rev() {
            let rows_taken = txs.len() >= limit;
            if rows_taken {
                next = Some(TxPlace {
                    height,
                    position: position as u32 + 1,
                });
                continue 'blocks;
            }
            let tx = &block.txs[position];
            let sent_to_target = range
                .target
                .as_ref()
                .is_none_or(|target| tx.target == *target);
            if sent_to_target {
                txs.push(summary_of(block, position as u32, tx));
            }
        }
        next = Some(TxPlace {
            height,
            position: 0,
        });
    }
    TxPage {
        txs,
        next: reach(next).and(next),
    }
}
