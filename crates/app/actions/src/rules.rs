//! What each op checks and writes: forge's events start runs, a run queues a check per job and
//! offers it to the runners whose labels cover it, and a check goes from queued to running to
//! how its runner says it ended.
use ::program::{
    Error, ExecCtx, MessageId, Outcome, Principal, QueryCtx, Reply, code, corrupt, invalid,
    not_found, unexpected_reply, wrong_state,
};
use forge::{ChangeState, CheckState, Content, EntryKind, Revision, Watched};
use store::Item;

use crate::state::{
    ASKS, BY_COMMIT, BY_RUN, CHECKS, LAST_CHECK, LAST_RUN, LINES, MET, NEEDS, OFFERING, OUTPUTS,
    PREVIEWING, QUEUED, RUNNERS, RUNS, SOURCES, TARGETS,
};
use crate::workflow::{self, Planned};
use crate::{
    Asks, CheckRow, Event, Need, Offer, Output, OutputsRow, PageRequest, PageResponse, RunRow,
    RunnerRow, State, WORKFLOWS,
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
        // a Preview's own ref: its checks start from `Previewed`
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
                    true => preview(ctx, &repo, n)?,
                    false => SOURCES.remove(ctx, &source),
                }
            }
            let into = TARGETS.range_of(ctx, &source, &PageRequest::default())?;
            for (_, _, n) in into.items {
                match still_open(ctx, &repo, n)? {
                    true => preview(ctx, &repo, n)?,
                    false => TARGETS.remove(ctx, &(repo.clone(), reference.clone(), n)),
                }
            }
            let why = format!(
                "superseded by a push of {} to {}",
                short(&commit),
                branch(&reference)
            );
            supersede(ctx, &repo, &reference, &why)?;
            start(ctx, &started(&repo, &commit, Event::Push { reference }, by))
        }
        Watched::Moved { new: None, .. } => Ok(()),
        Watched::Closed { repo, n, .. } => {
            let why = format!("change #{n} closed");
            supersede(ctx, &repo, preview_ref(n).as_bytes(), &why)
        }
        Watched::Merged { repo, n, .. } => {
            let why = format!("change #{n} merged");
            supersede(ctx, &repo, preview_ref(n).as_bytes(), &why)
        }
        Watched::Opened {
            repo,
            n,
            from,
            into,
            ..
        } => {
            if let Revision::Ref(reference) = from {
                SOURCES.put(ctx, &(repo.clone(), reference), &n);
            }
            TARGETS.insert(ctx, &(repo.clone(), into, n));
            preview(ctx, &repo, n)
        }
        Watched::Previewed {
            repo, n, merge, by, ..
        } => {
            adopt(ctx, &repo, n)?;
            let why = format!("superseded by Preview {} of #{n}", short(&merge));
            supersede(ctx, &repo, preview_ref(n).as_bytes(), &why)?;
            start(ctx, &started(&repo, &merge, Event::Preview { n }, Some(by)))
        }
    }
}

// The change's Preview is asked for in actions' own run the block after (`Op::Preview`), so its
// checks' offers start a frame of their own rather than nest under the op that moved a head.
fn preview(ctx: &ExecCtx, repo: &str, n: u64) -> Result<(), Error> {
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

// forge builds the change's Preview, the squash its merge would land, and says so with
// `Previewed`; one it refuses (a conflict) makes an errored run that says why (`replied`). Open:
// anyone may ask actions to preview a change.
pub(crate) fn previewing(ctx: &ExecCtx, repo: &str, n: u64) -> Result<(), Error> {
    let op = forge::Op::Preview {
        repo: repo.to_owned(),
        n,
    };
    let sent = ctx.emit(forge::PROGRAM, &op, Reply::Wanted);
    PREVIEWING.put(ctx, &sent.seq, &(repo.to_owned(), n));
    Ok(())
}

// A change previewed that actions holds no rows for (one opened before actions watched forge)
// joins: its source and target are kept, so their moves preview it again.
fn adopt(ctx: &ExecCtx, repo: &str, n: u64) -> Result<(), Error> {
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
    TARGETS.insert(ctx, &(repo.to_owned(), change.into, n));
    Ok(())
}

// forge's answer to what actions sent it: a Preview it refused makes a run on the change's head,
// errored, that names why (a conflict names every path); any other answer changes nothing.
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
    let Some(head) = open_head(ctx, &repo, n)? else {
        return Ok(());
    };
    let why = format!("change {n} does not preview: {}", refused.message);
    let preview = preview_ref(n);
    let started = started(&repo, &head, Event::Preview { n }, None);
    record(ctx, &started, &preview, Named::default(), Some(why))?;
    Ok(())
}

// A runner refused the offer of a check: unless another runner's offer of it is still out or it
// left the queue, it ends errored, saying why, never silently queued.
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
    tell_forge(ctx, &check);
    release(ctx, check.run)?;
    let run = RUNS
        .get(ctx, &check.run)?
        .ok_or_else(|| corrupt("r/", &check.run.to_be_bytes(), "a check's run is missing"))?;
    if let (Event::Push { reference }, Some(channel)) = (&run.event, &run.notify) {
        red(ctx, &run, &check, reference, channel);
    }
    Ok(())
}

