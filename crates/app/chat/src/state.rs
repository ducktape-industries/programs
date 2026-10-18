//! Everything chat keeps, declared once: each table's key and value types,
//! and the writes that keep a message and its indexes in step. Keys are
//! typed (`store::KeyCodec`: integers big-endian, strings and principals
//! NUL-terminated, so names list by name), values borsh. A tuple key scans by its leading
//! elements, which is how every "in this channel" read works.
use std::collections::BTreeMap;

use ::program::{Error, ExecCtx, QueryCtx, not_found};
use store::{Map, Set};

use crate::{ChannelRow, MemberRow, MsgRow, Principal, tokens};

type ChannelId = String;
type Seq = u64;

pub const CHANNELS: Map<ChannelId, ChannelRow> = Map::new("channel/");
pub const CHANNEL_META: Map<ChannelId, BTreeMap<String, String>> = Map::new("channel-meta/");
/// The newest message's seq, per channel.
pub const HEADS: Map<ChannelId, Seq> = Map::new("head/");
pub const MESSAGES: Map<(ChannelId, Seq), MsgRow> = Map::new("message/");
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
/// Search postings: `(word, channel, seq)`.
pub const WORDS: Set<(String, ChannelId, Seq)> = Set::new("word/");
/// Tag postings, newest first: `(tag, newest_first(time), channel, seq)`.
pub const TAGS: Set<(String, u64, ChannelId, Seq)> = Set::new("tag/");
/// Tag postings within a channel, newest first:
/// `(channel, tag, newest_first(seq))`.
pub const CHANNEL_TAGS: Set<(ChannelId, String, Seq)> = Set::new("channel-tag/");

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
    MESSAGES
        .get(ctx, &(channel_id.to_owned(), seq))?
        .ok_or_else(|| not_found(format!("no message {channel_id}/{seq}")))
}

/// The rows at these addresses; one gone (never, in chat) is skipped.
pub(crate) fn messages(
    ctx: &QueryCtx,
    at: impl IntoIterator<Item = (ChannelId, Seq)>,
) -> Result<Vec<MsgRow>, Error> {
    at.into_iter()
        .filter_map(|key| MESSAGES.get(ctx, &key).transpose())
        .collect()
}

pub(crate) fn replace_message(ctx: &ExecCtx, old: Option<&MsgRow>, row: &MsgRow) {
    if let Some(old) = old {
        postings(ctx, old, false);
    }
    postings(ctx, row, true);
    MESSAGES.put(ctx, &(row.channel_id.clone(), row.seq), row);
}

/// Every search and tag posting a row makes, on or off.
fn postings(ctx: &ExecCtx, row: &MsgRow, on: bool) {
    let (channel, seq) = (row.channel_id.clone(), row.seq);
    for word in tokens(&row.text) {
        toggle(ctx, &WORDS, &(word, channel.clone(), seq), on);
    }
    for tag in &row.tags {
        let when = newest_first(row.time);
        toggle(ctx, &TAGS, &(tag.clone(), when, channel.clone(), seq), on);
        let newest = newest_first(seq);
        toggle(
            ctx,
            &CHANNEL_TAGS,
            &(channel.clone(), tag.clone(), newest),
            on,
        );
    }
}

pub(crate) fn toggle<K: store::KeyCodec>(ctx: &ExecCtx, set: &Set<K>, key: &K, on: bool) {
    if on {
        set.insert(ctx, key);
    } else {
        set.remove(ctx, key);
    }
}
