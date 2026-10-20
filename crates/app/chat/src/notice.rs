//! Whether a message is meant for an account, and why: the one rule chat's notifier tells by, and
//! `Query::Unread` counts by, from the tables chat keeps of who is where.
use std::collections::BTreeSet;

use ::program::{Error, QueryCtx};

use crate::attributed::mentioned;
use crate::listen::followed;
use crate::state::{HEADS, LISTENING, NOTICES, Stored, stored_between};
use crate::{AccountNumber, Followed, MsgRow, Notices, Principal, UnreadCount, dm_peers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    Direct,
    Mention,
    Thread,
    Room,
}

// `row` for `me`, who is where `followed` says in its room, listens there (`listening`) or cut
// what it tells (`cut`)
pub fn why(
    row: &MsgRow,
    me: AccountNumber,
    followed: &Followed,
    listening: bool,
    cut: Option<Notices>,
) -> Option<Why> {
    let after = |since: u64| row.seq > since;
    let thread_since = |root| {
        let mut threads = followed.threads.iter();
        threads.find_map(|&(at, since)| (at == root).then_some(since))
    };
    let direct = dm_peers(&row.channel_id).is_some_and(|(a, b)| me == a || me == b);
    if row.deleted || row.author == Principal::Account(me) || cut == Some(Notices::Off) {
        None
    } else if direct {
        Some(Why::Direct)
    } else if mentioned(&row.blocks).any(|number| number == me) {
        Some(Why::Mention)
    } else if cut == Some(Notices::Mentions) {
        None
    } else if row.thread.and_then(thread_since).is_some_and(after) {
        Some(Why::Thread)
    } else if listening || (row.thread.is_none() && followed.room.is_some_and(after)) {
        Some(Why::Room)
    } else {
        None
    }
}

pub(crate) fn unread(
    ctx: &QueryCtx,
    account: AccountNumber,
    rooms: &[(String, u64)],
) -> Result<Vec<UnreadCount>, Error> {
    let mut counted = Vec::new();
    for (channel_id, since) in rooms {
        let head = HEADS.get(ctx, channel_id)?.unwrap_or(0).max(*since);
        let rows: Vec<MsgRow> = match head > *since {
            true => stored_between(ctx, channel_id, since + 1, head + 1)?
                .into_iter()
                .map(Stored::row)
                .collect(),
            false => Vec::new(),
        };
        let roots: BTreeSet<u64> = rows.iter().filter_map(|row| row.thread).collect();
        let roots: Vec<u64> = roots.into_iter().collect();
        let followed = followed(ctx, channel_id.clone(), account, &roots)?;
        let chosen = (account, channel_id.clone());
        let listening = LISTENING.has(ctx, &chosen);
        let cut = NOTICES.get(ctx, &chosen)?;
        let meant = rows
            .iter()
            .filter(|row| why(row, account, &followed, listening, cut).is_some())
            .count();
        counted.push(UnreadCount {
            channel_id: channel_id.clone(),
            to: head,
            meant: meant as u64,
        });
    }
    Ok(counted)
}
