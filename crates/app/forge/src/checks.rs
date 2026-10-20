//! Checks: the statuses reporters give commits, and the branches that move only by a merge of a
//! change green on its Preview, or on its head while the target left that status's reach alone.
use std::collections::BTreeMap;

use ::program::{Error, ExecCtx, QueryCtx, capacity, invalid, wrong_state};
use gitcore::{Mode, Oid, TreeEntry};
use store::Listing;

use crate::contract::{
    CheckState, CommitStatus, PageResponse, PreviewInfo, Principal, Required, RequiredBranch,
};
use crate::objects::ObjectStore;
use crate::ops::{cap, refusal_of};
use crate::reads::Reading;
use crate::state::{
    PREVIEWS, REACHES, REQUIRED, STATUSES, load_bounds, load_repo, parse_oid, ref_key, repo_hash,
};

pub(crate) struct Report {
    pub commit: String,
    pub context: String,
    pub state: CheckState,
    pub url: Option<String>,
    pub reach: Option<Vec<Vec<u8>>>,
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
    let unsound = |path: &&Vec<u8>| {
        path.split(|byte| *byte == b'/')
            .any(|part| part.is_empty() || part == b"." || part == b"..")
    };
    if let Some(path) = report.reach.iter().flatten().find(unsound) {
        return Err(invalid(format!(
            "reach path {:?} is not a path from the repository's root: name it with no leading, trailing or doubled /, and no . or .. part (an empty reach is the whole tree)",
            String::from_utf8_lossy(path)
        )));
    }
    if let Some(reach) = &report.reach
        && abi::encode(reach).len() as u64 > bound
    {
        return Err(capacity(format!(
            "a status's reach is at most {bound} bytes (Bounds.record_bytes)"
        )));
    }
    let key = (repo.to_owned(), commit, actor.clone(), row.context.clone());
    STATUSES.put(ctx, &key, &row);
    match &report.reach {
        Some(reach) => REACHES.put(ctx, &key, reach),
        None => REACHES.remove(ctx, &key),
    }
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
        "{} requires checks: it moves only by SquashMerge or Merge of a change whose Preview or head is green. {}",
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
            "{branch} requires checks: a merge into it names its change, and lands only on that change's green Preview or head. {}",
            break_glass(repo, into)
        )));
    };
    let preview = PREVIEWS.get(ctx, &(repo.to_owned(), n))?;
    let fresh = preview.as_ref().filter(|preview| {
        preview.into_head == expected_into.to_hex() && preview.from_head == expected_from.to_hex()
    });
    let mut head = Head {
        ctx,
        repo,
        branch: &branch,
        into: *expected_into,
        from: *expected_from,
        base: None,
    };
    let mut red = Vec::new();
    let mut silent = Vec::new();
    let mut unmet = Vec::new();
    for context in &required.contexts {
        let on_preview = match fresh {
            Some(preview) => Some(reported(
                ctx,
                repo,
                &preview.merge,
                &required.reporter,
                context,
            )?),
            None => None,
        };
        match &on_preview {
            Some(Some(status)) if status.state == CheckState::Passed => continue,
            Some(Some(status)) => {
                red.push(worded(context, status));
                continue;
            }
            Some(None) | None => {}
        }
        let Some(why) = head.unmet(&required.reporter, context)? else {
            continue;
        };
        unmet.push(why);
        if on_preview.is_some() {
            silent.push(context.as_str());
        }
    }
    if red.is_empty() && unmet.is_empty() {
        return Ok(());
    }
    let previewed = match (&preview, fresh) {
        (None, _) => format!(
            "change {n} has no Preview: send Preview {{ repo: {repo:?}, n: {n} }} and let its checks run"
        ),
        (Some(stale), None) => format!(
            "change {n}'s Preview is of target {} and head {}, not the heads being merged: Preview it again and let its checks run",
            short(&stale.into_head),
            short(&stale.from_head)
        ),
        (_, Some(preview)) => {
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
            format!(
                "change {n}'s Preview {} is not green: {}",
                short(&preview.merge),
                why.join("; ")
            )
        }
    };
    if unmet.is_empty() {
        return Err(wrong_state(previewed));
    }
    Err(wrong_state(format!(
        "{previewed}; nor is its head {} green: {}",
        short(&expected_from.to_hex()),
        unmet.join("; ")
    )))
}

struct Head<'a> {
    ctx: &'a QueryCtx,
    repo: &'a str,
    branch: &'a str,
    into: Oid,
    from: Oid,
    base: Option<Option<Oid>>,
}

