//! Who a change's own room is listed for: forge opens it with chat meta `listed = involved` and
//! seats its author, each reviewer asked and each who reviews; `ListInvolved` gives the rooms
//! opened before that the same, one change a block.
use std::collections::{BTreeMap, BTreeSet};

use ::program::{AccountNumber, Error, ExecCtx, Principal, Reply, unexpected_reply};
use chat::{Op as Chat, Query, Reply as Answer};
use ownership::again_next_block;

use crate::Change;
use crate::contract::Op;
use crate::discussion::talks_elsewhere;
use crate::state::CHANGES;

pub fn meta() -> BTreeMap<String, String> {
    BTreeMap::from([(chat::LISTED.to_owned(), chat::INVOLVED.to_owned())])
}

fn taking_part(change: &Change) -> impl Iterator<Item = Principal> + '_ {
    std::iter::once(change.author.clone()).chain(change.reviewers.iter().cloned())
}

fn accounts(who: impl IntoIterator<Item = Principal>) -> BTreeSet<AccountNumber> {
    who.into_iter()
        .filter_map(|principal| principal.account())
        .collect()
}

fn seat_op(change: &Change, account: AccountNumber) -> Chat {
    Chat::SetMembership {
        channel_id: change.channel.clone(),
        principal: Principal::Account(account),
        member: true,
    }
}

pub(crate) fn seat_opened(ctx: &ExecCtx, change: &Change) {
    for account in accounts(taking_part(change)) {
        ctx.emit(chat::PROGRAM, &seat_op(change, account), Reply::None);
    }
}

pub(crate) fn seat(
    ctx: &ExecCtx,
    repo: &str,
    change: &Change,
    who: impl IntoIterator<Item = Principal>,
) -> Result<(), Error> {
    if talks_elsewhere(repo, change) {
        return Ok(());
    }
    for account in accounts(who) {
        if seated(ctx, &change.channel, account)? {
            continue;
        }
        ctx.emit(chat::PROGRAM, &seat_op(change, account), Reply::Wanted);
    }
    Ok(())
}

fn seated(ctx: &ExecCtx, channel: &str, account: AccountNumber) -> Result<bool, Error> {
    let query = Query::Seated {
        principal: Principal::Account(account),
        channel_ids: vec![channel.to_owned()],
    };
    match ctx.query::<Query, Answer>(chat::PROGRAM, &query)? {
        Answer::Seated(ids) => Ok(!ids.is_empty()),
        other => Err(unexpected_reply(chat::PROGRAM, "Seated", &other)),
    }
}

pub(crate) fn list_involved(ctx: &ExecCtx, after: Option<Vec<u8>>) -> Result<(), Error> {
    let page = CHANGES.batch(ctx, after.as_deref(), 1)?;
    for ((repo, _), change) in &page.rows {
        list(ctx, repo, change)?;
    }
    ctx.set_return_data(abi::encode(&page.next));
    let Some(after) = page.next else {
        return Ok(());
    };
    let again = Op::ListInvolved { after: Some(after) };
    again_next_block(ctx, &again)
}

fn list(ctx: &ExecCtx, repo: &str, change: &Change) -> Result<(), Error> {
    if talks_elsewhere(repo, change) {
        return Ok(());
    }
    let query = Query::Channel {
        channel_id: change.channel.clone(),
    };
    let held = match ctx.query::<Query, Answer>(chat::PROGRAM, &query)? {
        Answer::Channel(info) => info,
        other => return Err(unexpected_reply(chat::PROGRAM, "Channel", &other)),
    };
    let unlisted = held.is_some_and(|info| !info.meta.contains_key(chat::LISTED));
    if !unlisted {
        return Ok(());
    }
    let listed = Chat::SetChannelMeta {
        channel_id: change.channel.clone(),
        key: chat::LISTED.into(),
        value: chat::INVOLVED.into(),
    };
    ctx.emit(chat::PROGRAM, &listed, Reply::Wanted);
    seat(ctx, repo, change, taking_part(change))
}
