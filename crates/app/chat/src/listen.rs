//! Who hears each channel. `Listen` starts or stops an account hearing every new message
//! there, and each message relates to the accounts that listened since before it was posted.
//! `SetNotices` is the other way: what of a channel still tells an account that cut it. The two
//! are one choice, so each takes the other off. Short of either, an account is in a room once it
//! writes in its timeline, sits in it (members-only) or opened it, and in a thread once it wrote
//! the root, replied or was mentioned there ([`followed`]), from that message on.
use ::program::{Error, ExecCtx, QueryCtx};

use crate::attributed::mentioned;
use crate::state::{
    CHANNELS, LISTENERS, LISTENING, MEMBERS, MENTIONED, NOTICES, PARTICIPANTS, POSTED, Stored,
    channel, stored,
};
use crate::{AccountNumber, Followed, MsgRow, Notices, PageRequest, PageResponse, Principal};

pub(crate) fn listen(ctx: &ExecCtx, channel_id: &str, listening: bool) -> Result<(), Error> {
    let account = ctx.sender_account()?;
    channel(ctx, channel_id)?;
    let by_channel = (channel_id.to_owned(), account);
    let by_account = (account, channel_id.to_owned());
    if !listening {
        LISTENERS.remove(ctx, &by_channel);
        LISTENING.remove(ctx, &by_account);
        return Ok(());
    }
    NOTICES.remove(ctx, &by_account);
    if LISTENERS.has(ctx, &by_channel) {
        return Ok(());
    }
    LISTENERS.put(ctx, &by_channel, &ctx.env().height);
    LISTENING.insert(ctx, &by_account);
    Ok(())
}

pub(crate) fn listened(
    ctx: &QueryCtx,
    account: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<String>, Error> {
    Ok(LISTENING
        .range_of(ctx, &account, page)?
        .map(|(_, channel_id)| channel_id))
}

pub(crate) fn set_notices(
    ctx: &ExecCtx,
    channel_id: &str,
    notices: Option<Notices>,
) -> Result<(), Error> {
    listen(ctx, channel_id, false)?;
    let by_account = (ctx.sender_account()?, channel_id.to_owned());
    match notices {
        Some(notices) => NOTICES.put(ctx, &by_account, &notices),
        None => NOTICES.remove(ctx, &by_account),
    }
    Ok(())
}

pub(crate) fn notices_cut(
    ctx: &QueryCtx,
    account: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<(String, Notices)>, Error> {
    Ok(NOTICES
        .range_of(ctx, &account, page)?
        .map(|((_, channel_id), notices)| (channel_id, notices)))
}

/// What `row`, as posted or edited, says of who is in its room and thread: its author is in the
/// room once it writes in the timeline, and each account a reply mentions is in its thread (a
/// root's mentions are read off the root). A first time is kept, never moved.
pub(crate) fn note(ctx: &ExecCtx, row: &MsgRow) {
    let Some(root) = row.thread else {
        let posted = (row.channel_id.clone(), row.author.clone());
        if !POSTED.has(ctx, &posted) {
            POSTED.put(ctx, &posted, &row.seq);
        }
        return;
    };
    for number in mentioned(&row.blocks) {
        let key = (row.channel_id.clone(), root, Principal::Account(number));
        if !MENTIONED.has(ctx, &key) {
            MENTIONED.put(ctx, &key, &row.seq);
        }
    }
}

/// Where `account` is in `channel_id`: the seq it came into the room at, and into each thread of
/// `roots` it is in. A seat in a members-only room, or the room's opening, is in it from the start.
pub(crate) fn followed(
    ctx: &QueryCtx,
    channel_id: String,
    account: AccountNumber,
    roots: &[u64],
) -> Result<Followed, Error> {
    let me = Principal::Account(account);
    let row = CHANNELS.get(ctx, &channel_id)?;
    let seated = match &row {
        Some(row) if row.owner == me => true,
        Some(row) if row.members_only() => MEMBERS.has(ctx, &(channel_id.clone(), me.clone())),
        _ => false,
    };
    let room = match seated {
        true => Some(0),
        false => POSTED.get(ctx, &(channel_id.clone(), me.clone()))?,
    };
    let mut threads = Vec::new();
    for &root in roots {
        // a root that is not there roots no thread
        let Some(row) = stored(ctx, &(channel_id.clone(), root))?.map(Stored::row) else {
            continue;
        };
        let rooted = row.author == me || mentioned(&row.blocks).any(|number| number == account);
        let thread = (channel_id.clone(), root, me.clone());
        let replied = PARTICIPANTS.get(ctx, &thread)?;
        let mentioned = MENTIONED.get(ctx, &thread)?;
        let since = [rooted.then_some(root), replied, mentioned]
            .into_iter()
            .flatten()
            .min();
        threads.extend(since.map(|since| (root, since)));
    }
    Ok(Followed { room, threads })
}

pub(crate) fn listeners(ctx: &QueryCtx, row: &MsgRow) -> Result<Vec<AccountNumber>, Error> {
    let listening = LISTENERS.scan(ctx, LISTENERS.prefix_of(&row.channel_id))?;
    Ok(listening
        .into_iter()
        .filter(|(_, since)| *since <= row.height)
        .map(|((_, account), _)| account)
        .collect())
}
