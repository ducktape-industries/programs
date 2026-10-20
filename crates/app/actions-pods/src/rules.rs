//! A check run as a pod: booked when actions offers it, run the block after, the pod made, then the
//! check claimed, the pod's end heard as a stream, `/out` kept, the check reported and the pod
//! deleted. A quota with no room leaves the check queued; `Sweep` settles the pods runs left.
use ::program::{
    BlobId, Cause, Ctx, Error, ExecCtx, HashKind, ProgramId, QueryCtx, code, decoded, invalid,
    unauthorized, unexpected_reply, wrong_state,
};
use abi::role::egress::{DEFAULT_WAIT, Delivery, Event, Op as Egress, Request, http};
use abi::role::programs as role;
use actions::{Asks, CheckRow, Output, PageRequest, State, bytes, millicores};
use borsh::{BorshDeserialize, BorshSerialize};
use serde_json::{Value, json};
use store::{Item, Map, Set};

use crate::{Op, PROGRAM, PodRow, RunRow, SERVICE};

pub(crate) const OFFERED: Map<u64, u64> = Map::new("o/");
pub(crate) const ENDED: Map<String, Vec<u8>> = Map::new("e/");
pub(crate) const RUNS: Map<u64, RunRow> = Map::new("r/");
pub(crate) const PODS: Map<u64, PodRow> = Map::new("p/");
pub(crate) const WAITING: Set<u64> = Set::new("w/");
pub(crate) const MAKING: Set<u64> = Set::new("m/");
pub(crate) const CONCURRENT: Item<u64> = Item::new("concurrent");
pub(crate) const ADOPTING: Map<u64, Adopting> = Map::new("a/");

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub(crate) struct Adopting {
    at: u64,
    time: u64,
}

struct Ending {
    state: State,
    summary: String,
    log: Option<String>,
}

enum Made {
    Pod(String),
    Full,
    Refused(String),
}

pub(crate) async fn offered(ctx: Ctx, check: u64) -> Result<(), Error> {
    ctx.env().sent_by(actions::PROGRAM)?;
    if OFFERED.has(&ctx, &check) {
        return Ok(());
    }
    OFFERED.put(&ctx, &check, &ctx.env().time);
    let own = ctx.env().program.clone();
    let next = ctx.env().height + 1;
    ctx.schedule(next, own, &Op::Run { check }).await?;
    Ok(())
}

fn scheduled_by_self(ctx: &Ctx) -> Result<(), Error> {
    let own = ctx.env().program.clone();
    let scheduled = matches!(ctx.env().cause, Cause::Scheduled { .. });
    if !scheduled || ctx.env().sending_program()? != own {
        return Err(unauthorized(
            "a check runs in this program's own scheduled run",
        ));
    }
    Ok(())
}

