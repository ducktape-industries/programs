//! The program: every op and every query, each handed to its rule.

use ::program::{Error, ExecCtx, MessageId, Outcome, Program, QueryCtx, decoded};

use crate::rules::{MEMBERS, account_of, execute, init, memberships, motions, validators, voices};
use crate::{Genesis, Membership, Op, PROGRAM, Query, Reply};

pub struct Valset;

impl Program for Valset {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;

    fn init(ctx: &ExecCtx, params: &[u8]) -> Result<(), Error> {
        init(ctx, decoded::<Genesis>(PROGRAM, "Genesis", params)?)
    }

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        execute(ctx, op)
    }

    /// The only message this program sends with a reply wanted is a passed `Role` motion's
    /// op. Its refusal is taken, not passed on: the role's program wrote nothing, and the
    /// motion stays gone from the pending set, so a vote never enacts it again.
    fn reply(_ctx: &ExecCtx, _id: &MessageId, _outcome: &Outcome) -> Result<(), Error> {
        Ok(())
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        Ok(match query {
            Query::Validators => Reply::Validators(validators(ctx)?),
            Query::Members => {
                Reply::Members(memberships(ctx)?.iter().map(Membership::member).collect())
            }
            Query::Memberships { page } => {
                Reply::Memberships(MEMBERS.range(ctx, &page)?.map(|(_, membership)| membership))
            }
            Query::Membership { key } => Reply::Membership(MEMBERS.get(ctx, &key)?),
            Query::Motions { page } => motions(ctx, &page)?,
            // a query has no sender: the account is the key's own
            Query::Voices { key } => {
                let voices = voices(ctx, &validators(ctx)?, &key, || account_of(ctx, &key))?;
                Reply::Voices(voices)
            }
        })
    }
}

#[cfg(feature = "program")]
::program::export!(Valset);
