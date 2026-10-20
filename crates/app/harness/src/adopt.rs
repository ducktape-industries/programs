//! One-time (J-011 ⑥): `AdoptOwners` records an owner for each model harness kept before the
//! ownership role was seated, and gives every agent a conversations row its manager owns, each
//! linked to the policy matching what the access program enforces today. Goes once duckpond ran it.
use ::program::{Error, ExecCtx, QueryCtx, Reply, invalid, unexpected_reply};
use abi::role::identity::{self, AccountNumber};
use ownership::{Adopted, Party, adopt, by_itself, rule_of, writers_of};

use crate::agents::harnessed_agent;
use crate::gate::{conversation_key, model_key, resource};
use crate::state::{CONVERSATIONS, Conversations, MODELS};
use crate::{Op, PROGRAM};

struct Accounts {
    numbers: Vec<AccountNumber>,
    next: Option<Vec<u8>>,
}

fn accounts(ctx: &QueryCtx, after: Option<&[u8]>, limit: u64) -> Result<Accounts, Error> {
    let after = after
        .map(|bytes| <[u8; 8]>::try_from(bytes).map(u64::from_be_bytes))
        .transpose()
        .map_err(|_| {
            invalid(
                "an account cursor is 8 bytes: use the next account cursor returned by the listing",
            )
        })?;
    let asked = identity::Query::Profiles {
        after,
        limit: u32::try_from(limit).unwrap_or(u32::MAX),
    };
    let role = ctx.env().role(abi::role::IDENTITY)?.clone();
    match ctx.query::<identity::Query, identity::Reply>(&role, &asked)? {
        identity::Reply::Profiles { profiles, next } => Ok(Accounts {
            numbers: profiles.iter().map(|profile| profile.number).collect(),
            next: next.map(|number| number.to_be_bytes().to_vec()),
        }),
        other => Err(unexpected_reply(&role, "Profiles", &other)),
    }
}

pub(crate) fn adopt_owners(
    ctx: &ExecCtx,
    kind: &str,
    after: Option<Vec<u8>>,
    limit: u64,
) -> Result<(), Error> {
    let at = after.as_deref();
    if !by_itself(ctx)? {
        let next = match kind {
            "harness.model" => MODELS.batch(ctx, at, limit)?.next,
            "harness.agent-conversation" => accounts(ctx, at, limit)?.next,
            other => return Err(invalid(format!("harness keeps no owned kind {other}"))),
        };
        ctx.set_return_data(abi::encode(&next));
        let again = Op::AdoptOwners {
            kind: kind.to_owned(),
            after,
            limit,
        };
        ctx.emit(PROGRAM, &again, Reply::None);
        return Ok(());
    }
    let today = |key: &str| writers_of(ctx, PROGRAM, key, |at| resource(ctx, at));
    let mut items = Vec::new();
    match kind {
        "harness.model" => {
            for (id, stored) in MODELS.batch(ctx, at, limit)?.rows {
                let owner = Party::Account(stored.owner);
                let rule = rule_of(&owner, &today(&model_key(&id))?);
                items.push(Adopted {
                    key: MODELS.key(&id),
                    owner,
                    kind: Some(kind.to_owned()),
                    rule,
                });
            }
        }
        "harness.agent-conversation" => {
            for number in accounts(ctx, at, limit)?.numbers {
                let Some(agent) = harnessed_agent(ctx, number)? else {
                    continue;
                };
                if !CONVERSATIONS.has(ctx, &number) {
                    let manager = agent.manager;
                    CONVERSATIONS.put(ctx, &number, &Conversations { manager, rev: 0 });
                }
                let owner = Party::Account(agent.manager);
                let rule = rule_of(&owner, &today(&conversation_key(number))?);
                items.push(Adopted {
                    key: CONVERSATIONS.key(&number),
                    owner,
                    kind: Some(kind.to_owned()),
                    rule,
                });
            }
        }
        other => return Err(invalid(format!("harness keeps no owned kind {other}"))),
    }
    adopt(ctx, items)
}
