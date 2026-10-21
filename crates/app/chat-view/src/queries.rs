//! Typed reads of the chat program. Every list takes a `PageRequest` and answers a
//! `PageResponse`; `next` is the cursor of the page after it.
use chat::view::Names;
use chat::{ChannelInfo, MemberRow, MsgRow, PageRequest, Principal, ask};
use identity::Ownership;
use std::collections::BTreeMap;
use view::prelude::*;

use crate::{Answers, Reactors};

fn page(after: Option<Vec<u8>>, limit: usize) -> PageRequest {
    PageRequest {
        after,
        limit: Some(limit as u64),
    }
}

// Every row of a listing in one answer: no limit is the program's own "every row".
fn every(after: Option<Vec<u8>>) -> PageRequest {
    PageRequest { after, limit: None }
}

pub(crate) async fn channels(host: Host) -> Result<Vec<ChannelInfo>, Error> {
    host.query_all(|after| ask::Channels { page: every(after) })
        .await
}

pub(crate) async fn roots(
    host: Host,
    channel_id: String,
    viewer: Vec<Principal>,
    below: Option<Vec<u8>>,
    limit: usize,
) -> Result<(Vec<MsgRow>, bool), Error> {
    let ask = move |ask| host.query(ask);
    ::chat::view::roots(ask, channel_id, viewer, below, limit, limit as u64).await
}

pub(crate) async fn roster(host: Host) -> Result<Names, Error> {
    ::chat::view::roster(move |ask| host.query(ask)).await
}

pub(crate) async fn ownerships(host: Host) -> Result<BTreeMap<u64, Ownership>, Error> {
    let narrowed = host
        .query_all(|after| identity::ask::Narrowed {
            page: identity::PageRequest { after, limit: None },
        })
        .await?;
    Ok(narrowed.into_iter().collect())
}

pub(crate) async fn reactors(
    host: Host,
    channel_id: String,
    seqs: Vec<u64>,
) -> Result<Reactors, Error> {
    Ok(reactors_of(
        host.query(ask::Reactors { channel_id, seqs }).await?,
    ))
}

pub(crate) fn reactors_of(chosen: Vec<chat::ReactedBy>) -> Reactors {
    let mut reactors = Reactors::new();
    for chosen in chosen {
        reactors
            .entry(chosen.seq)
            .or_default()
            .insert(chosen.emoji, chosen.by);
    }
    reactors
}

pub(crate) async fn answers(
    host: Host,
    channel_id: String,
    roots: Vec<u64>,
) -> Result<Answers, Error> {
    let threads = host.query(ask::Repliers { channel_id, roots }).await?;
    Ok(threads.into_iter().collect())
}

