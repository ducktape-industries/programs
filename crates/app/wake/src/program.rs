//! The program: every op and query handed to its rule. An op that schedules, cancels or asks egress
//! awaits it; attribution's refusal to hear an end is kept on the wake (`rules::replied`).
use ::program::schema::Return;
use ::program::tools::{Described, Tools};
use ::program::{Awaiting, Ctx, Error, ExecCtx, MessageId, Outcome, Program, QueryCtx};

use crate::rules::{
    cancel, due, fanned, filed, forged, placed, pod_event, pod_wait, replied, wake_on, wake_row,
    wakes,
};
use crate::{Op, PROGRAM, Query, Reply};

pub struct Wake;

impl Program for Wake {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;
    const RETURNS: &'static [Return] = <Op as Described>::RETURNS;

    fn tools() -> Tools {
        Tools::of::<Op, Query>()
    }

    fn reply(ctx: &ExecCtx, id: &MessageId, outcome: &Outcome) -> Result<(), Error> {
        replied(ctx, id, outcome)
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        Ok(match query {
            Query::Wake { id } => Reply::Wake(wake_row(ctx, id)?),
            Query::Wakes { account, page } => Reply::Wakes(wakes(ctx, account, &page)?),
        })
    }
}

impl Awaiting for Wake {
    async fn execute(ctx: Ctx, op: Op) -> Result<(), Error> {
        match op {
            Op::WakeOn { event } => wake_on(&ctx, event).await,
            Op::Cancel { id } => cancel(&ctx, id).await,
            Op::Due => due(&ctx).await,
            Op::PodWait { id } => pod_wait(&ctx, id).await,
            Op::PodEvent { id, event } => pod_event(&ctx, id, event),
            Op::Forged(event) => forged(&ctx, event),
            Op::Filed(row) => filed(&ctx, row),
            Op::Fan { firing } => fanned(&ctx, firing),
            Op::Placed(event) => placed(&ctx, event),
        }
    }
}

#[cfg(feature = "program")]
::program::export_awaiting!(Wake);
