//! What each op checks and writes: forge's events start runs, a run queues a check per job and
//! offers it to the runners whose labels cover it, and a check goes from queued to running to
//! how its runner says it ended.
use ::program::{
    Error, ExecCtx, MessageId, Outcome, Principal, QueryCtx, Reply, code, corrupt, invalid,
    not_found, unauthorized, unexpected_reply, wrong_state,
};
use forge::{ChangeState, CheckState, Content, EntryKind, Revision, Watched};
use store::Item;

use crate::state::{
    ASKS, BY_COMMIT, BY_EVENT, BY_RUN, CHECKS, KEEP_STARTED, LAST_CHECK, LAST_RUN, LINES, MET,
    NAMED, NEEDS, OFFERING, OUTPUTS, PREVIEWING, QUEUED, RUNNERS, RUNS, SOURCES,
};
use crate::workflow::{self, Planned};
use crate::{
    Asks, CheckRow, Event, Need, Offer, Output, OutputsRow, PageRequest, PageResponse, RunRow,
    RunnerRow, State, WORKFLOWS,
};

struct Started<'a> {
    repo: &'a str,
    commit: &'a str,
    event: Event,
    by: Option<Principal>,
    payload: Option<Vec<u8>>,
}

const DISPATCH_PAYLOAD: &str = "DISPATCH_PAYLOAD";

pub(crate) fn watched(ctx: &ExecCtx, event: Watched) -> Result<(), Error> {
    ctx.env().sent_by(forge::PROGRAM)?;
    match event {
        Watched::Moved { reference, .. } if reference.starts_with(b"refs/changes/") => Ok(()),
        Watched::Moved {
            repo,
            reference,
            new: Some(commit),
            by,
            ..
        } => {
            let source = (repo.clone(), reference.clone());
            if let Some(n) = SOURCES.get(ctx, &source)? {
                match still_open(ctx, &repo, n)? {
                    true => schedule_preview_next_block(ctx, &repo, n)?,
                    false => SOURCES.remove(ctx, &source),
                }
            }
            let why = format!(
                "superseded by a push of {} to {}",
                short(&commit),
                branch(&reference)
            );
            supersede_earlier_runs(ctx, &repo, &reference, &why)?;
            supersede_previews_on_an_old_tip(ctx, &repo, &reference, &commit)?;
            start_matching_workflows(ctx, &started(&repo, &commit, Event::Push { reference }, by))
        }
        Watched::Moved { new: None, .. } => Ok(()),
        Watched::Closed { repo, n, .. } => {
            let why = format!("change #{n} closed");
            supersede_earlier_runs(ctx, &repo, preview_ref(n).as_bytes(), &why)
        }
        Watched::Merged { repo, n, .. } => {
            let why = format!("change #{n} merged");
            supersede_earlier_runs(ctx, &repo, preview_ref(n).as_bytes(), &why)
        }
        Watched::Opened { repo, n, from, .. } => {
            if let Revision::Ref(reference) = from {
                SOURCES.put(ctx, &(repo.clone(), reference), &n);
            }
            schedule_preview_next_block(ctx, &repo, n)
        }
        Watched::Previewed {
            repo, n, merge, by, ..
        } => {
            adopt_untracked_change(ctx, &repo, n)?;
            let why = format!("superseded by Preview {} of #{n}", short(&merge));
            supersede_earlier_runs(ctx, &repo, preview_ref(n).as_bytes(), &why)?;
            start_matching_workflows(ctx, &started(&repo, &merge, Event::Preview { n }, Some(by)))
        }
        Watched::Reviewed { .. } | Watched::LeftQueue { .. } => Ok(()),
        Watched::Dispatched {
            repo,
            event_type,
            payload,
            commit,
            by,
        } => dispatched(ctx, &repo, event_type, payload, &commit, by),
    }
}

fn dispatched(
    ctx: &ExecCtx,
    repo: &str,
    event_type: String,
    payload: Vec<u8>,
    commit: &str,
    by: Principal,
) -> Result<(), Error> {
    let event = Event::Dispatch {
        event_type: event_type.clone(),
    };
    let why = format!(
        "superseded by a dispatch of {event_type} at {}",
        short(commit)
    );
    supersede_earlier_runs(ctx, repo, &dispatch_line(&event_type), &why)?;
    let started = Started {
        payload: Some(payload),
        ..started(repo, commit, event, Some(by))
    };
    start_matching_workflows(ctx, &started)
}

fn schedule_preview_next_block(ctx: &ExecCtx, repo: &str, n: u64) -> Result<(), Error> {
    let env = ctx.env();
    let scheduler = env.role(abi::role::SCHEDULER)?.clone();
    let height = env
        .height
        .checked_add(1)
        .ok_or_else(|| Error::new(code::EXHAUSTED, "the height is exhausted"))?;
    let op = crate::Op::Preview {
        repo: repo.to_owned(),
        n,
    };
    let schedule = abi::role::scheduler::Op::Schedule {
        height,
        target: env.program.clone(),
        payload: abi::encode(&op),
    };
    ctx.emit(scheduler, &schedule, Reply::None);
    Ok(())
}