// A screenful on each side of `seq`: the pane fills wherever the row is drawn in it.
pub(crate) async fn around(
    host: Host,
    channel_id: String,
    seq: u64,
    viewer: Vec<Principal>,
    screenful: usize,
) -> Result<Vec<MsgRow>, Error> {
    let rows = host
        .query(ask::MessagesAround {
            channel_id,
            seq,
            viewer,
            page: page(None, 2 * screenful),
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
    screenful: usize,
) -> Result<Vec<MsgRow>, Error> {
    let half = screenful as u64 / 2;
    let centre = match side {
        Side::Older => seq.saturating_sub(half + 1),
        Side::Newer => seq.saturating_add(half + 1),
    };
    let rows = host
        .query(ask::MessagesAround {
            channel_id,
            seq: centre,
            viewer,
            page: page(None, screenful),
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
    let width = usize::try_from(last - first + 1).unwrap_or(usize::MAX);
    let rows = host
        .query(ask::MessagesAround {
            channel_id,
            seq: first.midpoint(last),
            viewer,
            page: page(None, width),
        })
        .await?;
    let held = |row: &MsgRow| (first..=last).contains(&row.seq);
    Ok(sorted(rows.into_iter().filter(held).collect()))
}

pub(crate) async fn newer(
    host: Host,
    channel_id: String,
    (newest_held, head): (u64, u64),
    viewer: Vec<Principal>,
) -> Result<Vec<MsgRow>, Error> {
    let missing = head.saturating_sub(newest_held);
    let rows = host
        .query(ask::MessagesAround {
            channel_id,
            seq: newest_held + 1 + missing / 2,
            viewer,
            page: page(None, usize::try_from(missing).unwrap_or(usize::MAX)),
        })
        .await?;
    let past = |row: &MsgRow| row.seq > newest_held;
    Ok(sorted(rows.into_iter().filter(past).collect()))
}

pub(crate) fn sorted(mut rows: Vec<MsgRow>) -> Vec<MsgRow> {
    rows.sort_by_key(|row| row.seq);
    rows
}

pub(crate) async fn listening(host: Host, account: u64) -> Result<Vec<String>, Error> {
    host.query_all(|after| ask::Listening {
        account,
        page: every(after),
    })
    .await
}

// The rooms `account` wrote in.
pub(crate) async fn posted_in(host: Host, account: u64) -> Result<Vec<String>, Error> {
    host.query_all(|after| ask::PostedIn {
        account,
        page: every(after),
    })
    .await
}

pub(crate) async fn seated(
    host: Host,
    principal: Principal,
    channel_ids: Vec<String>,
) -> Result<Vec<String>, Error> {
    host.query(ask::Seated {
        principal,
        channel_ids,
    })
    .await
}

pub(crate) async fn members(host: Host, channel_id: String) -> Result<Vec<MemberRow>, Error> {
    host.query_all(|after| ask::Members {
        channel_id: channel_id.clone(),
        page: every(after),
    })
    .await
}

pub(crate) async fn thread(
    host: Host,
    channel_id: String,
    root_seq: u64,
    viewer: Vec<Principal>,
    after: Option<Vec<u8>>,
) -> Result<(Vec<MsgRow>, Option<Vec<u8>>), Error> {
    let (_root, replies) = host
        .query(ask::Thread {
            channel_id,
            root_seq,
            viewer,
            page: PageRequest { after, limit: None },
        })
        .await?;
    Ok((sorted(replies.items), replies.next))
}

pub(crate) async fn search_hits(
    host: Host,
    text: String,
    channel_id: Option<String>,
    viewer: Vec<Principal>,
    after: Option<Vec<u8>>,
    screenful: usize,
) -> Result<(Vec<MsgRow>, Option<Vec<u8>>), Error> {
    let asked = page(after, screenful);
    let found = match text.strip_prefix('#') {
        Some(tag) if !tag.is_empty() => {
            let tag = tag.to_owned();
            host.query(ask::TagSearch {
                tag,
                viewer,
                channel_id,
                page: asked,
            })
            .await?
        }
        _ => {
            host.query(ask::TextSearch {
                text,
                viewer,
                channel_id,
                page: asked,
            })
            .await?
        }
    };
    Ok((found.items, found.next))
}

pub(crate) async fn tagged(
    host: Host,
    tag: String,
    channel_id: String,
    viewer: Vec<Principal>,
    after: Option<Vec<u8>>,
    screenful: usize,
) -> Result<(Vec<MsgRow>, Option<Vec<u8>>), Error> {
    let found = host
        .query(ask::TagSearch {
            tag,
            viewer,
            channel_id: Some(channel_id),
            page: page(after, screenful),
        })
        .await?;
    Ok((found.items, found.next))
}

// Who is answering in a room right now: each draft, with its text so far.
pub(crate) async fn answering(host: Host, channel_id: String) -> Result<Vec<chat::Draft>, Error> {
    host.query(ask::Drafts { channel_id }).await
}

pub(crate) async fn busy(
    host: Host,
    channel_id: String,
    thread: Option<u64>,
    for_account: u64,
) -> Result<Option<chat::Holder>, Error> {
    let asked = ask::Busy {
        channel_id,
        thread,
        for_account,
    };
    host.query(asked).await
}
