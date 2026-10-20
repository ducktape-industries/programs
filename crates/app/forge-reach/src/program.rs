//! The program: forge watched from `init`, then one match over every op and one over every
//! query, each handed to its rule.
use ::program::schema::Return;
use ::program::tools::{Described, Tools};
use ::program::{Error, ExecCtx, Program, QueryCtx, Reply as Wanted};

use crate::rules::{Reported, report, reports, verdict, watched};
use crate::{Op, PROGRAM, Query, Reply};

pub struct ForgeReach;

impl Program for ForgeReach {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;
    const RETURNS: &'static [Return] = <Op as Described>::RETURNS;

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
            Op::Report {
                repo,
                commit,
                context,
                state,
                url,
                reach,
                change,
            } => report(
                ctx,
                Reported {
                    repo,
                    commit,
                    context,
                    state,
                    url,
                    reach,
                    change,
                },
            ),
        }
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        Ok(match query {
            Query::Reports { repo, commit, page } => {
                Reply::Reports(reports(ctx, &repo, &commit, &page)?)
            }
            Query::Verdict { repo, n } => Reply::Verdict(verdict(ctx, &repo, n)?),
        })
    }
}

#[cfg(feature = "program")]
::program::export!(ForgeReach);
