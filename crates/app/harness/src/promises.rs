//! Bounded retirement of legacy promise rows and their indexed callbacks, without completing work.
use std::collections::BTreeSet;

use ::program::{Ctx, Error, Range, code, invalid};
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use store::{Item, Map, Set};

use crate::AccountNumber;

pub(crate) const PROMISES: Map<u64, PromiseRow> = Map::new("promise/");
pub(crate) const PROMISING: Set<(AccountNumber, u64)> = Set::new("promising/");
pub(crate) const ASKING: Set<(AccountNumber, u64)> = Set::new("asking/");
pub(crate) const ENTRIES: Map<u64, u64> = Map::new("promise-entry/");
const COUNT: Item<u64> = Item::new("promise-count");

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub(crate) struct PromiseRow {
    pub id: u64,
    pub owed_by: AccountNumber,
    pub asked_by: AccountNumber,
    pub thread: u64,
    pub channel: String,
    pub seq: u64,
    pub text: String,
    pub made: u64,
    pub due: u64,
    pub next: u64,
    pub closed: Option<PromiseClosed>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub(crate) struct PromiseClosed {
    pub height: u64,
    pub kept: bool,
    pub how: String,
}

fn scan(prefix: &str, after: Option<u64>, limit: u64) -> Range {
    let prefix = prefix.as_bytes();
    let mut range = Range::prefix(prefix).limit(limit.saturating_add(1));
    if let Some(after) = after {
        range.start = [prefix, &after.to_be_bytes(), &[0]].concat();
    }
    range
}

pub(crate) async fn retire(ctx: &Ctx, after: Option<u64>, limit: u64) -> Result<(), Error> {
    if limit == 0 {
        return Err(invalid("retirement needs a nonzero page limit"));
    }
    let rows = PROMISES.scan(ctx, scan(PROMISES.prefix(), after, limit))?;
    let entries = ENTRIES.scan(ctx, scan(ENTRIES.prefix(), after, limit))?;
    let ids: BTreeSet<u64> = rows
        .iter()
        .map(|(id, _)| *id)
        .chain(entries.iter().map(|(id, _)| *id))
        .collect();
    let mut last = None;
    let mut more = false;
    for (n, id) in ids.into_iter().enumerate() {
        if n as u64 == limit {
            more = true;
            break;
        }
        if let Some(entry) = ENTRIES.get(ctx, &id)? {
            match ctx.cancel(entry).await {
                Err(error)
                    if matches!(error.code.as_str(), code::WRONG_STATE | code::NOT_FOUND) => {}
                cancelled => cancelled?,
            }
            ENTRIES.remove(ctx, &id);
        }
        if let Some(row) = PROMISES.get(ctx, &id)? {
            PROMISING.remove(ctx, &(row.owed_by, id));
            ASKING.remove(ctx, &(row.asked_by, id));
            PROMISES.remove(ctx, &id);
        }
        last = Some(id);
    }
    let next = more.then_some(last).flatten();
    if next.is_none() {
        let empty = PROMISES
            .scan(ctx, scan(PROMISES.prefix(), None, 1))?
            .is_empty()
            && ENTRIES
                .scan(ctx, scan(ENTRIES.prefix(), None, 1))?
                .is_empty();
        if empty {
            ctx.delete(COUNT.key());
        }
    }
    ctx.set_return_data(abi::encode(&next));
    Ok(())
}
