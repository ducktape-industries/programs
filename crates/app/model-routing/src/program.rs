//! The program: one match over every op and one over every query, each handed to its rule.
use ::program::schema::Return;
use ::program::tools::{Described, Tools};
use ::program::{Error, ExecCtx, MessageId, Outcome, Program, QueryCtx};

use crate::rules::{available, choose, forget, full, fulls, pin, pins, pool, pools};
use crate::{Op, PROGRAM, Query, Reply};

pub struct ModelRouting;

impl Program for ModelRouting {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;
    const RETURNS: &'static [Return] = <Op as Described>::RETURNS;

    fn tools() -> Tools {
        Tools::of::<Op, Query>()
    }

    fn init(_ctx: &ExecCtx, _params: &[u8]) -> Result<(), Error> {
        Ok(())
    }

    fn reply(_ctx: &ExecCtx, _id: &MessageId, _outcome: &Outcome) -> Result<(), Error> {
        Ok(())
    }

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        let consumer = ctx.sender_account()?;
        match op {
            Op::Pool {
                provider,
                service,
                accounts,
            } => pool(ctx, (consumer, provider, service), accounts),
            Op::Full {
                provider,
                service,
                account,
                until,
            } => full(ctx, (consumer, provider, service, account), until),
            Op::Available {
                provider,
                service,
                account,
                read_at,
            } => available(ctx, (consumer, provider, service, account), read_at),
            Op::Pin {
                conversation,
                provider,
                services,
            } => pin(ctx, (consumer, conversation, provider), &services),
            Op::Forget { conversation } => forget(ctx, consumer, conversation),
        }
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        Ok(match query {
            Query::Resolve {
                consumer,
                conversation,
                provider,
                services,
            } => Reply::Choice(choose(ctx, &(consumer, conversation, provider), &services)?),
            Query::Pools { consumer, page } => Reply::Pools(pools(ctx, consumer, &page)?),
            Query::Fulls { consumer, page } => Reply::Fulls(fulls(ctx, consumer, &page)?),
            Query::Pins { consumer, page } => Reply::Pins(pins(ctx, consumer, &page)?),
        })
    }
}

#[cfg(feature = "program")]
::program::export!(ModelRouting);
