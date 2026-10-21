//! What each op checks and writes: a reported result kept with its reach, and on each Preview
//! every result reported for the change's head compared path by path and told forge as a status.
use std::collections::BTreeMap;

use ::program::{Error, ExecCtx, QueryCtx, Reply as Wanted, code, invalid, unexpected_reply};
use forge::{CheckState, EntryKind, PreviewInfo, Watched};

use crate::state::{Kept, PREVIEWED, PREVIEWING, REACH, REPORTS, Reporting};
use crate::{AccountNumber, Carried, PROGRAM, PageRequest, PageResponse, PreviewVerdict, Report};

pub(crate) struct Reported {
    pub repo: String,
    pub commit: String,
    pub context: String,
    pub state: CheckState,
    pub url: Option<String>,
    pub reach: Vec<String>,
    pub change: Option<u64>,
}

pub(crate) fn watched(ctx: &ExecCtx, event: Watched) -> Result<(), Error> {
    ctx.env().sent_by(forge::PROGRAM)?;
    match event {
        Watched::Previewed { repo, n, .. } => {
            heard_preview(ctx, &repo, n)?;
            tell(ctx, &repo, n)
        }
        Watched::Merged { repo, n, .. } | Watched::Closed { repo, n, .. } => {
            forget_preview(ctx, &repo, n)
        }
        _ => Ok(()),
    }
}

fn heard_preview(ctx: &ExecCtx, repo: &str, n: u64) -> Result<(), Error> {
    forget_preview(ctx, repo, n)?;
    let Some(preview) = latest_preview(ctx, repo, n)? else {
        return Ok(());
    };
    for commit in [&preview.from_head, &preview.merge] {
        PREVIEWING.insert(ctx, &(repo.to_owned(), commit.clone(), n));
    }
    let heads = (preview.from_head, preview.merge);
    PREVIEWED.put(ctx, &(repo.to_owned(), n), &heads);
    Ok(())
}

fn forget_preview(ctx: &ExecCtx, repo: &str, n: u64) -> Result<(), Error> {
    let key = (repo.to_owned(), n);
    let Some((head, merge)) = PREVIEWED.get(ctx, &key)? else {
        return Ok(());
    };
    for commit in [head, merge] {
        let row = (repo.to_owned(), commit, n);
        if PREVIEWING.has(ctx, &row) {
            PREVIEWING.remove(ctx, &row);
        }
    }
    PREVIEWED.remove(ctx, &key);
    Ok(())
}

fn previewing(ctx: &QueryCtx, repo: &str, commit: &str) -> Result<Vec<u64>, Error> {
    let head = (repo.to_owned(), commit.to_owned());
    let rows = PREVIEWING.scan(ctx, PREVIEWING.prefix_of(&head))?;
    Ok(rows.into_iter().map(|(_, _, n)| n).collect())
}

pub(crate) fn report(ctx: &ExecCtx, reported: Reported) -> Result<(), Error> {
    let reporter = ctx.sender_account()?;
    if !is_full_oid(&reported.commit) {
        return Err(invalid(
            "a report names its commit by its full lowercase hex id",
        ));
    }
    if reported.context.trim().is_empty() {
        return Err(invalid("a report names its context"));
    }
    if reported.reach.is_empty() {
        return Err(invalid(
            "a report names the paths its check read; \"\" is the whole tree",
        ));
    }
    for path in &reported.reach {
        check_path(path)?;
    }
    let key: Reporting = (
        reported.repo.clone(),
        reported.commit.clone(),
        reporter,
        reported.context.clone(),
    );
    for old in REACH.scan(ctx, REACH.prefix_of(&key))? {
        REACH.remove(ctx, &old);
    }
    for path in reported.reach {
        REACH.insert(ctx, &(key.clone(), path));
    }
    let kept = Kept {
        state: reported.state,
        url: reported.url,
        height: ctx.env().height,
    };
    REPORTS.put(ctx, &key, &kept);
    let changes = match reported.change {
        Some(n) => vec![n],
        None => previewing(ctx, &reported.repo, &reported.commit)?,
    };
    for n in changes {
        tell(ctx, &reported.repo, n)?;
    }
    Ok(())
}

fn is_full_oid(commit: &str) -> bool {
    let full_length = commit.len() == 40 || commit.len() == 64;
    let lowercase_hex = commit
        .bytes()
        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
    full_length && lowercase_hex
}

fn check_path(path: &str) -> Result<(), Error> {
    if path.is_empty() {
        return Ok(());
    }
    let malformed = path
        .split('/')
        .any(|segment| matches!(segment, "" | "." | "..") || segment.contains('\0'));
    match malformed {
        true => Err(invalid(format!(
            "a reach path is relative to the repository's root, with no empty, . or .. segment and no leading or trailing /, and {path:?} is not"
        ))),
        false => Ok(()),
    }
}

fn tell(ctx: &ExecCtx, repo: &str, n: u64) -> Result<(), Error> {
    let Some(verdict) = verdict(ctx, repo, n)? else {
        return Ok(());
    };
    let link = verdict_link(ctx, repo, n);
    for carried in verdict.carried {
        let url = match carried.differs.is_empty() {
            true => carried.url,
            false => link.clone(),
        };
        let status = forge::Op::Status {
            repo: repo.to_owned(),
            commit: verdict.merge.clone(),
            context: carried.context,
            state: carried.state,
            url,
        };
        ctx.emit(forge::PROGRAM, &status, Wanted::None);
    }
    Ok(())
}

