//! What each op checks and writes: a wake armed by its account, heard from the scheduler, a pod's
//! stream, forge's or files' watchers or the programs role, ended once, and told to its account
//! through attribution as a `Result` on `wake/<id>`; an event ends its key's wakes one frame each.
use std::collections::BTreeSet;

use ::program::{
    BlobId, Cause, Ctx, Error, ExecCtx, MessageId, Outcome, Principal, QueryCtx, Reply as Wanted,
    code, corrupt, decoded, invalid, not_found, unauthorized, unexpected_reply, wrong_state,
};
use abi::role::egress::{DEFAULT_WAIT, Delivery, Event, Request, http};
use abi::role::programs::Entry;
use attribution::{Actor, Object, Reason, Relation, Update};
use serde_json::Value;

use crate::state::{
    BY_ACCOUNT, CHANGE_WAKES, ENTRIES, FILE_COUNT, FILE_WAKES, FIRED, FORGE_COUNT, FileWatch, LAST,
    PATH_WAKES, PODS, PROGRAM_WAKES, PodWatch, REF_WAKES, SCHEDULED, TELLING, WAKE_REFS, WAKES,
};
use crate::{
    AccountNumber, Ended, Fired, Firing, KIND, Key, Op, PROGRAM, PageRequest, PageResponse, Until,
    WaitFor, WakeRow, filed_as, forged_as, pod_event_as,
};

const PODS_SERVICE: &str = "pods";

pub(crate) fn what(event: &WaitFor) -> String {
    match event {
        WaitFor::Height { height } => format!("block {height}"),
        WaitFor::Pod { name } => format!("pod {name} to end"),
        WaitFor::Message { channel, .. } => format!("a message in chat channel {channel}"),
        WaitFor::Change { repo, n } => format!("change {n} in {repo} to move, close or merge"),
        WaitFor::Ref { repo, reference } => format!("{reference} in {repo} to move"),
        WaitFor::File { path } => format!("{path} to change in files"),
        WaitFor::Settled { repo, n, until } => {
            let how = match until {
                Until::Merged => "be merged",
                Until::Closed => "be closed",
                Until::Either => "be merged or closed",
            };
            format!("change {n} in {repo} to {how}")
        }
        WaitFor::Program { program } => format!("program {program}'s code to change"),
    }
}

fn kept(ctx: &QueryCtx, id: u64) -> Result<WakeRow, Error> {
    WAKES
        .get(ctx, &id)?
        .ok_or_else(|| not_found(format!("no wake {id}")))
}

fn open(ctx: &QueryCtx, id: u64) -> Result<Option<WakeRow>, Error> {
    Ok(WAKES.get(ctx, &id)?.filter(|row| row.ended.is_none()))
}

fn by_itself(ctx: &QueryCtx) -> bool {
    ctx.env()
        .sending_program()
        .is_ok_and(|program| program == PROGRAM)
}

fn its_own_run(ctx: &QueryCtx) -> Option<u64> {
    match (ctx.env().cause.clone(), by_itself(ctx)) {
        (Cause::Scheduled { entry }, true) => Some(entry),
        _ => None,
    }
}

fn normalized(event: WaitFor) -> Result<WaitFor, Error> {
    Ok(match event {
        WaitFor::Ref { repo, reference } if !reference.starts_with("refs/") => WaitFor::Ref {
            repo,
            reference: format!("refs/heads/{reference}"),
        },
        WaitFor::File { path } => WaitFor::File {
            path: files::paths::join(&files::paths::segments(&path)?),
        },
        other => other,
    })
}

