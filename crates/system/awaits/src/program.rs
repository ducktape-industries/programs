//! The program: every op and every query, each handed to its rule.

use ::program::{Error, ExecCtx, Program, QueryCtx};

use crate::rules::{close, owed, park, resolve, ticket};
use crate::{Op, PROGRAM, Query, Reply};

pub struct Awaits;

impl Program for Awaits {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        match op {
            Op::Park {
                frame,
                program,
                calls,
            } => park(ctx, frame, program, calls),
            Op::Resolve { ticket, answer } => resolve(ctx, ticket, answer),
            Op::Close { frame } => close(ctx, frame),
        }
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        Ok(match query {
            Query::Ticket { ticket: number } => Reply::Ticket(ticket(ctx, number)?),
            Query::Owed {
                target,
                after,
                limit,
            } => Reply::Tickets(owed(ctx, target, after, limit)?),
        })
    }
}

#[cfg(feature = "program")]
::program::export!(Awaits);