pub(crate) fn previewing(ctx: &ExecCtx, repo: &str, n: u64) -> Result<(), Error> {
    let op = forge::Op::Preview {
        repo: repo.to_owned(),
        n,
    };
    let sent = ctx.emit(forge::PROGRAM, &op, Reply::Wanted);
    PREVIEWING.put(ctx, &sent.seq, &(repo.to_owned(), n));
    Ok(())
}

fn adopt_untracked_change(ctx: &ExecCtx, repo: &str, n: u64) -> Result<(), Error> {
    let asked = forge::Query::Change {
        repo: repo.to_owned(),
        n,
        page: PageRequest::first(1),
    };
    let change = match ctx.query::<forge::Query, forge::Reply>(forge::PROGRAM, &asked)? {
        forge::Reply::Change { change, .. } => change,
        other => return Err(unexpected_reply(forge::PROGRAM, "Change", &other)),
    };
    if let Revision::Ref(reference) = change.from {
        SOURCES.put(ctx, &(repo.to_owned(), reference), &n);
    }
    Ok(())
}

pub(crate) fn replied(ctx: &ExecCtx, id: &MessageId, outcome: &Outcome) -> Result<(), Error> {
    if let Some((check, runner)) = OFFERING.get(ctx, &id.seq)? {
        OFFERING.remove(ctx, &id.seq);
        if let Outcome::Rejected(refused) = outcome {
            refused_offer(ctx, check, &runner, &refused.message)?;
        }
        return Ok(());
    }
    let Some((repo, n)) = PREVIEWING.get(ctx, &id.seq)? else {
        return Ok(());
    };
    PREVIEWING.remove(ctx, &id.seq);
    let Outcome::Rejected(refused) = outcome else {
        return Ok(());
    };
    let Some(head) = open_change_head(ctx, &repo, n)? else {
        return Ok(());
    };
    let why = format!("change {n} does not preview: {}", refused.message);
    let preview = preview_ref(n);
    let started = started(&repo, &head, Event::Preview { n }, None);
    record(ctx, &started, &preview, Named::default(), Some(why))?;
    Ok(())
}

fn refused_offer(ctx: &ExecCtx, id: u64, runner: &str, why: &str) -> Result<(), Error> {
    let pending = OFFERING
        .all(ctx)?
        .into_iter()
        .any(|(_, (check, _))| check == id);
    let mut check = load(ctx, id)?;
    if pending || check.state != State::Queued {
        return Ok(());
    }
    check.state = State::Errored;
    check.finished = Some(ctx.env().height);
    check.summary = format!("{runner} refused the offer: {why}");
    QUEUED.remove(ctx, &id);
    CHECKS.put(ctx, &id, &check);
    report_commit_status(ctx, &check);
    release(ctx, check.run)?;
    let run = RUNS
        .get(ctx, &check.run)?
        .ok_or_else(|| corrupt("r/", &check.run.to_be_bytes(), "a check's run is missing"))?;
    notify_failed(ctx, &run, &check);
    Ok(())
}

fn open_change_head(ctx: &QueryCtx, repo: &str, n: u64) -> Result<Option<String>, Error> {
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
        }) => Ok(source_head.filter(|_| change.state == ChangeState::Open)),
        Ok(other) => Err(unexpected_reply(forge::PROGRAM, "Change", &other)),
        Err(refused) if refused.code == code::NOT_FOUND => Ok(None),
        Err(refused) => Err(refused),
    }
}

fn report_commit_status(ctx: &ExecCtx, check: &CheckRow) {
    let state = match check.state {
        State::Queued | State::Running | State::Waiting => CheckState::Pending,
        State::Passed => CheckState::Passed,
        State::Failed => CheckState::Failed,
        // forge has no word for a superseded or skipped check, and a merge reads only the newest
        // Preview's
        State::Errored | State::Superseded | State::Skipped => CheckState::Errored,
    };
    let op = forge::Op::Status {
        repo: check.repo.clone(),
        commit: check.commit.clone(),
        context: format!("{}#{}", check.workflow, check.job.id),
        state,
        url: check.log.clone(),
    };
    ctx.emit(forge::PROGRAM, &op, Reply::Wanted);
}

fn started<'a>(repo: &'a str, commit: &'a str, event: Event, by: Option<Principal>) -> Started<'a> {
    Started {
        repo,
        commit,
        event,
        by,
        payload: None,
    }
}

pub(crate) fn trigger(ctx: &ExecCtx, repo: &str, commit: &str) -> Result<(), Error> {
    let by = Principal::Account(ctx.sender_account()?);
    start_matching_workflows(ctx, &started(repo, commit, Event::Manual, Some(by)))
}

