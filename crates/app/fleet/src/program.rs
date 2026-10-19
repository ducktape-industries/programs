//! The program: one match over every op and one over every query, each handed to its rule.
use ::program::schema::Return;
use ::program::tools::{Described, Tools};
use ::program::{Error, ExecCtx, MessageId, Outcome, Program, QueryCtx};

use crate::rules::{
    ask, beat, cancel, claim, decline, holds, lapse, machine, machines, place_queued, queued,
    ready, replied, report, requeue, run_row, unclaimed, workload_row,
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

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        match op {
            Op::Ask { spec } => ask(ctx, spec),
            Op::Ready {
                name,
                labels,
                slots,
                serves,
            } => ready(ctx, name, labels, slots, serves),
            Op::Claim { run, attempt } => claim(ctx, run, attempt),
            Op::Beat { name } => beat(ctx, name),
            Op::Report {
                run,
                attempt,
                state,
                summary,
                out,
            } => report(ctx, run, attempt, state, summary, out),
            Op::Decline { run, attempt, why } => decline(ctx, run, attempt, why),
            Op::Cancel { run } => cancel(ctx, run),
            Op::Requeue { run } => requeue(ctx, run),
            Op::Lapse { machine } => lapse(ctx, machine),
            Op::Unclaimed { run, attempt } => unclaimed(ctx, run, attempt),
            Op::Place { run } => place_queued(ctx, run),
        }
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
        })
    }
}

#[cfg(feature = "program")]
::program::export!(Fleet);
