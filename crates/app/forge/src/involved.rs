//! Who a change's own room is listed for: forge opens it with chat meta `listed = involved` and
//! seats its author, each reviewer asked and each who reviews.
use std::collections::{BTreeMap, BTreeSet};

use ::program::{AccountNumber, Error, ExecCtx, Principal, Reply, unexpected_reply};
use chat::{Op as Chat, Query, Reply as Answer};

use crate::Change;
use crate::discussion::talks_elsewhere;

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