fn still_open(ctx: &QueryCtx, repo: &str, n: u64) -> Result<bool, Error> {
    let asked = forge::Query::Change {
        repo: repo.to_owned(),
        n,
        page: PageRequest::first(1),
    };
    match ctx.query::<forge::Query, forge::Reply>(forge::PROGRAM, &asked) {
        Ok(forge::Reply::Change { change, .. }) => Ok(change.state == ChangeState::Open),
        Ok(other) => Err(unexpected_reply(forge::PROGRAM, "Change", &other)),
        Err(refused) if refused.code == code::NOT_FOUND => Ok(false),
        Err(refused) => Err(refused),
    }
}

fn start_matching_workflows(ctx: &ExecCtx, started: &Started) -> Result<(), Error> {
    let files = match workflow_files(ctx, started.repo, started.commit) {
        Ok(files) => files,
        Err(refused) => {
            record(
                ctx,
                started,
                WORKFLOWS,
                Named::default(),
                Some(refused.message),
            )?;
            return Ok(());
        }
    };
    for (file, text) in files {
        match text.and_then(|text| workflow::parse(&text)) {
            Err(error) => {
                record(ctx, started, &file, Named::default(), Some(error))?;
            }
            Ok(workflow) if workflow.starts_on(&started.event) => {
                let named = Named {
                    name: workflow.name,
                    notify: workflow.notify,
                    keeps_started: workflow.keeps_started,
                };
                let handed = match &started.payload {
                    None => Ok(None),
                    Some(payload) => String::from_utf8(payload.clone()).map(Some),
                };
                let Ok(handed) = handed else {
                    let why = format!(
                        "the dispatch's payload is not UTF-8 text, which {DISPATCH_PAYLOAD} hands its jobs"
                    );
                    record(ctx, started, &file, named, Some(why))?;
                    continue;
                };
                let run = record(ctx, started, &file, named, None)?;
                for mut planned in workflow.jobs {
                    if let Some(text) = &handed {
                        planned.job.env.retain(|(name, _)| name != DISPATCH_PAYLOAD);
                        planned
                            .job
                            .env
                            .push((DISPATCH_PAYLOAD.to_owned(), text.clone()));
                    }
                    queue(ctx, &run, planned, 1)?;
                }
            }
            Ok(_) => {}
        }
    }
    Ok(())
}

type Files = Vec<(String, Result<String, String>)>;

fn workflow_files(ctx: &QueryCtx, repo: &str, commit: &str) -> Result<Files, Error> {
    let mut files = Vec::new();
    let mut after = None;
    loop {
        let asked = forge::Query::Tree {
            repo: repo.to_owned(),
            at: commit.to_owned(),
            path: WORKFLOWS.as_bytes().to_vec(),
            page: PageRequest { after, limit: None },
        };
        let page = match ctx.query::<forge::Query, forge::Reply>(forge::PROGRAM, &asked) {
            Ok(forge::Reply::Tree { page, .. }) => page,
            Ok(other) => return Err(unexpected_reply(forge::PROGRAM, "Tree", &other)),
            Err(refused) if refused.code == code::NOT_FOUND => return Ok(files),
            Err(refused) => return Err(refused),
        };
        for entry in page.items {
            let name = String::from_utf8_lossy(&entry.name).into_owned();
            let file = matches!(entry.kind, EntryKind::File | EntryKind::Executable);
            if file && name.ends_with(".toml") {
                let text = text_of(ctx, repo, &entry.oid)?;
                files.push((name, text));
            }
        }
        match page.next {
            Some(next) => after = Some(next),
            None => return Ok(files),
        }
    }
}

fn text_of(ctx: &QueryCtx, repo: &str, oid: &str) -> Result<Result<String, String>, Error> {
    let asked = forge::Query::Blob {
        repo: repo.to_owned(),
        oid: oid.to_owned(),
        range: None,
    };
    let blob = match ctx.query::<forge::Query, forge::Reply>(forge::PROGRAM, &asked)? {
        forge::Reply::Blob { blob, .. } => blob,
        other => return Err(unexpected_reply(forge::PROGRAM, "Blob", &other)),
    };
    Ok(match blob.content {
        Content::Text => String::from_utf8(blob.bytes).map_err(|_| "it is not UTF-8".to_owned()),
        Content::Oversize => Err(format!(
            "it is {} bytes, more than forge answers whole (its Bounds.blob_bytes)",
            blob.size
        )),
        Content::Binary | Content::Gitlink => Err("it is not text".to_owned()),
    })
}

#[derive(Default)]
struct Named {
    name: Option<String>,
    notify: Option<String>,
    keeps_started: bool,
}

