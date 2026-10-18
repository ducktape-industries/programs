//! The program: every op and every query, each handed to its rule.

use ::program::schema::{Return, returns};
use ::program::{EntryId, Error, ExecCtx, Program, QueryCtx};

use crate::rules::{cancel, due, entry, ran, schedule};
use crate::{Op, PROGRAM, Query, Reply};

pub struct Scheduler;

impl Program for Scheduler {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;
    const TABLES: &'static [::program::Table] = crate::state::TABLES;
    const RETURNS: &'static [Return] = &[returns::<EntryId>("Schedule")];

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        match op {
            Op::Schedule {
                height,
                target,
                payload,
            } => schedule(ctx, height, target, payload),
            Op::Cancel { entry: id } => cancel(ctx, id),
            Op::Ran { entry: id, receipt } => ran(ctx, id, receipt),
        }
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        match query {
            Query::Due {
                height,
                after,
                limit,
            } => due(ctx, height, after, limit),
            Query::Entry { id } => Ok(Reply::Entry(entry(ctx, id)?)),
        }
    }
}

#[cfg(feature = "program")]
::program::export!(Scheduler);
