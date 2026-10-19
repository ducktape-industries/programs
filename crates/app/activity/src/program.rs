//! The program: every frame the kernel tells it of, and every query, each handed to its rule.

use ::program::{Error, ExecCtx, Program, QueryCtx, code, hook};

use crate::rules::{acted, frames, keep, signed, used};
use crate::{Op, PROGRAM, Query, Reply};

pub struct Activity;

impl Program for Activity {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;
    const TABLES: &'static [::program::Table] = crate::rules::TABLES;

    fn tools() -> ::program::tools::Tools {
        ::program::tools::Tools::of::<Op, Query>()
    }

    fn execute(_ctx: &ExecCtx, (): Op) -> Result<(), Error> {
        Err(Error::new(
            code::UNSUPPORTED,
            "activity takes no op: the kernel tells it of each frame",
        ))
    }

    fn hook(ctx: &ExecCtx, record: hook::Record) -> Result<(), Error> {
        keep(ctx, record)
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        Ok(match query {
            Query::Frames { page } => Reply::Frames(frames(ctx, &page)?),
            Query::Acted { account, page } => Reply::Frames(acted(ctx, account, &page)?),
            Query::Signed { key, page } => Reply::Frames(signed(ctx, key, &page)?),
            Query::Used { page } => Reply::Used(used(ctx, &page)?),
        })
    }
}

#[cfg(feature = "program")]
::program::export!(Activity);