pub(crate) async fn run(ctx: Ctx, check: u64) -> Result<(), Error> {
    scheduled_by_self(&ctx)?;
    let Some(offered) = OFFERED.get(&ctx, &check)? else {
        return Err(wrong_state(format!(
            "check {check} was not offered to this runner"
        )));
    };
    OFFERED.remove(&ctx, &check);
    WAITING.remove(&ctx, &check);
    let row = match check_row(&ctx, check) {
        Ok(row) => row,
        Err(refused) => {
            report(&ctx, check, Ending::errored(refused.message)).await;
            return Ok(());
        }
    };
    if row.state != State::Queued {
        return Ok(());
    }
    let mut kept = RunRow {
        check,
        block_ms: ctx.env().time.saturating_sub(offered),
        ..RunRow::default()
    };
    settle(&ctx).await?;
    if pods_counting_against_quota(&ctx, &row) >= concurrent(&ctx)? {
        WAITING.insert(&ctx, &check);
        return Ok(());
    }
    let made_at = ctx.env().time;
    MAKING.insert(&ctx, &check);
    let made = create_pod_with_quota(&ctx, &row).await;
    MAKING.remove(&ctx, &check);
    let pod = match made {
        Made::Pod(pod) => pod,
        Made::Full => {
            WAITING.insert(&ctx, &check);
            return Ok(());
        }
        Made::Refused(why) => {
            report(&ctx, check, Ending::errored(why)).await;
            return Ok(());
        }
    };
    kept.trip_ms = ctx.env().time.saturating_sub(made_at);
    kept.pod = Some(pod.clone());
    let held = PodRow {
        pod: pod.clone(),
        code: code_at_current_height(&ctx),
    };
    PODS.put(&ctx, &check, &held);
    let claim = actions::Op::Claim { check };
    if let Err(refused) = ctx.call_raw(actions::PROGRAM, abi::encode(&claim)).await {
        delete(&ctx, &pod).await;
        PODS.remove(&ctx, &check);
        if refused.code != code::WRONG_STATE {
            let why = format!("the claim was refused: {}", refused.message);
            report(&ctx, check, Ending::errored(why)).await;
        }
        return Ok(());
    }
    if supersede_earlier_attempts(&ctx, &row).await {
        retry_waiting_next_block(&ctx).await;
    }
    let ending = wait_for_pod_and_copy_outputs(&ctx, &row, &pod, &mut kept)
        .await
        .unwrap_or_else(Ending::errored);
    finish_pod_and_retry_waiting(&ctx, check, &pod, kept, ending).await;
    Ok(())
}

pub(crate) fn concurrent(ctx: &QueryCtx) -> Result<u64, Error> {
    Ok(CONCURRENT.get(ctx)?.unwrap_or(1))
}

pub(crate) async fn set_concurrent(ctx: Ctx, most: u64) -> Result<(), Error> {
    if most == 0 {
        return Err(invalid(
            "a runner holds at least one pod at once, or it runs no check",
        ));
    }
    CONCURRENT.put(&ctx, &most);
    retry_waiting_next_block(&ctx).await;
    Ok(())
}

fn pods_counting_against_quota(ctx: &QueryCtx, check: &CheckRow) -> u64 {
    let making = MAKING.all(ctx).map(|making| making.len()).unwrap_or(0);
    let held = PODS
        .all(ctx)
        .unwrap_or_default()
        .into_iter()
        .filter(|(id, _)| match check_row(ctx, *id) {
            Ok(other) => {
                other.state == State::Running
                    && !(same_job(&other, check) && other.attempt < check.attempt)
            }
            Err(_) => true,
        });
    (making + held.count()) as u64
}

pub(crate) async fn sweep(ctx: Ctx) -> Result<(), Error> {
    settle(&ctx).await?;
    retry_waiting_next_block(&ctx).await;
    Ok(())
}

async fn settle(ctx: &Ctx) -> Result<(), Error> {
    let now = code_at_current_height(ctx);
    for (check, held) in PODS.all(ctx)? {
        swept(ctx, check, &held, now).await;
    }
    Ok(())
}

async fn swept(ctx: &Ctx, check: u64, held: &PodRow, now: Option<BlobId>) {
    let Ok(found) = check_row(ctx, check) else {
        return;
    };
    if found.state != State::Running {
        delete(ctx, &held.pod).await;
        PODS.remove(ctx, &check);
        ADOPTING.remove(ctx, &check);
        return;
    }
    let newest = checks_of_commit(ctx, &found)
        .unwrap_or_default()
        .into_iter()
        .filter(|other| same_job(other, &found))
        .max_by_key(|other| other.attempt);
    if let Some(newest) = newest.filter(|newest| newest.attempt > found.attempt) {
        retire_superseded_check(ctx, &found, held, &newest).await;
        return;
    }
    let cut_off = matches!((held.code, now), (Some(then), Some(now)) if then != now);
    let booked = ADOPTING
        .get(ctx, &check)
        .ok()
        .flatten()
        .is_some_and(|adopting| adopting.at > ctx.env().height);
    if cut_off && !booked {
        adopt(ctx, check).await;
    }
}

