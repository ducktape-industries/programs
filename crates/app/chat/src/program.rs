//! The program: the signer resolved to its principal, then every op and
//! every query, each handed to its function in `ops.rs` or `queries.rs`.

use std::collections::BTreeMap;

use ::program::{Error, ExecCtx, MessageId, Outcome, Program, QueryCtx, invalid};

use crate::listen::{
    choose_for_many, chosen, chosen_by, followed, listen, listened, notices_cut, set_notices,
    watch, watched,
};
use crate::ops::{
    create_channel, delete, drafting, edit, open_dm, post, react, remove_meta, rename,
    set_archived, set_membership, set_meta, undraft,
};
use crate::queries::{
    around, attention, by_id, channel, channels, drafts, names, reactors, roots, seen_by, tagged,
    text_search, thread,
};
use crate::reader::Reader;
use crate::state::MEMBERS;
use crate::{Op, Query, Reply};

pub struct Chat;

impl Program for Chat {
    const NAME: &'static str = crate::PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;
    const RETURNS: &'static [::program::schema::Return] =
        <Op as ::program::tools::Described>::RETURNS;

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        let sender = ctx.sender()?;
        crate::linked::admitted(ctx, &sender, &op)?;
        crate::sealed::admitted(ctx, &op)?;
        crate::linked::heard(ctx, &sender);
        match op {
            Op::CreateChannel {
                channel_id,
                name,
                post_policy,
            } => {
                let meta = BTreeMap::new();
                create_channel(ctx, &sender, channel_id, name, post_policy, meta)
            }
            Op::CreateDmChannel { counterpart, name } => open_dm(ctx, &sender, counterpart, name),
            Op::RenameChannel { channel_id, name } => rename(ctx, &channel_id, name),
            Op::SetChannelArchived {
                channel_id,
                archived,
            } => set_archived(ctx, &channel_id, archived),
            Op::PostMessage {
                channel_id,
                message_id,
                blocks,
                thread,
            } => post(ctx, &sender, channel_id, message_id, blocks, thread),
            Op::EditMessage {
                channel_id,
                seq,
                blocks,
                base_rev,
            } => edit(ctx, &sender, &channel_id, seq, blocks, base_rev),
            Op::DeleteMessage { channel_id, seq } => delete(ctx, &sender, &channel_id, seq),
            Op::AddReaction {
                channel_id,
                seq,
                emoji,
            } => react(ctx, &sender, &channel_id, seq, &emoji, true),
            Op::RemoveReaction {
                channel_id,
                seq,
                emoji,
            } => react(ctx, &sender, &channel_id, seq, &emoji, false),
            Op::SetMembership {
                channel_id,
                principal,
                member,
            } => set_membership(ctx, &channel_id, principal, member),
            Op::CreateChannelWithMeta {
                channel_id,
                name,
                post_policy,
                meta,
            } => create_channel(ctx, &sender, channel_id, name, post_policy, meta),
            Op::SetChannelMeta {
                channel_id,
                key,
                value,
            } => set_meta(ctx, &channel_id, key, value),
            Op::RemoveChannelMeta { channel_id, key } => remove_meta(ctx, &channel_id, &key),
            Op::Listen {
                channel_id,
                listening,
            } => listen(ctx, ctx.sender_account()?, &channel_id, listening),
            Op::SetNotices {
                channel_id,
                notices,
            } => set_notices(ctx, ctx.sender_account()?, &channel_id, notices),
            Op::AdoptOwners { kind, after, limit } => {
                crate::adopt::adopt_owners(ctx, &kind, after, limit)
            }
            Op::Watch {
                channel_id,
                watching,
            } => watch(ctx, &channel_id, watching),
            Op::Drafting {
                channel_id,
                thread,
                text,
                anew,
                doing,
            } => drafting(ctx, &sender, &channel_id, thread, text, anew, doing),
            Op::Undraft { channel_id } => undraft(ctx, &sender, &channel_id),
            Op::Post {
                channel_id,
                message_id,
                text,
                thread,
            } => {
                let blocks = crate::parse_message(&text);
                post(ctx, &sender, channel_id, message_id, blocks, thread)
            }
            Op::ChooseForMany {
                accounts,
                channel_id,
                listening,
                notices,
            } => choose_for_many(ctx, &accounts, &channel_id, listening, notices),
            Op::Notify {
                channel_id,
                message_id,
                thread,
                notice,
            } => crate::ops::notify(ctx, &sender, channel_id, message_id, thread, notice),
            Op::Take { channel_id, thread } => {
                crate::leases::take(ctx, &sender, &channel_id, thread)
            }
            Op::Release { channel_id, thread } => {
                crate::leases::release(ctx, &sender, &channel_id, thread)
            }
            Op::WatchPlaces { deliver } => crate::leases::watch_places(ctx, deliver),
            Op::Break { channel_id, thread } => {
                crate::leases::break_place(ctx, &channel_id, thread)
            }
            Op::SetPostPolicy {
                channel_id,
                post_policy,
            } => crate::ops::set_post_policy(ctx, &channel_id, post_policy),
            Op::NotifyWith {
                channel_id,
                message_id,
                thread,
                notice,
                by,
                text,
            } => {
                let blocks = crate::ops::noticed(ctx, notice, text)?;
                crate::ops::notify_with(ctx, &sender, channel_id, message_id, thread, blocks, by)
            }
            Op::Link { channel_id, peer } => crate::linked::link(ctx, &channel_id, peer),
            Op::FromPeer { channel_id, write } => {
                crate::linked::from_peer(ctx, &sender, channel_id, write)
            }
            Op::PublishDeviceKey { key, sig } => crate::sealed::publish(ctx, key, sig),
            Op::RetireDeviceKey { key } => crate::sealed::retire(ctx, key),
            Op::Seal { channel_id, wraps } => crate::sealed::seal(ctx, &sender, &channel_id, wraps),
            Op::PostSealed {
                channel_id,
                message_id,
                thread,
                epoch,
                nonce,
                ct,
            } => {
                let body = crate::state::Body { epoch, nonce, ct };
                crate::sealed::post_sealed(ctx, &sender, channel_id, message_id, thread, body)
            }
            Op::Rekey {
                channel_id,
                epoch,
                wraps,
            } => crate::sealed::rekey(ctx, &sender, &channel_id, epoch, wraps),
            Op::Wrap { channel_id, wraps } => crate::sealed::wrap(ctx, &sender, &channel_id, wraps),
            Op::Unopenable {
                channel_id,
                epoch,
                device,
            } => crate::sealed::unopenable(ctx, &channel_id, epoch, device),
        }
    }

    fn tools() -> ::program::tools::Tools {
        ::program::tools::Tools::of::<Op, Query>()
    }

    fn reply(_ctx: &ExecCtx, _id: &MessageId, _outcome: &Outcome) -> Result<(), Error> {
        Ok(())
    }

    fn access(ctx: &QueryCtx, op: &Op) -> Result<Vec<::program::Need>, Error> {
        crate::gate::needs(ctx, op)
    }

    fn resource(ctx: &QueryCtx, key: &str) -> Result<::program::Resource, Error> {
        crate::gate::resource(ctx, key)
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        match query {
            Query::As { reader, query } => {
                let asked = abi::decode::<Query>(&query)
                    .map_err(|_| invalid("As carries the borsh bytes of one chat query"))?;
                answer(ctx, &Reader::naming(ctx, reader), asked)
            }
            query => answer(ctx, &Reader::of(ctx), query),
        }
    }
}

