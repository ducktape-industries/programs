//! What each harness op needs of the access program, and who owns each key: an agent, its
//! conversation and its threads by its manager. NewThread by the agent itself needs nothing, nor
//! Close of a bound or closed one; Renew and other Close need the agent or its thread's key.
use ::program::{Error, Need, Ownership, Party, Principal, QueryCtx, Resource, unexpected_reply};
use abi::role::identity::{AccountNumber, Kind};

use crate::Op;
use crate::agents::harnessed_agent;
use crate::conversations::binding;
use crate::state::THREADS;

pub fn agent_key(agent: AccountNumber) -> String {
    format!("agents/{agent}")
}

pub fn conversation_key(agent: AccountNumber) -> String {
    format!("agents/{agent}/conversation")
}

pub fn thread_key(thread: u64) -> String {
    format!("threads/{thread}")
}

pub(crate) fn sent_by_agent(ctx: &QueryCtx, agent: AccountNumber) -> bool {
    ctx.env().sender == Some(Principal::Account(agent))
}

pub(crate) fn needs(ctx: &QueryCtx, op: &Op) -> Result<Vec<Need>, Error> {
    Ok(match op {
        Op::Close { thread } => match open_to_anyone(ctx, *thread)? {
            true => Vec::new(),
            false => thread_needs(ctx, *thread)?,
        },
        Op::Renew { thread } => thread_needs(ctx, *thread)?,
        Op::NewThread { agent } if sent_by_agent(ctx, *agent) => Vec::new(),
        Op::NewThread { agent } | Op::ForkThread { agent } => match harnessed_agent(ctx, *agent)? {
            Some(_) => vec![Need::write(conversation_key(*agent))],
            None => Vec::new(),
        },
        Op::Attributed(_)
        | Op::Streamed { .. }
        | Op::OpenThread { .. }
        | Op::Book { .. }
        | Op::Unbook { .. }
        | Op::RunBooked
        | Op::Resume { .. }
        | Op::Retry { .. }
        | Op::Report { .. }
        | Op::Settle { .. }
        | Op::Watch { .. }
        | Op::Heard { .. }
        | Op::Wait { .. }
        | Op::Unwait { .. }
        | Op::ClaimConversations { .. }
        | Op::AdoptOwners { .. }
        | Op::SetRoute { .. }
        | Op::RemoveRoute { .. }
        | Op::AdoptTallies { .. }
        | Op::HoldSealed
        | Op::RunEnded { .. }
        | Op::RetirePromises { .. }
        | Op::Sweep => Vec::new(),
    })
}

fn thread_needs(ctx: &QueryCtx, thread: u64) -> Result<Vec<Need>, Error> {
    let by_agent = THREADS
        .get(ctx, &thread)?
        .is_some_and(|row| sent_by_agent(ctx, row.agent));
    Ok(match by_agent {
        true => Vec::new(),
        false => vec![Need::write(thread_key(thread))],
    })
}

fn open_to_anyone(ctx: &QueryCtx, thread: u64) -> Result<bool, Error> {
    let Some(row) = THREADS.get(ctx, &thread)? else {
        return Ok(false);
    };
    if row.closed {
        return Ok(true);
    }
    Ok(binding(ctx, &row)?.is_some())
}

pub(crate) fn resource(ctx: &QueryCtx, key: &str) -> Result<Resource, Error> {
    let owner = match key.split_once('/') {
        Some(("agents", agent)) => match agent.trim_end_matches("/conversation").parse() {
            Ok(agent) => manager(ctx, agent)?,
            Err(_) => None,
        },
        Some(("threads", thread)) => match thread.parse() {
            Ok(thread) => THREADS.get(ctx, &thread)?.map(|row| row.manager),
            Err(_) => None,
        },
        _ => None,
    };
    Ok(Resource {
        owner: owner.map(Party::Account),
        default: Some(Ownership::Owner),
    })
}

fn manager(ctx: &QueryCtx, agent: AccountNumber) -> Result<Option<AccountNumber>, Error> {
    use abi::role::identity::{Query, Reply};
    let identity = &ctx.env().role(abi::role::IDENTITY)?.clone();
    let profile = match ctx.query::<Query, Reply>(identity, &Query::Profile(agent))? {
        Reply::Profile(profile) => profile,
        other => return Err(unexpected_reply(identity, "Profile", &other)),
    };
    Ok(profile.and_then(|profile| match profile.kind {
        Kind::Program { manager, .. } => Some(manager),
        Kind::User | Kind::System(_) => None,
    }))
}
