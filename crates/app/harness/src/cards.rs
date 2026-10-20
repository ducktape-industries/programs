//! `Cards`: every agent harness runs as a list of agents draws it, in one read: its setup without
//! its system prompt, its conversations counted, its newest and each open one asking with their
//! topics, and how many member nodes serve its model's service.
use ::program::{Error, QueryCtx, not_found, unexpected_reply};
use abi::role::egress;
use abi::role::identity::{self, AccountNumber};
use store::{PageRequest, PageResponse};

use crate::agents::{card, harnessed};
use crate::queries::{threads, topic};
use crate::{AgentCard, Asking, Spoken, ThreadRow};

// A page of the agents, in account order: the cursor is the last agent's number.
pub(crate) fn cards(ctx: &QueryCtx, page: &PageRequest) -> Result<PageResponse<AgentCard>, Error> {
    let listing = page.listing(ctx, PageRequest::scope_of(&"cards"))?;
    let agents = agents(ctx)?
        .into_iter()
        .map(|number| (number.to_be_bytes().to_vec(), number));
    listing
        .reply(agents)
        .try_map(|number| agent_card(ctx, number))
}

// every account whose script calls harness, in account order
fn agents(ctx: &QueryCtx) -> Result<Vec<AccountNumber>, Error> {
    let role = ctx.env().role(abi::role::IDENTITY)?.clone();
    let mut numbers = Vec::new();
    let mut after = None;
    loop {
        let asked = identity::Query::Profiles {
            after,
            limit: u32::MAX,
        };
        let (profiles, next) = match ctx.query::<identity::Query, identity::Reply>(&role, &asked)? {
            identity::Reply::Profiles { profiles, next } => (profiles, next),
            other => return Err(unexpected_reply(&role, "Profiles", &other)),
        };
        let harnessing = profiles
            .iter()
            .filter(|profile| harnessed(profile).is_some());
        numbers.extend(harnessing.map(|profile| profile.number));
        match next {
            Some(number) => after = Some(number),
            None => return Ok(numbers),
        }
    }
}

fn agent_card(ctx: &QueryCtx, number: AccountNumber) -> Result<AgentCard, Error> {
    let setup = card(ctx, number)?.ok_or_else(|| {
        not_found(format!(
            "account {number} is no agent whose script calls harness"
        ))
    })?;
    let rows = threads(ctx, number, &PageRequest::default())?.items;
    let spoken = |thread: &ThreadRow| -> Result<Spoken, Error> {
        Ok(Spoken {
            thread: thread.clone(),
            topic: topic(ctx, thread.id)?,
        })
    };
    let open = rows.iter().filter(|row| !row.closed).count() as u64;
    let asking = rows
        .iter()
        .filter(|row| !row.closed && row.asking != Asking::Idle)
        .map(spoken)
        .collect::<Result<Vec<_>, _>>()?;
    let serving = match setup.configured.reached() {
        Some(reached) => Some(serving(ctx, reached.service)?),
        None => None,
    };
    Ok(AgentCard {
        open,
        closed: rows.len() as u64 - open,
        last_time: rows.iter().map(|row| row.last_time).max().unwrap_or(0),
        newest: rows.first().map(spoken).transpose()?,
        asking,
        serving,
        setup,
    })
}

// how many member nodes the egress role elects for `service` now
fn serving(ctx: &QueryCtx, service: &str) -> Result<u64, Error> {
    let program = crate::turns::bound(ctx)?;
    let asked = egress::Query::Serving {
        service: service.into(),
    };
    match ctx.query(program.clone(), &asked)? {
        egress::Reply::Serving(keys) => Ok(keys.len() as u64),
        other => Err(unexpected_reply(&program, "Serving", &other)),
    }
}
