//! One-time (J-011 ⑥): what a program sends the ownership role to record owners for the owned rows
//! it kept before the role was seated, each linked to the policy that matches what the access
//! program enforces for it today. Goes once duckpond has run every program's `AdoptOwners`.
use ::program::{Error, ExecCtx, Origin, Principal, QueryCtx, Reply, Resource, unexpected_reply};
use abi::role::access::{Ownership as Listing, Party as Lister, Writers as Today, writers};
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

use crate::{Op, Party, Writers};

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Adopted {
    pub key: Vec<u8>,
    pub owner: Party,
    pub kind: Option<String>,
    pub rule: Rule,
}

// Who may write the row today, as a policy: anyone (the owner's open policy), its owner alone
// (their "Only me (as before)"), or the list set at `at` by `by` (one policy per list).
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Rule {
    Open,
    OwnerOnly,
    List {
        at: String,
        write: Writers,
        by: Party,
    },
}

impl From<Lister> for Party {
    fn from(party: Lister) -> Party {
        match party {
            Lister::Account(number) => Party::Account(number),
            Lister::Program(program) => Party::Program(program),
        }
    }
}

// A row's owner from the principal a program stored for it; the chain's own rows are the program's.
pub fn owner_of(principal: &Principal, program: &str) -> Party {
    match principal {
        Principal::Account(number) => Party::Account(*number),
        Principal::Root => Party::Program(program.to_owned()),
    }
}

// What the access program enforces for a row owned by `owner`, as a rule.
pub fn rule_of(owner: &Party, today: &Today) -> Rule {
    let Today::Listed {
        at,
        owner: set_by,
        list,
    } = today
    else {
        return Rule::Open;
    };
    let mut parties: Vec<Party> = match list {
        Listing::Anyone => return Rule::Open,
        Listing::Owner => Vec::new(),
        Listing::Listed(parties) => parties.iter().cloned().map(Party::from).collect(),
    };
    let by = set_by.clone().map_or_else(|| owner.clone(), Party::from);
    if by != *owner && !parties.contains(&by) {
        parties.push(by.clone());
    }
    if parties.is_empty() {
        return Rule::OwnerOnly;
    }
    Rule::List {
        at: at.clone(),
        write: Writers::Listed(parties),
        by,
    }
}

// The access program's answer for `key` of `program`, asked as it asks it: its list at or above
// the key (a `List` query), else the default `resource` (the program's own) declares.
pub fn writers_of(
    ctx: &QueryCtx,
    program: &str,
    key: &str,
    resource: impl FnMut(&str) -> Result<Resource, Error>,
) -> Result<Today, Error> {
    let checker = ctx.env().role(abi::role::ACCESS)?.clone();
    let list = |at: &str| -> Result<Option<Listing>, Error> {
        let asked = access::Query::List {
            program: program.to_owned(),
            key: at.to_owned(),
        };
        match ctx.query::<access::Query, access::Reply>(&checker, &asked)? {
            access::Reply::List(list) => Ok(list),
            other => Err(unexpected_reply(&checker, "List", &other)),
        }
    };
    writers(key, list, resource)
}

// Whether this run is the program's own message to itself, sent as its own account: a program's
// `AdoptOwners` sent by anyone only pages and sends itself the same op, whose run writes and
// records. An account's op that the program relays in its own name (harness running an agent's
// op) acts as that account, so it only pages.
pub fn by_itself(ctx: &QueryCtx) -> Result<bool, Error> {
    let env = ctx.env();
    let from_itself = matches!(&env.origin, Origin::Program(program) if *program == env.program);
    if !from_itself {
        return Ok(false);
    }
    let own = crate::rules::account_of(ctx, &env.program)?;
    let as_itself = matches!(env.sender, Some(Principal::Account(number)) if Some(number) == own);
    Ok(as_itself)
}

// Sends the ownership role `items`, as this program.
pub fn adopt(ctx: &ExecCtx, items: Vec<Adopted>) -> Result<(), Error> {
    if items.is_empty() {
        return Ok(());
    }
    let role = ctx.env().role(abi::role::OWNERSHIP)?.clone();
    ctx.emit(role, &Op::Adopt { items }, Reply::None);
    Ok(())
}

pub fn again_next_block(ctx: &ExecCtx, op: &impl BorshSerialize) -> Result<(), Error> {
    let env = ctx.env();
    let scheduler = env.role(abi::role::SCHEDULER)?.clone();
    let next = abi::role::scheduler::Op::Schedule {
        height: env.height.saturating_add(1),
        target: env.program.clone(),
        payload: abi::encode(op),
    };
    ctx.emit(scheduler, &next, Reply::None);
    Ok(())
}