pub(crate) async fn wake_on(ctx: &Ctx, event: WaitFor) -> Result<(), Error> {
    let account = ctx.sender_account()?;
    if let WaitFor::Message { channel, .. } = &event {
        return Err(invalid(format!(
            "{PROGRAM} hears no chat message: attribution already tells account {account} each message for it, in {channel} as anywhere, so read ChangesTo"
        )));
    }
    let event = normalized(event)?;
    let id = LAST
        .get(ctx)?
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| Error::new(code::EXHAUSTED, "wake ids are exhausted"))?;
    LAST.put(ctx, &id);
    match &event {
        WaitFor::Height { height } => book(ctx, id, *height).await?,
        WaitFor::Pod { name } => {
            named_pod(name)?;
            watch_from_next_block(ctx, id).await?;
        }
        WaitFor::Message { .. } => {}
        WaitFor::Change { repo, n } => {
            if let Some(source) = open_change(ctx, repo, *n)? {
                watch_ref(ctx, id, repo, source);
            }
            CHANGE_WAKES.insert(ctx, &((repo.clone(), *n), id));
            forge_wakes(ctx, true)?;
        }
        WaitFor::Settled { repo, n, .. } => {
            open_change(ctx, repo, *n)?;
            CHANGE_WAKES.insert(ctx, &((repo.clone(), *n), id));
            forge_wakes(ctx, true)?;
        }
        WaitFor::Ref { repo, reference } => {
            known_repo(ctx, repo)?;
            watch_ref(ctx, id, repo, reference.clone());
            forge_wakes(ctx, true)?;
        }
        WaitFor::File { path } => {
            let node = node_at(ctx, path, None)?;
            let watch = FileWatch {
                path: path.clone(),
                node,
            };
            FILE_WAKES.put(ctx, &id, &watch);
            PATH_WAKES.insert(ctx, &(path.clone(), id));
            files_wakes(ctx, true)?;
        }
        WaitFor::Program { program } => {
            let Some(at) = next_change(ctx, program)? else {
                return Err(wrong_state(format!(
                    "nothing is scheduled to change program {program}: wake on its change once one is scheduled, or on a height"
                )));
            };
            PROGRAM_WAKES.put(ctx, &id, &running(ctx, program)?);
            book(ctx, id, at).await?;
        }
    }
    let row = WakeRow {
        id,
        account,
        made: ctx.env().height,
        event,
        ended: None,
    };
    WAKES.put(ctx, &id, &row);
    BY_ACCOUNT.insert(ctx, &(account, id));
    ctx.set_return_data(abi::encode(&id));
    Ok(())
}

async fn book(ctx: &Ctx, id: u64, height: u64) -> Result<(), Error> {
    let entry = ctx.schedule(height, PROGRAM, &Op::Due).await?;
    ENTRIES.put(ctx, &entry, &id);
    SCHEDULED.put(ctx, &id, &entry);
    Ok(())
}

pub(crate) async fn cancel(ctx: &Ctx, id: u64) -> Result<(), Error> {
    let row = kept(ctx, id)?;
    if ctx.sender_account()? != row.account {
        return Err(unauthorized(format!(
            "wake {id} is account {}'s to cancel",
            row.account
        )));
    }
    if let Some(ended) = &row.ended {
        return Err(wrong_state(format!(
            "wake {id} ended at block {}: arm a new one instead of cancelling the ended one",
            ended.height
        )));
    }
    if let Some(entry) = SCHEDULED.get(ctx, &id)? {
        ctx.cancel(entry).await?;
    }
    if let Some(PodWatch {
        entry: Some(entry), ..
    }) = PODS.get(ctx, &id)?
    {
        ctx.cancel(entry).await?;
    }
    disarm(ctx, &row)?;
    WAKES.remove(ctx, &id);
    BY_ACCOUNT.remove(ctx, &(row.account, id));
    Ok(())
}

fn disarm(ctx: &ExecCtx, row: &WakeRow) -> Result<(), Error> {
    if let Some(entry) = SCHEDULED.get(ctx, &row.id)? {
        ENTRIES.remove(ctx, &entry);
        SCHEDULED.remove(ctx, &row.id);
    }
    PODS.remove(ctx, &row.id);
    PROGRAM_WAKES.remove(ctx, &row.id);
    if let Some(key) = WAKE_REFS.get(ctx, &row.id)? {
        REF_WAKES.remove(ctx, &(key, row.id));
        WAKE_REFS.remove(ctx, &row.id);
    }
    match &row.event {
        WaitFor::Change { repo, n } | WaitFor::Settled { repo, n, .. } => {
            CHANGE_WAKES.remove(ctx, &((repo.clone(), *n), row.id));
            forge_wakes(ctx, false)
        }
        WaitFor::Ref { .. } => forge_wakes(ctx, false),
        WaitFor::File { path } => {
            FILE_WAKES.remove(ctx, &row.id);
            PATH_WAKES.remove(ctx, &(path.clone(), row.id));
            files_wakes(ctx, false)
        }
        WaitFor::Height { .. }
        | WaitFor::Pod { .. }
        | WaitFor::Message { .. }
        | WaitFor::Program { .. } => Ok(()),
    }
}

