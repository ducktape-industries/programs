//! Exact-commit statuses reported by their senders; no merge scheduling or result carry.
use ::program::{Error, ExecCtx, QueryCtx, capacity, invalid};
use store::Listing;

use crate::contract::{CheckState, CommitStatus, PageResponse, Principal};
use crate::state::{STATUSES, load_bounds, load_repo, parse_oid, repo_hash};

pub(crate) struct Report {
    pub commit: String,
    pub context: String,
    pub state: CheckState,
    pub url: Option<String>,
}

pub(crate) fn status(
    ctx: &ExecCtx,
    actor: &Principal,
    repo: &str,
    report: Report,
) -> Result<(), Error> {
    let hash = repo_hash(&load_repo(ctx, repo)?);
    let commit = parse_oid(hash, &report.commit)?.to_hex();
    if report.context.trim().is_empty() {
        return Err(invalid("a status names its context"));
    }
    let row = CommitStatus {
        reporter: actor.clone(),
        context: report.context,
        state: report.state,
        url: report.url,
        height: ctx.env().height,
    };
    let bound = load_bounds(ctx)?.record_bytes;
    if abi::encode(&row).len() as u64 > bound {
        return Err(capacity(format!(
            "a status is at most {bound} bytes (Bounds.record_bytes)"
        )));
    }
    let key = (repo.to_owned(), commit, actor.clone(), row.context.clone());
    STATUSES.put(ctx, &key, &row);
    Ok(())
}

pub(crate) fn statuses(
    ctx: &QueryCtx,
    repo: &str,
    commit: &str,
    listing: &Listing,
) -> Result<PageResponse<CommitStatus>, Error> {
    let hash = repo_hash(&load_repo(ctx, repo)?);
    let commit = parse_oid(hash, commit)?.to_hex();
    Ok(STATUSES
        .page_of(ctx, &(repo.to_owned(), commit), listing)?
        .map(|(_, status)| status))
}
