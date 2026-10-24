//! Exact receipt writes and reads, without outbound scheduling.
use ::program::schema::Return;
use ::program::tools::{Described, Tools};
use ::program::{Error, ExecCtx, MessageId, Outcome, Program, QueryCtx};

use crate::rules::{Patched, Reported, report, report_patch, reports};
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

    fn init(_ctx: &ExecCtx, _params: &[u8]) -> Result<(), Error> {
        Ok(())
    }

    fn reply(_ctx: &ExecCtx, _id: &MessageId, _outcome: &Outcome) -> Result<(), Error> {
        Ok(())
    }

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        match op {
            Op::Report {
                repo,
                commit,
                context,
                state,
                url,
                reach,
                change: _,
            } => report(
                ctx,
                Reported {
                    repo,
                    commit,
                    context,
                    state,
                    url,
                    reach,
                },
            ),
            Op::ReportPatch {
                repo,
                n: _,
                commit,
                context,
                state,
                url,
            } => report_patch(
                ctx,
                Patched {
                    repo,
                    commit,
                    context,
                    state,
                    url,
                },
            ),
            Op::Retell { .. } => Err(::program::wrong_state("result carry is retired")),
        }
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        Ok(match query {
            Query::Reports { repo, commit, page } => {
                Reply::Reports(reports(ctx, &repo, &commit, &page)?)
            }
            Query::Wanted { .. } => {
                return Err(::program::wrong_state("automatic re-request is retired"));
            }
        })
    }
}

#[cfg(feature = "program")]
::program::export!(ForgeReach);
