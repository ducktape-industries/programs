//! The change queries: a repository's changes, one change with its
//! reviews, and the reviews a person is asked for across every repository.

use ::program::{Error, QueryCtx};
use store::Listing;

use crate::contract::*;
use crate::ops::require_named;
use crate::state::{
    AUTHORED, CHANGES, INVOLVED, REVIEWS, load_change, load_ref, load_repo, load_review, repo_hash,
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

pub fn changes(
    ctx: &QueryCtx,
    repo: &str,
    filter: &ChangeFilter,
    listing: &Listing,
) -> Result<PageResponse<ChangeSummary>, Error> {
    load_repo(ctx, repo)?;
    let mut scan = listing.scan_ahead(&CHANGES.key(&repo.to_owned()));
    let mut matching = Vec::new();
    loop {
        let rows = CHANGES.scan(ctx, scan.clone())?;
        let Some((last, _)) = rows.last() else {
            return Ok(listing.reply(matching));
        };
        scan = scan.after(CHANGES.key(last));
        for ((repo, n), change) in rows {
            let matches_filter = filter.state.is_none_or(|state| change.state == state)
                && filter
                    .author
                    .as_ref()
                    .is_none_or(|principal| &change.author == principal)
                && filter.involves.as_ref().is_none_or(|principal| {
                    INVOLVED.has(ctx, &(principal.clone(), repo.clone(), n))
                });
            if !matches_filter {
                continue;
            }
            matching.push((CHANGES.key(&(repo.clone(), n)), summary(&repo, &change)));
            let has_lookahead = matching.len() as u64 > listing.limit();
            if has_lookahead {
                return Ok(listing.reply(matching));
            }
        }
    }
}

pub fn review_requests(
    ctx: &QueryCtx,
    principal: &Principal,
    listing: &Listing,
) -> Result<PageResponse<ChangeSummary>, Error> {
    require_named(principal)?;
    let page = INVOLVED.page_of(ctx, principal, listing)?;
    let mut items = Vec::new();
    for (_, repo, n) in &page.items {
        let change = load_change(ctx, repo, *n)?;
        if asks_review(ctx, repo, &change, principal)? {
            items.push(summary(repo, &change));
        }
    }
    Ok(PageResponse {
        height: page.height,
        items,
        next: page.next,
    })
}

fn asks_review(
    ctx: &QueryCtx,
    repo: &str,
    change: &Change,
    principal: &Principal,
) -> Result<bool, Error> {
    let asked = change.state == ChangeState::Open && change.reviewers.contains(principal);
    if !asked {
        return Ok(false);
    }
    let theirs = AUTHORED.prefix_of(&(repo.to_owned(), change.n, principal.clone()));
    let Some((_, _, _, newest)) = AUTHORED.scan(ctx, theirs.reverse().limit(1))?.pop() else {
        return Ok(true);
    };
    let (source, _) = change_endpoints(ctx, repo, change)?;
    let review = load_review(ctx, repo, change.n, newest)?;
    Ok(source.as_ref() != Some(&review.draft.commit_oid))
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
