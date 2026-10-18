//! The program: forge watched from `init`, then one match over every op and one over every
//! query, each handed to its rule.
use ::program::{Error, ExecCtx, MessageId, Outcome, Program, QueryCtx, Reply as Wanted};

use crate::rules::{
    check, checks, claim, queued, register, report, rerun, run, runners, trigger, watched,
};
use crate::{Op, PROGRAM, Query, Reply};

pub struct Actions;

impl Program for Actions {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;

    fn init(ctx: &ExecCtx, _params: &[u8]) -> Result<(), Error> {
        let watch = forge::Op::Watch {
            deliver: crate::watching(),
        };
        ctx.emit(forge::PROGRAM, &watch, Wanted::None);
        Ok(())
    }

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        match op {
            Op::Watched(event) => watched(ctx, event),
            Op::Trigger { repo, commit } => trigger(ctx, &repo, &commit),
            Op::Register { labels, deliver } => register(ctx, labels, deliver),
            Op::Claim { check } => claim(ctx, check),
            Op::Report {
                check,
                state,
                summary,
                log,
            } => report(ctx, check, state, summary, log),
            Op::Rerun { check } => rerun(ctx, check),
        }
    }

    // A runner that refuses an offer leaves the check queued for another.
    fn reply(_ctx: &ExecCtx, _id: &MessageId, _outcome: &Outcome) -> Result<(), Error> {
        Ok(())
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        Ok(match query {
            Query::Check { id } => Reply::Check(check(ctx, id)?),
            Query::Checks { repo, commit, page } => {
                Reply::Checks(checks(ctx, &repo, &commit, &page)?)
            }
            Query::Queued { page } => Reply::Queued(queued(ctx, &page)?),
            Query::Run { id } => Reply::Run(run(ctx, id)?),
            Query::Runners { page } => Reply::Runners(runners(ctx, &page)?),
        })
    }
}

#[cfg(feature = "program")]
::program::export!(Actions);
