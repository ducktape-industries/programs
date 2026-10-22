//! The program: the schedule folded at each block, then every op and every
//! query, each handed to its rule.

use ::program::tools::Tools;
use ::program::{Error, ExecCtx, Program, QueryCtx, decoded};

use crate::rules::{
    BY, Roster, at, cancel, fold, holder_of, hooks_at, init, own, renames_at, roles_at, roster,
    schedule_over, schedule_over_in, sources, to_land, views_at,
};
use crate::{Genesis, Op, PROGRAM, PageResponse, Pending, Query, Reply};

pub struct Programs;

impl Program for Programs {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;

    fn tools() -> Tools {
        Tools::of::<Op, Query>()
    }

    fn init(ctx: &ExecCtx, params: &[u8]) -> Result<(), Error> {
        init(ctx, decoded::<Genesis>(PROGRAM, "Genesis", params)?);
        Ok(())
    }

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        fold(ctx, ctx.env().height)?;
        match op {
            Op::Cancel { height, program } => cancel(ctx, height, program),
            Op::Own { program, holder } => own(ctx, program, holder),
            Op::ScheduleOver {
                scheduled,
                expected,
            } => schedule_over(ctx, scheduled, expected),
            Op::ScheduleOverIn {
                blocks,
                change,
                expected,
            } => schedule_over_in(ctx, blocks, change, expected),
        }
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        let height = ctx.env().height;
        Ok(match query {
            Query::At(height) | Query::ProgramsAt { at_height: height } => {
                Reply::Programs(at(ctx, height)?)
            }
            Query::ViewsAt { at_height: height } => Reply::Views(views_at(ctx, height)?),
            Query::Scheduled { page } => {
                let PageResponse {
                    height,
                    items,
                    next,
                } = to_land(ctx, &page)?;
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
            Query::Hooks(height) | Query::HooksAt { at_height: height } => {
                Reply::Hooks(hooks_at(ctx, height)?)
            }
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
            Query::Sources => Reply::Sources {
                height,
                programs: sources(ctx, height)?,
            },
            Query::Owner(program) => Reply::Owner {
                height,
                holder: holder_of(ctx, &program)?,
            },
            Query::Renames(height) => Reply::Renames(renames_at(ctx, height)?),
        })
    }
}

#[cfg(feature = "program")]
::program::export!(Programs);