// An open change's head, as forge resolves its source now.
fn open_head(ctx: &QueryCtx, repo: &str, n: u64) -> Result<Option<String>, Error> {
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

// forge hears each check's state as its commit's status in `<workflow>#<job>`, which a branch's
// required checks name.
fn tell_forge(ctx: &ExecCtx, check: &CheckRow) {
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
                for planned in workflow.jobs {
                    queue(ctx, &run, planned, 1)?;
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
    if let Some(line) = line_of(&run.event) {
        LINES.insert(ctx, &(run.repo.clone(), line, id));
    }
    if run.error.is_some() {
        // a workflow that does not read is its file's status, errored
        let op = forge::Op::Status {
            repo: run.repo.clone(),
            commit: run.commit.clone(),
            context: run.workflow.clone(),
            state: CheckState::Errored,
            url: None,
        };
        ctx.emit(forge::PROGRAM, &op, Reply::Wanted);
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
    let Planned { job, asks, needs } = planned;
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
    BY_COMMIT.insert(ctx, &(check.repo.clone(), check.commit.clone(), id));
    BY_RUN.insert(ctx, &(run.id, id));
    if waits {
        NEEDS.put(ctx, &id, &needs);
        tell_forge(ctx, &check);
        return release(ctx, run.id);
    }
    offered(ctx, &check)
}

// A check nothing holds back: queued, told to forge and offered to every runner.
fn offered(ctx: &ExecCtx, check: &CheckRow) -> Result<(), Error> {
    QUEUED.insert(ctx, &check.id);
    tell_forge(ctx, check);
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
                    tell_forge(ctx, &check);
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
    if covers {
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
    tell_forge(ctx, &check);
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
    // a runner whose claim was refused says so, and a check made moot is superseded: a queued
    // check may end either way unclaimed
    let moot = matches!(state, State::Errored | State::Superseded);
    let unclaimed = check.state == State::Queued && moot;
    if check.state != State::Running && !unclaimed {
        return Err(wrong_state(format!(
            "check {id} is {}, not running",
            check.state.word()
        )));
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
    tell_forge(ctx, &check);
    release(ctx, check.run)?;
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

// A newer run on a line (a push to the branch, a Preview of the change), or the change's merge
// or close, makes the checks of the line's earlier runs moot: each still queued or running ends
// superseded, saying why, and posts no line. Those runs leave the line.
fn supersede(ctx: &ExecCtx, repo: &str, line: &[u8], why: &str) -> Result<(), Error> {
    let on = (repo.to_owned(), line.to_vec());
    for (_, _, run) in LINES.scan(ctx, LINES.prefix_of(&on))? {
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
            tell_forge(ctx, &check);
        }
        LINES.remove(ctx, &(repo.to_owned(), line.to_vec(), run));
    }
    Ok(())
}

// The line a run is on, whose next run supersedes it: a push's branch, or a Preview's ref.
fn line_of(event: &Event) -> Option<Vec<u8>> {
    match event {
        Event::Push { reference } => Some(reference.clone()),
        Event::Preview { n } => Some(preview_ref(*n).into_bytes()),
        Event::Opened { .. } | Event::Updated { .. } | Event::Manual => None,
    }
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

// A push's check ended red: one line in the workflow's channel, naming who pushed. A channel
// that refuses it leaves the report as it is (`Program::reply` absorbs the refusal).
fn red(ctx: &ExecCtx, run: &RunRow, check: &CheckRow, reference: &[u8], channel: &str) {
    let branch = branch(reference);
    let workflow = run.name.as_deref().unwrap_or(&run.workflow);
    let commit = short(&check.commit);
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
    if matches!(check.state, State::Queued | State::Waiting) {
        return Err(wrong_state(format!(
            "check {id} is still {}",
            check.state.word()
        )));
    }
    let run = RUNS
        .get(ctx, &check.run)?
        .ok_or_else(|| corrupt("r/", &check.run.to_be_bytes(), "a check's run is missing"))?;
    let attempt = check
        .attempt
        .checked_add(1)
        .ok_or_else(|| Error::new(code::EXHAUSTED, "attempts exhausted"))?;
    let planned = Planned {
        job: check.job,
        asks: ASKS.get(ctx, &id)?.unwrap_or_default(),
        needs: NEEDS.get(ctx, &id)?.unwrap_or_default(),
    };
    queue(ctx, &run, planned, attempt)
}

// The files a running check produced, as a runner says: who said so is kept with them.
pub(crate) fn outputs(ctx: &ExecCtx, id: u64, files: Vec<Output>) -> Result<(), Error> {
    let by = ctx.sender_account()?;
    let check = load(ctx, id)?;
    if check.state != State::Running {
        return Err(wrong_state(format!(
            "check {id} is {}, not running",
            check.state.word()
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
