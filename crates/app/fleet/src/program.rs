//! The program: one match over every op and one over every query, each handed to its rule.
use ::program::schema::Return;
use ::program::tools::{Described, Tools};
use ::program::{Awaiting, Ctx, Error, ExecCtx, MessageId, Outcome, Program, QueryCtx};

use crate::rules::{
    ask, ask_in, beat, cancel, claim, decline, expire, filed_row, hear, holds, lapse, ledger,
    machine, machine_kind, machines, pieces, place_queued, promote, queued, ready, ready_in, relay,
    replied, report, requeue, run_row, unclaimed, walk, workload_row,
};
use crate::{Op, PROGRAM, Query, Reply};

pub struct Fleet;

impl Program for Fleet {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;
    const RETURNS: &'static [Return] = <Op as Described>::RETURNS;

    fn tools() -> Tools {
        Tools::of::<Op, Query>()
    }

    fn reply(ctx: &ExecCtx, id: &MessageId, outcome: &Outcome) -> Result<(), Error> {
        replied(ctx, id, outcome)
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        Ok(match query {
            Query::Workload { id } => Reply::Workload(workload_row(ctx, id)?),
            Query::Run { id } => Reply::Run(run_row(ctx, id)?),
            Query::Queued { page } => Reply::Runs(queued(ctx, &page)?),
            Query::Machine { machine: id } => Reply::Machine(machine(ctx, &id)?),
            Query::Machines { page } => Reply::Machines(machines(ctx, &page)?),
            Query::Holds { machine, page } => Reply::Runs(holds(ctx, &machine, &page)?),
            Query::Pieces {
                run,
                attempt,
                call,
                page,
            } => Reply::Pieces(pieces(ctx, run, attempt, call, &page)?),
            Query::Filed { run } => Reply::Filed(filed_row(ctx, run)?),
            Query::KindOf { machine } => Reply::Kind(machine_kind(ctx, &machine)?),
            Query::Ledger { kind, group, page } => Reply::Ledger(ledger(ctx, kind, group, &page)?),
        })
    }
}

impl Awaiting for Fleet {
    async fn execute(ctx: Ctx, op: Op) -> Result<(), Error> {
        match op {
            Op::Ask { spec } => ask(&ctx, spec),
            Op::Ready {
                name,
                labels,
                slots,
                serves,
            } => ready(&ctx, name, labels, slots, serves),
            Op::Claim { run, attempt } => claim(&ctx, run, attempt),
            Op::Beat { name } => beat(&ctx, name),
            Op::Report {
                run,
                attempt,
                state,
                summary,
                out,
            } => report(&ctx, run, attempt, state, summary, out),
            Op::Decline { run, attempt, why } => decline(&ctx, run, attempt, why),
            Op::Cancel { run } => cancel(&ctx, run),
            Op::Requeue { run } => requeue(&ctx, run),
            Op::Lapse { machine } => lapse(&ctx, machine),
            Op::Unclaimed { run, attempt } => unclaimed(&ctx, run, attempt),
            Op::Place { run } => place_queued(&ctx, run),
            Op::Call {
                run,
                attempt,
                call,
                request,
                stream,
                wait,
            } => relay(&ctx, run, attempt, call, request, stream, wait).await,
            Op::Heard {
                run,
                attempt,
                call,
                event,
            } => hear(&ctx, run, attempt, call, event),
            Op::AskIn {
                spec,
                kind,
                class,
                limit,
            } => ask_in(&ctx, spec, kind, class, limit),
            Op::ReadyIn {
                name,
                labels,
                slots,
                serves,
                kind,
            } => ready_in(&ctx, name, labels, slots, serves, kind),
            Op::Promote { run, class } => promote(&ctx, run, class),
            Op::Walk { kind } => walk(&ctx, kind),
            Op::Expire { run, attempt } => expire(&ctx, run, attempt),
        }
    }
}

#[cfg(feature = "program")]
::program::export_awaiting!(Fleet);
