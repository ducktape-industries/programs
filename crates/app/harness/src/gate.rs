//! What each harness op needs of the access program, and who owns each key: an agent
//! (`agents/<n>`), its conversation (`agents/<n>/conversation`) and its threads (`threads/<id>`)
//! by the agent's manager, a model (`models/<id>`) by the account that set it. Each is its
//! owner's alone unless it lists others.
use ::program::{Error, Need, Ownership, Party, QueryCtx, Resource, unexpected_reply};
use abi::role::identity::{AccountNumber, Kind};

use crate::Op;
use crate::agents::harnessed_agent;
use crate::state::{MODELS, THREADS};

pub fn agent_key(agent: AccountNumber) -> String {
    format!("agents/{agent}")
}

pub fn conversation_key(agent: AccountNumber) -> String {
    format!("agents/{agent}/conversation")
}

pub fn thread_key(thread: u64) -> String {
    format!("threads/{thread}")
}

pub fn model_key(id: &str) -> String {
    format!("models/{id}")
}

pub(crate) fn needs(ctx: &QueryCtx, op: &Op) -> Result<Vec<Need>, Error> {
    Ok(match op {
        Op::Close { thread } => vec![Need::write(thread_key(*thread))],
        Op::SetModel { id, .. } => match MODELS.has(ctx, id) {
            true => vec![Need::write(model_key(id))],
            false => Vec::new(),
        },
        Op::RemoveModel { id } => match MODELS.has(ctx, id) {
            true => vec![Need::delete(model_key(id))],
            false => Vec::new(),
        },
        Op::NewThread { agent } | Op::ForkThread { agent } => match harnessed_agent(ctx, *agent)? {
            Some(_) => vec![Need::write(conversation_key(*agent))],
            None => Vec::new(),
        },
        Op::Attributed(_) | Op::Streamed { .. } => Vec::new(),
    })
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
        Some(("models", id)) => MODELS.get(ctx, &id.to_owned())?.map(|row| row.owner),
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