fn record(
    ctx: &ExecCtx,
    started: &Started,
    workflow: &str,
    named: Named,
    error: Option<String>,
) -> Result<RunRow, Error> {
    let id = next(ctx, &LAST_RUN)?;
    let run = RunRow {
        id,
        repo: started.repo.to_owned(),
        commit: started.commit.to_owned(),
        workflow: workflow.to_owned(),
        name: named.name,
        event: started.event.clone(),
        by: started.by.clone(),
        height: ctx.env().height,
        error,
        notify: named.notify,
    };
    RUNS.put(ctx, &id, &run);
    BY_EVENT.insert(ctx, &(run.repo.clone(), abi::encode(&run.event), id));
    if let Some(line) = superseding_ref(&run.event) {
        LINES.insert(ctx, &(run.repo.clone(), line, id));
        if named.keeps_started {
            KEEP_STARTED.insert(ctx, &id);
        }
    }
    if let Some(error) = &run.error {
        let op = forge::Op::Status {
            repo: run.repo.clone(),
            commit: run.commit.clone(),
            context: run.workflow.clone(),
            state: CheckState::Errored,
            url: None,
        };
        ctx.emit(forge::PROGRAM, &op, Reply::Wanted);
        notify_errored_run(ctx, &run, error);
    }
    Ok(run)
}

fn next(ctx: &ExecCtx, counter: &Item<u64>) -> Result<u64, Error> {
    let n = counter
        .get(ctx)?
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| Error::new(code::EXHAUSTED, "counter exhausted"))?;
    counter.put(ctx, &n);
    Ok(n)
}

// A check of the job: queued and offered, or, when it needs other jobs, waiting until they pass.
fn queue(ctx: &ExecCtx, run: &RunRow, planned: Planned, attempt: u32) -> Result<(), Error> {
    let Planned {
        job,
        asks,
        needs,
        runners,
    } = planned;
    let id = next(ctx, &LAST_CHECK)?;
    let waits = !needs.is_empty();
    let check = CheckRow {
        id,
        run: run.id,
        repo: run.repo.clone(),
        commit: run.commit.clone(),
        workflow: run.workflow.clone(),
        attempt,
        job,
        state: if waits { State::Waiting } else { State::Queued },
        claimant: None,
        reporter: None,
        queued: ctx.env().height,
        started: None,
        finished: None,
        summary: String::new(),
        log: None,
    };
    CHECKS.put(ctx, &id, &check);
    if asks != Asks::default() {
        ASKS.put(ctx, &id, &asks);
    }
    if !runners.is_empty() {
        NAMED.put(ctx, &id, &runners);
    }
    BY_COMMIT.insert(ctx, &(check.repo.clone(), check.commit.clone(), id));
    BY_RUN.insert(ctx, &(run.id, id));
    if waits {
        NEEDS.put(ctx, &id, &needs);
        report_commit_status(ctx, &check);
        return release(ctx, run.id);
    }
    offered(ctx, &check)
}

// A check nothing holds back: queued, told to forge and offered to every runner.
fn offered(ctx: &ExecCtx, check: &CheckRow) -> Result<(), Error> {
    QUEUED.insert(ctx, &check.id);
    report_commit_status(ctx, check);
    for (_, runner) in RUNNERS.all(ctx)? {
        offer(ctx, &runner, check);
    }
    Ok(())
}

// Each waiting check of the run whose needs have all passed is queued and offered, and one whose
// need ended otherwise is skipped, saying which; again, until no waiting check moves, so a skip
// reaches everything that waits on it.
fn release(ctx: &ExecCtx, run: u64) -> Result<(), Error> {
    loop {
        let checks = of_run(ctx, run)?;
        let mut moved = false;
        for waiting in checks.iter().filter(|check| check.state == State::Waiting) {
            let needs = NEEDS.get(ctx, &waiting.id)?.unwrap_or_default();
            let mut check = waiting.clone();
            match settled(&checks, &needs) {
                Settled::Waiting => continue,
                Settled::Passed(met) => {
                    check.state = State::Queued;
                    CHECKS.put(ctx, &check.id, &check);
                    MET.put(ctx, &check.id, &met);
                    offered(ctx, &check)?;
                }
                Settled::Blocked(why) => {
                    check.state = State::Skipped;
                    check.finished = Some(ctx.env().height);
                    check.summary = why;
                    CHECKS.put(ctx, &check.id, &check);
                    report_commit_status(ctx, &check);
                }
            }
            moved = true;
        }
        if !moved {
            return Ok(());
        }
    }
}

fn of_run(ctx: &QueryCtx, run: u64) -> Result<Vec<CheckRow>, Error> {
    let mut checks = Vec::new();
    for (_, id) in BY_RUN.scan(ctx, BY_RUN.prefix_of(&run))? {
        checks.push(load(ctx, id)?);
    }
    Ok(checks)
}

enum Settled {
    Waiting,
    Passed(Vec<Need>),
    Blocked(String),
}

