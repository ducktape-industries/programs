//! The program: every op and every query, each handed to its rule.

use ::program::schema::{Return, returns};
use ::program::tools::Tools;
use ::program::{Error, ExecCtx, Program, QueryCtx};

use crate::rules::{self, create, edit, relink, retire, set_default, switch, transfer};
use crate::{Op, PROGRAM, PolicyId, Query, Reply};

pub struct Ownership;

impl Program for Ownership {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;
    const RETURNS: &'static [Return] = &[returns::<PolicyId>("Create")];

    fn tools() -> Tools {
        Tools::of::<Op, Query>()
    }

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        match op {
            Op::Guard(guard) => rules::guard(ctx, guard),
            Op::Create { name, write } => create(ctx, name, write),
            Op::Edit {
                policy,
                name,
                write,
            } => edit(ctx, policy, name, write),
            Op::Retire { policy } => retire(ctx, policy),
            Op::Switch { from, to } => switch(ctx, from, to),
            Op::Relink {
                program,
                key,
                policy,
            } => relink(ctx, program, key, policy),
            Op::Transfer { program, key, to } => transfer(ctx, program, key, to),
            Op::SetDefault { scope, policy } => set_default(ctx, scope, policy),
            Op::Adopt { items } => rules::adopt(ctx, items),
            Op::Moved { from_key, to_key } => rules::moved(ctx, from_key, to_key),
        }
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        Ok(match query {
            Query::Record { program, key } => Reply::Record(rules::record(ctx, program, key)?),
            Query::Effective { program, key } => {
                Reply::Policy(rules::effective_of(ctx, program, key)?)
            }
            Query::Policy { id } => Reply::Policy(rules::policy_of(ctx, id)?),
            Query::Policies { owner, page } => {
                Reply::Policies(rules::policies(ctx, &owner, &page)?)
            }
            Query::Linked { policy, page } => Reply::Linked(rules::linked(ctx, policy, &page)?),
            Query::Defaults { owner, page } => {
                Reply::Defaults(rules::defaults(ctx, &owner, &page)?)
            }
            Query::Open { owner } => Reply::Open(rules::open(ctx, &owner)?),
            Query::OnlyMe { owner } => Reply::OnlyMe(rules::only_me_of(ctx, &owner)?),
        })
    }
}

#[cfg(feature = "program")]
::program::export!(Ownership);
