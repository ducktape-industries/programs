//! What chat tells attribution as a message is posted, edited or deleted: the accounts it
//! mentions, in a direct message the counterpart it is written to, and in a thread reply the
//! accounts that took part in the thread before it.
use std::collections::BTreeSet;

use ::program::{Error, ExecCtx, QueryCtx, Reply};
use attribution::{Actor, Object, Reason, Relation, Update};

use crate::listen::listeners;
use crate::state::{PARTICIPANTS, message};
use crate::{AccountNumber, Block, Mark, MsgRow, Principal, dm_peers};

pub const MESSAGE: &str = "message";
pub const DIRECT: &str = "direct";
pub const THREAD: &str = "thread";
// each account listening to the message's channel that it relates to in no other way
pub const LISTEN: &str = "listen";

fn mentioned(blocks: &[Block]) -> impl Iterator<Item = AccountNumber> + '_ {
    blocks
        .iter()
        .flat_map(|block| match block {
            Block::Paragraph(spans) | Block::Quote(spans) => spans.as_slice(),
            Block::Code { .. } | Block::Divider | Block::Attachment(_) => &[],
        })
        .flat_map(|span| &span.marks)
        .filter_map(|mark| match mark {
            Mark::Mention(Principal::Account(number)) => Some(*number),
            _ => None,
        })
}

fn thread_before(ctx: &QueryCtx, row: &MsgRow) -> Result<Vec<AccountNumber>, Error> {
    let Some(root) = row.thread else {
        return Ok(Vec::new());
    };
    let starter = message(ctx, &row.channel_id, root)?.author;
    let scope = PARTICIPANTS.prefix_of(&(row.channel_id.clone(), root));
    let repliers = PARTICIPANTS
        .scan(ctx, scope)?
        .into_iter()
        .filter(|(_, first)| *first < row.seq)
        .map(|((_, _, author), _)| author);
    Ok(std::iter::once(starter)
        .chain(repliers)
        .filter_map(|principal| principal.account())
        .collect())
}

pub fn relations(
    row: &MsgRow,
    thread: &[AccountNumber],
    listeners: &[AccountNumber],
) -> Vec<Relation> {
    if row.deleted {
        return Vec::new();
    }
    let mut held = BTreeSet::new();
    for number in thread {
        held.insert((*number, Reason::Defined(THREAD.into())));
    }
    for number in mentioned(&row.blocks) {
        held.insert((number, Reason::Mention));
    }
    if let Some((lo, hi)) = dm_peers(&row.channel_id) {
        for peer in [lo, hi] {
            held.insert((peer, Reason::Defined(DIRECT.into())));
        }
    }
    for number in listeners {
        let related = held.iter().any(|(account, _)| account == number);
        if !related {
            held.insert((*number, Reason::Defined(LISTEN.into())));
        }
    }
    held.into_iter()
        .filter(|(number, _)| *number != 0 && row.author != Principal::Account(*number))
        .map(|(recipient, reason)| Relation {
            recipient,
            reason,
            detail: Vec::new(),
        })
        .collect()
}

pub fn revision(row: &MsgRow) -> u64 {
    2 * u64::from(row.rev) + u64::from(row.deleted)
}

pub(crate) fn report(
    ctx: &ExecCtx,
    sender: &Principal,
    old: Option<&MsgRow>,
    row: &MsgRow,
) -> Result<(), Error> {
    let thread = thread_before(ctx, row)?;
    let listening = listeners(ctx, row)?;
    let after = relations(row, &thread, &listening);
    let before = old.map(|old| relations(old, &thread, &listening));
    let unrelated = after.is_empty() && before.is_none_or(|before| before.is_empty());
    if unrelated {
        return Ok(());
    }
    let actor = match sender {
        Principal::Account(number) => Actor::Account(*number),
        Principal::Root => Actor::System,
    };
    let update = Update {
        object: Object {
            kind: MESSAGE.into(),
            id: row.message_id.clone(),
        },
        revision: revision(row),
        actor,
        relations: after,
        transfers: Vec::new(),
    };
    ctx.emit(
        attribution::PROGRAM,
        &attribution::Op::Attribute(update),
        Reply::Wanted,
    );
    Ok(())
}
