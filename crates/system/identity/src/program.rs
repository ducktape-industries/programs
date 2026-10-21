//! The program: every op and every query, each handed to its rule.

use ::program::schema::Return;
use ::program::tools::{Described, Tools};
use ::program::{Error, ExecCtx, Program, QueryCtx};

use crate::rules::{
    ACCOUNTS, ANSWERS, MANAGED, OF_PROGRAM, account, add_key, answers, create, create_agent,
    generation, hears, migrate, needs, of_key, profiles, register_program, remove_key,
    rename_program, resolve, resource, resume, revoke, runs, set_manager, set_name, set_ownership,
    set_profile, set_script, suspend,
};
use crate::{Op, Query, Reply};

pub struct Identity;

impl Program for Identity {
    const NAME: &'static str = crate::PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;
    const RETURNS: &'static [Return] = <Op as Described>::RETURNS;

    fn tools() -> Tools {
        Tools::of::<Op, Query>()
    }

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        match op {
            Op::RegisterProgram { program } => register_program(ctx, program),
            Op::Create { name, scheme } => create(ctx, name, scheme),
            Op::CreateAgent { name } => create_agent(ctx, name),
            Op::AddKey {
                scheme,
                label,
                consent,
            } => add_key(ctx, scheme, label, consent),
            Op::RemoveKey { account, key } => remove_key(ctx, account, &key),
            Op::SetName { account, name } => set_name(ctx, account, name),
            Op::SetProfile {
                account,
                avatar,
                bio,
            } => set_profile(ctx, account, avatar, bio),
            Op::Suspend { account } => suspend(ctx, account),
            Op::Resume { account } => resume(ctx, account),
            Op::Revoke { account } => revoke(ctx, account),
            Op::SetOwnership { account, ownership } => set_ownership(ctx, account, ownership),
            Op::SetScript { account, script } => set_script(ctx, account, script),
            Op::RenameProgram { from, to } => rename_program(ctx, from, to),
            Op::Migrate {
                height,
                first,
                last,
            } => migrate(ctx, height, first, last),
            Op::SetManager { account, manager } => set_manager(ctx, account, manager),
        }
    }

    fn access(_: &QueryCtx, op: &Op) -> Result<Vec<::program::Need>, Error> {
        Ok(needs(op))
    }

    fn resource(ctx: &QueryCtx, key: &str) -> Result<::program::Resource, Error> {
        resource(ctx, key)
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        Ok(match query {
            Query::OfKey { key } => Reply::Number(of_key(ctx, &key)?),
            Query::OfProgram { program } => Reply::Number(OF_PROGRAM.get(ctx, &program)?),
            Query::Profile { number } => {
                Reply::Profile(ACCOUNTS.get(ctx, &number)?.map(|account| account.profile()))
            }
            Query::Profiles { after, limit } => profiles(ctx, after, limit)?,
            Query::Runs { program, account } => Reply::Number(runs(ctx, &program, account)?),
            Query::Get { number } => Reply::Account(ACCOUNTS.get(ctx, &number)?),
            Query::Generation { key } => Reply::Generation(generation(ctx, &key)?),
            Query::Resolve { references } => Reply::Resolved(
                references
                    .iter()
                    .map(|reference| resolve(ctx, reference))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Query::List { page } => {
                Reply::Accounts(ACCOUNTS.range(ctx, &page)?.map(|(_, account)| account))
            }
            Query::Managed { by, page } => Reply::Accounts(
                MANAGED
                    .range_of(ctx, &by, &page)?
                    .try_map(|(_, number)| account(ctx, number))?,
            ),
            Query::Answers { account } => Reply::Answers(answers(ctx, account)?),
            Query::Hears { account, from } => Reply::Hears(hears(ctx, account, from)?),
            Query::Narrowed { page } => Reply::Narrowed(ANSWERS.range(ctx, &page)?),
        })
    }
}

#[cfg(feature = "program")]
::program::export!(Identity);