async fn adopt(ctx: &Ctx, check: u64) {
    let own = ctx.env().program.clone();
    let (at, time) = (ctx.env().height + 1, ctx.env().time);
    if ctx.schedule(at, own, &Op::Resume { check }).await.is_ok() {
        ADOPTING.put(ctx, &check, &Adopting { at, time });
    }
}

pub(crate) async fn resume(ctx: Ctx, check: u64) -> Result<(), Error> {
    scheduled_by_self(&ctx)?;
    let Some(adopting) = ADOPTING.get(&ctx, &check)? else {
        return Ok(());
    };
    ADOPTING.remove(&ctx, &check);
    let Some(mut held) = PODS.get(&ctx, &check)? else {
        return Ok(());
    };
    held.code = code_at_current_height(&ctx);
    PODS.put(&ctx, &check, &held);
    let mut kept = RunRow {
        check,
        pod: Some(held.pod.clone()),
        block_ms: ctx.env().time.saturating_sub(adopting.time),
        ..RunRow::default()
    };
    let ending = match check_row(&ctx, check) {
        Ok(found) => wait_for_pod_and_copy_outputs(&ctx, &found, &held.pod, &mut kept)
            .await
            .unwrap_or_else(Ending::errored),
        Err(refused) => Ending::errored(refused.message),
    };
    finish_pod_and_retry_waiting(&ctx, check, &held.pod, kept, ending).await;
    Ok(())
}

async fn supersede_earlier_attempts(ctx: &Ctx, by: &CheckRow) -> bool {
    let earlier: Vec<CheckRow> = checks_of_commit(ctx, by)
        .unwrap_or_default()
        .into_iter()
        .filter(|other| same_job(other, by) && other.attempt < by.attempt)
        .collect();
    let mut retired = false;
    for check in earlier {
        if let Ok(Some(held)) = PODS.get(ctx, &check.id) {
            retire_superseded_check(ctx, &check, &held, by).await;
            retired = true;
        }
    }
    retired
}

async fn retire_superseded_check(ctx: &Ctx, check: &CheckRow, held: &PodRow, by: &CheckRow) {
    delete(ctx, &held.pod).await;
    PODS.remove(ctx, &check.id);
    ADOPTING.remove(ctx, &check.id);
    if check.state == State::Running {
        let why = format!("superseded by check {} (attempt {})", by.id, by.attempt);
        let superseded = Ending {
            state: State::Superseded,
            summary: why,
            log: None,
        };
        report(ctx, check.id, superseded).await;
    }
}

fn same_job(one: &CheckRow, other: &CheckRow) -> bool {
    one.run == other.run && one.job.id == other.job.id
}

async fn finish_pod_and_retry_waiting(
    ctx: &Ctx,
    check: u64,
    pod: &str,
    kept: RunRow,
    ending: Ending,
) {
    delete(ctx, pod).await;
    PODS.remove(ctx, &check);
    RUNS.put(ctx, &check, &kept);
    report(ctx, check, ending).await;
    retry_waiting_next_block(ctx).await;
}

async fn retry_waiting_next_block(ctx: &Ctx) {
    let Ok(waiting) = WAITING.all(ctx) else {
        return;
    };
    let own = ctx.env().program.clone();
    let (at, time) = (ctx.env().height + 1, ctx.env().time);
    for check in waiting {
        if ctx
            .schedule(at, own.clone(), &Op::Run { check })
            .await
            .is_ok()
        {
            WAITING.remove(ctx, &check);
            OFFERED.put(ctx, &check, &time);
        }
    }
}

async fn delete(ctx: &Ctx, pod: &str) {
    let deleted = format!("/v1/pods/{pod}");
    let _ = ask(
        ctx,
        http::Method::Delete,
        deleted,
        Vec::new(),
        Delivery::Whole,
    )
    .await;
}

fn code_at_current_height(ctx: &QueryCtx) -> Option<BlobId> {
    let programs = ctx.env().role(abi::role::PROGRAMS).ok()?.clone();
    let asked = role::Query::At(ctx.env().height);
    let entries = match ctx.query(programs, &asked).ok()? {
        role::Reply::Programs(entries) => entries,
        _ => return None,
    };
    let own = &ctx.env().program;
    entries
        .into_iter()
        .find(|entry| entry.program == *own)
        .map(|entry| entry.code)
}

