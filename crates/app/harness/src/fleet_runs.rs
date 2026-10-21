//! Runs an agent asks fleet for with the `fleet_run` tool: fleet tells harness when one ends or is
//! lost (`Op::RunEnded`), and the conversation that asked hears its state, summary and out folder.
use ::program::{Ctx, Error, ExecCtx, QueryCtx, not_found, unauthorized, unexpected_reply};

use crate::bookings::tell;
use crate::state::FLEET_RUNS;

pub(crate) fn asked(ctx: &ExecCtx, thread: u64, run: u64) {
    FLEET_RUNS.insert(ctx, &(thread, run));
}

fn ask(ctx: &QueryCtx, query: fleet::Query) -> Result<fleet::Reply, Error> {
    ctx.query::<fleet::Query, fleet::Reply>(fleet::PROGRAM, &query)
}

fn asker(ctx: &QueryCtx, run: u64) -> Result<u64, Error> {
    match ask(ctx, fleet::Query::Workload { id: run })? {
        fleet::Reply::Workload(Some(workload)) => Ok(workload.by),
        fleet::Reply::Workload(None) => Err(not_found(format!("fleet holds no run {run}"))),
        other => Err(unexpected_reply(fleet::PROGRAM, "Workload", &other)),
    }
}

fn row(ctx: &QueryCtx, run: u64) -> Result<Option<fleet::RunRow>, Error> {
    match ask(ctx, fleet::Query::Run { id: run })? {
        fleet::Reply::Run(row) => Ok(row),
        other => Err(unexpected_reply(fleet::PROGRAM, "Run", &other)),
    }
}

pub(crate) async fn ended(ctx: &Ctx, thread: u64, ended: fleet::RunEnd) -> Result<(), Error> {
    let fleet::RunEnd {
        run,
        attempt,
        state,
    } = ended;
    ctx.env().sent_by(fleet::PROGRAM)?;
    let agent = crate::agents::thread(ctx, thread)?.agent;
    let asker = asker(ctx, run)?;
    if asker != agent {
        return Err(unauthorized(format!(
            "run {run} was asked for by account {asker}, and conversation {thread} is account {agent}'s"
        )));
    }
    if state.ended() {
        FLEET_RUNS.remove(ctx, &(thread, run));
    }
    let row = row(ctx, run)?.filter(|row| row.attempt == attempt);
    tell(ctx, agent, Some(thread), told(run, attempt, state, row)).await
}

fn told(run: u64, attempt: u32, state: fleet::State, row: Option<fleet::RunRow>) -> String {
    let mut text = match state {
        fleet::State::Lost => format!(
            "Your run {run} was lost at attempt {attempt}: its machine went silent, and fleet holds it until that machine reports it or you queue it again (fleet Requeue)"
        ),
        _ => format!("Your run {run} {} at attempt {attempt}", state.word()),
    };
    let Some(row) = row else {
        return text;
    };
    if !row.summary.is_empty() {
        text.push_str(&format!(": {}", row.summary));
    }
    if let Some(out) = row.out {
        text.push_str(&format!(
            ". Its log and outputs are in files at {} (snapshot {})",
            out.path,
            abi::hex(out.snapshot.digest())
        ));
    }
    text
}
