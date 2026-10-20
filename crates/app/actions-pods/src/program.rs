//! The program: it registers with actions in `init`, then awaits egress as each check runs.
use ::program::{Awaiting, Ctx, Error, ExecCtx, Program, QueryCtx, Reply as Wanted, decoded};

use crate::rules::{
    PODS, RUNS, concurrent, heard, offered, resume, run, set_concurrent, still_waiting, sweep,
};
use crate::{Config, Op, PROGRAM, Query, Reply};

pub struct ActionsPods;

impl Program for ActionsPods {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;

    fn init(ctx: &ExecCtx, params: &[u8]) -> Result<(), Error> {
        let config: Config = decoded(PROGRAM, "Config", params)?;
        let register = actions::Op::Register {
            labels: config.labels,
            deliver: crate::offered_as(),
        };
        ctx.emit(actions::PROGRAM, &register, Wanted::None);
        Ok(())
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        match query {
            Query::Run { check } => Ok(Reply::Run(RUNS.get(ctx, &check)?)),
            Query::Pods => Ok(Reply::Pods(PODS.all(ctx)?)),
            Query::Waiting => Ok(Reply::Waiting(still_waiting(ctx)?)),
            Query::Concurrent => Ok(Reply::Concurrent(concurrent(ctx)?)),
        }
    }
}

impl Awaiting for ActionsPods {
    async fn execute(ctx: Ctx, op: Op) -> Result<(), Error> {
        match op {
            Op::Offer(offer) => offered(ctx, offer.check).await,
            Op::Run { check } => run(ctx, check).await,
            Op::Heard(event) => heard(&ctx, event),
            Op::Sweep => sweep(ctx).await,
            Op::Resume { check } => resume(ctx, check).await,
            Op::SetConcurrent { most } => set_concurrent(ctx, most).await,
        }
    }
}

#[cfg(feature = "program")]
::program::export_awaiting!(ActionsPods);