fn end(ctx: &ExecCtx, id: u64, what: String) -> Result<(), Error> {
    let Some(mut row) = open(ctx, id)? else {
        return Ok(());
    };
    let stop = if what.contains('\n') { "" } else { "." };
    let detail = format!("Wake {id} is over: {what}{stop}");
    row.ended = Some(Ended {
        height: ctx.env().height,
        text: what,
    });
    WAKES.put(ctx, &id, &row);
    disarm(ctx, &row)?;
    let update = Update {
        object: Object {
            kind: KIND.into(),
            id: id.to_string(),
        },
        revision: 1,
        actor: Actor::Program(PROGRAM.into()),
        relations: vec![Relation {
            recipient: row.account,
            reason: Reason::Result,
            detail: detail.into_bytes(),
        }],
        transfers: Vec::new(),
    };
    let told = attribution::Op::Attribute(update);
    let sent = ctx.emit(attribution::PROGRAM, &told, Wanted::Wanted);
    TELLING.put(ctx, &sent.seq, &id);
    Ok(())
}

pub(crate) fn replied(ctx: &ExecCtx, message: &MessageId, outcome: &Outcome) -> Result<(), Error> {
    let Some(id) = TELLING.get(ctx, &message.seq)? else {
        return Ok(());
    };
    TELLING.remove(ctx, &message.seq);
    let Outcome::Rejected(refused) = outcome else {
        return Ok(());
    };
    let Some(mut row) = WAKES.get(ctx, &id)? else {
        return Ok(());
    };
    if let Some(ended) = &mut row.ended {
        ended.text = format!("{}; not told: {}", ended.text, refused.message);
    }
    WAKES.put(ctx, &id, &row);
    Ok(())
}

pub(crate) async fn due(ctx: &Ctx) -> Result<(), Error> {
    let Some(entry) = its_own_run(ctx) else {
        return Err(unauthorized(format!(
            "a wake's block is due only as {PROGRAM}'s own scheduled entry, at that block"
        )));
    };
    let Some(id) = ENTRIES.get(ctx, &entry)? else {
        return Ok(());
    };
    let Some(row) = open(ctx, id)? else {
        return Ok(());
    };
    let height = ctx.env().height;
    match &row.event {
        WaitFor::Program { program } => program_due(ctx, id, program).await,
        _ => end(ctx, id, format!("block {height} has come")),
    }
}

async fn program_due(ctx: &Ctx, id: u64, program: &str) -> Result<(), Error> {
    let Some(before) = PROGRAM_WAKES.get(ctx, &id)? else {
        let key = PROGRAM_WAKES.key(&id);
        return Err(corrupt(
            "program wake",
            &key,
            "an open program wake keeps no code",
        ));
    };
    let now = running(ctx, program)?;
    let height = ctx.env().height;
    if let Some(words) = changed(program, before.as_ref(), now.as_ref(), height) {
        return end(ctx, id, words);
    }
    if let Some(entry) = SCHEDULED.get(ctx, &id)? {
        ENTRIES.remove(ctx, &entry);
        SCHEDULED.remove(ctx, &id);
    }
    match next_change(ctx, program)? {
        Some(at) => book(ctx, id, at).await,
        None => end(
            ctx,
            id,
            format!(
                "program {program}'s change for block {height} did not land, and none is scheduled now"
            ),
        ),
    }
}

fn changed(
    program: &str,
    before: Option<&Entry>,
    now: Option<&Entry>,
    height: u64,
) -> Option<String> {
    let code = |entry: &Entry| abi::hex(entry.code.digest());
    match (before, now) {
        (None, None) => None,
        (Some(before), Some(now)) if before == now => None,
        (None, Some(now)) => Some(format!(
            "program {program} was set at block {height}, code {}",
            code(now)
        )),
        (Some(_), None) => Some(format!("program {program} was removed at block {height}")),
        (Some(before), Some(now)) if before.code == now.code => Some(format!(
            "program {program} was set again at block {height} with new params, code {}",
            code(now)
        )),
        (Some(before), Some(now)) => Some(format!(
            "program {program}'s code changed at block {height} from {} to {}",
            code(before),
            code(now)
        )),
    }
}

