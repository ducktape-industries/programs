//! A check run as a pod: claimed when actions offers it and run the block after, so the block
//! time is measured between the two; the pod made, its end heard as a stream, `/out` copied into
//! files, the check reported and the pod deleted. A step that fails reports the check errored.
use ::program::{
    BlobId, Cause, Ctx, Error, ExecCtx, HashKind, ProgramId, QueryCtx, decoded, unauthorized,
    unexpected_reply, wrong_state,
};
use abi::role::egress::{DEFAULT_WAIT, Delivery, Event, Op as Egress, Request, http};
use actions::{CheckRow, State};
use serde_json::{Value, json};
use store::Map;

use crate::{Op, PROGRAM, RunRow, SERVICE};

// The block time each claimed check was offered at, until its run takes it.
pub(crate) const OFFERED: Map<u64, u64> = Map::new("o/");
// The data of each pod's `ended` event, until its run reads it.
pub(crate) const ENDED: Map<String, Vec<u8>> = Map::new("e/");
pub(crate) const RUNS: Map<u64, RunRow> = Map::new("r/");

struct Ending {
    state: State,
    summary: String,
    log: Option<String>,
}

pub(crate) async fn offered(ctx: Ctx, check: u64) -> Result<(), Error> {
    ctx.env().sent_by(actions::PROGRAM)?;
    let claim = actions::Op::Claim { check };
    let claimed = ctx.call_raw(actions::PROGRAM, abi::encode(&claim)).await;
    if claimed.is_err() {
        // another runner took it first, or it is no longer queued
        return Ok(());
    }
    OFFERED.put(&ctx, &check, &ctx.env().time);
    let own = ctx.env().program.clone();
    let next = ctx.env().height + 1;
    if let Err(refused) = ctx.schedule(next, own, &Op::Run { check }).await {
        let why = format!("its run was not scheduled: {}", refused.message);
        report(&ctx, check, Ending::errored(why)).await;
    }
    Ok(())
}