impl Ending {
    fn errored(summary: String) -> Ending {
        Ending {
            state: State::Errored,
            summary,
            log: None,
        }
    }
}

fn check_row(ctx: &QueryCtx, id: u64) -> Result<CheckRow, Error> {
    let asked = actions::Query::Check { id };
    match ctx.query::<actions::Query, actions::Reply>(actions::PROGRAM, &asked)? {
        actions::Reply::Check(Some(row)) => Ok(row),
        actions::Reply::Check(None) => Err(wrong_state(format!("actions holds no check {id}"))),
        other => Err(unexpected_reply(actions::PROGRAM, "Check", &other)),
    }
}

fn checks_of_commit(ctx: &QueryCtx, check: &CheckRow) -> Result<Vec<CheckRow>, Error> {
    let asked = actions::Query::Checks {
        repo: check.repo.clone(),
        commit: check.commit.clone(),
        page: PageRequest::default(),
    };
    match ctx.query::<actions::Query, actions::Reply>(actions::PROGRAM, &asked)? {
        actions::Reply::Checks(page) => Ok(page.items),
        other => Err(unexpected_reply(actions::PROGRAM, "Checks", &other)),
    }
}

fn pod_requirements(ctx: &QueryCtx, check: u64) -> Result<Asks, Error> {
    let asked = actions::Query::Asks { check };
    match ctx.query::<actions::Query, actions::Reply>(actions::PROGRAM, &asked)? {
        actions::Reply::Asks(asks) => Ok(asks.unwrap_or_default()),
        other => Err(unexpected_reply(actions::PROGRAM, "Asks", &other)),
    }
}

async fn create_pod_with_quota(ctx: &Ctx, check: &CheckRow) -> Made {
    let asks = match pod_requirements(ctx, check.id) {
        Ok(asks) => asks,
        Err(refused) => return Made::Refused(refused.message),
    };
    let needed = match needed(ctx, check.id) {
        Ok(needed) => needed,
        Err(why) => return Made::Refused(why),
    };
    let body = pod_body(check, &asks, needed);
    let answer = match send(ctx, "/v1/pods".to_owned(), &body).await {
        Ok(answer) => answer,
        Err(why) => return Made::Refused(why),
    };
    if answer.status == 403 {
        let error = error_of(&answer);
        if error.contains("exceeded quota") {
            return match request_exceeds_whole_quota(&error) {
                Some(why) => Made::Refused(why),
                None => Made::Full,
            };
        }
    }
    if !(200..300).contains(&answer.status) {
        return Made::Refused(refusal("/v1/pods", &answer));
    }
    let created = match serde_json::from_slice::<Value>(&answer.body) {
        Ok(created) => created,
        Err(error) => return Made::Refused(error.to_string()),
    };
    match created["pod"]["name"].as_str() {
        Some(pod) => Made::Pod(pod.to_owned()),
        None => Made::Refused("the pods service named no pod".to_owned()),
    }
}

fn request_exceeds_whole_quota(error: &str) -> Option<String> {
    let amount = |label: &str, resource: &str, read: fn(&str) -> Option<u64>| {
        let (_, rest) = error.split_once(label)?;
        let listed = rest.split(", ").next()?;
        listed
            .split(',')
            .find_map(|item| item.strip_prefix(resource))
            .and_then(read)
    };
    let over = |resource: &str, read: fn(&str) -> Option<u64>| {
        let asked = amount("requested: ", resource, read);
        let whole = amount("limited: ", resource, read);
        matches!((asked, whole), (Some(asked), Some(whole)) if asked > whole)
    };
    let never = over("requests.cpu=", millicores) || over("requests.memory=", bytes);
    never.then(|| format!("the pod asks more than the quota holds: {error}"))
}