fn programs_role(ctx: &QueryCtx) -> Result<String, Error> {
    Ok(ctx.env().role(abi::role::PROGRAMS)?.clone())
}

fn running(ctx: &QueryCtx, program: &str) -> Result<Option<Entry>, Error> {
    let role = programs_role(ctx)?;
    let asked = programs::Query::Program(program.to_owned());
    match ctx.query(role.clone(), &asked)? {
        programs::Reply::Program { entry, .. } => Ok(entry),
        other => Err(unexpected_reply(&role, "Program", &other)),
    }
}

fn changes_code(change: &programs::Change, program: &str) -> bool {
    match change {
        programs::Change::Set(entry) | programs::Change::Publish { entry, .. } => {
            entry.program == program
        }
        programs::Change::Remove(removed) => removed == program,
        programs::Change::SetView(_)
        | programs::Change::RemoveView(_)
        | programs::Change::SetHook(_)
        | programs::Change::RemoveHook(_) => false,
    }
}

fn next_change(ctx: &QueryCtx, program: &str) -> Result<Option<u64>, Error> {
    let role = programs_role(ctx)?;
    let mut page = PageRequest::default();
    loop {
        let asked = programs::Query::Scheduled { page };
        let pending = match ctx.query(role.clone(), &asked)? {
            programs::Reply::Scheduled(pending) => pending,
            other => return Err(unexpected_reply(&role, "Scheduled", &other)),
        };
        let found = pending
            .items
            .iter()
            .find(|pending| changes_code(&pending.change, program));
        if let Some(found) = found {
            return Ok(Some(found.height));
        }
        let Some(next) = pending.next else {
            return Ok(None);
        };
        page = PageRequest {
            after: Some(next),
            limit: None,
        };
    }
}

fn open_change(ctx: &QueryCtx, repo: &str, n: u64) -> Result<Option<String>, Error> {
    let asked = forge::Query::Change {
        repo: repo.to_owned(),
        n,
        page: PageRequest::first(1),
    };
    let change = match ctx.query(forge::PROGRAM, &asked)? {
        forge::Reply::Change { change, .. } => change,
        other => return Err(unexpected_reply(forge::PROGRAM, "Change", &other)),
    };
    let ended = match change.state {
        forge::ChangeState::Open => {
            return Ok(match change.from {
                forge::Revision::Ref(name) => Some(String::from_utf8_lossy(&name).into_owned()),
                forge::Revision::Oid(_) => None,
            });
        }
        forge::ChangeState::Closed => "closed",
        forge::ChangeState::Merged => "merged",
    };
    Err(wrong_state(format!(
        "change {n} in {repo} is already {ended}: read the change's final state or wake on an open change"
    )))
}

fn known_repo(ctx: &QueryCtx, repo: &str) -> Result<(), Error> {
    let asked = forge::Query::Repo {
        repo: repo.to_owned(),
    };
    match ctx.query(forge::PROGRAM, &asked)? {
        forge::Reply::Repo { .. } => Ok(()),
        other => Err(unexpected_reply(forge::PROGRAM, "Repo", &other)),
    }
}

fn watch_ref(ctx: &ExecCtx, id: u64, repo: &str, reference: String) {
    let key = (repo.to_owned(), reference);
    REF_WAKES.insert(ctx, &(key.clone(), id));
    WAKE_REFS.put(ctx, &id, &key);
}

fn forge_wakes(ctx: &ExecCtx, opened: bool) -> Result<(), Error> {
    let before = FORGE_COUNT.get(ctx)?.unwrap_or(0);
    let after = match opened {
        true => before + 1,
        false => before.saturating_sub(1),
    };
    FORGE_COUNT.put(ctx, &after);
    let deliver = match (before, after) {
        (0, 1) => forged_as(),
        (1, 0) => Vec::new(),
        _ => return Ok(()),
    };
    let watch = forge::Op::Watch { deliver };
    ctx.emit(forge::PROGRAM, &watch, Wanted::None);
    Ok(())
}

fn who(by: Option<&Principal>) -> String {
    match by {
        Some(Principal::Account(number)) => format!("account {number}"),
        Some(Principal::Root) => "the network".to_owned(),
        None => "someone".to_owned(),
    }
}

