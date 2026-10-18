//! Everything chat keeps, declared once: each table's key and value types,
//! and the one place a message row is stored and read back. Keys are
//! typed (`store::KeyCodec`: integers big-endian, strings and principals
//! NUL-terminated, so names list by name), values borsh. A tuple key scans by its leading
//! elements, which is how every "in this channel" read works.
use std::collections::BTreeMap;

use ::program::Range;
use ::program::{Error, ExecCtx, QueryCtx, not_found};
use borsh::{BorshDeserialize, BorshSerialize};
use store::{Map, Set};

use crate::legacy;
use crate::{
    AccountNumber, Block, ChannelRow, MemberRow, MsgRow, Principal, Reaction, plain_text, tags,
};

type ChannelId = String;
type Seq = u64;

pub const CHANNELS: Map<ChannelId, ChannelRow> = Map::new("channel/");
pub const CHANNEL_META: Map<ChannelId, BTreeMap<String, String>> = Map::new("channel-meta/");
/// The newest message's seq, per channel.
pub const HEADS: Map<ChannelId, Seq> = Map::new("head/");
// each message as chat keeps it; a channel not migrated yet still holds old rows (`legacy`)
pub const MESSAGES: Map<(ChannelId, Seq), Stored> = Map::new("stored/");
/// Where each message id lives: ids are unique across channels.
pub const MESSAGE_IDS: Map<String, (ChannelId, Seq)> = Map::new("message-id/");
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
    // each emoji chosen and how many chose it, in emoji order
    reactions: Vec<(String, u64)>,
}

impl Stored {
    // The row a read answers.
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

/// A key part that scans newest first. Its own inverse.
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

/// The rows at these addresses; one gone (never, in chat) is skipped.
pub(crate) fn messages(
    ctx: &QueryCtx,
    at: impl IntoIterator<Item = (ChannelId, Seq)>,
) -> Result<Vec<MsgRow>, Error> {
    at.into_iter()
        .filter_map(|key| stored(ctx, &key).transpose())
        .map(|row| row.map(Stored::row))
        .collect()
}

// Stores `row` as chat keeps it ([`Stored`]): what a read derives from
// the rest is never stored, so a row's `text`, `tags` and
// `reacted_by_me` are dropped here, whatever they hold.
pub(crate) fn put_message(ctx: &ExecCtx, row: &MsgRow) -> Result<(), Error> {
    let key = (row.channel_id.clone(), row.seq);
    legacy::forget(ctx, &key)?;
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

// the row at `key`: the one chat keeps, else an old row its channel has not migrated yet
pub(crate) fn stored(ctx: &QueryCtx, key: &(ChannelId, Seq)) -> Result<Option<Stored>, Error> {
    if let Some(row) = MESSAGES.get(ctx, key)? {
        return Ok(Some(row));
    }
    Ok(legacy::OLD_MESSAGES.get(ctx, key)?.map(Stored::from))
}

// `channel_id`'s rows from seq `lo` to below `hi` in seq order, old rows among them
pub(crate) fn stored_between(
    ctx: &QueryCtx,
    channel_id: &str,
    lo: Seq,
    hi: Seq,
) -> Result<Vec<Stored>, Error> {
    let at = |seq: Seq| (channel_id.to_owned(), seq);
    let old = legacy::OLD_MESSAGES;
    let range = Range::new(old.key(&at(lo)), Some(old.key(&at(hi))));
    let mut rows: BTreeMap<Seq, Stored> = (old.scan(ctx, range)?.into_iter())
        .map(|((_, seq), row)| (seq, Stored::from(row)))
        .collect();
    let range = Range::new(MESSAGES.key(&at(lo)), Some(MESSAGES.key(&at(hi))));
    rows.extend((MESSAGES.scan(ctx, range)?.into_iter()).map(|((_, seq), row)| (seq, row)));
    Ok(rows.into_values().collect())
}

pub(crate) fn toggle<K: store::KeyCodec>(ctx: &ExecCtx, set: &Set<K>, key: &K, on: bool) {
    if on {
        set.insert(ctx, key);
    } else {
        set.remove(ctx, key);
    }
}