fn answer(ctx: &QueryCtx, reader: &Reader, query: Query) -> Result<Reply, Error> {
    let (reply, viewer) = match query {
        Query::Channels { page } => (Reply::Channels(channels(ctx, &page)?), vec![]),
        Query::Channel { channel_id } => (Reply::Channel(channel(ctx, &channel_id)?), vec![]),
        Query::MessageById { message_id } => {
            let row = by_id(ctx, &message_id)?;
            if let Some(row) = &row {
                reader.room(ctx, &row.channel_id)?;
            }
            (Reply::Message(row), vec![])
        }
        Query::ThreadAttention { channel_id, author } => {
            reader.room(ctx, &channel_id)?;
            let attended = attention(ctx, &channel_id, author)?;
            (Reply::Attention(attended), vec![])
        }
        Query::Roots {
            channel_id,
            viewer,
            page,
        } => {
            reader.room(ctx, &channel_id)?;
            (Reply::Roots(roots(ctx, channel_id, &page)?), viewer)
        }
        Query::MessagesAround {
            channel_id,
            seq,
            viewer,
            page,
        } => {
            reader.room(ctx, &channel_id)?;
            let rows = around(ctx, channel_id, seq, &page)?;
            (Reply::Messages(rows), viewer)
        }
        Query::Thread {
            channel_id,
            root_seq,
            viewer,
            page,
        } => {
            reader.room(ctx, &channel_id)?;
            (thread(ctx, channel_id, root_seq, &page)?, viewer)
        }
        Query::Members { channel_id, page } => {
            reader.room(ctx, &channel_id)?;
            let members = MEMBERS.range_of(ctx, &channel_id, &page)?;
            (Reply::Members(members.map(|(_, member)| member)), vec![])
        }
        Query::TagSearch {
            tag,
            viewer,
            channel_id,
            page,
        } => {
            if let Some(channel_id) = &channel_id {
                reader.room(ctx, channel_id)?;
            }
            let hits = tagged(ctx, reader, &tag, channel_id, &page)?;
            (Reply::TagHits(hits), viewer)
        }
        Query::Accounts { page } => (Reply::Accounts(crate::origin::accounts(ctx, page)?), vec![]),
        Query::Reactors { channel_id, seqs } => {
            reader.room(ctx, &channel_id)?;
            (Reply::Reactors(reactors(ctx, channel_id, &seqs)?), vec![])
        }
        Query::Clients { channel_id, seqs } => {
            reader.room(ctx, &channel_id)?;
            let clients = crate::queries::clients(ctx, &channel_id, &seqs)?;
            (Reply::Clients(clients), vec![])
        }
        Query::Listening { account, page } => {
            (Reply::Listening(listened(ctx, account, &page)?), vec![])
        }
        Query::Notices { account, page } => {
            (Reply::Notices(notices_cut(ctx, account, &page)?), vec![])
        }
        Query::Followed {
            channel_id,
            account,
            roots,
        } => (
            Reply::Followed(followed(ctx, channel_id, account, &roots)?),
            vec![],
        ),
        Query::Watching { account, page } => {
            (Reply::Watching(watched(ctx, account, &page)?), vec![])
        }
        Query::Chosen {
            account,
            channel_id,
        } => (chosen(ctx, account, &channel_id)?, vec![]),
        Query::Drafts { channel_id } => {
            reader.room(ctx, &channel_id)?;
            (Reply::Drafts(drafts(ctx, &channel_id)?), vec![])
        }
        Query::Names { ids } => (Reply::Names(names(ctx, &ids)?), vec![]),
        Query::PostedIn { account, page } => (
            Reply::PostedIn(crate::order::posted_in(ctx, account, &page)?),
            vec![],
        ),
        Query::ChosenBy {
            channel_id,
            accounts,
        } => (chosen_by(ctx, &channel_id, &accounts)?, vec![]),
        Query::Unread { account, rooms } => (
            Reply::Unread(crate::notice::unread(ctx, account, &rooms)?),
            vec![],
        ),
        Query::TextSearch {
            text,
            viewer,
            channel_id,
            page,
        } => {
            if let Some(channel_id) = &channel_id {
                reader.room(ctx, channel_id)?;
            }
            let hits = text_search(ctx, reader, &text, channel_id, &page)?;
            (Reply::TextHits(hits), viewer)
        }
        Query::Busy {
            channel_id,
            thread,
            for_account,
        } => {
            let me = crate::Principal::Account(for_account);
            let holder = crate::leases::busy(ctx, &channel_id, thread, &me)?;
            (Reply::Busy(holder), vec![])
        }
        Query::Holders {
            channel_id,
            thread,
            page,
        } => {
            let holders = crate::leases::holders(ctx, &channel_id, thread, &page)?;
            (Reply::Holders(holders), vec![])
        }
        Query::Seated {
            principal,
            channel_ids,
        } => {
            let seated = channel_ids
                .into_iter()
                .filter(|id| MEMBERS.has(ctx, &(id.clone(), principal.clone())))
                .collect();
            (Reply::Seated(seated), vec![])
        }
        Query::Repliers { channel_id, roots } => {
            reader.room(ctx, &channel_id)?;
            let replied = crate::queries::repliers(ctx, &channel_id, roots)?;
            (Reply::Repliers(replied), vec![])
        }
        Query::Peer { channel_id } => (Reply::Peer(crate::linked::peer(ctx, &channel_id)?), vec![]),
        Query::Foreigners { page } => (
            Reply::Foreigners(crate::linked::foreigners(ctx, &page)?),
            vec![],
        ),
        Query::DeviceKeys { accounts } => (
            Reply::DeviceKeys(crate::sealed::device_keys(ctx, &accounts)?),
            vec![],
        ),
        Query::Sealing { channel_ids } => (
            Reply::Sealing(crate::sealed::sealing(ctx, &channel_ids)?),
            vec![],
        ),
        Query::Wraps {
            channel_id,
            devices,
        } => {
            reader.room(ctx, &channel_id)?;
            let wraps = crate::sealed::wraps(ctx, &channel_id, &devices)?;
            (Reply::Wraps(wraps), vec![])
        }
        Query::Sealed { channel_id, seqs } => {
            reader.room(ctx, &channel_id)?;
            let sealed = crate::sealed::sealed(ctx, &channel_id, &seqs)?;
            (Reply::Sealed(sealed), vec![])
        }
        Query::As { .. } => {
            return Err(invalid(
                "an As inside an As is refused: a proxy names one reader",
            ));
        }
    };
    seen_by(ctx, reply, viewer)
}

#[cfg(feature = "program")]
::program::export!(Chat);