fn moved_words((old, new): (&Option<String>, &Option<String>), by: Option<&Principal>) -> String {
    let by = who(by);
    match (old, new) {
        (Some(old), Some(new)) => format!("moved from {old} to {new} by {by}"),
        (None, Some(new)) => format!("was made at {new} by {by}"),
        (_, None) => format!("was deleted by {by}"),
    }
}

fn settled_words(until: Until, ending: Until, words: String) -> String {
    match (until, ending) {
        (Until::Merged, Until::Closed) => format!("{words}, so it will not be merged"),
        (Until::Closed, Until::Merged) => format!("{words}, so it will not be closed"),
        _ => words,
    }
}

pub(crate) fn forged(ctx: &ExecCtx, event: forge::Watched) -> Result<(), Error> {
    ctx.env().sent_by(forge::PROGRAM)?;
    match event {
        forge::Watched::Moved {
            repo,
            reference,
            old,
            new,
            by,
        } => {
            let reference = String::from_utf8_lossy(&reference).into_owned();
            let moved = moved_words((&old, &new), by.as_ref());
            let key = Key::Ref {
                repo: repo.clone(),
                reference: reference.clone(),
            };
            let fired = Fired::Moved {
                repo,
                reference,
                moved,
            };
            fire(ctx, key, fired)
        }
        forge::Watched::Opened { .. } | forge::Watched::Previewed { .. } => Ok(()),
        forge::Watched::Closed { repo, n, by } => {
            let how = format!("was closed by {}", who(Some(&by)));
            ended_change(ctx, repo, n, how, Until::Closed)
        }
        forge::Watched::Merged { repo, n, oid, by } => {
            let how = format!("was merged by {} as {oid}", who(Some(&by)));
            ended_change(ctx, repo, n, how, Until::Merged)
        }
    }
}

fn ended_change(
    ctx: &ExecCtx,
    repo: String,
    n: u64,
    how: String,
    ending: Until,
) -> Result<(), Error> {
    let key = Key::Change {
        repo: repo.clone(),
        n,
    };
    let fired = Fired::Ended {
        repo,
        n,
        how,
        ending,
    };
    fire(ctx, key, fired)
}

fn node_at(ctx: &QueryCtx, path: &str, at: Option<BlobId>) -> Result<Option<files::Node>, Error> {
    let asked = files::Query::Stat {
        path: path.to_owned(),
        at,
    };
    match ctx.query(files::PROGRAM, &asked)? {
        files::Reply::Stat { entry, .. } => Ok(entry.map(|entry| entry.node)),
        other => Err(unexpected_reply(files::PROGRAM, "Stat", &other)),
    }
}

fn files_wakes(ctx: &ExecCtx, opened: bool) -> Result<(), Error> {
    let before = FILE_COUNT.get(ctx)?.unwrap_or(0);
    let after = match opened {
        true => before + 1,
        false => before.saturating_sub(1),
    };
    FILE_COUNT.put(ctx, &after);
    let deliver = match (before, after) {
        (0, 1) => filed_as(),
        (1, 0) => Vec::new(),
        _ => return Ok(()),
    };
    let watch = files::Op::Watch { deliver };
    ctx.emit(files::PROGRAM, &watch, Wanted::None);
    Ok(())
}

pub(crate) fn filed(ctx: &ExecCtx, row: files::SnapshotRow) -> Result<(), Error> {
    ctx.env().sent_by(files::PROGRAM)?;
    let mut touched = BTreeSet::new();
    for path in changed_paths(ctx, &row)? {
        touched.extend(ancestors(&path));
    }
    for path in touched {
        fire(ctx, Key::Path { path }, Fired::Filed(row.clone()))?;
    }
    Ok(())
}

fn changed_paths(ctx: &QueryCtx, row: &files::SnapshotRow) -> Result<Vec<String>, Error> {
    let Some(parent) = row.parent else {
        let asked = files::Query::Find {
            prefix: String::new(),
            at: Some(row.id),
            page: PageRequest::default(),
        };
        return match ctx.query(files::PROGRAM, &asked)? {
            files::Reply::Listing { entries, .. } => {
                Ok(entries.items.into_iter().map(|entry| entry.path).collect())
            }
            other => Err(unexpected_reply(files::PROGRAM, "Find", &other)),
        };
    };
    let asked = files::Query::Diff {
        from: parent,
        to: row.id,
        prefix: String::new(),
    };
    match ctx.query(files::PROGRAM, &asked)? {
        files::Reply::Diff(entries) => Ok(entries.into_iter().map(|entry| entry.path).collect()),
        other => Err(unexpected_reply(files::PROGRAM, "Diff", &other)),
    }
}

