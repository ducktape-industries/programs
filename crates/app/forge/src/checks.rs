//! Checks: the statuses reporters give commits, and the branches that move only by a merge of a
//! change whose Preview (`changes::preview`, the squash the merge would land) is green.
use ::program::{Error, ExecCtx, QueryCtx, capacity, invalid, wrong_state};
use gitcore::Oid;
use store::Listing;

use crate::contract::{
    CheckState, CommitStatus, PageResponse, PreviewInfo, Principal, Required, RequiredBranch,
};
use crate::state::{
    PREVIEWS, REQUIRED, STATUSES, load_bounds, load_repo, parse_oid, ref_key, repo_hash,
};

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

pub(crate) fn require(
    ctx: &ExecCtx,
    repo: &str,
    branch: &[u8],
    required: Option<Required>,
) -> Result<(), Error> {
    load_repo(ctx, repo)?;
    if !branch.starts_with(b"refs/heads/") {
        return Err(invalid("required checks name a branch under refs/heads/"));
    }
    let key = ref_key(repo, branch);
    match required {
        Some(required) => {
            let blank = required.contexts.iter().any(|c| c.trim().is_empty());
            if required.contexts.is_empty() || blank {
                return Err(invalid("required checks name each context, at least one"));
            }
            REQUIRED.put(ctx, &key, &required);
        }
        None => REQUIRED.remove(ctx, &key),
    }
    Ok(())
}

pub(crate) fn refuse_push(ctx: &QueryCtx, repo: &str, reference: &[u8]) -> Result<(), Error> {
    if !REQUIRED.has(ctx, &ref_key(repo, reference)) {
        return Ok(());
    }
    Err(wrong_state(format!(
        "{} requires checks: it moves only by SquashMerge or Merge of a change whose Preview is green. {}",
        String::from_utf8_lossy(reference),
        break_glass(repo, reference)
    )))
}

pub(crate) fn gate(
    ctx: &QueryCtx,
    repo: &str,
    into: &[u8],
    change: Option<u64>,
    (expected_into, expected_from): (&Oid, &Oid),
) -> Result<(), Error> {
    let Some(required) = REQUIRED.get(ctx, &ref_key(repo, into))? else {
        return Ok(());
    };
    let branch = String::from_utf8_lossy(into);
    let Some(n) = change else {
        return Err(wrong_state(format!(
            "{branch} requires checks: a merge into it names its change, and lands only on that change's green Preview. {}",
            break_glass(repo, into)
        )));
    };
    let Some(preview) = PREVIEWS.get(ctx, &(repo.to_owned(), n))? else {
        return Err(wrong_state(format!(
            "change {n} has no Preview: send Preview {{ repo: {repo:?}, n: {n} }} and let its checks run"
        )));
    };
    if preview.into_head != expected_into.to_hex() || preview.from_head != expected_from.to_hex() {
        return Err(wrong_state(format!(
            "change {n}'s Preview is of target {} and head {}, not the heads being merged: Preview it again and let its checks run",
            short(&preview.into_head),
            short(&preview.from_head)
        )));
    }
    let mut red = Vec::new();
    let mut silent = Vec::new();
    for context in &required.contexts {
        let key = (
            repo.to_owned(),
            preview.merge.clone(),
            required.reporter.clone(),
            context.clone(),
        );
        match STATUSES.get(ctx, &key)? {
            Some(status) if status.state == CheckState::Passed => {}
            Some(status) => red.push(match &status.url {
                Some(url) => format!("{context} {} ({url})", status.state.word()),
                None => format!("{context} {}", status.state.word()),
            }),
            None => silent.push(context.as_str()),
        }
    }
    if red.is_empty() && silent.is_empty() {
        return Ok(());
    }
    let mut why = Vec::new();
    if !red.is_empty() {
        why.push(red.join(", "));
    }
    if !silent.is_empty() {
        why.push(format!(
            "no status from {} for {} (look at CI, not the code; {})",
            who(&required.reporter),
            silent.join(", "),
            break_glass(repo, into)
        ));
    }
    Err(wrong_state(format!(
        "change {n}'s Preview {} is not green: {}",
        short(&preview.merge),
        why.join("; ")
    )))
}

fn break_glass(repo: &str, branch: &[u8]) -> String {
    format!(
        "Break-glass: the repository's settings owner sends RequireChecks {{ repo: {repo:?}, branch: {:?}, required: None }}, one op on chain, and the branch moves as any other",
        String::from_utf8_lossy(branch)
    )
}

fn who(principal: &Principal) -> String {
    match principal {
        Principal::Account(number) => format!("account {number}"),
        Principal::Root => "the system".to_owned(),
        Principal::Foreign(foreign) => foreign.to_string(),
    }
}

fn short(oid: &str) -> &str {
    oid.get(..12).unwrap_or(oid)
}

pub(crate) fn preview_of(ctx: &QueryCtx, repo: &str, n: u64) -> Result<Option<PreviewInfo>, Error> {
    load_repo(ctx, repo)?;
    PREVIEWS.get(ctx, &(repo.to_owned(), n))
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

pub(crate) fn required(
    ctx: &QueryCtx,
    repo: &str,
    listing: &Listing,
) -> Result<PageResponse<RequiredBranch>, Error> {
    load_repo(ctx, repo)?;
    Ok(REQUIRED
        .page_of(ctx, &repo.to_owned(), listing)?
        .map(|((_, branch), required)| RequiredBranch { branch, required }))
}
