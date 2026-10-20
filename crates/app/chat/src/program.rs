//! The program: the signer resolved to its principal, then every op and
//! every query, each handed to its function in `ops.rs` or `queries.rs`.

use std::collections::BTreeMap;

use ::program::{Error, ExecCtx, MessageId, Outcome, Program, QueryCtx};

use crate::listen::{
    chooser, chosen, followed, listen, listened, notices_cut, set_notices, watch, watched,
};
use crate::ops::{
    create_channel, delete, drafting, edit, open_dm, post, react, remove_meta, rename,
    set_archived, set_membership, set_meta, undraft,
};
use crate::queries::{
    around, attention, by_id, channel, channels, drafts, names, reactors, roots, search, seen_by,
    tagged, thread,
};
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
            Op::Migrate { channel_id } => crate::state::channel(ctx, &channel_id).map(drop),
            Op::Listen {
                channel_id,
                listening,
            } => listen(ctx, ctx.sender_account()?, &channel_id, listening),
            Op::MigrateSome { .. } => Ok(()),
            Op::SetNotices {
                channel_id,
                notices,
            } => set_notices(ctx, ctx.sender_account()?, &channel_id, notices),
            Op::AdoptOwners { kind, after, limit } => {
                crate::adopt::adopt_owners(ctx, &kind, after, limit)
            }
            Op::ListenFor {
                account,
                channel_id,
                listening,
            } => {
                chooser(ctx, account)?;
                listen(ctx, account, &channel_id, listening)
            }
            Op::SetNoticesFor {
                account,
                channel_id,
                notices,
            } => {
                chooser(ctx, account)?;
                set_notices(ctx, account, &channel_id, notices)
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
            Op::SetRoomOrder { order } => crate::order::set_order(ctx, order),
            Op::Post {
                channel_id,
                message_id,
                text,
                thread,
            } => {
                let blocks = crate::parse_message(&text);
                post(ctx, &sender, channel_id, message_id, blocks, thread)
            }
        }
    }

    fn tools() -> ::program::tools::Tools {
        ::program::tools::Tools::of::<Op, Query>()
    }

    fn reply(_ctx: &ExecCtx, _id: &MessageId, _outcome: &Outcome) -> Result<(), Error> {
        Ok(())
    }

    fn access(_: &QueryCtx, op: &Op) -> Result<Vec<::program::Need>, Error> {
        Ok(crate::gate::needs(op))
    }

    fn resource(ctx: &QueryCtx, key: &str) -> Result<::program::Resource, Error> {
        crate::gate::resource(ctx, key)
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        let (reply, viewer) = match query {
            Query::Channels { page } => (Reply::Channels(channels(ctx, &page)?), vec![]),
            Query::Channel { channel_id } => (Reply::Channel(channel(ctx, &channel_id)?), vec![]),
            Query::MessageById { message_id } => (Reply::Message(by_id(ctx, &message_id)?), vec![]),
            Query::ThreadAttention { channel_id, author } => (
                Reply::Attention(attention(ctx, &channel_id, author)?),
                vec![],
            ),
            Query::Roots {
                channel_id,
                viewer,
                page,
            } => (Reply::Roots(roots(ctx, channel_id, &page)?), viewer),
            Query::MessagesAround {
                channel_id,
                seq,
                viewer,
                page,
            } => (
                Reply::Messages(around(ctx, channel_id, seq, &page)?),
                viewer,
            ),
            Query::Thread {
                channel_id,
                root_seq,
                viewer,
                page,
            } => (thread(ctx, channel_id, root_seq, &page)?, viewer),
            Query::Members { channel_id, page } => {
                let members = MEMBERS.range_of(ctx, &channel_id, &page)?;
                (Reply::Members(members.map(|(_, member)| member)), vec![])
            }
            Query::Search {
                text,
                viewer,
                channel_id,
                page,
            } => (Reply::Hits(search(ctx, &text, channel_id, &page)?), viewer),
            Query::TagSearch {
                tag,
                viewer,
                channel_id,
                page,
            } => (
                Reply::TagHits(tagged(ctx, &tag, channel_id, &page)?),
                viewer,
            ),
            Query::Accounts { page } => {
                (Reply::Accounts(crate::origin::accounts(ctx, page)?), vec![])
            }
            Query::Reactors { channel_id, seqs } => {
                (Reply::Reactors(reactors(ctx, channel_id, &seqs)?), vec![])
            }
            Query::Unmigrated { .. } => {
                let none = crate::PageResponse {
                    height: ctx.env().height,
                    items: Vec::new(),
                    next: None,
                };
                (Reply::ChannelIds(none), vec![])
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
            Query::Drafts { channel_id } => (Reply::Drafts(drafts(ctx, &channel_id)?), vec![]),
            Query::Names { ids } => (Reply::Names(names(ctx, &ids)?), vec![]),
            Query::RoomOrder { account } => {
                (Reply::RoomOrder(crate::order::order(ctx, account)?), vec![])
            }
            Query::PostedIn { account, page } => (
                Reply::PostedIn(crate::order::posted_in(ctx, account, &page)?),
                vec![],
            ),
        };
        seen_by(ctx, reply, viewer)
    }
}

#[cfg(feature = "program")]
::program::export!(Chat);