fn verdict_link(ctx: &QueryCtx, repo: &str, n: u64) -> Option<String> {
    let chain = String::from_utf8(ctx.env().chain_id.clone()).ok()?;
    let number = n.to_string();
    ducklink::mint(&chain, PROGRAM, &[repo, number.as_str()])
}

pub(crate) fn verdict(ctx: &QueryCtx, repo: &str, n: u64) -> Result<Option<PreviewVerdict>, Error> {
    let Some(preview) = latest_preview(ctx, repo, n)? else {
        return Ok(None);
    };
    let mut reported: BTreeMap<(AccountNumber, String), (String, Kept)> = BTreeMap::new();
    for commit in [&preview.from_head, &preview.merge] {
        let head = (repo.to_owned(), commit.clone());
        for ((_, _, reporter, context), kept) in REPORTS.scan(ctx, REPORTS.prefix_of(&head))? {
            reported.insert((reporter, context), (commit.clone(), kept));
        }
    }
    let mut carried = Vec::new();
    for ((reporter, context), (gated, kept)) in reported {
        let key: Reporting = (repo.to_owned(), gated.clone(), reporter, context.clone());
        let differs = differing_paths(ctx, repo, &key, &preview.merge)?;
        let state = match differs.is_empty() {
            true => kept.state,
            false => CheckState::Pending,
        };
        carried.push(Carried {
            context: format!("{reporter}/{context}"),
            gated,
            reported: kept.state,
            state,
            url: kept.url,
            differs,
        });
    }
    Ok(Some(PreviewVerdict {
        n,
        head: preview.from_head,
        merge: preview.merge,
        carried,
    }))
}

fn latest_preview(ctx: &QueryCtx, repo: &str, n: u64) -> Result<Option<PreviewInfo>, Error> {
    let asked = forge::Query::Preview {
        repo: repo.to_owned(),
        n,
    };
    match ctx.query::<forge::Query, forge::Reply>(forge::PROGRAM, &asked) {
        Ok(forge::Reply::Preview { preview, .. }) => Ok(preview),
        Ok(other) => Err(unexpected_reply(forge::PROGRAM, "Preview", &other)),
        Err(refused) if refused.code == code::NOT_FOUND => Ok(None),
        Err(refused) => Err(refused),
    }
}

fn differing_paths(
    ctx: &QueryCtx,
    repo: &str,
    key: &Reporting,
    merge: &str,
) -> Result<Vec<String>, Error> {
    let gated = &key.1;
    if gated == merge {
        return Ok(Vec::new());
    }
    let mut differs = Vec::new();
    for (_, path) in REACH.scan(ctx, REACH.prefix_of(key))? {
        let was = entry_at(ctx, repo, gated, &path)?;
        let is = entry_at(ctx, repo, merge, &path)?;
        if was != is {
            differs.push(path);
        }
    }
    Ok(differs)
}

fn entry_at(
    ctx: &QueryCtx,
    repo: &str,
    at: &str,
    path: &str,
) -> Result<Option<(EntryKind, String)>, Error> {
    if path.is_empty() {
        let root = tree_page(ctx, repo, at, "", PageRequest::first(1))?;
        return Ok(root.map(|(tree, _)| (EntryKind::Directory, tree)));
    }
    let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
    let mut after = None;
    loop {
        let page = PageRequest { after, limit: None };
        let Some((_, listed)) = tree_page(ctx, repo, at, parent, page)? else {
            return Ok(None);
        };
        let entry = listed
            .items
            .iter()
            .find(|entry| entry.name == name.as_bytes());
        if let Some(entry) = entry {
            return Ok(Some((entry.kind, entry.oid.clone())));
        }
        match listed.next {
            Some(next) => after = Some(next),
            None => return Ok(None),
        }
    }
}

type Listed = (String, PageResponse<forge::TreeInfo>);

fn tree_page(
    ctx: &QueryCtx,
    repo: &str,
    at: &str,
    path: &str,
    page: PageRequest,
) -> Result<Option<Listed>, Error> {
    let asked = forge::Query::Tree {
        repo: repo.to_owned(),
        at: at.to_owned(),
        path: path.as_bytes().to_vec(),
        page,
    };
    match ctx.query::<forge::Query, forge::Reply>(forge::PROGRAM, &asked) {
        Ok(forge::Reply::Tree { tree, page, .. }) => Ok(Some((tree, page))),
        Ok(other) => Err(unexpected_reply(forge::PROGRAM, "Tree", &other)),
        Err(refused) => {
            let no_such_directory =
                refused.code == code::NOT_FOUND || refused.code == code::INVALID_INPUT;
            match no_such_directory {
                true => Ok(None),
                false => Err(refused),
            }
        }
    }
}

pub(crate) fn reports(
    ctx: &QueryCtx,
    repo: &str,
    commit: &str,
    page: &PageRequest,
) -> Result<PageResponse<Report>, Error> {
    let head = (repo.to_owned(), commit.to_owned());
    REPORTS.range_of(ctx, &head, page)?.try_map(|(key, kept)| {
        let reach = REACH
            .scan(ctx, REACH.prefix_of(&key))?
            .into_iter()
            .map(|(_, path)| path)
            .collect();
        Ok(Report {
            reporter: key.2,
            context: key.3,
            state: kept.state,
            url: kept.url,
            height: kept.height,
            reach,
        })
    })
}
