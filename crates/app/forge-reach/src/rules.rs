//! What each op checks and writes: a reported result kept with its reach, and on each Preview
//! every result reported for the change's head, or for an earlier head with the same patch,
//! compared path by path, told forge as a status, and asked for again where the target moved.
use std::collections::{BTreeMap, BTreeSet};

use ::program::{Error, ExecCtx, QueryCtx, Reply as Wanted, code, invalid, unexpected_reply};
use forge::{
    CheckState, EntryKind, FileStatus, PreviewInfo, Principal, Required, Revision, Watched,
};

use crate::state::{
    HEADS, Kept, PATCHES, PREVIEWED, PREVIEWING, REACH, REPORTS, Reporting, WANTED, WANTING,
};
use crate::{
    AccountNumber, Carried, PROGRAM, PageRequest, PageResponse, PreviewVerdict, Report, Rerun,
};

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
        Watched::Merged { repo, n, .. } => {
            forget_preview(ctx, &repo, n)?;
            forget_heads(ctx, &repo, n)?;
            preview_the_rest(ctx, &repo, n)
        }
        Watched::Closed { repo, n, .. } => {
            forget_preview(ctx, &repo, n)?;
            forget_heads(ctx, &repo, n)
        }
        _ => Ok(()),
    }
}

pub(crate) fn retell(ctx: &ExecCtx, repo: &str, n: u64) -> Result<(), Error> {
    heard_preview(ctx, repo, n)?;
    tell(ctx, repo, n)
}

