//! The program: the schedule folded at each block, then every op and every
//! query, each handed to its rule.

use ::program::{Error, ExecCtx, Program, QueryCtx, decoded};

use crate::rules::{
    BY, GRANTS, Roster, SCHEDULE, at, cancel, fold, grant, hooks_at, init, revoke, roles_at,
    roster, schedule, views_at,
};
use crate::{Genesis, Grant, Op, PROGRAM, PageResponse, Pending, Query, Reply};

pub struct Programs;

impl Program for Programs {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;

    fn init(ctx: &ExecCtx, params: &[u8]) -> Result<(), Error> {
        init(ctx, decoded::<Genesis>(PROGRAM, "Genesis", params)?);
        Ok(())
    }

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        fold(ctx, ctx.env().height)?;
        match op {
            Op::Schedule(scheduled) => schedule(ctx, scheduled),
            Op::Cancel { height, program } => cancel(ctx, height, program),
            Op::Grant(granted) => grant(ctx, granted),
            Op::Revoke(granted) => revoke(ctx, granted),
        }
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        let height = ctx.env().height;
        Ok(match query {
            Query::At(height) => Reply::Programs(at(ctx, height)?),
            Query::Views(height) => Reply::Views(views_at(ctx, height)?),
            Query::Scheduled { page } => {
                let PageResponse {
                    height,
                    items,
                    next,
                } = SCHEDULE.range(ctx, &page)?;
                let items = items
                    .into_iter()
                    .map(|(at, change)| {
                        let by = BY.get(ctx, &at)?;
                        Ok(Pending {
                            height: at.0,
                            change,
                            by,
                        })
                    })
                    .collect::<Result<_, Error>>()?;
                Reply::Scheduled(PageResponse {
                    height,
                    items,
                    next,
                })
            }
            Query::Program(program) => Reply::Program {
                height,
                entry: at(ctx, height)?
                    .into_iter()
                    .find(|entry| entry.program == program),
            },
            Query::Roles => Reply::Roles(roles_at(ctx, height)?),
            Query::Hooks(height) => Reply::Hooks(hooks_at(ctx, height)?),
            Query::Grants { page } => Reply::Grants(
                GRANTS
                    .range(ctx, &page)?
                    .map(|(scope, account)| Grant { scope, account }),
            ),
            Query::Roster => {
                let Roster {
                    programs, views, ..
                } = roster(ctx, height, &[])?;
                Reply::Roster {
                    height,
                    programs: programs.into_values().collect(),
                    views: views.into_values().collect(),
                }
            }
        })
    }
}

#[cfg(feature = "program")]
::program::export!(Programs);
