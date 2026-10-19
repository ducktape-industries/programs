//! Typed reads of the chat program. Every list takes a `PageRequest` and answers a
//! `PageResponse`; `next` is the cursor of the page after it.
use chat::view::Names;
use chat::{ChannelInfo, MemberRow, MessageHits, MsgRow, PageRequest, Principal, ask};
use identity::Ownership;
use std::collections::BTreeMap;
use view::prelude::*;

use crate::{PAGE, Reactors, SPAN, WINDOW};

fn page(after: Option<Vec<u8>>, limit: usize) -> PageRequest {
    PageRequest {
        after,
        limit: Some(limit as u64),
    }
}

/// Every room.
pub(crate) async fn channels(host: Host) -> Result<Vec<ChannelInfo>, Error> {
    host.query_all(|after| ask::Channels {
        page: page(after, PAGE),
    })
    .await
}

/// The `limit` roots below `below` (or the newest), oldest first, with
/// whether older ones remain.
pub(crate) async fn roots(
    host: Host,
    channel_id: String,
    viewer: Vec<Principal>,
    below: Option<Vec<u8>>,
    limit: usize,
) -> Result<(Vec<MsgRow>, bool), Error> {
    let ask = move |ask| host.query(ask);
    let per_page = limit.min(PAGE) as u64;
    ::chat::view::roots(ask, channel_id, viewer, below, limit, per_page).await
}

/// Every account's profile, folded into [`Names`].
pub(crate) async fn roster(host: Host) -> Result<Names, Error> {
    ::chat::view::roster(move |ask| host.query(ask)).await
}

pub(crate) async fn ownerships(
    host: Host,
    names: &Names,
) -> Result<BTreeMap<u64, Ownership>, Error> {
    let agents = names
        .people()
        .filter(|number| run_by_harness(names, &Principal::Account(*number)));
    let asks = agents.map(|number| {
        let host = host.clone();
        async move {
            let list = host
                .query(access::ask::List {
                    program: harness::PROGRAM.to_owned(),
                    key: harness::agent_key(number),
                })
                .await?;
            // an agent with no list answers anyone
            let ownership = list.unwrap_or(Ownership::Anyone);
            Ok::<_, Error>((number, ownership))
        }
    });
    Ok(futures::future::try_join_all(asks)
        .await?
        .into_iter()
        .collect())
}

// Who chose each reaction on the rows at `seqs`.
pub(crate) async fn reactors(
    host: Host,
    channel_id: String,
    seqs: Vec<u64>,
) -> Result<Reactors, Error> {
    let mut reactors = Reactors::new();
    for chosen in host.query(ask::Reactors { channel_id, seqs }).await? {
        reactors
            .entry(chosen.seq)
            .or_default()
            .insert(chosen.emoji, chosen.by);
    }
    Ok(reactors)
}

/// The rows around a landing seq, oldest first.
pub(crate) async fn around(
    host: Host,
    channel_id: String,
    seq: u64,
    viewer: Vec<Principal>,
) -> Result<Vec<MsgRow>, Error> {
    let rows = host
        .query(ask::MessagesAround {
            channel_id,
            seq,
            viewer,
            page: page(None, WINDOW / 2),
        })
        .await?;
    Ok(sorted(rows))
}

