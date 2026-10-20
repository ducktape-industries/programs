//! Everything chat keeps, declared once: each table's key and value types,
//! and the one place a message row is stored and read back. Keys are
//! typed (`store::KeyCodec`: integers big-endian, strings and principals
//! NUL-terminated, so names list by name), values borsh. A tuple key scans by its leading
//! elements, which is how every "in this channel" read works.
use std::collections::BTreeMap;

use ::program::Range;
use ::program::{Error, ExecCtx, QueryCtx, not_found};
use borsh::{BorshDeserialize, BorshSerialize};
use store::{Kind, Map, Owned, Set};

use crate::{
    AccountNumber, Block, ChannelRow, MemberRow, MsgRow, Notices, Principal, Reaction, plain_text,
    tags,
};

type ChannelId = String;
type Seq = u64;

// Each channel is its opener's, of the kind `chat.channel`. Every op that reshapes a channel
// writes its row, its meta and its seats included, so the channel's policy decides who may.
pub const CHANNELS: Map<ChannelId, Owned<ChannelRow>> = Map::new("channel/");
// a channel's meta, beside its row: the ops that write it rewrite the row too
pub const CHANNEL_META: Map<ChannelId, BTreeMap<String, String>> = Map::new("channel-meta/");
/// The newest message's seq, per channel.
pub const HEADS: Map<ChannelId, Seq> = Map::new("head/");
// each message as chat keeps it
pub const MESSAGES: Map<(ChannelId, Seq), Stored> = Map::new("stored/");
/// Where each message id lives: ids are unique across channels.
// Each id is its author's, of the kind `chat.message`: only a post writes it, and the author's
// own edit or delete writes it again, so the message's policy decides them. The message itself
// (`MESSAGES`) is shared, since replies and reactions by others rewrite its row; a moderator's
// delete writes the tombstone there and the channel's row, never the author's id.
pub const MESSAGE_IDS: Map<String, Owned<(ChannelId, Seq), Authored>> = Map::new("message-id/");
pub const MEMBERS: Map<(ChannelId, Principal), MemberRow> = Map::new("member/");
/// Who chose which emoji: `(channel, seq, emoji, principal)`.
pub const REACTIONS: Set<(ChannelId, Seq, String, Principal)> = Set::new("reaction/");

/// Timeline roots, newest first: `(channel, newest_first(seq))`.
pub const ROOTS: Set<(ChannelId, Seq)> = Set::new("root/");
/// Thread replies in post order: `(channel, root, reply)`.
pub const REPLIES: Set<(ChannelId, Seq, Seq)> = Set::new("reply/");
/// Each author's answered threads, newest answer first:
/// `(channel, root author, newest_first(last reply))` → the root's seq.
pub const ANSWERED: Map<(ChannelId, Principal, Seq), Seq> = Map::new("answered/");
pub const PARTICIPANTS: Map<(ChannelId, Seq, Principal), Seq> = Map::new("participant/");
// who listens to each channel, and the height each began: `(channel, account)` → height
pub const LISTENERS: Map<(ChannelId, AccountNumber), u64> = Map::new("listener/");
// the channels each account listens to: `(account, channel)`
pub const LISTENING: Set<(AccountNumber, ChannelId)> = Set::new("listening/");
// each principal's first message in each channel's timeline: `(channel, principal)` → its seq
pub const POSTED: Map<(ChannelId, Principal), Seq> = Map::new("posted/");
// each principal's first mention in each thread's replies (its root says its own):
// `(channel, root, principal)` → the seq of the reply that mentioned it
pub const MENTIONED: Map<(ChannelId, Seq, Principal), Seq> = Map::new("mentioned/");
// what of each channel still tells an account that cut its notices: `(account, channel)`
pub const NOTICES: Map<(AccountNumber, ChannelId), Notices> = Map::new("notices/");
// who watches each channel for a wait, and the height each began: `(channel, account)` → height
pub const WATCHERS: Map<(ChannelId, AccountNumber), u64> = Map::new("watcher/");
// the channels each account watches: `(account, channel)`
pub const WATCHING: Set<(AccountNumber, ChannelId)> = Set::new("watching/");
// each author's draft in each room (`Op::Drafting`): `(channel, author)`
pub const DRAFTS: Map<(ChannelId, Principal), Drafted> = Map::new("draft/");
// a draft's text, a row per piece, folded on read: a growing draft adds a row, never rewrites one
pub const DRAFT_PIECES: Map<(ChannelId, Principal, u64), String> = Map::new("draft-piece/");