// Where the jobs a check needs stand, each by its newest attempt in the run.
fn settled(checks: &[CheckRow], needs: &[String]) -> Settled {
    let mut met = Vec::new();
    let mut waiting = false;
    for need in needs {
        let newest = checks
            .iter()
            .filter(|check| &check.job.id == need)
            .max_by_key(|check| check.attempt);
        match newest {
            Some(check) if check.state == State::Passed => met.push(Need {
                job: need.clone(),
                check: Some(check.id),
            }),
            Some(check)
                if !matches!(check.state, State::Queued | State::Running | State::Waiting) =>
            {
                return Settled::Blocked(format!(
                    "needs `{need}`, whose check {} ended {}",
                    check.id,
                    check.state.word()
                ));
            }
            _ => waiting = true,
        }
    }
    match waiting {
        true => Settled::Waiting,
        false => Settled::Passed(met),
    }
}

// A program runner whose labels cover the job hears the check; one that refuses leaves it
// queued (`Program::reply` absorbs the refusal).
fn offer(ctx: &ExecCtx, runner: &RunnerRow, check: &CheckRow) {
    let Some(program) = &runner.program else {
        return;
    };
    let covers = check
        .job
        .runs_on
        .iter()
        .all(|label| runner.labels.contains(label));
    if covers && named(ctx, check.id, runner.account) {
        let mut payload = runner.deliver.clone();
        payload.extend(abi::encode(&Offer { check: check.id }));
        let sent = ctx.emit_raw(program.clone(), payload, Reply::Wanted);
        OFFERING.put(ctx, &sent.seq, &(check.id, program.clone()));
    }
}

pub(crate) fn register(ctx: &ExecCtx, labels: Vec<String>, deliver: Vec<u8>) -> Result<(), Error> {
    let account = ctx.sender_account()?;
    if labels.is_empty() {
        RUNNERS.remove(ctx, &account);
        return Ok(());
    }
    let runner = RunnerRow {
        account,
        labels,
        program: ctx.env().sending_program().ok(),
        deliver,
    };
    RUNNERS.put(ctx, &account, &runner);
    for id in QUEUED.all(ctx)? {
        offer(ctx, &runner, &load(ctx, id)?);
    }
    Ok(())
}

// Whether `account` may run check `id`: any account, unless its job names its runners.
fn named(ctx: &QueryCtx, id: u64, account: u64) -> bool {
    match NAMED.get(ctx, &id) {
        Ok(Some(runners)) => runners.contains(&account),
        Ok(None) => true,
        Err(_) => false,
    }
}

fn unnamed(id: u64) -> Error {
    unauthorized(format!(
        "check {id}'s job names its runners in its workflow, and this account is not one"
    ))
}

pub(crate) fn claim(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    let account = ctx.sender_account()?;
    let mut check = load(ctx, id)?;
    if !named(ctx, id, account) {
        return Err(unnamed(id));
    }
    if check.state != State::Queued {
        return Err(wrong_state(format!(
            "check {id} is {}, not queued: read the check again and claim a queued check",
            check.state.word()
        )));
    }
    check.state = State::Running;
    check.claimant = Some(account);
    check.started = Some(ctx.env().height);
    QUEUED.remove(ctx, &id);
    CHECKS.put(ctx, &id, &check);
    report_commit_status(ctx, &check);
    Ok(())
}

pub(crate) fn report(
    ctx: &ExecCtx,
    id: u64,
    state: State,
    summary: String,
    log: Option<String>,
) -> Result<(), Error> {
    let account = ctx.sender_account()?;
    let mut check = load(ctx, id)?;
    let moot = matches!(state, State::Errored | State::Superseded);
    let unclaimed = check.state == State::Queued && moot;
    if check.state != State::Running && !unclaimed {
        return Err(wrong_state(format!(
            "check {id} is {}, not running: read the check again and report only its current running attempt",
            check.state.word()
        )));
    }
    // a running check's result is its claimant's to say: a reader of results trusts who ran it
    if check.state == State::Running && check.claimant != Some(account) {
        return Err(unauthorized(format!(
            "only the account that claimed check {id} reports how it ended"
        )));
    }
    if unclaimed && !named(ctx, id, account) {
        return Err(unnamed(id));
    }
    if unclaimed {
        QUEUED.remove(ctx, &id);
    }
    if matches!(state, State::Queued | State::Running) {
        return Err(invalid(
            "a report says how a check ended: passed, failed, errored or superseded",
        ));
    }
    check.state = state;
    check.reporter = Some(account);
    check.finished = Some(ctx.env().height);
    check.summary = summary;
    check.log = log;
    CHECKS.put(ctx, &id, &check);
    report_commit_status(ctx, &check);
    if state == State::Passed {
        bring_back_skipped(ctx, &check)?;
    }
    release(ctx, check.run)?;
    if matches!(state, State::Failed | State::Errored) {
        let run = RUNS
            .get(ctx, &check.run)?
            .ok_or_else(|| corrupt("r/", &check.run.to_be_bytes(), "a check's run is missing"))?;
        notify_failed(ctx, &run, &check);
    }
    Ok(())
}

