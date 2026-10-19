//! The program: every op and every query, each handed to its rule. A program that refuses a
//! change handed to it is absorbed, so it never fails the write that reported the change.
use ::program::{Error, ExecCtx, MessageId, Outcome, Program, QueryCtx};

use crate::rules::{attribute, changes, changes_of, changes_to, register, registered, relations};
use crate::{Op, PROGRAM, Query, Reply};

pub struct Attribution;

impl Program for Attribution {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        match op {
            Op::Attribute(update) => attribute(ctx, vec![update]),
            Op::AttributeBatch { updates } => attribute(ctx, updates),
            Op::Register { deliver } => register(ctx, deliver),
        }
    }

    fn reply(_ctx: &ExecCtx, _id: &MessageId, _outcome: &Outcome) -> Result<(), Error> {
        Ok(())
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        match query {
            Query::Relations { source } => Ok(Reply::Relations(relations(ctx, &source)?)),
            Query::Changes { page } => Ok(Reply::Changes(changes(ctx, &page)?)),
            Query::ChangesTo { recipient, page } => {
                Ok(Reply::Changes(changes_to(ctx, recipient, &page)?))
            }
            Query::ChangesOf { source, page } => {
                Ok(Reply::Changes(changes_of(ctx, &source, &page)?))
            }
            Query::Registered => Ok(Reply::Registered(registered(ctx)?)),
        }
    }
}

#[cfg(feature = "program")]
::program::export!(Attribution);
