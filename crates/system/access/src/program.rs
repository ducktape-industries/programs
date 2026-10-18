//! The program: every op and every query, each handed to its rule.

use ::program::{Error, ExecCtx, Need, Program, QueryCtx, Resource};

use crate::rules::{check, list, lists, moved, resource_of, set, writers_of};
use crate::{Op, PROGRAM, Query, Reply};

pub struct Access;

impl Program for Access {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;
    const TABLES: &'static [::program::Table] = crate::rules::TABLES;

    fn init(_: &ExecCtx, _: &[u8]) -> Result<(), Error> {
        Ok(())
    }

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        match op {
            Op::Check {
                program,
                actor,
                needs,
            } => check(ctx, program, actor, needs),
            Op::Set { program, key, list } => set(ctx, program, key, list),
            Op::Move { program, from, to } => moved(ctx, program, &from, &to),
        }
    }

    fn access(_: &QueryCtx, _: &Op) -> Result<Vec<Need>, Error> {
        Ok(Vec::new())
    }

    fn resource(_: &QueryCtx, _: &str) -> Result<Resource, Error> {
        Ok(Resource::default())
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        Ok(match query {
            Query::List { program, key } => Reply::List(list(ctx, &program, &key)?),
            Query::Resource { program, key } => Reply::Resource(resource_of(ctx, &program, &key)?),
            Query::Writers { program, key } => Reply::Writers(writers_of(ctx, &program, &key)?),
            Query::Lists { program, page } => Reply::Lists(lists(ctx, &program, &page)?),
        })
    }
}

#[cfg(feature = "program")]
::program::export!(Access);