fn supersede_earlier_runs(ctx: &ExecCtx, repo: &str, line: &[u8], why: &str) -> Result<(), Error> {
    let on = (repo.to_owned(), line.to_vec());
    for (_, _, run) in LINES.scan(ctx, LINES.prefix_of(&on))? {
        if KEEP_STARTED.has(ctx, &run) {
            KEEP_STARTED.remove(ctx, &run);
            let claimed = of_run(ctx, run)?
                .iter()
                .any(|check| check.started.is_some());
            if claimed {
                LINES.remove(ctx, &(repo.to_owned(), line.to_vec(), run));
                continue;
            }
        }
        for (_, id) in BY_RUN.scan(ctx, BY_RUN.prefix_of(&run))? {
            let mut check = load(ctx, id)?;
            if !matches!(check.state, State::Queued | State::Running | State::Waiting) {
                continue;
            }
            check.state = State::Superseded;
            check.finished = Some(ctx.env().height);
            check.summary = why.to_owned();
            QUEUED.remove(ctx, &id);
            CHECKS.put(ctx, &id, &check);
            report_commit_status(ctx, &check);
        }
        LINES.remove(ctx, &(repo.to_owned(), line.to_vec(), run));
    }
    Ok(())
}

fn supersede_previews_on_an_old_tip(
    ctx: &ExecCtx,
    repo: &str,
    branch_ref: &[u8],
    tip: &str,
) -> Result<(), Error> {
    if !requires_checks(ctx, repo, branch_ref)? {
        return Ok(());
    }
    let why = format!(
        "superseded: {} moved to {}, past the tip this Preview was built on",
        branch(branch_ref),
        short(tip)
    );
    for (_, line, run) in LINES.scan(ctx, LINES.prefix_of(&repo.to_owned()))? {
        let Some(n) = previewed_change(&line) else {
            continue;
        };
        if change_target(ctx, repo, n)?.as_deref() != Some(branch_ref) {
            continue;
        }
        let Some(row) = RUNS.get(ctx, &run)? else {
            continue;
        };
        if built_on(ctx, repo, n, &row.commit)?.as_deref() == Some(tip) {
            continue;
        }
        for (_, id) in BY_RUN.scan(ctx, BY_RUN.prefix_of(&run))? {
            let mut check = load(ctx, id)?;
            if !matches!(check.state, State::Queued | State::Waiting) {
                continue;
            }
            check.state = State::Superseded;
            check.finished = Some(ctx.env().height);
            check.summary = why.clone();
            QUEUED.remove(ctx, &id);
            CHECKS.put(ctx, &id, &check);
            report_commit_status(ctx, &check);
        }
    }
    Ok(())
}

fn requires_checks(ctx: &QueryCtx, repo: &str, branch_ref: &[u8]) -> Result<bool, Error> {
    let mut page = PageRequest::default();
    loop {
        let asked = forge::Query::Required {
            repo: repo.to_owned(),
            page,
        };
        let answered = match ctx.query::<forge::Query, forge::Reply>(forge::PROGRAM, &asked)? {
            forge::Reply::Required { page, .. } => page,
            other => return Err(unexpected_reply(forge::PROGRAM, "Required", &other)),
        };
        if answered.items.iter().any(|row| row.branch == branch_ref) {
            return Ok(true);
        }
        let Some(next) = answered.next else {
            return Ok(false);
        };
        page = PageRequest {
            after: Some(next),
            limit: None,
        };
    }
}

fn previewed_change(line: &[u8]) -> Option<u64> {
    std::str::from_utf8(line)
        .ok()?
        .strip_prefix("refs/changes/")?
        .strip_suffix("/merge")?
        .parse()
        .ok()
}

fn change_target(ctx: &QueryCtx, repo: &str, n: u64) -> Result<Option<Vec<u8>>, Error> {
    let asked = forge::Query::Change {
        repo: repo.to_owned(),
        n,
        page: PageRequest::first(1),
    };
    match ctx.query::<forge::Query, forge::Reply>(forge::PROGRAM, &asked) {
        Ok(forge::Reply::Change { change, .. }) => {
            let open = change.state == ChangeState::Open;
            Ok(open.then_some(change.into))
        }
        Ok(other) => Err(unexpected_reply(forge::PROGRAM, "Change", &other)),
        Err(refused) if refused.code == code::NOT_FOUND => Ok(None),
        Err(refused) => Err(refused),
    }
}