fn ancestors(path: &str) -> Vec<String> {
    let mut found = vec!["/".to_owned()];
    let mut at = String::new();
    for segment in path.split('/').filter(|segment| !segment.is_empty()) {
        at = format!("{at}/{segment}");
        found.push(at.clone());
    }
    found
}

fn filed_words(watch: &FileWatch, there: bool, row: &files::SnapshotRow) -> String {
    let how = match (watch.node.is_some(), there) {
        (false, _) => "was made",
        (true, false) => "was removed",
        (true, true) => "changed",
    };
    let snapshot = abi::hex(row.id.digest());
    let by = who(Some(&row.author));
    let said = match row.message.trim() {
        "" => String::new(),
        message => format!(" (\"{message}\")"),
    };
    format!(
        "{} {how} in files snapshot {snapshot} by {by}{said}",
        watch.path
    )
}

fn fire(ctx: &ExecCtx, key: Key, fired: Fired) -> Result<(), Error> {
    let at = abi::encode(&key);
    let lo = FIRED.get(ctx, &at)?.unwrap_or(0);
    let hi = LAST.get(ctx)?.unwrap_or(0);
    if first_in(ctx, &key, lo, hi)?.is_none() {
        return Ok(());
    }
    FIRED.put(ctx, &at, &hi);
    fan(ctx, Firing { key, fired, lo, hi })
}

fn first_in(ctx: &QueryCtx, key: &Key, lo: u64, hi: u64) -> Result<Option<u64>, Error> {
    let found = match key {
        Key::Ref { repo, reference } => {
            let head = (repo.clone(), reference.clone());
            let range = REF_WAKES
                .prefix_of(&head)
                .after(REF_WAKES.key(&(head, lo)))
                .limit(1);
            REF_WAKES.scan(ctx, range)?.first().map(|(_, id)| *id)
        }
        Key::Change { repo, n } => {
            let head = (repo.clone(), *n);
            let range = CHANGE_WAKES
                .prefix_of(&head)
                .after(CHANGE_WAKES.key(&(head, lo)))
                .limit(1);
            CHANGE_WAKES.scan(ctx, range)?.first().map(|(_, id)| *id)
        }
        Key::Path { path } => {
            let range = PATH_WAKES
                .prefix_of(path)
                .after(PATH_WAKES.key(&(path.clone(), lo)))
                .limit(1);
            PATH_WAKES.scan(ctx, range)?.first().map(|(_, id)| *id)
        }
    };
    Ok(found.filter(|id| *id <= hi))
}

fn fired_words(ctx: &QueryCtx, row: &WakeRow, fired: &Fired) -> Result<String, Error> {
    Ok(match fired {
        Fired::Moved {
            repo,
            reference,
            moved,
        } => match row.event {
            WaitFor::Change { n, .. } => {
                format!("change {n} in {repo}: its source {reference} {moved}")
            }
            _ => format!("{reference} in {repo} {moved}"),
        },
        Fired::Ended {
            repo,
            n,
            how,
            ending,
        } => {
            let words = format!("change {n} in {repo} {how}");
            match row.event {
                WaitFor::Settled { until, .. } => settled_words(until, *ending, words),
                _ => words,
            }
        }
        Fired::Filed(snapshot) => {
            let Some(watch) = FILE_WAKES.get(ctx, &row.id)? else {
                let key = FILE_WAKES.key(&row.id);
                return Err(corrupt(
                    "file wake",
                    &key,
                    "an open file wake keeps no path",
                ));
            };
            let there = node_at(ctx, &watch.path, Some(snapshot.id))?.is_some();
            filed_words(&watch, there, snapshot)
        }
    })
}

