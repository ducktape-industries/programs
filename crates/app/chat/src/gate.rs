//! What each chat op needs of the access program, and who owns each key: a channel by whoever
//! opened it, a message (`<channel>/<seq>`) by its author, a user of another network too; only
//! the owner writes it unless the owner lists others, and a channel's owner deletes its messages.
use ::program::{Error, Need, Ownership, Party, Principal, QueryCtx, Resource};

use crate::Op;
use crate::state::{CHANNELS, stored};

pub fn message_key(channel_id: &str, seq: u64) -> String {
    format!("{channel_id}/{seq}")
}

pub(crate) fn needs(ctx: &QueryCtx, op: &Op) -> Result<Vec<Need>, Error> {
    Ok(match op {
        Op::RenameChannel { channel_id, .. }
        | Op::SetChannelArchived { channel_id, .. }
        | Op::SetMembership { channel_id, .. }
        | Op::SetChannelMeta { channel_id, .. }
        | Op::RemoveChannelMeta { channel_id, .. }
        | Op::Break { channel_id, .. }
        | Op::SetPostPolicy { channel_id, .. }
        | Op::Link { channel_id, .. } => vec![Need::write(channel_id.clone())],
        Op::FromPeer { channel_id, write } => crate::linked::needs(ctx, channel_id, write)?,
        Op::EditMessage {
            channel_id, seq, ..
        } => vec![Need::write(message_key(channel_id, *seq))],
        Op::DeleteMessage { channel_id, seq } => vec![Need::delete(message_key(channel_id, *seq))],
        Op::CreateChannel { .. }
        | Op::CreateChannelWithMeta { .. }
        | Op::CreateDmChannel { .. }
        | Op::PostMessage { .. }
        | Op::Post { .. }
        | Op::AddReaction { .. }
        | Op::RemoveReaction { .. }
        | Op::Listen { .. }
        | Op::SetNotices { .. }
        | Op::AdoptOwners { .. }
        | Op::Watch { .. }
        | Op::Drafting { .. }
        | Op::Undraft { .. }
        | Op::ChooseForMany { .. }
        | Op::Notify { .. }
        | Op::NotifyWith { .. }
        | Op::Take { .. }
        | Op::Release { .. }
        | Op::WatchPlaces { .. } => Vec::new(),
    })
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
    let owner = owner.and_then(|owner| match owner {
        Principal::Account(number) => Some(Party::Account(number)),
        Principal::Foreign(foreign) => Some(Party::Foreign(foreign.into())),
        Principal::Root => None,
    });
    Ok(Resource {
        owner,
        default: Some(Ownership::Owner),
    })
}

fn author(ctx: &QueryCtx, channel_id: &str, seq: u64) -> Result<Option<Principal>, Error> {
    Ok(stored(ctx, &(channel_id.to_owned(), seq))?.map(|row| row.author))
}