fn built_on(ctx: &QueryCtx, repo: &str, n: u64, merge: &str) -> Result<Option<String>, Error> {
    let asked = forge::Query::Preview {
        repo: repo.to_owned(),
        n,
    };
    match ctx.query::<forge::Query, forge::Reply>(forge::PROGRAM, &asked)? {
        forge::Reply::Preview { preview, .. } => Ok(preview
            .filter(|preview| preview.merge == merge)
            .map(|preview| preview.into_head)),
        other => Err(unexpected_reply(forge::PROGRAM, "Preview", &other)),
    }
}

fn superseding_ref(event: &Event) -> Option<Vec<u8>> {
    match event {
        Event::Push { reference } => Some(reference.clone()),
        Event::Preview { n } => Some(preview_ref(*n).into_bytes()),
        Event::Dispatch { event_type } => Some(dispatch_line(event_type)),
        Event::Opened { .. } | Event::Updated { .. } | Event::Manual => None,
    }
}

fn dispatch_line(event_type: &str) -> Vec<u8> {
    format!("dispatch/{event_type}").into_bytes()
}

fn preview_ref(n: u64) -> String {
    format!("refs/changes/{n}/merge")
}

fn short(commit: &str) -> &str {
    commit.get(..12).unwrap_or(commit)
}

fn branch(reference: &[u8]) -> String {
    let branch = String::from_utf8_lossy(reference);
    branch
        .strip_prefix("refs/heads/")
        .unwrap_or(&branch)
        .to_owned()
}

struct Red<'a> {
    word: &'a str,
    about: String,
    summary: &'a str,
    log: Option<&'a str>,
    message_id: String,
    group: String,
}

fn notify_failed(ctx: &ExecCtx, run: &RunRow, check: &CheckRow) {
    let red = Red {
        word: check.state.word(),
        about: format!(
            "check {} (job {}, attempt {})",
            check.id, check.job.id, check.attempt
        ),
        summary: &check.summary,
        log: check.log.as_deref(),
        message_id: format!("actions-check-{}", check.id),
        group: format!("check {}", check.id),
    };
    tell_red(ctx, run, red);
}

fn notify_errored_run(ctx: &ExecCtx, run: &RunRow, error: &str) {
    let red = Red {
        word: State::Errored.word(),
        about: format!("run {}, before it queued a check", run.id),
        summary: error,
        log: None,
        message_id: format!("actions-run-{}", run.id),
        group: format!("run {}", run.id),
    };
    tell_red(ctx, run, red);
}

fn tell_red(ctx: &ExecCtx, run: &RunRow, red: Red) {
    let Some(channel) = &run.notify else {
        return;
    };
    let place = match &run.event {
        Event::Push { reference } => branch(reference),
        Event::Dispatch { event_type } => format!("dispatch {event_type}"),
        Event::Opened { .. } | Event::Updated { .. } | Event::Preview { .. } | Event::Manual => {
            return;
        }
    };
    let workflow = run.name.as_deref().unwrap_or(&run.workflow);
    let mut detail = format!("{} at {}: {}", red.word, short(&run.commit), red.about);
    if !red.summary.is_empty() {
        detail.push_str(&format!(". {}", red.summary));
    }
    let concerns = match &run.by {
        Some(Principal::Account(account)) => vec![*account],
        _ => Vec::new(),
    };
    let post = chat::Op::Notify {
        channel_id: channel.to_owned(),
        message_id: red.message_id,
        thread: None,
        notice: chat::Notice {
            kind: "check".into(),
            group: red.group,
            subject: format!("{workflow} on {} {place}", run.repo),
            link: red.log.and_then(log_link),
            status: chat::message::Status::Failed,
            detail,
            concerns,
        },
    };
    ctx.emit(chat::PROGRAM, &post, Reply::Wanted);
}

fn log_link(log: &str) -> Option<String> {
    if log.contains("://") {
        return Some(log.to_owned());
    }
    let path: Vec<&str> = log.split('/').filter(|part| !part.is_empty()).collect();
    ducklink::path("files", &path)
}

pub(crate) fn rerun(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    ctx.sender_account()?;
    let check = load(ctx, id)?;
    if matches!(check.state, State::Queued | State::Waiting) {
        return Err(wrong_state(format!(
            "check {id} is still {}: wait for it to end before rerunning it",
            check.state.word()
        )));
    }
    let run = RUNS
        .get(ctx, &check.run)?
        .ok_or_else(|| corrupt("r/", &check.run.to_be_bytes(), "a check's run is missing"))?;
    queue_again(ctx, &run, &check)
}

// The check's job queued as its next attempt, asking, needing and naming what the check did.
fn queue_again(ctx: &ExecCtx, run: &RunRow, check: &CheckRow) -> Result<(), Error> {
    let attempt = check
        .attempt
        .checked_add(1)
        .ok_or_else(|| Error::new(code::EXHAUSTED, "attempts exhausted"))?;
    let planned = Planned {
        job: check.job.clone(),
        asks: ASKS.get(ctx, &check.id)?.unwrap_or_default(),
        needs: NEEDS.get(ctx, &check.id)?.unwrap_or_default(),
        runners: NAMED.get(ctx, &check.id)?.unwrap_or_default(),
    };
    queue(ctx, run, planned, attempt)
}