fn fan(ctx: &ExecCtx, firing: Firing) -> Result<(), Error> {
    let Some(id) = first_in(ctx, &firing.key, firing.lo, firing.hi)? else {
        return Ok(());
    };
    if let Some(row) = open(ctx, id)? {
        let what = fired_words(ctx, &row, &firing.fired)?;
        end(ctx, id, what)?;
    }
    let mid = id + (firing.hi - id) / 2;
    for (lo, hi) in [(id, mid), (mid, firing.hi)] {
        if hi > lo {
            let half = Firing {
                lo,
                hi,
                ..firing.clone()
            };
            next_block(ctx, &Op::Fan { firing: half })?;
        }
    }
    Ok(())
}

fn next_block(ctx: &ExecCtx, op: &Op) -> Result<(), Error> {
    let scheduler = ctx.env().role(abi::role::SCHEDULER)?.clone();
    let schedule = abi::role::scheduler::Op::Schedule {
        height: ctx.env().height.saturating_add(1),
        target: PROGRAM.into(),
        payload: abi::encode(op),
    };
    ctx.emit(scheduler, &schedule, Wanted::None);
    Ok(())
}

pub(crate) fn fanned(ctx: &ExecCtx, firing: Firing) -> Result<(), Error> {
    if its_own_run(ctx).is_none() {
        return Err(unauthorized(format!(
            "a wake's event is carried on only as {PROGRAM}'s own scheduled entry"
        )));
    }
    fan(ctx, firing)
}

fn named_pod(name: &str) -> Result<(), Error> {
    let fits = !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-.".contains(&byte)
        });
    match fits {
        true => Ok(()),
        false => Err(invalid(format!(
            "{name:?} is no pod's name: lowercase letters, digits, - and ."
        ))),
    }
}

async fn ask_pods(ctx: &Ctx, target: String, delivery: Delivery) -> Result<http::Response, Error> {
    let head = http::Head {
        method: http::Method::Get,
        target,
        headers: Vec::new(),
    };
    let request = Request::Http {
        service: PODS_SERVICE.to_owned(),
        head,
        body: Vec::new(),
    };
    let op = abi::role::egress::Op::Request {
        request,
        delivery,
        wait: None,
    };
    let egress = ctx.env().role(abi::role::EGRESS)?.clone();
    let answer = ctx.call_raw(egress, abi::encode(&op)).await?;
    decoded(PROGRAM, "http::Response", &answer)
}

fn refused_by_pods(answer: &http::Response) -> String {
    let error = serde_json::from_slice::<Value>(&answer.body)
        .ok()
        .and_then(|body| body["error"].as_str().map(str::to_owned))
        .unwrap_or_else(|| String::from_utf8_lossy(&answer.body).into_owned());
    format!("the pods service answered {}: {error}", answer.status)
}

fn alive_every(block_ms: u64) -> Result<u64, String> {
    let lasts = DEFAULT_WAIT.saturating_mul(block_ms);
    if lasts < 2000 {
        return Err(format!(
            "a request lasts {lasts} ms ({DEFAULT_WAIT} blocks of {block_ms} ms), under the 2 s two whole-second beats need"
        ));
    }
    Ok(lasts / 2 / 1000)
}

async fn watch_from_next_block(ctx: &Ctx, id: u64) -> Result<(), Error> {
    let height = ctx.env().height;
    let token = PODS
        .get(ctx, &id)?
        .map_or(0, |watch| watch.token.wrapping_add(1));
    let entry = ctx
        .schedule(height + 1, PROGRAM, &Op::PodWait { id })
        .await?;
    let watch = PodWatch {
        height,
        time: ctx.env().time,
        entry: Some(entry),
        token,
    };
    PODS.put(ctx, &id, &watch);
    Ok(())
}