pub(crate) async fn run(ctx: Ctx, check: u64) -> Result<(), Error> {
    let own = ctx.env().program.clone();
    let scheduled = matches!(ctx.env().cause, Cause::Scheduled { .. });
    if !scheduled || ctx.env().sending_program()? != own {
        return Err(unauthorized(
            "a check runs in this program's own scheduled run",
        ));
    }
    let Some(offered) = OFFERED.get(&ctx, &check)? else {
        return Err(wrong_state(format!(
            "check {check} was not offered to this runner"
        )));
    };
    OFFERED.remove(&ctx, &check);
    let mut kept = RunRow {
        check,
        block_ms: ctx.env().time.saturating_sub(offered),
        ..RunRow::default()
    };
    let ending = match check_row(&ctx, check) {
        Ok(row) => pod_run(&ctx, &row, &mut kept)
            .await
            .unwrap_or_else(Ending::errored),
        Err(refused) => Ending::errored(refused.message),
    };
    if let Some(pod) = &kept.pod {
        let deleted = format!("/v1/pods/{pod}");
        let _ = ask(
            &ctx,
            http::Method::Delete,
            deleted,
            Vec::new(),
            Delivery::Whole,
        )
        .await;
    }
    RUNS.put(&ctx, &check, &kept);
    report(&ctx, check, ending).await;
    Ok(())
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

// The pod made, its end awaited, its outputs copied. The alive events come as often as a request
// lasts (DEFAULT_WAIT blocks of the measured block time) less the trip the pod's creation took
// to land, so each lands before the request it keeps alive expires.
async fn pod_run(ctx: &Ctx, check: &CheckRow, kept: &mut RunRow) -> Result<Ending, String> {
    let made_at = ctx.env().time;
    let created = post(ctx, "/v1/pods".to_owned(), &pod_body(check)).await?;
    kept.trip_ms = ctx.env().time.saturating_sub(made_at);
    let pod = created["pod"]["name"]
        .as_str()
        .ok_or("the pods service named no pod")?
        .to_owned();
    kept.pod = Some(pod.clone());
    let lasts = DEFAULT_WAIT.saturating_mul(kept.block_ms);
    let every = lasts.saturating_sub(kept.trip_ms) / 1000;
    if every == 0 {
        return Err(format!(
            "a request lasts {lasts} ms ({DEFAULT_WAIT} blocks of {} ms) and the pod's creation took {} ms to land, which leaves no whole second between alive events",
            kept.block_ms, kept.trip_ms
        ));
    }
    kept.every = every;
    let wait = format!("/v1/pods/{pod}/wait?every={every}");
    let events = Delivery::Events {
        deliver: crate::heard_as(),
    };
    let waited = ask(ctx, http::Method::Get, wait, Vec::new(), events)
        .await
        .map_err(|refused| refused.message)?;
    if waited.status != 200 {
        return Err(refusal("the wait", &waited));
    }
    let ended = ENDED
        .get(ctx, &pod)
        .map_err(|refused| refused.message)?
        .ok_or("the wait ended without an ended event")?;
    ENDED.remove(ctx, &pod);
    let ended: Value = serde_json::from_slice(&ended).map_err(|error| error.to_string())?;
    let state = match ended["exit_code"].as_i64() {
        Some(0) => State::Passed,
        Some(_) => State::Failed,
        None => State::Errored,
    };
    let mut summary = ended["line"].as_str().unwrap_or_default().to_owned();
    let (log, missed) = copy_out(ctx, check, &pod).await;
    if let Some(missed) = missed {
        summary.push_str(&format!("; {missed}"));
    }
    Ok(Ending {
        state,
        summary,
        log,
    })
}

// The check's repository at its commit in `/work/<repo>`, its inputs, and its steps run there in
// order, stopping at the first that fails, their output in `/out/log`.
fn pod_body(check: &CheckRow) -> Value {
    let work = format!("/work/{}", check.repo);
    let mut inputs = vec![json!({
        "path": work,
        "forge": {"repo": check.repo, "commit": check.commit},
    })];
    for input in &check.job.inputs {
        inputs.push(json!({"path": input.path, "files": {"path": input.files}}));
    }
    let env: serde_json::Map<String, Value> = check
        .job
        .env
        .iter()
        .map(|(name, value)| (name.clone(), Value::from(value.as_str())))
        .collect();
    json!({
        "image": check.job.image,
        "command": ["sh", "-c", script(&work, &check.job.steps)],
        "env": env,
        "inputs": inputs,
    })
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

// Every file under the pod's `/out` into files, under
// `/shared/actions/<repo>/<commit>/<workflow>/<job>-<attempt>/`: where its log is, and what
// was not copied.
async fn copy_out(ctx: &Ctx, check: &CheckRow, pod: &str) -> (Option<String>, Option<String>) {
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
    let files: Vec<String> = listed["stdout"]
        .as_str()
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect();
    let mut missed = Vec::new();
    let mut log = None;
    for file in &files {
        let Some(relative) = file.strip_prefix("/out/") else {
            continue;
        };
        let to = format!("{dir}/{relative}");
        let message = format!("actions: check {} of {}", check.id, check.repo);
        let copy = json!({"path": file, "to": to, "container": "out", "message": message});
        match post(ctx, format!("/v1/pods/{pod}/file"), &copy).await {
            Ok(_) if relative == "log" => log = Some(to),
            Ok(_) => {}
            Err(why) => missed.push(format!("{file}: {why}")),
        }
    }
    let missed = (!missed.is_empty()).then(|| format!("not copied: {}", missed.join("; ")));
    (log, missed)
}

async fn post(ctx: &Ctx, target: String, body: &Value) -> Result<Value, String> {
    let bytes = serde_json::to_vec(body).map_err(|error| error.to_string())?;
    let part = ctx
        .blob_put(HashKind::Sha256, "part", bytes)
        .map_err(|refused| refused.message)?;
    let answer = ask(
        ctx,
        http::Method::Post,
        target.clone(),
        vec![part],
        Delivery::Whole,
    )
    .await
    .map_err(|refused| refused.message)?;
    if !(200..300).contains(&answer.status) {
        return Err(refusal(&target, &answer));
    }
    serde_json::from_slice(&answer.body).map_err(|error| error.to_string())
}

fn refusal(what: &str, answer: &http::Response) -> String {
    let error = serde_json::from_slice::<Value>(&answer.body)
        .ok()
        .and_then(|body| body["error"].as_str().map(str::to_owned))
        .unwrap_or_else(|| String::from_utf8_lossy(&answer.body).into_owned());
    format!("{what} answered {}: {error}", answer.status)
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
    // a check another reporter already ended keeps what it said
    let _ = ctx.call_raw(actions::PROGRAM, abi::encode(&report)).await;
}

// An `ended` event keeps its data for the run that waits on its pod; an `alive` one has done its
// work by landing, which moved the wait's expiry on.
pub(crate) fn heard(ctx: &ExecCtx, event: Event) -> Result<(), Error> {
    let egress = ctx.env().role(abi::role::EGRESS)?.clone();
    if ctx.env().sending_program()? != egress {
        return Err(unauthorized("a wait's events come from the egress role"));
    }
    let Some(data) = ended_data(&event.data) else {
        return Ok(());
    };
    let pod = waited_on(ctx, &egress, event.id)?;
    ENDED.put(ctx, &pod, &data);
    Ok(())
}

// An event is its raw lines up to the blank line.
fn ended_data(raw: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(raw).ok()?;
    let ended = text.lines().any(|line| line == "event: ended");
    let data = text.lines().find_map(|line| line.strip_prefix("data: "));
    ended
        .then(|| data.map(|data| data.as_bytes().to_vec()))
        .flatten()
}

// The pod a wait request names in its target, `/v1/pods/<pod>/wait?every=…`.
fn waited_on(ctx: &QueryCtx, egress: &ProgramId, id: u64) -> Result<String, Error> {
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
