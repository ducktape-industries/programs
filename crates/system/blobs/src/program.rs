//! The program: every op and every query, each handed to its rule.

use ::program::schema::{Return, returns};
use ::program::tools::Tools;
use ::program::{Error, ExecCtx, Program, QueryCtx};

use crate::rules::{bundle, held, put, receipt};
use crate::{Op, PROGRAM, Query, Reply};

pub struct Blobs;

impl Program for Blobs {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;
    const RETURNS: &'static [Return] = &[returns::<u64>("Put")];

    fn tools() -> Tools {
        Tools::of::<crate::tools::Ops, crate::tools::Queries>()
    }

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        match op {
            Op::Put { blobs } => put(ctx, blobs),
            Op::Receipt { bundle } => receipt(ctx, bundle),
        }
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        Ok(match query {
            Query::Held { blob } => Reply::Held(held(ctx, blob)?),
            Query::Bundle { n } => Reply::Bundle(bundle(ctx, n)?),
        })
    }
}

#[cfg(feature = "program")]
::program::export!(Blobs);