async fn wait_for_pod_and_copy_outputs(
    ctx: &Ctx,
    check: &CheckRow,
    pod: &str,
    kept: &mut RunRow,
) -> Result<Ending, String> {
    let mut opened = (ctx.env().height, ctx.env().time);
    let ended = loop {
        let every = alive_every(kept.block_ms)?;
        kept.every = every;
        let wait = format!("/v1/pods/{pod}/wait?every={every}");
        let events = Delivery::Events {
            deliver: crate::heard_as(),
        };
        let waited = ask(ctx, http::Method::Get, wait, Vec::new(), events).await;
        if let Some(ended) = ENDED
            .get(ctx, &pod.to_owned())
            .map_err(|refused| refused.message)?
        {
            ENDED.remove(ctx, &pod.to_owned());
            break serde_json::from_slice::<Value>(&ended).map_err(|error| error.to_string())?;
        }
        if let Ok(answer) = &waited
            && answer.status != 200
        {
            return Err(refusal("the wait", answer));
        }
        if let Some(ended) = read_pod_ending(ctx, pod).await? {
            break ended;
        }
        let (height, time) = (ctx.env().height, ctx.env().time);
        if let Some(blocks) = height.checked_sub(opened.0).filter(|blocks| *blocks > 0) {
            kept.block_ms = time.saturating_sub(opened.1) / blocks;
        }
        opened = (height, time);
    };
    let state = match ended["exit_code"].as_i64() {
        Some(0) => State::Passed,
        Some(_) => State::Failed,
        None => State::Errored,
    };
    let mut summary = ended["line"].as_str().unwrap_or_default().to_owned();
    let (log, missed) = kept_out(ctx, check, pod).await;
    if let Some(missed) = missed {
        summary.push_str(&format!("; {missed}"));
    }
    Ok(Ending {
        state,
        summary,
        log,
    })
}

pub(crate) fn alive_every(block_ms: u64) -> Result<u64, String> {
    let lasts = DEFAULT_WAIT.saturating_mul(block_ms);
    if lasts < 2000 {
        return Err(format!(
            "a request lasts {lasts} ms ({DEFAULT_WAIT} blocks of {block_ms} ms), under the 2 s two whole-second beats need"
        ));
    }
    Ok(lasts / 2 / 1000)
}

async fn read_pod_ending(ctx: &Ctx, pod: &str) -> Result<Option<Value>, String> {
    let target = format!("/v1/pods/{pod}");
    let read = ask(ctx, http::Method::Get, target, Vec::new(), Delivery::Whole)
        .await
        .map_err(|refused| refused.message)?;
    if read.status != 200 {
        return Err(refusal("the pod's read", &read));
    }
    let view: Value = serde_json::from_slice(&read.body).map_err(|error| error.to_string())?;
    let view = &view["pod"];
    let run = view["containers"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|container| container["name"] == "run");
    let status = run.map(|container| &container["status"]);
    let terminated = status.is_some_and(|status| status["state"] == "terminated");
    let phase = view["phase"].as_str().unwrap_or_default();
    if !terminated && !matches!(phase, "Succeeded" | "Failed") {
        return Ok(None);
    }
    let exit_code = status.map_or(Value::Null, |status| status["exit_code"].clone());
    let why = status
        .and_then(|status| status["reason"].as_str())
        .or(view["reason"].as_str())
        .unwrap_or(phase);
    let line = match exit_code.as_i64() {
        Some(code) => format!("pod {pod} ended: exit code {code} ({why})"),
        None => format!("pod {pod} ended: {why}"),
    };
    Ok(Some(json!({"line": line, "exit_code": exit_code})))
}