// A message as chat keeps it: the [`MsgRow`] a read answers, less what a
// read derives (its flat text and `#tags`, from `blocks`; whether the
// reader chose a reaction). [`put_message`] and [`Stored::row`] are the one
// mapping between the two.
#[derive(BorshSerialize, BorshDeserialize)]
pub struct Stored {
    pub(crate) channel_id: ChannelId,
    pub(crate) seq: Seq,
    message_id: String,
    pub(crate) author: Principal,
    height: u64,
    pub(crate) time: u64,
    pub(crate) blocks: Vec<Block>,
    deleted: bool,
    edited: bool,
    rev: u32,
    edited_at: Option<u64>,
    base_rev: Option<u32>,
    thread: Option<Seq>,
    reply_count: u64,
    last_reply_seq: Option<Seq>,
    reactions: Vec<(String, u64)>,
}

impl Stored {
    pub(crate) fn row(self) -> MsgRow {
        MsgRow {
            text: plain_text(&self.blocks),
            tags: tags(&self.blocks),
            reactions: (self.reactions.into_iter())
                .map(|(emoji, count)| Reaction {
                    emoji,
                    count,
                    reacted_by_me: false,
                })
                .collect(),
            channel_id: self.channel_id,
            seq: self.seq,
            message_id: self.message_id,
            author: self.author,
            height: self.height,
            time: self.time,
            blocks: self.blocks,
            deleted: self.deleted,
            edited: self.edited,
            rev: self.rev,
            edited_at: self.edited_at,
            base_rev: self.base_rev,
            thread: self.thread,
            reply_count: self.reply_count,
            last_reply_seq: self.last_reply_seq,
        }
    }
}

pub(crate) fn newest_first(n: u64) -> u64 {
    u64::MAX - n
}

pub(crate) fn channel(ctx: &QueryCtx, id: &str) -> Result<ChannelRow, Error> {
    CHANNELS
        .get(ctx, &id.to_owned())?
        .ok_or_else(|| not_found(format!("no channel {id}")))
}

pub(crate) fn message(ctx: &QueryCtx, channel_id: &str, seq: Seq) -> Result<MsgRow, Error> {
    stored(ctx, &(channel_id.to_owned(), seq))?
        .map(Stored::row)
        .ok_or_else(|| not_found(format!("no message {channel_id}/{seq}")))
}

pub(crate) fn messages(
    ctx: &QueryCtx,
    at: impl IntoIterator<Item = (ChannelId, Seq)>,
) -> Result<Vec<MsgRow>, Error> {
    at.into_iter()
        .filter_map(|key| stored(ctx, &key).transpose())
        .map(|row| row.map(Stored::row))
        .collect()
}

pub(crate) fn put_message(ctx: &ExecCtx, row: &MsgRow) -> Result<(), Error> {
    let key = (row.channel_id.clone(), row.seq);
    MESSAGES.put(ctx, &key, &Stored::from(row.clone()));
    Ok(())
}

impl From<MsgRow> for Stored {
    fn from(row: MsgRow) -> Stored {
        let MsgRow {
            channel_id,
            seq,
            message_id,
            author,
            height,
            time,
            blocks,
            text: _,
            deleted,
            edited,
            rev,
            edited_at,
            base_rev,
            thread,
            reply_count,
            last_reply_seq,
            reactions,
            tags: _,
        } = row;
        Stored {
            channel_id,
            seq,
            message_id,
            author,
            height,
            time,
            blocks,
            deleted,
            edited,
            rev,
            edited_at,
            base_rev,
            thread,
            reply_count,
            last_reply_seq,
            reactions: (reactions.into_iter())
                .map(|reaction| (reaction.emoji, reaction.count))
                .collect(),
        }
    }
}

pub(crate) fn stored(ctx: &QueryCtx, key: &(ChannelId, Seq)) -> Result<Option<Stored>, Error> {
    MESSAGES.get(ctx, key)
}

pub(crate) fn stored_between(
    ctx: &QueryCtx,
    channel_id: &str,
    lo: Seq,
    hi: Seq,
) -> Result<Vec<Stored>, Error> {
    let at = |seq: Seq| (channel_id.to_owned(), seq);
    let range = Range::new(MESSAGES.key(&at(lo)), Some(MESSAGES.key(&at(hi))));
    let rows = MESSAGES.scan(ctx, range)?.into_iter();
    Ok(rows.map(|(_, row)| row).collect())
}

pub(crate) fn toggle<K: store::KeyCodec>(ctx: &ExecCtx, set: &Set<K>, key: &K, on: bool) {
    if on {
        set.insert(ctx, key);
    } else {
        set.remove(ctx, key);
    }
}

pub struct Authored;

impl Kind for Authored {
    const KIND: &'static str = "chat.message";
}

impl Kind for ChannelRow {
    const KIND: &'static str = "chat.channel";
}

// A draft as chat keeps it beside its pieces: the thread root it answers under, the block and time
// it opened, what its author says it is doing, and how many pieces its text has.
#[derive(BorshSerialize, BorshDeserialize)]
pub struct Drafted {
    pub(crate) thread: Option<Seq>,
    pub(crate) height: u64,
    pub(crate) time: u64,
    pub(crate) doing: String,
    pub(crate) pieces: u64,
}
