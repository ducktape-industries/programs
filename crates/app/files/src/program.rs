//! The program: every op acts as the account the host resolved, and every op and
//! query is handed to its function in `ops.rs`, `commit.rs` or `queries.rs`.
use ::program::{Error, ExecCtx, Program, QueryCtx};

use crate::commit::commit;
use crate::ops::{pin, unpin};
use crate::queries;
use crate::{Op, Query, Reply, carried};

pub struct Files;

impl Program for Files {
    const NAME: &'static str = crate::PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        let writer = ctx.sender()?;
        match op {
            Op::Stage { .. } => Err(carried()),
            Op::Commit {
                base,
                message,
                changes,
            } => commit(ctx, writer, base, message, changes),
            Op::Pin { name, snapshot } => pin(ctx, writer, name, snapshot),
            Op::Unpin { name } => unpin(ctx, name),
        }
    }

    fn access(_: &QueryCtx, op: &Op) -> Result<Vec<::program::Need>, Error> {
        Ok(crate::gate::needs(op))
    }

    fn resource(ctx: &QueryCtx, key: &str) -> Result<::program::Resource, Error> {
        crate::gate::resource(ctx, key)
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        match query {
            Query::Head => Ok(Reply::Head(queries::head(ctx)?)),
            Query::Snapshot { id } => Ok(Reply::Snapshot(queries::snapshot(ctx, &id)?)),
            Query::History { page } => Ok(Reply::History(queries::history(ctx, &page)?)),
            Query::Stat { path, at } => {
                let (snapshot, entry) = queries::stat(ctx, &path, at.as_ref())?;
                Ok(Reply::Stat { snapshot, entry })
            }
            Query::List { path, at, page } => {
                let (snapshot, entries) = queries::list(ctx, &path, at.as_ref(), &page)?;
                Ok(Reply::Listing { snapshot, entries })
            }
            Query::Read {
                path,
                at,
                offset,
                len,
            } => Ok(Reply::Contents(queries::read(
                ctx,
                &path,
                at.as_ref(),
                offset,
                len,
            )?)),
            Query::Find { prefix, at, page } => {
                let (snapshot, entries) = queries::find(ctx, &prefix, at.as_ref(), &page)?;
                Ok(Reply::Listing { snapshot, entries })
            }
            Query::Grep {
                pattern,
                prefix,
                at,
                page,
            } => {
                let (snapshot, hits) = queries::grep(ctx, &pattern, &prefix, at.as_ref(), &page)?;
                Ok(Reply::Hits { snapshot, hits })
            }
            Query::Diff { from, to, prefix } => {
                Ok(Reply::Diff(queries::diff(ctx, &from, &to, &prefix)?))
            }
            Query::Pins { page } => Ok(Reply::Pins(queries::pins(ctx, &page)?)),
            Query::Pin { name } => Ok(Reply::Pin(queries::pin(ctx, &name)?)),
        }
    }
}

#[cfg(feature = "program")]
::program::export!(Files);
