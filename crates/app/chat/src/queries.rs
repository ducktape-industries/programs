//! One function per [`Query`](crate::Query), each named by [`Chat::query`](crate::Chat)'s
//! match and each a read of the tables in `state.rs`. Chat's listings only grow, so a page cursor from any height
//! resumes where it left off.
use ::program::Range;
use ::program::{Error, QueryCtx, invalid};
use std::collections::BTreeSet;
use store::{PageRequest, PageResponse};

use crate::state::{
    ANSWERED, CHANNEL_META, CHANNEL_TAGS, CHANNELS, HEADS, MESSAGE_IDS, MESSAGES, REACTIONS,
    REPLIES, ROOTS, TAGS, WORDS, message, messages, newest_first,
};
use crate::text::tag_label;
use crate::{ChannelInfo, ChannelRow, MessageHits, MsgRow, Principal, ReactedBy, Reply, tokens};

/// `reply` as `viewer` reads it: each reaction learns whether one of
/// `viewer` chose it.
pub(crate) fn seen_by(
    ctx: &QueryCtx,
    mut reply: Reply,
    viewer: Vec<Principal>,
) -> Result<Reply, Error> {
    for row in rows_in(&mut reply) {
        mark_reacted(ctx, &viewer, row);
    }
    Ok(reply)
}

/// The `Roots` cursor that resumes below `seq`: `PageRequest::after` for the page
/// of roots older than the one on screen.
pub fn roots_below(channel_id: &str, seq: u64) -> Vec<u8> {
    let channel_id = channel_id.to_owned();
    abi::encode(&store::Cursor {
        height: 0,
        scope: ROOTS.key(&channel_id),
        after: ROOTS.key(&(channel_id.clone(), newest_first(seq))),
    })
}

fn info(ctx: &QueryCtx, channel: ChannelRow) -> Result<ChannelInfo, Error> {
    let head_seq = HEADS.get(ctx, &channel.id)?.unwrap_or(0);
    let meta = CHANNEL_META.get(ctx, &channel.id)?.unwrap_or_default();
    Ok(ChannelInfo {
        channel,
        head_seq,
        meta,
    })
}

pub(crate) fn channels(
    ctx: &QueryCtx,
    page: &PageRequest,
) -> Result<PageResponse<ChannelInfo>, Error> {
    CHANNELS
        .range(ctx, page)?
        .try_map(|(_, channel)| info(ctx, channel))
}

pub(crate) fn channel(ctx: &QueryCtx, id: &String) -> Result<Option<ChannelInfo>, Error> {
    CHANNELS
        .get(ctx, id)?
        .map(|channel| info(ctx, channel))
        .transpose()
}

pub(crate) fn by_id(ctx: &QueryCtx, message_id: &String) -> Result<Option<MsgRow>, Error> {
    MESSAGE_IDS
        .get(ctx, message_id)?
        .map(|(channel_id, seq)| message(ctx, &channel_id, seq))
        .transpose()
}

/// The root of the author's most recently answered thread.
pub(crate) fn attention(
    ctx: &QueryCtx,
    channel_id: &str,
    author: Principal,
) -> Result<Option<MsgRow>, Error> {
    let channel_id = channel_id.to_owned();
    let newest = ANSWERED.prefix_of(&(channel_id.clone(), author)).limit(1);
    ANSWERED
        .scan(ctx, newest)?
        .first()
        .map(|(_, root)| message(ctx, &channel_id, *root))
        .transpose()
}

pub(crate) fn roots(
    ctx: &QueryCtx,
    channel_id: String,
    page: &PageRequest,
) -> Result<PageResponse<MsgRow>, Error> {
    let keys = ROOTS.range_of(ctx, &channel_id, page)?;
    rows_at(
        ctx,
        keys.map(|(channel_id, newest)| (channel_id, newest_first(newest))),
    )
}

pub(crate) fn thread(
    ctx: &QueryCtx,
    channel_id: String,
    root: u64,
    page: &PageRequest,
) -> Result<Reply, Error> {
    let keys = REPLIES.range_of(ctx, &(channel_id.clone(), root), page)?;
    Ok(Reply::Thread {
        root: MESSAGES.get(ctx, &(channel_id, root))?,
        replies: rows_at(ctx, keys.map(|(channel_id, _, reply)| (channel_id, reply)))?,
    })
}

/// Up to `page.limit` messages, half before `seq` and half from it.
pub(crate) fn around(
    ctx: &QueryCtx,
    channel_id: String,
    seq: u64,
    page: &PageRequest,
) -> Result<Vec<MsgRow>, Error> {
    let half = page.limit() / 2;
    let lo = MESSAGES.key(&(channel_id.clone(), seq.saturating_sub(half)));
    let hi = MESSAGES.key(&(channel_id, seq.saturating_add(half + 1)));
    let rows = MESSAGES.scan(ctx, Range::new(lo, Some(hi)))?;
    Ok(rows.into_iter().map(|(_, row)| row).collect())
}

