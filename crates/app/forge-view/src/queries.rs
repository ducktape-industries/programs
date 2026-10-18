//! Typed reads. Every forge list is cursored, so one read follows `next`
//! until the program stops offering one and hands the screen a single
//! reply. A typed refusal becomes an `Error`, so
//! the four states of a `Loadable` slot stay honest. A log is the one list
//! read a page at a time ([`log_page`]): a history is as long as the
//! repository is old, and its list shows a screenful.
use std::rc::Rc;
use view::host::{Page, all_pages, malformed};
use view::prelude::*;

use crate::api::Ask as Forge;
use forge::{CommitInfo, PageRequest, PageResponse, Query, Reply};
use identity::Ownership;

pub(crate) const PAGE: PageRequest = PageRequest {
    after: None,
    limit: None,
};

/// One read of forge, `next` followed: the pages after the first fold into
/// it. A cursor refused `stale` (an op landed between two pages) starts the
/// read over from its first page ([`all_pages`]). The reply is shared: a
/// screen that lays it out keeps the layout beside it, not a copy of it.
pub(crate) async fn fetch(host: Host, query: Query) -> Result<Rc<Reply>, Error> {
    let pages = all_pages(|after| {
        let ask = match after {
            None => Some(query.clone()),
            Some(after) => with_cursor(&query, after),
        }
        .map(|query| host.ask::<Forge>(query));
        async move {
            let Some(ask) = ask else {
                return Ok((Vec::new(), None));
            };
            let page = ask.await?;
            let next = next_cursor(&page).cloned();
            Ok((vec![page], next))
        }
    })
    .await?;
    pages
        .into_iter()
        .reduce(|mut reply, page| {
            extend(&mut reply, page);
            reply
        })
        .map(Rc::new)
        .ok_or_else(|| malformed("forge answered no page".into()))
}

/// One page of the log `query` asks, as a
/// [`Paged`](view::Paged) reads it: the page after `after`,
/// its commits and the cursor of the one after.
pub(crate) async fn log_page(
    host: Host,
    mut query: Query,
    after: Option<Vec<u8>>,
) -> Result<Page<CommitInfo>, Error> {
    if let Some(page) = query.page_mut() {
        page.after = after;
    }
    match host.ask::<Forge>(query).await? {
        Reply::Log { page, .. } => Ok((page.items, page.next)),
        other => Err(malformed(format!("a log answered {other:?}"))),
    }
}

fn next_cursor(reply: &Reply) -> Option<&Vec<u8>> {
    match reply {
        Reply::Repos { page, .. } => page.next.as_ref(),
        Reply::Refs { page, .. } => page.next.as_ref(),
        Reply::Tree { page, .. } => page.next.as_ref(),
        Reply::Diff { page, .. } => page.next.as_ref(),
        Reply::Changes { page, .. } => page.next.as_ref(),
        Reply::Change { reviews, .. } => reviews.next.as_ref(),
        Reply::Judgment { page, .. } => page.next.as_ref(),
        Reply::Protected { page, .. } => page.next.as_ref(),
        Reply::RefNames { page, .. } => page.next.as_ref(),
        Reply::DiffStat { page, .. } => page.next.as_ref(),
        // a log is never read whole: `log_page`
        Reply::Log { .. }
        | Reply::Repo { .. }
        | Reply::Compare { .. }
        | Reply::Blob { .. }
        | Reply::Activity { .. }
        | Reply::Closure { .. } => None,
    }
}

/// The same question, continued. An unpaged query has no continuation.
fn with_cursor(query: &Query, after: Vec<u8>) -> Option<Query> {
    let mut query = query.clone();
    query.page_mut()?.after = Some(after);
    Some(query)
}

fn absorb<T>(page: &mut PageResponse<T>, more: PageResponse<T>) {
    page.items.extend(more.items);
    page.next = more.next;
}

fn extend(into: &mut Reply, more: Reply) {
    match (into, more) {
        (Reply::Repos { page, .. }, Reply::Repos { page: more, .. }) => absorb(page, more),
        (Reply::Refs { page, .. }, Reply::Refs { page: more, .. }) => absorb(page, more),
        (Reply::Tree { page, .. }, Reply::Tree { page: more, .. }) => absorb(page, more),
        (Reply::Diff { page, .. }, Reply::Diff { page: more, .. }) => absorb(page, more),
        (Reply::Changes { page, .. }, Reply::Changes { page: more, .. }) => absorb(page, more),
        (Reply::Change { reviews, .. }, Reply::Change { reviews: more, .. }) => {
            absorb(reviews, more)
        }
        (Reply::Judgment { page, .. }, Reply::Judgment { page: more, .. }) => absorb(page, more),
        _ => {}
    }
}

pub(crate) async fn writers(host: Host, repo: String) -> Result<Option<Ownership>, Error> {
    host.query(access::ask::List {
        program: forge::PROGRAM.to_owned(),
        key: forge::repo_key(&repo),
    })
    .await
}

/// A change's hidden channel, oldest first.
pub(crate) async fn conversation(
    host: Host,
    channel_id: String,
    viewer: Vec<forge::Principal>,
) -> Result<Vec<chat::MsgRow>, Error> {
    let ask = move |ask| host.query(ask);
    let (rows, _) = chat::view::roots(ask, channel_id, viewer, None, usize::MAX, u64::MAX).await?;
    Ok(rows)
}

/// Every account's profile, folded into [`Names`](chat::view::Names).
pub(crate) async fn roster(host: Host) -> Result<chat::view::Names, Error> {
    chat::view::roster(move |ask| host.query(ask)).await
}
