//! The program: forge watched from `init`, then one match over every op and one over every
//! query, each handed to its rule.
use ::program::tools::Tools;
use ::program::{Error, ExecCtx, MessageId, Outcome, Program, QueryCtx, Reply as Wanted};

use crate::rules::{
    asks, check, checks, claim, needs, outputs, outputs_of, previewing, queued, register, replied,
    report, rerun, run, runners, runs, time_limit, trigger, watched,
};
use crate::{Op, PROGRAM, Query, Reply};

pub struct Actions;

impl Program for Actions {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;

    fn tools() -> Tools {
        Tools::of::<Op, Query>()
    }

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
            Op::Preview { repo, n } => previewing(ctx, &repo, n),
            Op::Outputs { check, files } => outputs(ctx, check, files),
        }
    }

    // A runner that refuses an offer errs its check, and a Preview forge refuses makes an errored
    // run, each saying why (`rules::replied`).
    fn reply(ctx: &ExecCtx, id: &MessageId, outcome: &Outcome) -> Result<(), Error> {
        replied(ctx, id, outcome)
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
            Query::Asks { check } => Reply::Asks(asks(ctx, check)?),
            Query::Needs { check } => Reply::Needs(needs(ctx, check)?),
            Query::Outputs { check } => Reply::Outputs(outputs_of(ctx, check)?),
            Query::Runs { repo, event, page } => Reply::Runs(runs(ctx, &repo, &event, &page)?),
            Query::TimeLimit { check } => Reply::TimeLimit(time_limit(ctx, check)?),
        })
    }
}

#[cfg(feature = "program")]
::program::export!(Actions);
