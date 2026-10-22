//! What chat tells attribution as a message is posted, edited or deleted: the accounts it
//! mentions, in a direct message the counterpart it is written to, and in a thread reply the
//! accounts that took part in the thread before it.
use std::collections::BTreeSet;

use ::program::{Error, ExecCtx, QueryCtx, Reply, unexpected_reply};
use attribution::{Actor, Object, Reason, Relation, Update};
use borsh::{BorshDeserialize, BorshSerialize};

use crate::listen::{listeners, watchers};
use crate::state::{PARTICIPANTS, message};
use crate::{AccountNumber, Block, Mark, MsgRow, Principal, dm_peers};

pub const MESSAGE: &str = "message";
pub const DIRECT: &str = "direct";
pub const THREAD: &str = "thread";
// each account listening to the message's channel that it relates to in no other way
pub const LISTEN: &str = "listen";
// each account watching the message's channel for a wait that it relates to in no other way
pub const WATCH: &str = "watch";
pub const NOTICE: &str = "notice";
pub const REACTION: &str = "reaction";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Reacted {
    pub message_id: String,
    pub emoji: String,
}

pub(crate) fn mentioned(blocks: &[Block]) -> impl Iterator<Item = AccountNumber> + '_ {
    blocks
        .iter()
        .flat_map(|block| match block {
            Block::Paragraph(spans) | Block::Quote(spans) => spans.as_slice(),
            Block::Code { .. } | Block::Divider | Block::Attachment(_) | Block::Notice(_) => &[],
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
    watchers: &[AccountNumber],
) -> Vec<Relation> {
    if row.deleted {
        return Vec::new();
    }
    let mut held = BTreeSet::new();
    match row.notice() {
        Some(notice) => {
            for number in &notice.concerns {
                held.insert((*number, Reason::Defined(NOTICE.into())));
            }
        }
        None => {
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
        }
    }
    for number in watchers {
        let related = held.iter().any(|(account, _)| account == number);
        if !related {
            held.insert((*number, Reason::Defined(WATCH.into())));
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
    let watching = watchers(ctx, row)?;
    let after = relations(row, &thread, &listening, &watching);
    let before = old.map(|old| relations(old, &thread, &listening, &watching));
    let unrelated = after.is_empty() && before.is_none_or(|before| before.is_empty());
    if unrelated {
        return Ok(());
    }
    let actor = match sender {
        Principal::Account(number) => Actor::Account(*number),
        Principal::Root => Actor::System,
        Principal::Foreign(foreign) => Actor::Foreign((*foreign).into()),
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

pub fn reaction_id(channel_id: &str, seq: u64, reactor: AccountNumber, emoji: &str) -> String {
    let emoji: String = emoji.bytes().map(|byte| format!("{byte:02x}")).collect();
    format!("{channel_id}:{seq}:{reactor}:{emoji}")
}

pub(crate) fn reaction(
    ctx: &QueryCtx,
    reactor: &Principal,
    row: &MsgRow,
    emoji: &str,
    on: bool,
) -> Result<Option<Update>, Error> {
    let (Principal::Account(reactor), Principal::Account(author)) = (reactor, &row.author) else {
        return Ok(None);
    };
    if reactor == author {
        return Ok(None);
    }
    let object = Object {
        kind: REACTION.into(),
        id: reaction_id(&row.channel_id, row.seq, *reactor, emoji),
    };
    let source = attribution::Source {
        program: ctx.env().program.clone(),
        object: object.clone(),
    };
    let asked = attribution::Query::Relations { source };
    let held = match ctx.query(attribution::PROGRAM, &asked) {
        Ok(attribution::Reply::Relations(held)) => held,
        Ok(other) => return Err(unexpected_reply(attribution::PROGRAM, "Relations", &other)),
        Err(_) => return Ok(None),
    };
    let related = held.as_ref().is_some_and(|held| !held.relations.is_empty());
    if related == on {
        return Ok(None);
    }
    let reacted = Reacted {
        message_id: row.message_id.clone(),
        emoji: emoji.to_owned(),
    };
    let relations = match on {
        true => vec![Relation {
            recipient: *author,
            reason: Reason::Defined(REACTION.into()),
            detail: abi::encode(&reacted),
        }],
        false => Vec::new(),
    };
    Ok(Some(Update {
        object,
        revision: held.map_or(0, |held| held.revision) + 1,
        actor: Actor::Account(*reactor),
        relations,
        transfers: Vec::new(),
    }))
}

pub(crate) fn reacted(ctx: &ExecCtx, updates: Vec<Update>) {
    if updates.is_empty() {
        return;
    }
    ctx.emit(
        attribution::PROGRAM,
        &attribution::Op::AttributeBatch { updates },
        Reply::Wanted,
    );
}
