//! One function per [`Query`](crate::Query), each named by [`Chat::query`](crate::Chat)'s
//! match and each a read of the tables in `state.rs`. Chat's listings only grow, so a page cursor from any height
//! resumes where it left off.
use ::program::{Error, QueryCtx, invalid};
use std::collections::BTreeSet;
use store::{KeyCodec, PageRequest, PageResponse};

use crate::state::{
    ANSWERED, CHANNEL_META, CHANNELS, DRAFT_PIECES, DRAFTS, HEADS, MESSAGE_IDS, REACTIONS, REPLIES,
    ROOTS, Stored, message, messages, newest_first, stored, stored_between,
};
use crate::text::{fold, tag_label};
use crate::{
    ChannelInfo, ChannelRow, Draft, MessageHits, MsgRow, Principal, ReactedBy, Reply, plain_text,
    tags,
};

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

pub fn replies_after(channel_id: &str, root: u64, seq: u64) -> Vec<u8> {
    let channel_id = channel_id.to_owned();
    abi::encode(&store::Cursor {
        height: 0,
        scope: REPLIES.key(&(channel_id.clone(), root)),
        after: REPLIES.key(&(channel_id, root, seq)),
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

pub(crate) fn names(ctx: &QueryCtx, ids: &[String]) -> Result<Vec<(String, String)>, Error> {
    let mut named = Vec::new();
    for id in ids {
        if let Some(channel) = CHANNELS.get(ctx, id)? {
            named.push((channel.id, channel.name));
        }
    }
    Ok(named)
}

pub(crate) fn by_id(ctx: &QueryCtx, message_id: &String) -> Result<Option<MsgRow>, Error> {
    MESSAGE_IDS
        .get(ctx, message_id)?
        .map(|(channel_id, seq)| message(ctx, &channel_id, seq))
        .transpose()
}

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
        root: stored(ctx, &(channel_id, root))?.map(Stored::row),
        replies: rows_at(ctx, keys.map(|(channel_id, _, reply)| (channel_id, reply)))?,
    })
}

// The stored messages from `seq - page.limit / 2` through `seq + page.limit / 2`.
pub(crate) fn around(
    ctx: &QueryCtx,
    channel_id: String,
    seq: u64,
    page: &PageRequest,
) -> Result<Vec<MsgRow>, Error> {
    let half = page.limit() / 2;
    let (lo, hi) = (seq.saturating_sub(half), seq.saturating_add(half + 1));
    let rows = stored_between(ctx, &channel_id, lo, hi)?;
    Ok(rows.into_iter().map(Stored::row).collect())
}

pub(crate) fn search(
    ctx: &QueryCtx,
    text: &str,
    channel_id: Option<String>,
    page: &PageRequest,
) -> Result<MessageHits, Error> {
    let terms = terms(text)?;
    let holds_every_term = |row: &Stored| holds_every(&terms, row);
    let limit = usize::try_from(page.limit()).unwrap_or(usize::MAX);
    let mut hits = newest(
        ctx,
        channel_id,
        None,
        limit.saturating_add(1),
        holds_every_term,
    )?;
    let capped = hits.len() > limit;
    hits.truncate(limit);
    Ok(MessageHits {
        hits: hits.into_iter().map(Stored::row).collect(),
        capped,
    })
}

pub(crate) fn text_search(
    ctx: &QueryCtx,
    text: &str,
    channel_id: Option<String>,
    page: &PageRequest,
) -> Result<PageResponse<MsgRow>, Error> {
    let terms = terms(text)?;
    let scope = PageRequest::scope_of(&(&terms, &channel_id));
    paged(ctx, scope, channel_id, page, |row| holds_every(&terms, row))
}

pub(crate) fn tagged(
    ctx: &QueryCtx,
    tag: &str,
    channel_id: Option<String>,
    page: &PageRequest,
) -> Result<PageResponse<MsgRow>, Error> {
    let label = tag_label(tag);
    let scope = PageRequest::scope_of(&(&label, &channel_id));
    paged(ctx, scope, channel_id, page, |row| {
        tags(&row.blocks).contains(&label)
    })
}

// The whitespace-separated terms of `text`, folded as a message's text is.
fn terms(text: &str) -> Result<Vec<String>, Error> {
    let terms: Vec<String> = fold(text).split_whitespace().map(str::to_owned).collect();
    match terms.is_empty() {
        true => Err(invalid("nothing to search for")),
        false => Ok(terms),
    }
}

fn holds_every(terms: &[String], row: &Stored) -> bool {
    let flat = fold(&plain_text(&row.blocks));
    terms.iter().all(|term| flat.contains(term.as_str()))
}