// The check's repository at its commit in `/work/<repo>`, its inputs, what the jobs it needs produced
// (`needed`), and its steps run there in
// order, stopping at the first that fails, their output in `/out/log`. What the job asks is the
// pod's request, cargo runs as many jobs as it asks CPUs (its own env still wins), and each
// scratch path is memory-backed, so a build's output never touches the node's disk.
fn pod_body(check: &CheckRow, asks: &Asks, needed: Vec<Value>) -> Value {
    let work = format!("/work/{}", check.repo);
    let mut inputs = vec![json!({
        "path": work,
        "forge": {"repo": check.repo, "commit": check.commit},
    })];
    for input in &check.job.inputs {
        inputs.push(json!({"path": input.path, "files": {"path": input.files}}));
    }
    inputs.extend(needed);
    for path in &asks.scratch {
        inputs.push(json!({"path": path, "scratch": {"medium": "Memory"}}));
    }
    // where a step writes what it keeps and finds what it needs, as every runner names them
    let mut env = serde_json::Map::new();
    env.insert("OUT".to_owned(), Value::from("/out"));
    env.insert("NEEDS".to_owned(), Value::from("/needs"));
    let jobs = asks
        .cpu
        .as_deref()
        .and_then(millicores)
        .map(|milli| milli.div_ceil(1000).max(1));
    if let Some(jobs) = jobs {
        env.insert("CARGO_BUILD_JOBS".to_owned(), Value::from(jobs.to_string()));
    }
    for (name, value) in &check.job.env {
        env.insert(name.clone(), Value::from(value.as_str()));
    }
    let mut body = json!({
        "image": check.job.image,
        "command": ["sh", "-c", script(&work, &check.job.steps)],
        "env": env,
        "inputs": inputs,
    });
    let mut requests = serde_json::Map::new();
    if let Some(cpu) = &asks.cpu {
        requests.insert("cpu".to_owned(), Value::from(cpu.as_str()));
    }
    if let Some(memory) = &asks.memory {
        requests.insert("memory".to_owned(), Value::from(memory.as_str()));
    }
    if !requests.is_empty() {
        body["resources"] = json!({ "requests": requests });
    }
    body
}

fn script(work: &str, steps: &[String]) -> String {
    let mut script = format!("exec > /out/log 2>&1\nset -e\ncd {}\n", quoted(work));
    for step in steps {
        let shown = quoted(&format!("$ {step}"));
        script.push_str(&format!("printf '%s\\n' {shown}\n{step}\n"));
    }
    script
}

fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

// The kind a check's outputs are put as.
const OUTPUT_KIND: &str = "output";
// A check's log, which the report links.
const LOG: &str = "/out/log";

// The pod's `/out` kept in two requests: its log copied into files, under
// `/shared/actions/<repo>/<commit>/<workflow>/<job>-<attempt>/log`, and every other file put as a
// blob and recorded as the check's outputs (`record`). Where the log is, and what was not kept.
async fn kept_out(ctx: &Ctx, check: &CheckRow, pod: &str) -> (Option<String>, Option<String>) {
    let workflow = check.workflow.trim_end_matches(".toml");
    let dir = format!(
        "/shared/actions/{}/{}/{workflow}/{}-{}",
        check.repo, check.commit, check.job.id, check.attempt
    );
    let find = json!({"command": ["find", "/out", "-type", "f"], "container": "out"});
    let listed = match post(ctx, format!("/v1/pods/{pod}/exec"), &find).await {
        Ok(listed) => listed,
        Err(why) => return (None, Some(format!("/out was not listed: {why}"))),
    };
    let files: Vec<&str> = listed["stdout"]
        .as_str()
        .unwrap_or_default()
        .lines()
        .filter(|file| file.starts_with("/out/"))
        .collect();
    let mut missed = Vec::new();
    let mut log = None;
    if files.contains(&LOG) {
        let to = format!("{dir}/log");
        let message = format!("actions: check {} of {}", check.id, check.repo);
        let copy = json!({"path": LOG, "to": to, "container": "out", "message": message});
        match post(ctx, format!("/v1/pods/{pod}/file"), &copy).await {
            Ok(_) => log = Some(to),
            Err(why) => missed.push(format!("{LOG}: {why}")),
        }
    }
    let outputs: Vec<&str> = files.into_iter().filter(|file| *file != LOG).collect();
    if !outputs.is_empty()
        && let Err(why) = record(ctx, check.id, pod, &outputs).await
    {
        missed.push(format!("the outputs: {why}"));
    }
    let missed = (!missed.is_empty()).then(|| format!("not kept: {}", missed.join("; ")));
    (log, missed)
}

