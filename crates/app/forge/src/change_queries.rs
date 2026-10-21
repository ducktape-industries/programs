//! The change queries: a repository's changes, one change with its
//! reviews, and the judgment a person owes across every repository.

use ::program::{Error, Range};
use ::program::{QueryCtx, capacity};
use store::Listing;

use crate::contract::*;
use crate::discussion;
use crate::ops::require_named;
use crate::state::{
    AUTHORED, CHANGES, INVOLVED, REVIEWS, load_bounds, load_change, load_ref, load_repo,
    load_review, repo_hash,
};

/// One change, its two heads, and a page of its reviews.
pub fn change(
    ctx: &QueryCtx,
    height: u64,
    repo: &str,
    n: u64,
    listing: &Listing,
) -> Result<Reply, Error> {
    let change = load_change(ctx, repo, n)?;
    let (source_head, target_head) = change_endpoints(ctx, repo, &change)?;
    let reviews = REVIEWS
        .page_of(ctx, &(repo.to_owned(), n), listing)?
        .map(|(_, review)| review);
    Ok(Reply::Change {
        height,
        change,
        source_head,
        target_head,
        reviews,
    })
}

/// One page of a repository's changes, filtered. A filter can empty a page
/// that still has a `next`.
pub fn changes(
    ctx: &QueryCtx,
    repo: &str,
    filter: &ChangeFilter,
    listing: &Listing,
) -> Result<PageResponse<ChangeSummary>, Error> {
    load_repo(ctx, repo)?;
    let PageResponse {
        height,
        items,
        next,
    } = CHANGES.page_of(ctx, &repo.to_owned(), listing)?;
    let items = items
        .into_iter()
        .filter(|((repo, n), change)| {
            filter.state.is_none_or(|state| change.state == state)
                && filter
                    .author
                    .as_ref()
                    .is_none_or(|principal| &change.author == principal)
                && filter.involves.as_ref().is_none_or(|principal| {
                    INVOLVED.has(ctx, &(principal.clone(), repo.clone(), *n))
                })
        })
        .map(|((repo, _), change)| summary(&repo, &change))
        .collect();
    Ok(PageResponse {
        height,
        items,
        next,
    })
}

/// One page of every open change, across repositories, that waits on
/// `principal`: its review is requested at the current head, or a thread it
/// started was answered. Chat participants need not have submitted a forge
/// op, so every change is paged.
// ponytail: pages every change of every repository and filters; an index of
// open changes by waiting principal replaces the scan once forge holds many.
pub fn judgment(
    ctx: &QueryCtx,
    principal: &Principal,
    listing: &Listing,
) -> Result<PageResponse<Judgment>, Error> {
    require_named(principal)?;
    let mut budget = load_bounds(ctx)?.log_walk;
    let page = CHANGES.page_of(ctx, &(), listing)?;
    let mut items = Vec::new();
    for ((repo, _), change) in &page.items {
        if change.state != ChangeState::Open {
            continue;
        }
        if let Some(judgment) = review_owed_by_principal(ctx, repo, change, principal, &mut budget)?
        {
            items.push(judgment);
        }
    }
    Ok(PageResponse {
        height: page.height,
        items,
        next: page.next,
    })
}

fn review_owed_by_principal(
    ctx: &QueryCtx,
    repo: &str,
    change: &Change,
    principal: &Principal,
    budget: &mut u64,
) -> Result<Option<Judgment>, Error> {
    let (source, _) = change_endpoints(ctx, repo, change)?;
    let authored = authored_newest_first(ctx, repo, change.n, principal, budget)?;
    let latest = authored
        .first()
        .map(|id| load_review(ctx, repo, change.n, *id))
        .transpose()?;
    let requested = change.reviewers.contains(principal)
        && latest
            .as_ref()
            .is_none_or(|review| source.as_ref() != Some(&review.draft.commit_oid));
    let started = match discussion::talks_elsewhere(repo, change) {
        true => None,
        false => discussion::attention(ctx, &change.channel, principal)?,
    };
    let mut replies = started.and_then(|root| {
        root.last_reply_seq.map(|last_reply_seq| ReplyAttention {
            review: None,
            root_seq: root.seq,
            last_reply_seq,
        })
    });
    for id in authored {
        let review = load_review(ctx, repo, change.n, id)?;
        if let Some(root) = discussion::message(ctx, &review.message_id)?
            && let Some(last_reply_seq) = root.last_reply_seq
            && replies
                .as_ref()
                .is_none_or(|newest| newest.last_reply_seq < last_reply_seq)
        {
            replies = Some(ReplyAttention {
                review: Some(id),
                root_seq: root.seq,
                last_reply_seq,
            });
        }
    }
    Ok((requested || replies.is_some()).then(|| Judgment {
        change: summary(repo, change),
        requested,
        replies,
    }))
}

fn authored_newest_first(
    ctx: &QueryCtx,
    repo: &str,
    n: u64,
    principal: &Principal,
    budget: &mut u64,
) -> Result<Vec<u64>, Error> {
    let scan: Range = AUTHORED.prefix_of(&(repo.to_owned(), n, principal.clone()));
    let ids: Vec<u64> = AUTHORED
        .scan(ctx, scan.reverse().limit(budget.saturating_add(1)))?
        .into_iter()
        .map(|(_, _, _, id)| id)
        .collect();
    if ids.len() as u64 > *budget {
        return Err(capacity("judgment review walk exceeds Bounds.log_walk"));
    }
    *budget -= ids.len() as u64;
    Ok(ids)
}

fn change_endpoints(
    ctx: &QueryCtx,
    repo: &str,
    change: &Change,
) -> Result<(Option<String>, Option<String>), Error> {
    if let Some(MergedHeads { source, target }) = &change.merged_heads {
        return Ok((Some(source.clone()), Some(target.clone())));
    }
    let hash = repo_hash(&load_repo(ctx, repo)?);
    let source = match &change.from {
        Revision::Ref(name) => load_ref(ctx, repo, name, hash)?.map(|oid| oid.to_hex()),
        Revision::Oid(oid) => Some(oid.clone()),
    };
    let target = load_ref(ctx, repo, &change.into, hash)?.map(|oid| oid.to_hex());
    Ok((source, target))
}

fn summary(repo: &str, change: &Change) -> ChangeSummary {
    ChangeSummary {
        repo: repo.into(),
        n: change.n,
        from: change.from.clone(),
        into: change.into.clone(),
        title: change.title.clone(),
        author: change.author.clone(),
        state: change.state,
        updated_height: change.updated_height,
        review_count: change.review_count,
        comment_count: change.comment_count,
        verdicts: change.verdicts,
    }
}