// One page of the messages `keep` admits, newest first, resumed from the cursor `page` carries
// in the listing `scope` names.
fn paged(
    ctx: &QueryCtx,
    scope: Vec<u8>,
    channel_id: Option<String>,
    page: &PageRequest,
    keep: impl Fn(&Stored) -> bool,
) -> Result<PageResponse<MsgRow>, Error> {
    let listing = page.listing(ctx, scope.clone())?;
    let after =
        match page.open(&scope)? {
            None => None,
            Some(cursor) => Some(Place::decode_key(&mut cursor.after.as_slice()).ok_or_else(
                || invalid("a search page cursor does not decode: search again without a cursor"),
            )?),
        };
    let want = usize::try_from(listing.limit()).unwrap_or(usize::MAX);
    let rows = newest(
        ctx,
        channel_id,
        after.as_ref(),
        want.saturating_add(1),
        keep,
    )?;
    Ok(listing.reply((rows.into_iter()).map(|row| (place(&row).key_bytes(), row.row()))))
}

type Place = (u64, u64, String);

fn place(row: &Stored) -> Place {
    let channel_id = row.channel_id.clone();
    (newest_first(row.time), newest_first(row.seq), channel_id)
}

fn newest(
    ctx: &QueryCtx,
    channel_id: Option<String>,
    after: Option<&Place>,
    want: usize,
    keep: impl Fn(&Stored) -> bool,
) -> Result<Vec<Stored>, Error> {
    let heads = match channel_id {
        Some(id) => HEADS
            .get(ctx, &id)?
            .map(|head| (id, head))
            .into_iter()
            .collect(),
        None => HEADS.all(ctx)?,
    };
    let mut found = Vec::new();
    for (channel_id, head) in heads {
        let from = match after {
            Some((_, seq, at)) if *at == channel_id => newest_first(*seq).saturating_sub(1),
            _ => head,
        };
        let admitted = |row: &Result<Stored, Error>| match row {
            Ok(row) => after.is_none_or(|after| place(row) > *after) && keep(row),
            Err(_) => true,
        };
        let hits = walk(ctx, &channel_id, from).filter(admitted).take(want);
        found.extend(hits.collect::<Result<Vec<_>, _>>()?);
    }
    found.sort_by_cached_key(place);
    found.truncate(want);
    Ok(found)
}

const STRIDE: u64 = 128;

fn walk<'a>(
    ctx: &'a QueryCtx,
    channel_id: &'a str,
    from: u64,
) -> impl Iterator<Item = Result<Stored, Error>> + 'a {
    let mut below = from.saturating_add(1);
    let mut batch: Vec<Stored> = Vec::new();
    std::iter::from_fn(move || {
        while batch.is_empty() && below > 1 {
            let lo = below.saturating_sub(STRIDE).max(1);
            let rows = stored_between(ctx, channel_id, lo, below);
            below = lo;
            match rows {
                Ok(rows) => batch = rows,
                Err(fault) => {
                    below = 0;
                    return Some(Err(fault));
                }
            }
        }
        batch.pop().map(Ok)
    })
}

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

// who is answering in `channel_id`, each draft's pieces folded into its text
pub(crate) fn drafts(ctx: &QueryCtx, channel_id: &str) -> Result<Vec<Draft>, Error> {
    let held = DRAFTS.scan(ctx, DRAFTS.prefix_of(&channel_id.to_owned()))?;
    held.into_iter()
        .map(|((channel_id, author), draft)| {
            let under = DRAFT_PIECES.prefix_of(&(channel_id, author.clone()));
            let pieces = DRAFT_PIECES.scan(ctx, under)?;
            Ok(Draft {
                author,
                thread: draft.thread,
                height: draft.height,
                time: draft.time,
                doing: draft.doing,
                text: pieces.into_iter().map(|(_, piece)| piece).collect(),
            })
        })
        .collect()
}

/// Every message row a reply carries.
fn rows_in(reply: &mut Reply) -> Vec<&mut MsgRow> {
    match reply {
        Reply::Roots(page) | Reply::TagHits(page) | Reply::TextHits(page) => {
            page.items.iter_mut().collect()
        }
        Reply::Messages(rows) | Reply::Hits(MessageHits { hits: rows, .. }) => {
            rows.iter_mut().collect()
        }
        Reply::Thread { root, replies } => root.iter_mut().chain(&mut replies.items).collect(),
        Reply::Message(row) | Reply::Attention(row) => row.iter_mut().collect(),
        Reply::Channels(_)
        | Reply::Channel(_)
        | Reply::Members(_)
        | Reply::Accounts(_)
        | Reply::Reactors(_)
        | Reply::ChannelIds(_)
        | Reply::Listening(_)
        | Reply::Notices(_)
        | Reply::Followed(_)
        | Reply::Watching(_)
        | Reply::Chosen { .. }
        | Reply::Drafts(_)
        | Reply::Names(_)
        | Reply::RoomOrder(_)
        | Reply::PostedIn(_)
        | Reply::ChosenBy(_)
        | Reply::Unread(_) => vec![],
    }
}

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