// Every output put as a blob in one request, then recorded on the check by its path under `/out`,
// its blob and its size, where a check that needs this job finds it (`actions::Query::Outputs`).
async fn record(ctx: &Ctx, check: u64, pod: &str, paths: &[&str]) -> Result<(), String> {
    let put = json!({"paths": paths, "kind": OUTPUT_KIND, "container": "out"});
    let held = post(ctx, format!("/v1/pods/{pod}/blob"), &put).await?;
    let blobs = held["blobs"]
        .as_array()
        .ok_or_else(|| "the pods service named no blobs".to_owned())?;
    let mut files = Vec::new();
    for blob in blobs {
        let path = blob["path"]
            .as_str()
            .and_then(|path| path.strip_prefix("/out/"));
        let id = blob["blob"].as_str().and_then(blob_of);
        let (Some(path), Some(id), Some(size)) = (path, id, blob["size"].as_u64()) else {
            return Err(format!("the pods service answered {blob} for a blob"));
        };
        files.push(Output {
            path: path.to_owned(),
            blob: id,
            size,
        });
    }
    let said = actions::Op::Outputs { check, files };
    ctx.call_raw(actions::PROGRAM, abi::encode(&said))
        .await
        .map(|_| ())
        .map_err(|refused| refused.message)
}

// What the jobs a check needs produced, each a blob input at `/needs/<job>/<path>` of the pod's one
// creation request.
fn needed(ctx: &QueryCtx, check: u64) -> Result<Vec<Value>, String> {
    let asked = actions::Query::Needs { check };
    let needs = match ctx.query::<actions::Query, actions::Reply>(actions::PROGRAM, &asked) {
        Ok(actions::Reply::Needs(needs)) => needs,
        Ok(other) => return Err(unexpected_reply(actions::PROGRAM, "Needs", &other).message),
        Err(refused) => return Err(refused.message),
    };
    let mut inputs = Vec::new();
    for need in needs {
        let Some(met) = need.check else {
            return Err(format!("needs `{}`, which no passed check met", need.job));
        };
        let asked = actions::Query::Outputs { check: met };
        let files = match ctx.query::<actions::Query, actions::Reply>(actions::PROGRAM, &asked) {
            Ok(actions::Reply::Outputs(row)) => row.map(|row| row.files).unwrap_or_default(),
            Ok(other) => return Err(unexpected_reply(actions::PROGRAM, "Outputs", &other).message),
            Err(refused) => return Err(refused.message),
        };
        for file in files {
            let path = placed(&need.job, &file.path)?;
            inputs.push(json!({"path": path, "blob": blob_text(&file.blob)}));
        }
    }
    Ok(inputs)
}

// Where a needed output goes, `/needs/<job>/<path>`; refused when the job or the path would leave
// that folder.
fn placed(job: &str, path: &str) -> Result<String, String> {
    let plain = |part: &str| !part.is_empty() && part != "." && part != "..";
    match plain(job) && !job.contains('/') && path.split('/').all(plain) {
        true => Ok(format!("/needs/{job}/{path}")),
        false => Err(format!(
            "check output {path:?} of `{job}` would land outside /needs/{job}"
        )),
    }
}

