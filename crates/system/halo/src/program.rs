//! The program: every op and every query, each handed to its rule.

use ::program::{Error, ExecCtx, Program, QueryCtx, decoded};

use crate::rules::{close, init, lane, lanes, open, set};
use crate::{Genesis, Op, PROGRAM, Query, Reply};

pub struct Halo;

impl Program for Halo {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;

    fn init(ctx: &ExecCtx, params: &[u8]) -> Result<(), Error> {
        init(ctx, decoded::<Genesis>(PROGRAM, "Genesis", params)?)
    }

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        ctx.env().authority()?;
        match op {
            Op::Open { name, cap } => open(ctx, name, cap),
            Op::Set { name, cap, enabled } => set(ctx, name, cap, enabled),
            Op::Close { name } => close(ctx, name),
        }
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        Ok(match query {
            Query::Lanes => Reply::Lanes(lanes(ctx)?),
            Query::Lane { name } => Reply::Lane(lane(ctx, &name)?),
        })
    }
}

#[cfg(feature = "program")]
::program::export!(Halo);