impl Head<'_> {
    fn unmet(&mut self, reporter: &Principal, context: &str) -> Result<Option<String>, Error> {
        let key = (
            self.repo.to_owned(),
            self.from.to_hex(),
            reporter.clone(),
            context.to_owned(),
        );
        match STATUSES.get(self.ctx, &key)? {
            Some(status) if status.state == CheckState::Passed => {}
            Some(status) => return Ok(Some(worded(context, &status))),
            None => return Ok(Some(format!("no {context} from {}", who(reporter)))),
        }
        let reach = REACHES
            .get(self.ctx, &key)?
            .filter(|reach| !reach.is_empty())
            .unwrap_or_else(|| vec![Vec::new()]);
        let base = match self.base {
            Some(base) => base,
            None => *self
                .base
                .insert(merge_base(self.ctx, &self.into, &self.from)?),
        };
        let Some(base) = base else {
            return Ok(Some(format!(
                "{context} passed, but the head shares no history with {}",
                self.branch
            )));
        };
        let moved = moved(self.ctx, self.repo, (base, self.into), &reach)?;
        if moved.is_empty() {
            return Ok(None);
        }
        Ok(Some(format!(
            "{context} passed, but {} changed {} since their merge base {}, inside the paths that status depends on: gate the head again",
            self.branch,
            moved.join(", "),
            short(&base.to_hex())
        )))
    }
}

fn reported(
    ctx: &QueryCtx,
    repo: &str,
    commit: &str,
    reporter: &Principal,
    context: &str,
) -> Result<Option<CommitStatus>, Error> {
    let key = (
        repo.to_owned(),
        commit.to_owned(),
        reporter.clone(),
        context.to_owned(),
    );
    STATUSES.get(ctx, &key)
}

fn worded(context: &str, status: &CommitStatus) -> String {
    match &status.url {
        Some(url) => format!("{context} {} ({url})", status.state.word()),
        None => format!("{context} {}", status.state.word()),
    }
}

fn merge_base(ctx: &QueryCtx, into: &Oid, from: &Oid) -> Result<Option<Oid>, Error> {
    let walk = cap(load_bounds(ctx)?.log_walk);
    gitcore::walk::merge_base(&ObjectStore::new(ctx), from, into, walk).map_err(refusal_of)
}

fn moved(
    ctx: &QueryCtx,
    repo: &str,
    (base, into): (Oid, Oid),
    reach: &[Vec<u8>],
) -> Result<Vec<String>, Error> {
    let bounds = load_bounds(ctx)?;
    let r = Reading {
        store: ObjectStore::querying(ctx, &bounds, bounds.tree_walk),
        hash: repo_hash(&load_repo(ctx, repo)?),
        bounds: &bounds,
    };
    let parts: Vec<Vec<&[u8]>> = reach
        .iter()
        .map(|path| {
            path.split(|byte| *byte == b'/')
                .filter(|part| !part.is_empty())
                .collect()
        })
        .collect();
    let paths: Vec<&[&[u8]]> = parts.iter().map(Vec::as_slice).collect();
    let sides = (r.commit(&base)?.tree, r.commit(&into)?.tree);
    let mut out = Vec::new();
    moved_within(&r, &[], sides, &paths, &mut out)?;
    Ok(out)
}

fn moved_within(
    r: &Reading,
    at: &[u8],
    (old, new): (Oid, Oid),
    paths: &[&[&[u8]]],
    out: &mut Vec<String>,
) -> Result<(), Error> {
    if old == new {
        return Ok(());
    }
    if paths.iter().any(|path| path.is_empty()) {
        out.push(shown(at));
        return Ok(());
    }
    let (was, is) = (r.tree(&old)?.entries, r.tree(&new)?.entries);
    let mut under: BTreeMap<&[u8], Vec<&[&[u8]]>> = BTreeMap::new();
    for path in paths {
        under.entry(path[0]).or_default().push(&path[1..]);
    }
    for (name, rest) in under {
        let here = match at.is_empty() {
            true => name.to_vec(),
            false => [at, b"/".as_slice(), name].concat(),
        };
        match (entry(&was, name), entry(&is, name)) {
            (before, after) if before == after => {}
            (Some((before, Mode::Directory)), Some((after, Mode::Directory))) => {
                moved_within(r, &here, (before, after), &rest, out)?
            }
            _ => out.push(shown(&here)),
        }
    }
    Ok(())
}

fn entry(entries: &[TreeEntry], name: &[u8]) -> Option<(Oid, Mode)> {
    entries
        .iter()
        .find(|entry| entry.name == name)
        .map(|entry| (entry.id, entry.mode))
}

fn shown(path: &[u8]) -> String {
    match path.is_empty() {
        true => "the whole tree".to_owned(),
        false => String::from_utf8_lossy(path).into_owned(),
    }
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