// A blob id as the pods service writes it, `sha256:<64 hex digits>`.
fn blob_of(text: &str) -> Option<BlobId> {
    let hex = text.strip_prefix("sha256:")?;
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut digest = [0; 32];
    for (byte, pair) in digest.iter_mut().zip(hex.as_bytes().chunks(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(BlobId::Sha256(digest))
}

fn blob_text(blob: &BlobId) -> String {
    match blob {
        BlobId::Sha256(digest) => format!("sha256:{}", abi::hex(digest)),
        BlobId::Sha1(digest) => format!("sha1:{}", abi::hex(digest)),
    }
}

async fn send(ctx: &Ctx, target: String, body: &Value) -> Result<http::Response, String> {
    let bytes = serde_json::to_vec(body).map_err(|error| error.to_string())?;
    let part = ctx
        .blob_put(HashKind::Sha256, "part", bytes)
        .map_err(|refused| refused.message)?;
    ask(ctx, http::Method::Post, target, vec![part], Delivery::Whole)
        .await
        .map_err(|refused| refused.message)
}

async fn post(ctx: &Ctx, target: String, body: &Value) -> Result<Value, String> {
    let answer = send(ctx, target.clone(), body).await?;
    if !(200..300).contains(&answer.status) {
        return Err(refusal(&target, &answer));
    }
    serde_json::from_slice(&answer.body).map_err(|error| error.to_string())
}

fn error_of(answer: &http::Response) -> String {
    serde_json::from_slice::<Value>(&answer.body)
        .ok()
        .and_then(|body| body["error"].as_str().map(str::to_owned))
        .unwrap_or_else(|| String::from_utf8_lossy(&answer.body).into_owned())
}

fn refusal(what: &str, answer: &http::Response) -> String {
    format!("{what} answered {}: {}", answer.status, error_of(answer))
}

async fn ask(
    ctx: &Ctx,
    method: http::Method,
    target: String,
    body: Vec<BlobId>,
    delivery: Delivery,
) -> Result<http::Response, Error> {
    let egress = ctx.env().role(abi::role::EGRESS)?.clone();
    let headers = match body.is_empty() {
        true => Vec::new(),
        false => vec![("content-type".to_owned(), "application/json".to_owned())],
    };
    let head = http::Head {
        method,
        target,
        headers,
    };
    let request = Request::Http {
        service: SERVICE.to_owned(),
        head,
        body,
    };
    let asked = Egress::Request {
        request,
        delivery,
        wait: None,
    };
    let answer = ctx.call_raw(egress, abi::encode(&asked)).await?;
    decoded(PROGRAM, "http::Response", &answer)
}

async fn report(ctx: &Ctx, check: u64, ending: Ending) {
    let report = actions::Op::Report {
        check,
        state: ending.state,
        summary: ending.summary,
        log: ending.log,
    };
    let _ = ctx.call_raw(actions::PROGRAM, abi::encode(&report)).await;
}

pub(crate) fn heard(ctx: &ExecCtx, event: Event) -> Result<(), Error> {
    let egress = ctx.env().role(abi::role::EGRESS)?.clone();
    if ctx.env().sending_program()? != egress {
        return Err(unauthorized("a wait's events come from the egress role"));
    }
    let Some(data) = ended_event_data(&event.data) else {
        return Ok(());
    };
    let pod = pod_named_by_wait(ctx, &egress, event.id)?;
    ENDED.put(ctx, &pod, &data);
    Ok(())
}

fn ended_event_data(raw: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(raw).ok()?;
    let ended = text.lines().any(|line| line == "event: ended");
    let data = text.lines().find_map(|line| line.strip_prefix("data: "));
    ended
        .then(|| data.map(|data| data.as_bytes().to_vec()))
        .flatten()
}

fn pod_named_by_wait(ctx: &QueryCtx, egress: &ProgramId, id: u64) -> Result<String, Error> {
    let asked = egress::Query::Request { id };
    let row = match ctx.query::<egress::Query, egress::Reply>(egress.clone(), &asked)? {
        egress::Reply::Request(Some(row)) => row,
        egress::Reply::Request(None) => {
            return Err(wrong_state(format!("egress holds no request {id}")));
        }
        other => return Err(unexpected_reply(egress, "Request", &other)),
    };
    let Request::Http { head, .. } = row.request else {
        return Err(wrong_state(format!("request {id} is not a pod's wait")));
    };
    head.target
        .strip_prefix("/v1/pods/")
        .and_then(|rest| rest.split('/').next())
        .map(str::to_owned)
        .ok_or_else(|| wrong_state(format!("request {id} is not a pod's wait")))
}
