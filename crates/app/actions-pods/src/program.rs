//! The program: it registers with actions in `init`, then awaits egress as each check runs.
use ::program::{Awaiting, Ctx, Error, ExecCtx, Program, QueryCtx, Reply as Wanted, decoded};

use crate::rules::{RUNS, heard, offered, run};
use crate::{Config, Op, PROGRAM, Query, Reply};

pub struct ActionsPods;

impl Program for ActionsPods {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;
    const TABLES: &'static [::program::Table] = crate::rules::TABLES;

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
        }
    }
}

impl Awaiting for ActionsPods {
    async fn execute(ctx: Ctx, op: Op) -> Result<(), Error> {
        match op {
            Op::Offer(offer) => offered(ctx, offer.check).await,
            Op::Run { check } => run(ctx, check).await,
            Op::Heard(event) => heard(&ctx, event),
        }
    }
}

#[cfg(feature = "program")]
::program::export_awaiting!(ActionsPods);