fn heard_preview(ctx: &ExecCtx, repo: &str, n: u64) -> Result<(), Error> {
    forget_preview(ctx, repo, n)?;
    let Some(preview) = latest_preview(ctx, repo, n)? else {
        return Ok(());
    };
    for commit in [&preview.from_head, &preview.merge] {
        PREVIEWING.insert(ctx, &(repo.to_owned(), commit.clone(), n));
        HEADS.insert(ctx, &(repo.to_owned(), n, commit.clone()));
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
    forget_wanted(ctx, repo, &merge)?;
    for commit in [head, merge] {
        let row = (repo.to_owned(), commit, n);
        if PREVIEWING.has(ctx, &row) {
            PREVIEWING.remove(ctx, &row);
        }
    }
    PREVIEWED.remove(ctx, &key);
    Ok(())
}

fn forget_wanted(ctx: &ExecCtx, repo: &str, merge: &str) -> Result<(), Error> {
    let preview = (repo.to_owned(), merge.to_owned());
    for (repo, commit, reporter, context) in WANTING.scan(ctx, WANTING.prefix_of(&preview))? {
        let row = (reporter, repo.clone(), commit.clone(), context.clone());
        WANTED.remove(ctx, &row);
        WANTING.remove(ctx, &(repo, commit, reporter, context));
    }
    Ok(())
}

fn forget_heads(ctx: &ExecCtx, repo: &str, n: u64) -> Result<(), Error> {
    let change = (repo.to_owned(), n);
    for head in HEADS.scan(ctx, HEADS.prefix_of(&change))? {
        HEADS.remove(ctx, &head);
    }
    Ok(())
}

fn previewing(ctx: &QueryCtx, repo: &str, commit: &str) -> Result<Vec<u64>, Error> {
    let head = (repo.to_owned(), commit.to_owned());
    let rows = PREVIEWING.scan(ctx, PREVIEWING.prefix_of(&head))?;
    Ok(rows.into_iter().map(|(_, _, n)| n).collect())
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
    if let Some(n) = reported.change {
        HEADS.insert(ctx, &(reported.repo.clone(), n, reported.commit.clone()));
    }
    let changes = match reported.change {
        Some(n) => vec![n],
        None => previewing(ctx, &reported.repo, &reported.commit)?,
    };
    for n in changes {
        tell(ctx, &reported.repo, n)?;
    }
    if reported.state == CheckState::Passed {
        enqueue_passed(
            ctx,
            &reported.repo,
            &reported.commit,
            reporter,
            &reported.context,
            reported.change,
        )?;
    }
    Ok(())
}

pub(crate) struct Patched {
    pub repo: String,
    pub n: u64,
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
    HEADS.insert(
        ctx,
        &(patched.repo.clone(), patched.n, patched.commit.clone()),
    );
    tell(ctx, &patched.repo, patched.n)?;
    if patched.state == CheckState::Passed {
        enqueue_passed(
            ctx,
            &patched.repo,
            &patched.commit,
            reporter,
            &patched.context,
            Some(patched.n),
        )?;
    }
    Ok(())
}

fn enqueue_passed(
    ctx: &ExecCtx,
    repo: &str,
    commit: &str,
    reporter: AccountNumber,
    context: &str,
    change: Option<u64>,
) -> Result<(), Error> {
    let context = format!("{reporter}/{context}");
    let branches = branches_requiring(ctx, repo, &context)?;
    if branches.is_empty() {
        return Ok(());
    }
    for (n, branch) in changes_at(ctx, repo, commit, change)? {
        let required = branches.contains(&branch);
        if !required {
            continue;
        }
        if queued(ctx, repo, &branch, n)? {
            continue;
        }
        let enqueue = forge::Op::Enqueue {
            repo: repo.to_owned(),
            n,
            expected_head: commit.to_owned(),
        };
        ctx.emit(forge::PROGRAM, &enqueue, Wanted::Wanted);
    }
    Ok(())
}

fn branches_requiring(
    ctx: &QueryCtx,
    repo: &str,
    context: &str,
) -> Result<BTreeSet<Vec<u8>>, Error> {
    let Some(account) = own_account(ctx)? else {
        return Ok(BTreeSet::new());
    };
    let reporter = Principal::Account(account);
    let mut branches = BTreeSet::new();
    let mut after = None;
    loop {
        let asked = forge::Query::Required {
            repo: repo.to_owned(),
            page: PageRequest { after, limit: None },
        };
        let page = match ctx.query::<forge::Query, forge::Reply>(forge::PROGRAM, &asked)? {
            forge::Reply::Required { page, .. } => page,
            other => return Err(unexpected_reply(forge::PROGRAM, "Required", &other)),
        };
        for branch in page.items {
            let carries = branch.required.reporter == reporter;
            let required = branch.required.contexts.iter().any(|each| each == context);
            let ours = carries && required;
            if ours {
                branches.insert(branch.branch);
            }
        }
        match page.next {
            Some(next) => after = Some(next),
            None => return Ok(branches),
        }
    }
}

fn changes_at(
    ctx: &QueryCtx,
    repo: &str,
    commit: &str,
    n: Option<u64>,
) -> Result<Vec<(u64, Vec<u8>)>, Error> {
    if let Some(n) = n {
        let Some((change, head)) = change_with_head(ctx, repo, n)? else {
            return Ok(Vec::new());
        };
        let open = change.state == forge::ChangeState::Open;
        let current = head.as_deref() == Some(commit);
        let eligible = open && current;
        return Ok(match eligible {
            true => vec![(n, change.into)],
            false => Vec::new(),
        });
    }
    let refs = refs_at(ctx, repo, commit)?;
    let mut changes = Vec::new();
    let mut after = None;
    loop {
        let asked = forge::Query::Changes {
            repo: repo.to_owned(),
            filter: forge::ChangeFilter {
                state: Some(forge::ChangeState::Open),
                ..Default::default()
            },
            page: PageRequest { after, limit: None },
        };
        let page = match ctx.query::<forge::Query, forge::Reply>(forge::PROGRAM, &asked)? {
            forge::Reply::Changes { page, .. } => page,
            other => return Err(unexpected_reply(forge::PROGRAM, "Changes", &other)),
        };
        for change in page.items {
            let current = match change.from {
                Revision::Ref(name) => refs.contains(&name),
                Revision::Oid(head) => head == commit,
            };
            if current {
                changes.push((change.n, change.into));
            }
        }
        match page.next {
            Some(next) => after = Some(next),
            None => return Ok(changes),
        }
    }
}

fn refs_at(ctx: &QueryCtx, repo: &str, commit: &str) -> Result<BTreeSet<Vec<u8>>, Error> {
    let mut refs = BTreeSet::new();
    let mut after = None;
    loop {
        let asked = forge::Query::Refs {
            repo: repo.to_owned(),
            page: PageRequest { after, limit: None },
        };
        let page = match ctx.query::<forge::Query, forge::Reply>(forge::PROGRAM, &asked)? {
            forge::Reply::Refs { page, .. } => page,
            other => return Err(unexpected_reply(forge::PROGRAM, "Refs", &other)),
        };
        for entry in page.items {
            if entry.target == commit {
                refs.insert(entry.name);
            }
        }
        match page.next {
            Some(next) => after = Some(next),
            None => return Ok(refs),
        }
    }
}

fn queued(ctx: &QueryCtx, repo: &str, branch: &[u8], n: u64) -> Result<bool, Error> {
    let mut after = None;
    loop {
        let asked = forge::Query::Queue {
            repo: repo.to_owned(),
            branch: branch.to_vec(),
            page: PageRequest { after, limit: None },
        };
        let page = match ctx.query::<forge::Query, forge::Reply>(forge::PROGRAM, &asked)? {
            forge::Reply::Queue { page, .. } => page,
            other => return Err(unexpected_reply(forge::PROGRAM, "Queue", &other)),
        };
        if page.items.iter().any(|entry| entry.n == n) {
            return Ok(true);
        }
        match page.next {
            Some(next) => after = Some(next),
            None => return Ok(false),
        }
    }
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

fn tell(ctx: &ExecCtx, repo: &str, n: u64) -> Result<(), Error> {
    let Some((preview, judged)) = judge(ctx, repo, n)? else {
        return Ok(());
    };
    let link = verdict_link(ctx, repo, n);
    for result in &judged {
        let url = match result.differs.is_empty() {
            true => result.kept.url.clone(),
            false => link.clone(),
        };
        let status = forge::Op::Status {
            repo: repo.to_owned(),
            commit: preview.merge.clone(),
            context: result.status_context(),
            state: result.state,
            url,
        };
        ctx.emit(forge::PROGRAM, &status, Wanted::None);
    }
    want_again(ctx, repo, n, &preview.merge, &judged)
}

fn verdict_link(ctx: &QueryCtx, repo: &str, n: u64) -> Option<String> {
    let chain = String::from_utf8(ctx.env().chain_id.clone()).ok()?;
    let number = n.to_string();
    ducklink::mint(&chain, PROGRAM, &[repo, number.as_str()])
}

struct Judged {
    reporter: AccountNumber,
    context: String,
    gated: String,
    kept: Kept,
    state: CheckState,
    differs: Vec<String>,
}

impl Judged {
    fn status_context(&self) -> String {
        format!("{}/{}", self.reporter, self.context)
    }

    fn gone_stale(&self) -> bool {
        let passed = self.kept.state == CheckState::Passed;
        passed && !self.differs.is_empty()
    }

    fn passes_or_is_asked(&self, context: &str) -> bool {
        let named = self.status_context() == context;
        let settled = self.state == CheckState::Passed || self.gone_stale();
        named && settled
    }
}

type Judgement = (PreviewInfo, Vec<Judged>);

fn judge(ctx: &QueryCtx, repo: &str, n: u64) -> Result<Option<Judgement>, Error> {
    let Some(preview) = latest_preview(ctx, repo, n)? else {
        return Ok(None);
    };
    let reported = results(ctx, repo, n, &preview.from_head, &preview.merge)?;
    let any_earlier = reported.values().any(|found| found.earlier);
    let ours = match any_earlier {
        true => changed(ctx, repo, &preview.into_head, &preview.merge),
        false => None,
    };
    let stacked = stacked_under_a_train(ctx, repo, n, &preview)?;
    let mut whole_train_patch: Option<Option<Patch>> = None;
    let mut judged = Vec::new();
    for ((reporter, context), found) in reported {
        let Found {
            commit: gated,
            kept,
            earlier,
            patch,
        } = found;
        let key: Reporting = (repo.to_owned(), gated.clone(), reporter, context.clone());
        let mut differs = match patch {
            true => Vec::new(),
            false => differing_paths(ctx, repo, &key, &preview.merge)?,
        };
        let reach_held = differs.is_empty();
        let same_reach_elsewhere = earlier && reach_held;
        let tree_reported_elsewhere = !patch && gated != preview.merge;
        let status_context = format!("{reporter}/{context}");
        let for_the_whole_train = stacked.as_ref().filter(|train| {
            let in_a_tip_context = train.tip_contexts.contains(&status_context);
            tree_reported_elsewhere && reach_held && in_a_tip_context
        });
        if let Some(train) = for_the_whole_train {
            let base = train.branch_base.as_deref();
            let whole = whole_train_patch.get_or_insert_with(|| {
                base.and_then(|base| changed(ctx, repo, base, &preview.merge))
            });
            let theirs = base.and_then(|base| patch_of(ctx, repo, &gated, base));
            differs = patch_differs(theirs.as_ref(), whole.as_ref());
        } else if same_reach_elsewhere {
            let theirs = patch_of(ctx, repo, &gated, &preview.into_head);
            differs = patch_differs(theirs.as_ref(), ours.as_ref());
        }
        let state = match differs.is_empty() {
            true => kept.state,
            false => CheckState::Pending,
        };
        judged.push(Judged {
            reporter,
            context,
            gated,
            kept,
            state,
            differs,
        });
    }
    Ok(Some((preview, judged)))
}

struct StackedOnATrain {
    tip_contexts: Vec<String>,
    branch_base: Option<String>,
}

fn stacked_under_a_train(
    ctx: &QueryCtx,
    repo: &str,
    n: u64,
    preview: &PreviewInfo,
) -> Result<Option<StackedOnATrain>, Error> {
    let Some(branch) = target_of(ctx, repo, n)? else {
        return Ok(None);
    };
    let tip_contexts = train_tip_contexts(ctx, repo, &branch)?;
    if tip_contexts.is_empty() {
        return Ok(None);
    }
    let branch_base = branch_base_of(ctx, repo, &preview.merge, branch);
    let stands_on_the_branch = branch_base.as_deref() == Some(preview.into_head.as_str());
    if stands_on_the_branch {
        return Ok(None);
    }
    Ok(Some(StackedOnATrain {
        tip_contexts,
        branch_base,
    }))
}

fn train_tip_contexts(ctx: &QueryCtx, repo: &str, branch: &[u8]) -> Result<Vec<String>, Error> {
    let asked = forge::Query::Train {
        repo: repo.to_owned(),
        branch: branch.to_vec(),
    };
    match ctx.query::<forge::Query, forge::Reply>(forge::PROGRAM, &asked)? {
        forge::Reply::Train { train, .. } => Ok(train.map(|train| train.tip).unwrap_or_default()),
        other => Err(unexpected_reply(forge::PROGRAM, "Train", &other)),
    }
}

fn branch_base_of(ctx: &QueryCtx, repo: &str, merge: &str, branch: Vec<u8>) -> Option<String> {
    let asked = forge::Query::Compare {
        repo: repo.to_owned(),
        from: Revision::Oid(merge.to_owned()),
        into: Revision::Ref(branch),
    };
    let reply = ctx
        .query::<forge::Query, forge::Reply>(forge::PROGRAM, &asked)
        .ok()?;
    let forge::Reply::Compare { comparison, .. } = reply else {
        return None;
    };
    comparison.base
}

pub(crate) fn verdict(ctx: &QueryCtx, repo: &str, n: u64) -> Result<Option<PreviewVerdict>, Error> {
    let Some((preview, judged)) = judge(ctx, repo, n)? else {
        return Ok(None);
    };
    let carried = judged
        .into_iter()
        .map(|result| Carried {
            context: result.status_context(),
            gated: result.gated,
            reported: result.kept.state,
            state: result.state,
            url: result.kept.url,
            differs: result.differs,
        })
        .collect();
    Ok(Some(PreviewVerdict {
        n,
        head: preview.from_head,
        merge: preview.merge,
        carried,
    }))
}

fn want_again(
    ctx: &ExecCtx,
    repo: &str,
    n: u64,
    merge: &str,
    judged: &[Judged],
) -> Result<(), Error> {
    let stale = judged.iter().any(Judged::gone_stale);
    let wants = match stale {
        true => wants_of(ctx, repo, n)?,
        false => None,
    };
    for result in judged {
        let asking = wants
            .as_ref()
            .filter(|(_, contexts)| asked_again(result, judged, contexts));
        let row = (
            result.reporter,
            repo.to_owned(),
            merge.to_owned(),
            result.context.clone(),
        );
        let index = (
            repo.to_owned(),
            merge.to_owned(),
            result.reporter,
            result.context.clone(),
        );
        match asking {
            Some((change, _)) => {
                let first = !WANTING.has(ctx, &index);
                WANTED.put(ctx, &row, &n);
                WANTING.insert(ctx, &index);
                if first {
                    ask(ctx, change, &index, result);
                }
            }
            None => {
                if WANTING.has(ctx, &index) {
                    WANTED.remove(ctx, &row);
                    WANTING.remove(ctx, &index);
                }
            }
        }
    }
    Ok(())
}

fn asked_again(result: &Judged, judged: &[Judged], required: &[String]) -> bool {
    let context = result.status_context();
    let candidate = result.gone_stale() && required.contains(&context);
    if !candidate {
        return false;
    }
    required
        .iter()
        .filter(|other| **other != context)
        .all(|other| judged.iter().any(|each| each.passes_or_is_asked(other)))
}

fn preview_the_rest(ctx: &ExecCtx, repo: &str, merged: u64) -> Result<(), Error> {
    let Some(into) = target_of(ctx, repo, merged)? else {
        return Ok(());
    };
    let Some(required) = required_by_us(ctx, repo, &into)? else {
        return Ok(());
    };
    let env = ctx.env();
    let scheduler = env.role(abi::role::SCHEDULER)?.clone();
    let open = PREVIEWED.scan(ctx, PREVIEWED.prefix_of(&repo.to_owned()))?;
    for ((_, n), (head, merge)) in open {
        let passed: BTreeSet<String> = results(ctx, repo, n, &head, &merge)?
            .into_iter()
            .filter(|(_, found)| found.kept.state == CheckState::Passed)
            .map(|((reporter, context), _)| format!("{reporter}/{context}"))
            .collect();
        let all_passed = required.iter().all(|context| passed.contains(context));
        if !all_passed {
            continue;
        }
        let same_target = target_of(ctx, repo, n)?.is_some_and(|target| target == into);
        if !same_target {
            continue;
        }
        let preview = forge::Op::Preview {
            repo: repo.to_owned(),
            n,
        };
        let next = abi::role::scheduler::Op::Schedule {
            height: env.height.saturating_add(1),
            target: forge::PROGRAM.to_owned(),
            payload: abi::encode(&preview),
        };
        ctx.emit(scheduler.clone(), &next, Wanted::None);
    }
    Ok(())
}

type Wants = (forge::Change, Vec<String>);

fn wants_of(ctx: &QueryCtx, repo: &str, n: u64) -> Result<Option<Wants>, Error> {
    let Some(change) = change_of(ctx, repo, n)? else {
        return Ok(None);
    };
    let required = required_by_us(ctx, repo, &change.into)?;
    Ok(required.map(|contexts| (change, contexts)))
}

fn ask(ctx: &ExecCtx, change: &forge::Change, index: &Reporting, result: &Judged) {
    let (repo, merge, _, _) = index;
    let digest = ctx.sha256(abi::encode(index));
    let id: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    let context = result.status_context();
    let notice = chat::Notice {
        kind: "check".into(),
        group: format!("{context} {merge}"),
        subject: format!("{repo} #{}: {}", change.n, change.title),
        link: verdict_link(ctx, repo, change.n),
        status: chat::message::Status::Running,
        detail: format!(
            "{context} wanted again on Preview {merge}: its result on {} no longer carries",
            result.gated
        ),
        concerns: vec![result.reporter],
    };
    let post = chat::Op::Notify {
        channel_id: change.channel.clone(),
        message_id: format!("{PROGRAM}:{id}"),
        thread: None,
        notice,
    };
    ctx.emit(chat::PROGRAM, &post, Wanted::Wanted);
}

fn target_of(ctx: &QueryCtx, repo: &str, n: u64) -> Result<Option<Vec<u8>>, Error> {
    Ok(change_of(ctx, repo, n)?.map(|change| change.into))
}

fn change_of(ctx: &QueryCtx, repo: &str, n: u64) -> Result<Option<forge::Change>, Error> {
    Ok(change_with_head(ctx, repo, n)?.map(|(change, _)| change))
}

fn change_with_head(
    ctx: &QueryCtx,
    repo: &str,
    n: u64,
) -> Result<Option<(forge::Change, Option<String>)>, Error> {
    let asked = forge::Query::Change {
        repo: repo.to_owned(),
        n,
        page: PageRequest::first(1),
    };
    match ctx.query::<forge::Query, forge::Reply>(forge::PROGRAM, &asked) {
        Ok(forge::Reply::Change {
            change,
            source_head,
            ..
        }) => Ok(Some((change, source_head))),
        Ok(other) => Err(unexpected_reply(forge::PROGRAM, "Change", &other)),
        Err(refused) if refused.code == code::NOT_FOUND => Ok(None),
        Err(refused) => Err(refused),
    }
}

fn required_by_us(ctx: &QueryCtx, repo: &str, branch: &[u8]) -> Result<Option<Vec<String>>, Error> {
    let mut after = None;
    loop {
        let asked = forge::Query::Required {
            repo: repo.to_owned(),
            page: PageRequest { after, limit: None },
        };
        let page = match ctx.query::<forge::Query, forge::Reply>(forge::PROGRAM, &asked)? {
            forge::Reply::Required { page, .. } => page,
            other => return Err(unexpected_reply(forge::PROGRAM, "Required", &other)),
        };
        let found = page.items.into_iter().find(|each| each.branch == branch);
        if let Some(found) = found {
            return ours_to_carry(ctx, found.required);
        }
        match page.next {
            Some(next) => after = Some(next),
            None => return Ok(None),
        }
    }
}

fn ours_to_carry(ctx: &QueryCtx, required: Required) -> Result<Option<Vec<String>>, Error> {
    let Some(account) = own_account(ctx)? else {
        return Ok(None);
    };
    let ours = required.reporter == Principal::Account(account);
    Ok(ours.then_some(required.contexts))
}

fn own_account(ctx: &QueryCtx) -> Result<Option<AccountNumber>, Error> {
    use abi::role::identity::{Query, Reply};
    let identity = &ctx.env().role(abi::role::IDENTITY)?.clone();
    let asked = Query::OfProgram(ctx.env().program.clone());
    match ctx.query::<Query, Reply>(identity, &asked)? {
        Reply::Account(account) => Ok(account),
        other => Err(unexpected_reply(identity, "OfProgram", &other)),
    }
}

struct Found {
    commit: String,
    kept: Kept,
    earlier: bool,
    patch: bool,
}

type Results = BTreeMap<(AccountNumber, String), Found>;

type KeptAt = Vec<((AccountNumber, String), Kept, bool)>;

fn kept_at(ctx: &QueryCtx, repo: &str, commit: &str) -> Result<KeptAt, Error> {
    let at = (repo.to_owned(), commit.to_owned());
    let trees = REPORTS
        .scan(ctx, REPORTS.prefix_of(&at))?
        .into_iter()
        .map(|((_, _, reporter, context), kept)| ((reporter, context), kept, false));
    let patches = PATCHES
        .scan(ctx, PATCHES.prefix_of(&at))?
        .into_iter()
        .map(|((_, _, reporter, context), kept)| ((reporter, context), kept, true));
    Ok(trees.chain(patches).collect())
}

fn results(ctx: &QueryCtx, repo: &str, n: u64, head: &str, merge: &str) -> Result<Results, Error> {
    let mut reported = reported_earlier(ctx, repo, n, head, merge)?;
    for commit in [head, merge] {
        for (key, kept, patch) in kept_at(ctx, repo, commit)? {
            let found = Found {
                commit: commit.to_owned(),
                kept,
                earlier: false,
                patch,
            };
            reported.insert(key, found);
        }
    }
    Ok(reported)
}

fn reported_earlier(
    ctx: &QueryCtx,
    repo: &str,
    n: u64,
    head: &str,
    merge: &str,
) -> Result<Results, Error> {
    let mut earlier = Results::new();
    let change = (repo.to_owned(), n);
    for (_, _, commit) in HEADS.scan(ctx, HEADS.prefix_of(&change))? {
        let previewed = commit == head || commit == merge;
        if previewed {
            continue;
        }
        for (key, kept, patch) in kept_at(ctx, repo, &commit)? {
            let newer = earlier
                .get(&key)
                .is_none_or(|was| kept.height > was.kept.height);
            if newer {
                let found = Found {
                    commit: commit.clone(),
                    kept,
                    earlier: true,
                    patch,
                };
                earlier.insert(key, found);
            }
        }
    }
    Ok(earlier)
}

type Entry = Option<(EntryKind, String)>;
type Patch = BTreeMap<String, (FileStatus, Entry, Entry)>;

fn patch_of(ctx: &QueryCtx, repo: &str, gated: &str, into: &str) -> Option<Patch> {
    let asked = forge::Query::Compare {
        repo: repo.to_owned(),
        from: Revision::Oid(gated.to_owned()),
        into: Revision::Oid(into.to_owned()),
    };
    let reply = ctx
        .query::<forge::Query, forge::Reply>(forge::PROGRAM, &asked)
        .ok()?;
    let forge::Reply::Compare { comparison, .. } = reply else {
        return None;
    };
    changed(ctx, repo, &comparison.base?, gated)
}

fn changed(ctx: &QueryCtx, repo: &str, base: &str, head: &str) -> Option<Patch> {
    let mut stats = Vec::new();
    let mut after = None;
    loop {
        let asked = forge::Query::DiffStat {
            repo: repo.to_owned(),
            base: Some(base.to_owned()),
            head: head.to_owned(),
            page: PageRequest { after, limit: None },
        };
        let reply = ctx
            .query::<forge::Query, forge::Reply>(forge::PROGRAM, &asked)
            .ok()?;
        let forge::Reply::DiffStat { page, .. } = reply else {
            return None;
        };
        stats.extend(page.items.into_iter().map(|file| (file.path, file.status)));
        match page.next {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    let folders: BTreeSet<String> = stats
        .iter()
        .map(|(path, _)| split_path(path).0.to_owned())
        .collect();
    let mut was = BTreeMap::new();
    let mut is = BTreeMap::new();
    for folder in folders {
        was.insert(folder.clone(), folder_at(ctx, repo, base, &folder).ok()?);
        is.insert(folder.clone(), folder_at(ctx, repo, head, &folder).ok()?);
    }
    let mut patch = Patch::new();
    for (path, status) in stats {
        let (folder, name) = split_path(&path);
        let old = was
            .get(folder)
            .and_then(|listed| listed.get(name.as_bytes()).cloned());
        let new = is
            .get(folder)
            .and_then(|listed| listed.get(name.as_bytes()).cloned());
        patch.insert(path, (status, old, new));
    }
    Some(patch)
}

fn split_path(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

type Folder = BTreeMap<Vec<u8>, (EntryKind, String)>;

fn folder_at(ctx: &QueryCtx, repo: &str, at: &str, path: &str) -> Result<Folder, Error> {
    let mut folder = Folder::new();
    let mut after = None;
    loop {
        let page = PageRequest { after, limit: None };
        let Some((_, listed)) = tree_page(ctx, repo, at, path, page)? else {
            return Ok(folder);
        };
        for entry in listed.items {
            folder.insert(entry.name, (entry.kind, entry.oid));
        }
        match listed.next {
            Some(next) => after = Some(next),
            None => return Ok(folder),
        }
    }
}

fn patch_differs(theirs: Option<&Patch>, ours: Option<&Patch>) -> Vec<String> {
    let (Some(theirs), Some(ours)) = (theirs, ours) else {
        return vec![String::new()];
    };
    let paths: BTreeSet<&String> = theirs.keys().chain(ours.keys()).collect();
    paths
        .into_iter()
        .filter(|path| theirs.get(*path) != ours.get(*path))
        .cloned()
        .collect()
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

#[cfg(test)]
thread_local! {
    pub(crate) static TREE_READS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn tree_page(
    ctx: &QueryCtx,
    repo: &str,
    at: &str,
    path: &str,
    page: PageRequest,
) -> Result<Option<Listed>, Error> {
    #[cfg(test)]
    TREE_READS.with(|reads| reads.set(reads.get() + 1));
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

pub(crate) fn wanted(
    ctx: &QueryCtx,
    reporter: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<Rerun>, Error> {
    WANTED
        .range_of(ctx, &reporter, page)?
        .try_map(|((_, repo, commit, context), n)| {
            Ok(Rerun {
                repo,
                n,
                commit,
                context,
            })
        })
}
