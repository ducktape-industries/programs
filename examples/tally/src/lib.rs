//! tally: one count anyone adds to, kept per account and in total. The example program of
//! docs/ducktape-for-self-improvement.md, which its view, tally-view, reads and writes.
mod examples;
#[cfg(test)]
mod tests;

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use program::schema::Return;
use program::tools::{Described, Tools};
use program::{AccountNumber, Error, ExecCtx, Program, QueryCtx, invalid};
use store::{Item, Map};

pub const PROGRAM: &str = "tally";

const TOTAL: Item<u64> = Item::new("total");
const ADDED: Map<AccountNumber, u64> = Map::new("added/");

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Described,
)]
#[tool_about("One count anyone adds to, kept per account and in total.")]
pub enum Op {
    #[tool_op(
        description = "Add to the count as your own account. Any account may. Returns the new total.",
        returns = u64,
        example = crate::examples::add
    )]
    Add { by: u64 },
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    ::program::Ask,
    ::program::Described,
)]
#[tool_about("The total, and what each account has added.")]
#[ask(Tally)]
pub enum Query {
    #[ask(Reply::Total(u64))]
    #[reads(TOTAL)]
    #[tool_query(description = "The total of every Add.", example = crate::examples::total)]
    Total,
    #[ask(Reply::Added(u64))]
    #[reads(ADDED)]
    #[tool_query(description = "What one account has added in all.", example = crate::examples::added)]
    Added { account: AccountNumber },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Total(u64),
    Added(u64),
}

pub struct Tally;

impl Program for Tally {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;
    const RETURNS: &'static [Return] = <Op as Described>::RETURNS;

    fn tools() -> Tools {
        Tools::of::<Op, Query>()
    }

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        let account = ctx.sender_account()?;
        match op {
            Op::Add { by } => add(ctx, account, by),
        }
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        Ok(match query {
            Query::Total => Reply::Total(TOTAL.get(ctx)?.unwrap_or(0)),
            Query::Added { account } => Reply::Added(ADDED.get(ctx, &account)?.unwrap_or(0)),
        })
    }
}

fn add(ctx: &ExecCtx, account: AccountNumber, by: u64) -> Result<(), Error> {
    let overflow = || invalid(format!("adding {by} would overflow the count"));
    let total = TOTAL
        .get(ctx)?
        .unwrap_or(0)
        .checked_add(by)
        .ok_or_else(overflow)?;
    let added = ADDED
        .get(ctx, &account)?
        .unwrap_or(0)
        .checked_add(by)
        .ok_or_else(overflow)?;
    TOTAL.put(ctx, &total);
    ADDED.put(ctx, &account, &added);
    ctx.set_return_data(program::abi::encode(&total));
    Ok(())
}

#[cfg(feature = "program")]
program::export!(Tally);

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let (title, fields) = match op {
        Op::Add { by } => (
            format!("Add {by} to the tally"),
            vec![field("by", Value::count(*by))],
        ),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);