pub(crate) async fn pod_wait(ctx: &Ctx, id: u64) -> Result<(), Error> {
    let Some(row) = open(ctx, id)? else {
        return Err(wrong_state(format!("wake {id} is not open")));
    };
    let WaitFor::Pod { name } = &row.event else {
        return Err(invalid(format!("wake {id} waits for no pod")));
    };
    let Some(watch) = PODS.get(ctx, &id)? else {
        let key = PODS.key(&id);
        return Err(corrupt("pod wake", &key, "an open pod wake has no watch"));
    };
    match its_own_run(ctx) {
        Some(entry) if watch.entry == Some(entry) => {}
        Some(_) => return Ok(()),
        None => {
            if let Some(entry) = watch.entry {
                ctx.cancel(entry).await?;
            }
            return watch_from_next_block(ctx, id).await;
        }
    }
    let env = ctx.env();
    let blocks = env.height.saturating_sub(watch.height).max(1);
    let block_ms = env.time.saturating_sub(watch.time) / blocks;
    let streaming = PodWatch {
        entry: None,
        ..watch
    };
    PODS.put(ctx, &id, &streaming);
    let read = ask_pods(ctx, format!("/v1/pods/{name}"), Delivery::Whole).await;
    if !current(ctx, id, watch.token)? {
        return Ok(());
    }
    if let Some(over) = unwatchable(name, read) {
        return end(ctx, id, over);
    }
    let every = match alive_every(block_ms) {
        Ok(every) => every,
        Err(why) => {
            return end(
                ctx,
                id,
                format!("{PROGRAM} can't wait on pod {name}: {why}"),
            );
        }
    };
    let target = format!("/v1/pods/{name}/wait?every={every}");
    let events = Delivery::Events {
        deliver: pod_event_as(id),
    };
    let streamed = ask_pods(ctx, target, events).await;
    if !current(ctx, id, watch.token)? {
        return Ok(());
    }
    match streamed {
        Ok(answer) if answer.status != 200 => {
            let over = unwatchable(name, Ok(answer)).unwrap_or_default();
            end(ctx, id, over)
        }
        Ok(_) | Err(_) => watch_from_next_block(ctx, id).await,
    }
}

fn current(ctx: &QueryCtx, id: u64, token: u64) -> Result<bool, Error> {
    let open = open(ctx, id)?.is_some();
    let ours = PODS
        .get(ctx, &id)?
        .is_some_and(|watch| watch.token == token && watch.entry.is_none());
    Ok(open && ours)
}

fn unwatchable(name: &str, answer: Result<http::Response, Error>) -> Option<String> {
    match answer {
        Ok(answer) if answer.status == 200 => None,
        Ok(answer) if answer.status == 404 => Some(format!("the pods service knows no pod {name}")),
        Ok(answer) => Some(format!(
            "{PROGRAM} can't wait on pod {name}: {}",
            refused_by_pods(&answer)
        )),
        Err(error) => Some(format!(
            "{PROGRAM} couldn't reach the pods service to wait on pod {name}: {}: {}",
            error.code, error.message
        )),
    }
}

pub(crate) fn pod_event(ctx: &ExecCtx, id: u64, event: Event) -> Result<(), Error> {
    let egress = ctx.env().role(abi::role::EGRESS)?.clone();
    let from_egress = ctx
        .env()
        .sending_program()
        .is_ok_and(|program| program == egress);
    if !from_egress {
        return Err(unauthorized(format!(
            "only the program bound to the egress role hands {PROGRAM} a pod's wait events"
        )));
    }
    let Some(row) = open(ctx, id)? else {
        return Ok(());
    };
    let WaitFor::Pod { name } = &row.event else {
        return Ok(());
    };
    let Some(line) = ended_line(&event.data, name) else {
        return Ok(());
    };
    end(ctx, id, line)
}

fn ended_line(raw: &[u8], name: &str) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    let is_ended = text.lines().any(|line| line.trim_end() == "event: ended");
    if !is_ended {
        return None;
    }
    let data = text
        .lines()
        .find_map(|line| line.strip_prefix("data:"))
        .and_then(|data| serde_json::from_str::<Value>(data.trim()).ok());
    let line = data.as_ref().and_then(|data| data["line"].as_str());
    Some(match line {
        Some(line) => line.to_owned(),
        None => format!("pod {name} ended"),
    })
}

pub(crate) fn wakes(
    ctx: &QueryCtx,
    account: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<WakeRow>, Error> {
    BY_ACCOUNT
        .range_of(ctx, &account, page)?
        .try_map(|(_, id)| {
            WAKES.get(ctx, &id)?.ok_or_else(|| {
                corrupt("by account", &id.to_be_bytes(), "names a wake that is gone")
            })
        })
}

pub(crate) fn wake_row(ctx: &QueryCtx, id: u64) -> Result<Option<WakeRow>, Error> {
    WAKES.get(ctx, &id)
}
