//! What each op checks and writes: forge's events start runs, a run queues a check per job and
//! offers it to the runners whose labels cover it, and a check goes from queued to running to
//! how its runner says it ended.
use ::program::{
    Error, ExecCtx, Principal, QueryCtx, Reply, code, corrupt, invalid, not_found,
    unexpected_reply, wrong_state,
};
use forge::{ChangeState, Content, EntryKind, Revision, Watched};
use store::Item;

use crate::state::{
    BY_COMMIT, BY_RUN, CHECKS, LAST_CHECK, LAST_RUN, QUEUED, RUNNERS, RUNS, SOURCES,
};
use crate::workflow;
use crate::{
    CheckRow, Event, Job, Offer, PageRequest, PageResponse, RunRow, RunnerRow, State, WORKFLOWS,
};

// What started a run: a repository's commit, the event and who made it.
struct Started<'a> {
    repo: &'a str,
    commit: &'a str,
    event: Event,
    by: Option<Principal>,
}

pub(crate) fn watched(ctx: &ExecCtx, event: Watched) -> Result<(), Error> {
    ctx.env().sent_by(forge::PROGRAM)?;
    match event {
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
                    true => {
                        let event = Event::Updated { n };
                        start(ctx, &started(&repo, &commit, event, by.clone()))?;
                    }
                    false => SOURCES.remove(ctx, &source),
                }
            }
            start(ctx, &started(&repo, &commit, Event::Push { reference }, by))
        }
        Watched::Moved { new: None, .. } => Ok(()),
        Watched::Opened {
            repo,
            n,
            from,
            head,
            by,
            ..
        } => {
            if let Revision::Ref(reference) = from {
                SOURCES.put(ctx, &(repo.clone(), reference), &n);
            }
            start(ctx, &started(&repo, &head, Event::Opened { n }, Some(by)))
        }
    }
}

fn started<'a>(repo: &'a str, commit: &'a str, event: Event, by: Option<Principal>) -> Started<'a> {
    Started {
        repo,
        commit,
        event,
        by,
    }
}

pub(crate) fn trigger(ctx: &ExecCtx, repo: &str, commit: &str) -> Result<(), Error> {
    let by = Principal::Account(ctx.sender_account()?);
    start(ctx, &started(repo, commit, Event::Manual, Some(by)))
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

// Each workflow at the commit: a run of the jobs of each that the event starts, and a run with
// its error for each that does not read.
fn start(ctx: &ExecCtx, started: &Started) -> Result<(), Error> {
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
                };
                let run = record(ctx, started, &file, named, None)?;
                for job in workflow.jobs {
                    queue(ctx, &run, job, 1)?;
                }
            }
            Ok(_) => {}
        }
    }
    Ok(())
}

type Files = Vec<(String, Result<String, String>)>;

// Every `.toml` file directly in the commit's workflows folder, with its text or why it has
// none; a commit with no such folder has no workflows.
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