pub(crate) fn search(
    ctx: &QueryCtx,
    text: &str,
    channel_id: Option<String>,
    page: &PageRequest,
) -> Result<MessageHits, Error> {
    let wanted = tokens(text);
    let Some(first) = wanted.first().cloned() else {
        return Err(invalid("nothing to search for"));
    };
    // ponytail: one posting list scanned, the rest filtered on the row;
    // intersect postings if search volume ever matters.
    let postings = match channel_id {
        Some(channel_id) => WORDS.prefix_of(&(first, channel_id)),
        None => WORDS.prefix_of(&first),
    };
    let postings = WORDS.scan(ctx, postings)?;
    let at = postings
        .into_iter()
        .map(|(_, channel_id, seq)| (channel_id, seq));
    let mut hits: Vec<MsgRow> = messages(ctx, at)?
        .into_iter()
        .filter(|row| wanted.is_subset(&tokens(&row.text)))
        .collect();
    hits.sort_by(|a, b| b.time.cmp(&a.time).then(b.seq.cmp(&a.seq)));
    let limit = page.limit() as usize;
    let capped = hits.len() > limit;
    hits.truncate(limit);
    Ok(MessageHits { hits, capped })
}

/// One page of the messages tagged `tag`, newest first.
pub(crate) fn tagged(
    ctx: &QueryCtx,
    tag: &str,
    channel_id: Option<String>,
    page: &PageRequest,
) -> Result<PageResponse<MsgRow>, Error> {
    let label = tag_label(tag);
    let keys = match channel_id {
        Some(channel_id) => CHANNEL_TAGS
            .range_of(ctx, &(channel_id, label), page)?
            .map(|(channel_id, _, newest)| (channel_id, newest_first(newest))),
        None => TAGS
            .range_of(ctx, &label, page)?
            .map(|(_, _, channel_id, seq)| (channel_id, seq)),
    };
    rows_at(ctx, keys)
}

/// A page of message addresses as the page of rows they name.
fn rows_at(
    ctx: &QueryCtx,
    keys: PageResponse<(String, u64)>,
) -> Result<PageResponse<MsgRow>, Error> {
    Ok(PageResponse {
        height: keys.height,
        items: messages(ctx, keys.items)?,
        next: keys.next,
    })
}

/// Who chose each reaction on the messages at `seqs`, each message once
/// and in seq order however `seqs` names them: a message's emoji in order,
/// each with its principals in key order.
pub(crate) fn reactors(
    ctx: &QueryCtx,
    channel_id: String,
    seqs: &[u64],
) -> Result<Vec<ReactedBy>, Error> {
    let mut reactors: Vec<ReactedBy> = Vec::new();
    for seq in seqs.iter().copied().collect::<BTreeSet<_>>() {
        let chosen = REACTIONS.scan(ctx, REACTIONS.prefix_of(&(channel_id.clone(), seq)))?;
        for (_, _, emoji, principal) in chosen {
            match reactors.last_mut() {
                Some(last) if last.seq == seq && last.emoji == emoji => last.by.push(principal),
                _ => reactors.push(ReactedBy {
                    seq,
                    emoji,
                    by: vec![principal],
                }),
            }
        }
    }
    Ok(reactors)
}

/// Every message row a reply carries.
fn rows_in(reply: &mut Reply) -> Vec<&mut MsgRow> {
    match reply {
        Reply::Roots(page) | Reply::TagHits(page) => page.items.iter_mut().collect(),
        Reply::Messages(rows) | Reply::Hits(MessageHits { hits: rows, .. }) => {
            rows.iter_mut().collect()
        }
        Reply::Thread { root, replies } => root.iter_mut().chain(&mut replies.items).collect(),
        Reply::Message(row) | Reply::Attention(row) => row.iter_mut().collect(),
        Reply::Channels(_)
        | Reply::Channel(_)
        | Reply::Members(_)
        | Reply::Accounts(_)
        | Reply::Reactors(_) => vec![],
    }
}

/// Each reaction on `row` learns whether one of `viewer` chose it.
fn mark_reacted(ctx: &QueryCtx, viewer: &[Principal], row: &mut MsgRow) {
    for reaction in &mut row.reactions {
        reaction.reacted_by_me = viewer.iter().any(|principal| {
            let key = (
                row.channel_id.clone(),
                row.seq,
                reaction.emoji.clone(),
                principal.clone(),
            );
            REACTIONS.has(ctx, &key)
        });
    }
}
