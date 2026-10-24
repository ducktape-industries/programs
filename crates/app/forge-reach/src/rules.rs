//! Exact-commit receipts stored under the reporting account, with no forwarding or carry.
use crate::state::{Kept, PATCHES, REACH, REPORTS, Reporting};
use crate::{PageRequest, PageResponse, Report};
use ::program::{Error, ExecCtx, QueryCtx, invalid};
use forge::CheckState;

pub(crate) struct Reported {
    pub repo: String,
    pub commit: String,
    pub context: String,
    pub state: CheckState,
    pub url: Option<String>,
    pub reach: Vec<String>,
}

pub(crate) fn report(ctx: &ExecCtx, reported: Reported) -> Result<(), Error> {
    let reporter = ctx.sender_account()?;
    check_named(&reported.commit, &reported.context)?;
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
    if PATCHES.get(ctx, &key)?.is_some() {
        PATCHES.remove(ctx, &key);
    }
    ctx.emit(
        forge::PROGRAM,
        &forge::Op::Watch {
            deliver: Vec::new(),
        },
        ::program::Reply::None,
    );
    Ok(())
}

pub(crate) struct Patched {
    pub repo: String,
    pub commit: String,
    pub context: String,
    pub state: CheckState,
    pub url: Option<String>,
}

pub(crate) fn report_patch(ctx: &ExecCtx, patched: Patched) -> Result<(), Error> {
    let reporter = ctx.sender_account()?;
    check_named(&patched.commit, &patched.context)?;
    let key: Reporting = (
        patched.repo.clone(),
        patched.commit.clone(),
        reporter,
        patched.context.clone(),
    );
    for old in REACH.scan(ctx, REACH.prefix_of(&key))? {
        REACH.remove(ctx, &old);
    }
    if REPORTS.get(ctx, &key)?.is_some() {
        REPORTS.remove(ctx, &key);
    }
    let kept = Kept {
        state: patched.state,
        url: patched.url,
        height: ctx.env().height,
    };
    PATCHES.put(ctx, &key, &kept);
    ctx.emit(
        forge::PROGRAM,
        &forge::Op::Watch {
            deliver: Vec::new(),
        },
        ::program::Reply::None,
    );
    Ok(())
}

fn check_named(commit: &str, context: &str) -> Result<(), Error> {
    if !is_full_oid(commit) {
        return Err(invalid(
            "a report names its commit by its full lowercase hex id",
        ));
    }
    if context.trim().is_empty() {
        return Err(invalid("a report names its context"));
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

pub(crate) fn reports(
    ctx: &QueryCtx,
    repo: &str,
    commit: &str,
    page: &PageRequest,
) -> Result<PageResponse<Report>, Error> {
    let head = (repo.to_owned(), commit.to_owned());
    let trees = REPORTS.key(&head);
    let patches = PATCHES.key(&head);
    let past_trees = page
        .open(&trees)?
        .is_some_and(|cursor| PATCHES.owns(&cursor.after));
    let listing = page.listing(ctx, trees.clone())?;
    let mut rows = Vec::new();
    if !past_trees {
        for (key, kept) in REPORTS.scan(ctx, listing.scan_ahead(&trees))? {
            let reach = REACH
                .scan(ctx, REACH.prefix_of(&key))?
                .into_iter()
                .map(|(_, path)| path)
                .collect();
            rows.push((REPORTS.key(&key), report_of(key, kept, reach)));
        }
    }
    for (key, kept) in PATCHES.scan(ctx, listing.scan_ahead(&patches))? {
        rows.push((PATCHES.key(&key), report_of(key, kept, Vec::new())));
    }
    Ok(listing.reply(rows))
}

fn report_of(key: Reporting, kept: Kept, reach: Vec<String>) -> Report {
    Report {
        reporter: key.2,
        context: key.3,
        state: kept.state,
        url: kept.url,
        height: kept.height,
        reach,
    }
}