// What a workflow that reads says of itself.
#[derive(Default)]
struct Named {
    name: Option<String>,
    notify: Option<String>,
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

fn queue(ctx: &ExecCtx, run: &RunRow, job: Job, attempt: u32) -> Result<(), Error> {
    let id = next(ctx, &LAST_CHECK)?;
    let check = CheckRow {
        id,
        run: run.id,
        repo: run.repo.clone(),
        commit: run.commit.clone(),
        workflow: run.workflow.clone(),
        attempt,
        job,
        state: State::Queued,
        claimant: None,
        reporter: None,
        queued: ctx.env().height,
        started: None,
        finished: None,
        summary: String::new(),
        log: None,
    };
    CHECKS.put(ctx, &id, &check);
    BY_COMMIT.insert(ctx, &(check.repo.clone(), check.commit.clone(), id));
    BY_RUN.insert(ctx, &(run.id, id));
    QUEUED.insert(ctx, &id);
    for (_, runner) in RUNNERS.all(ctx)? {
        offer(ctx, &runner, &check);
    }
    Ok(())
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
    if covers {
        let mut payload = runner.deliver.clone();
        payload.extend(abi::encode(&Offer { check: check.id }));
        ctx.emit_raw(program.clone(), payload, Reply::Wanted);
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

pub(crate) fn claim(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    let account = ctx.sender_account()?;
    let mut check = load(ctx, id)?;
    if check.state != State::Queued {
        return Err(wrong_state(format!(
            "check {id} is {}, not queued",
            check.state.word()
        )));
    }
    check.state = State::Running;
    check.claimant = Some(account);
    check.started = Some(ctx.env().height);
    QUEUED.remove(ctx, &id);
    CHECKS.put(ctx, &id, &check);
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
    if check.state != State::Running {
        return Err(wrong_state(format!(
            "check {id} is {}, not running",
            check.state.word()
        )));
    }
    if matches!(state, State::Queued | State::Running) {
        return Err(invalid(
            "a report says how a check ended: passed, failed or errored",
        ));
    }
    check.state = state;
    check.reporter = Some(account);
    check.finished = Some(ctx.env().height);
    check.summary = summary;
    check.log = log;
    CHECKS.put(ctx, &id, &check);
    if matches!(state, State::Failed | State::Errored) {
        let run = RUNS
            .get(ctx, &check.run)?
            .ok_or_else(|| corrupt("r/", &check.run.to_be_bytes(), "a check's run is missing"))?;
        if let (Event::Push { reference }, Some(channel)) = (&run.event, &run.notify) {
            red(ctx, &run, &check, reference, channel);
        }
    }
    Ok(())
}

// A push's check ended red: one line in the workflow's channel, naming who pushed. A channel
// that refuses it leaves the report as it is (`Program::reply` absorbs the refusal).
fn red(ctx: &ExecCtx, run: &RunRow, check: &CheckRow, reference: &[u8], channel: &str) {
    let branch = String::from_utf8_lossy(reference);
    let branch = branch.strip_prefix("refs/heads/").unwrap_or(&branch);
    let workflow = run.name.as_deref().unwrap_or(&run.workflow);
    let commit = check.commit.get(..12).unwrap_or(&check.commit);
    let mut line = format!(
        "**{workflow} {} on {} `{branch}`** at `{commit}`: check {} (job `{}`, attempt {})",
        check.state.word(),
        run.repo,
        check.id,
        check.job.id,
        check.attempt
    );
    if let Some(Principal::Account(account)) = &run.by {
        line.push_str(&format!(", pushed by <@{account}>"));
    }
    if !check.summary.is_empty() {
        line.push_str(&format!(". {}", check.summary));
    }
    if let Some(log) = &check.log {
        line.push_str(&format!(". Log: `{log}`"));
    }
    let post = chat::Op::PostMessage {
        channel_id: channel.to_owned(),
        message_id: format!("actions-check-{}", check.id),
        blocks: chat::parse_message(&line),
        thread: None,
    };
    ctx.emit(chat::PROGRAM, &post, Reply::Wanted);
}

pub(crate) fn rerun(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    ctx.sender_account()?;
    let check = load(ctx, id)?;
    if check.state == State::Queued {
        return Err(wrong_state(format!("check {id} is still queued")));
    }
    let run = RUNS
        .get(ctx, &check.run)?
        .ok_or_else(|| corrupt("r/", &check.run.to_be_bytes(), "a check's run is missing"))?;
    let attempt = check
        .attempt
        .checked_add(1)
        .ok_or_else(|| Error::new(code::EXHAUSTED, "attempts exhausted"))?;
    queue(ctx, &run, check.job, attempt)
}

fn load(ctx: &QueryCtx, id: u64) -> Result<CheckRow, Error> {
    CHECKS
        .get(ctx, &id)?
        .ok_or_else(|| not_found(format!("no check {id}")))
}

pub(crate) fn check(ctx: &QueryCtx, id: u64) -> Result<Option<CheckRow>, Error> {
    CHECKS.get(ctx, &id)
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

pub(crate) fn runners(
    ctx: &QueryCtx,
    page: &PageRequest,
) -> Result<PageResponse<RunnerRow>, Error> {
    Ok(RUNNERS.range(ctx, page)?.map(|(_, runner)| runner))
}
