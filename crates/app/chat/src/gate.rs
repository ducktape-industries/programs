//! What each chat op needs of the access program, and who owns each key: a channel (its id) by
//! the account that opened it, a message (`<channel>/<seq>`) by its author. Each is written by
//! its owner alone unless the owner lists others; a channel's owner also deletes its messages.
use ::program::{Error, Need, Ownership, Party, Principal, QueryCtx, Resource};

use crate::Op;
use crate::state::{CHANNELS, MESSAGES};

pub fn message_key(channel_id: &str, seq: u64) -> String {
    format!("{channel_id}/{seq}")
}

pub(crate) fn needs(op: &Op) -> Vec<Need> {
    match op {
        Op::RenameChannel { channel_id, .. }
        | Op::SetChannelArchived { channel_id, .. }
        | Op::SetMembership { channel_id, .. } => vec![Need::write(channel_id.clone())],
        Op::EditMessage {
            channel_id, seq, ..
        } => vec![Need::write(message_key(channel_id, *seq))],
        Op::DeleteMessage { channel_id, seq } => vec![Need::delete(message_key(channel_id, *seq))],
        Op::CreateChannel { .. }
        | Op::CreateDmChannel { .. }
        | Op::PostMessage { .. }
        | Op::AddReaction { .. }
        | Op::RemoveReaction { .. } => Vec::new(),
    }
}

pub(crate) fn resource(ctx: &QueryCtx, key: &str) -> Result<Resource, Error> {
    let owner = match key.split_once('/') {
        None => CHANNELS
            .get(ctx, &key.to_owned())?
            .map(|channel| channel.owner),
        Some((channel_id, seq)) => match seq.parse::<u64>() {
            Ok(seq) => author(ctx, channel_id, seq)?,
            Err(_) => None,
        },
    };
    let owner = owner
        .as_ref()
        .and_then(Principal::account)
        .map(Party::Account);
    Ok(Resource {
        owner,
        default: Some(Ownership::Owner),
    })
}

fn author(ctx: &QueryCtx, channel_id: &str, seq: u64) -> Result<Option<Principal>, Error> {
    Ok(MESSAGES
        .get(ctx, &(channel_id.to_owned(), seq))?
        .map(|row| row.author))
}