// A passed attempt of a job brings back each job of its run whose newest attempt it had skipped:
// a next attempt of each, waiting on the newest attempts of what it needs as a rerun does.
fn bring_back_skipped(ctx: &ExecCtx, passed: &CheckRow) -> Result<(), Error> {
    let checks = of_run(ctx, passed.run)?;
    let newest = |check: &CheckRow| {
        checks
            .iter()
            .all(|other| other.job.id != check.job.id || other.attempt <= check.attempt)
    };
    let mut skipped = Vec::new();
    for check in checks.iter().filter(|check| check.state == State::Skipped) {
        let needs = NEEDS.get(ctx, &check.id)?.unwrap_or_default();
        if newest(check) && needs.contains(&passed.job.id) {
            skipped.push(check);
        }
    }
    if skipped.is_empty() {
        return Ok(());
    }
    let run = RUNS
        .get(ctx, &passed.run)?
        .ok_or_else(|| corrupt("r/", &passed.run.to_be_bytes(), "a check's run is missing"))?;
    for check in skipped {
        queue_again(ctx, &run, check)?;
    }
    Ok(())
}

// The files a running check produced, as a runner says: who said so is kept with them.
pub(crate) fn outputs(ctx: &ExecCtx, id: u64, files: Vec<Output>) -> Result<(), Error> {
    let by = ctx.sender_account()?;
    let check = load(ctx, id)?;
    if check.state != State::Running {
        return Err(wrong_state(format!(
            "check {id} is {}, not running: read the check again and report only its current running attempt",
            check.state.word()
        )));
    }
    if check.claimant != Some(by) {
        return Err(unauthorized(format!(
            "only the account that claimed check {id} records its outputs"
        )));
    }
    OUTPUTS.put(ctx, &id, &OutputsRow { by, files });
    Ok(())
}

fn load(ctx: &QueryCtx, id: u64) -> Result<CheckRow, Error> {
    CHECKS
        .get(ctx, &id)?
        .ok_or_else(|| not_found(format!("no check {id}")))
}

pub(crate) fn check(ctx: &QueryCtx, id: u64) -> Result<Option<CheckRow>, Error> {
    CHECKS.get(ctx, &id)
}

pub(crate) fn asks(ctx: &QueryCtx, id: u64) -> Result<Option<Asks>, Error> {
    ASKS.get(ctx, &id)
}

pub(crate) fn checks(
    ctx: &QueryCtx,
    repo: &str,
    commit: &str,
    page: &PageRequest,
) -> Result<PageResponse<CheckRow>, Error> {
    let commit = (repo.to_owned(), commit.to_owned());
    BY_COMMIT
        .range_of(ctx, &commit, page)?
        .try_map(|(_, _, id)| load(ctx, id))
}

pub(crate) fn queued(ctx: &QueryCtx, page: &PageRequest) -> Result<PageResponse<CheckRow>, Error> {
    QUEUED.range(ctx, page)?.try_map(|id| load(ctx, id))
}

pub(crate) fn run(ctx: &QueryCtx, id: u64) -> Result<Option<RunRow>, Error> {
    RUNS.get(ctx, &id)
}

pub(crate) fn runs(
    ctx: &QueryCtx,
    repo: &str,
    event: &Event,
    page: &PageRequest,
) -> Result<PageResponse<RunRow>, Error> {
    let event = (repo.to_owned(), abi::encode(event));
    BY_EVENT.range_of(ctx, &event, page)?.try_map(|(_, _, id)| {
        RUNS.get(ctx, &id)?
            .ok_or_else(|| corrupt("r/", &id.to_be_bytes(), "an indexed run is missing"))
    })
}

pub(crate) fn runners(
    ctx: &QueryCtx,
    page: &PageRequest,
) -> Result<PageResponse<RunnerRow>, Error> {
    Ok(RUNNERS.range(ctx, page)?.map(|(_, runner)| runner))
}

pub(crate) fn needs(ctx: &QueryCtx, id: u64) -> Result<Vec<Need>, Error> {
    let met = MET.get(ctx, &id)?.unwrap_or_default();
    let needs = NEEDS
        .get(ctx, &id)?
        .unwrap_or_default()
        .into_iter()
        .map(|job| {
            let check = met
                .iter()
                .find(|need| need.job == job)
                .and_then(|need| need.check);
            Need { job, check }
        });
    Ok(needs.collect())
}

pub(crate) fn outputs_of(ctx: &QueryCtx, id: u64) -> Result<Option<OutputsRow>, Error> {
    OUTPUTS.get(ctx, &id)
}
