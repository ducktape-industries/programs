//! The program: every frame the kernel tells it of, and every query, each handed to its rule.

use ::program::{Error, ExecCtx, Program, QueryCtx, hook};

use crate::rules::{acted, keep, migrate, unkept, used};
use crate::{Op, PROGRAM, Query, Reply};

pub struct Activity;

impl Program for Activity {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;

    fn tools() -> ::program::tools::Tools {
        ::program::tools::Tools::of::<Op, Query>()
    }

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        match op {
            Op::Migrate {
                height,
                first,
                last,
            } => migrate(ctx, height, first, last),
        }
    }

    fn hook(ctx: &ExecCtx, record: hook::Record) -> Result<(), Error> {
        keep(ctx, record)
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        Ok(match query {
            Query::Frames { .. } => return Err(unkept("every frame")),
            Query::Acted { account, page } => Reply::Frames(acted(ctx, account, &page)?),
            Query::Used { page } => Reply::Used(used(ctx, &page)?),
        })
    }
}

#[cfg(feature = "program")]
::program::export!(Activity);
