//! The program: forge watched from `init`, then one match over every op and one over every query,
//! each handed to its rule.
use ::program::schema::Return;
use ::program::tools::{Described, Tools};
use ::program::{Error, ExecCtx, MessageId, Outcome, Program, QueryCtx, Reply as Wanted};

use crate::rules::{mirrored, room_of, rules_of, set_rules, watched};
use crate::{Op, PROGRAM, Query, Reply};

pub struct ForgeReviewChat;

impl Program for ForgeReviewChat {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;
    const RETURNS: &'static [Return] = <Op as Described>::RETURNS;

    fn tools() -> Tools {
        Tools::of::<Op, Query>()
    }

    fn init(ctx: &ExecCtx, _params: &[u8]) -> Result<(), Error> {
        let watch = forge::Op::Watch {
            deliver: crate::watching(),
        };
        ctx.emit(forge::PROGRAM, &watch, Wanted::None);
        Ok(())
    }

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        match op {
            Op::Watched(event) => watched(ctx, event),
            Op::SetRules { rules } => set_rules(ctx, rules),
        }
    }

    fn reply(_ctx: &ExecCtx, _id: &MessageId, _outcome: &Outcome) -> Result<(), Error> {
        Ok(())
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        Ok(match query {
            Query::Rules => Reply::Rules(rules_of(ctx)?),
            Query::Room { repo, n } => Reply::Room(room_of(ctx, repo, n)?),
            Query::Mirrored { page } => Reply::Mirrored(mirrored(ctx, &page)?),
        })
    }
}

#[cfg(feature = "program")]
::program::export!(ForgeReviewChat);