pub(crate) async fn newest(
    host: Host,
    channel_id: String,
    seq: u64,
    viewer: Vec<Principal>,
) -> Result<Option<u64>, Error> {
    let rows = host
        .query(ask::MessagesAround {
            channel_id,
            seq,
            viewer,
            page: page(None, 1),
        })
        .await?;
    Ok(rows
        .into_iter()
        .find(|row| row.seq == seq)
        .map(|row| row.time))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Side {
    Older,
    Newer,
}

pub(crate) async fn beside(
    host: Host,
    channel_id: String,
    seq: u64,
    side: Side,
    viewer: Vec<Principal>,
) -> Result<Vec<MsgRow>, Error> {
    let half = PAGE as u64 / 2;
    let centre = match side {
        Side::Older => seq.saturating_sub(half + 1),
        Side::Newer => seq.saturating_add(half + 1),
    };
    let rows = host
        .query(ask::MessagesAround {
            channel_id,
            seq: centre,
            viewer,
            page: page(None, PAGE),
        })
        .await?;
    let past = |row: &MsgRow| match side {
        Side::Older => row.seq < seq,
        Side::Newer => row.seq > seq,
    };
    Ok(sorted(rows.into_iter().filter(past).collect()))
}

pub(crate) async fn span(
    host: Host,
    channel_id: String,
    (first, last): (u64, u64),
    viewer: Vec<Principal>,
) -> Result<Vec<MsgRow>, Error> {
    let rows = host
        .query(ask::MessagesAround {
            channel_id,
            seq: first.midpoint(last),
            viewer,
            page: page(None, SPAN),
        })
        .await?;
    let held = |row: &MsgRow| (first..=last).contains(&row.seq);
    Ok(sorted(rows.into_iter().filter(held).collect()))
}

pub(crate) fn sorted(mut rows: Vec<MsgRow>) -> Vec<MsgRow> {
    rows.sort_by_key(|row| row.seq);
    rows
}

// The rooms `account` listens to.
pub(crate) async fn listening(host: Host, account: u64) -> Result<Vec<String>, Error> {
    host.query_all(|after| ask::Listening {
        account,
        page: page(after, PAGE),
    })
    .await
}

/// Every member of a room, in account order.
pub(crate) async fn members(host: Host, channel_id: String) -> Result<Vec<MemberRow>, Error> {
    host.query_all(|after| ask::Members {
        channel_id: channel_id.clone(),
        page: page(after, WINDOW),
    })
    .await
}

// One page of `limit` of a thread's replies after `after`, and the cursor to page on.
pub(crate) async fn thread(
    host: Host,
    channel_id: String,
    root_seq: u64,
    viewer: Vec<Principal>,
    after: Option<Vec<u8>>,
    limit: usize,
) -> Result<(Vec<MsgRow>, Option<Vec<u8>>), Error> {
    let (_root, replies) = host
        .query(ask::Thread {
            channel_id,
            root_seq,
            viewer,
            page: page(after, limit),
        })
        .await?;
    Ok((sorted(replies.items), replies.next))
}

// A search: `#tag` pages through the messages tagged so, anything else
// is a substring search of every term, capped at one page. Returns the
// hits, whether the search was capped, and the cursor of the next tag page.
pub(crate) async fn search_hits(
    host: Host,
    text: String,
    channel_id: Option<String>,
    viewer: Vec<Principal>,
    after: Option<Vec<u8>>,
) -> Result<(Vec<MsgRow>, bool, Option<Vec<u8>>), Error> {
    match text.strip_prefix('#') {
        Some(tag) if !tag.is_empty() => {
            let tagged = host
                .query(ask::TagSearch {
                    tag: tag.to_owned(),
                    viewer,
                    channel_id,
                    page: page(after, PAGE),
                })
                .await?;
            Ok((tagged.items, false, tagged.next))
        }
        _ => {
            let MessageHits { hits, capped } = host
                .query(ask::Search {
                    text,
                    viewer,
                    channel_id,
                    page: page(None, PAGE),
                })
                .await?;
            Ok((hits, capped, None))
        }
    }
}

// an agent whose script calls harness: harness keeps whom it answers; any other agent runs
// outside harness, and no rule of harness's says whom it answers
pub(crate) fn run_by_harness(names: &Names, principal: &Principal) -> bool {
    matches!(
        names.kind(principal),
        Some(chat::Kind::Program {
            category: chat::Category::Agent,
            script: Some(script),
            ..
        }) if script.calls == harness::PROGRAM
    )
}
