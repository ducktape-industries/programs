//! The rules: the guard's verdict on each run the kernel reports, and each owner's op, over the
//! program's tables (the ownership program is never guarded).

use ::program::kernel::error_from;
use ::program::{
    AccountNumber, Error, ExecCtx, Principal, ProgramId, QueryCtx, already_exists, corrupt,
    invalid, not_found, unauthorized, wrong_state,
};
use abi::role::{self, identity, ownership::Guard};
use store::{Item, Map, PageRequest, PageResponse, Set};

use crate::{
    Adopted, Linked, ONLY_ME, OPEN_POLICY, Party, Policy, PolicyId, Record, Rule, Scope,
    ScopeDefault, Writers,
};

pub(crate) const POLICIES: Map<PolicyId, Policy> = Map::new("policy/");
// each owner's policies, by the owner's borsh
pub(crate) const OWNED: Set<(Vec<u8>, PolicyId)> = Set::new("owned/");
pub(crate) const OPEN: Map<Vec<u8>, PolicyId> = Map::new("open/");
pub(crate) const RECORDS: Map<(ProgramId, Vec<u8>), Record> = Map::new("record/");
pub(crate) const LINKED: Set<(PolicyId, ProgramId, Vec<u8>)> = Set::new("linked/");
// (owner's borsh, scope's borsh) → the policy a new item there links to
pub(crate) const DEFAULTS: Map<(Vec<u8>, Vec<u8>), PolicyId> = Map::new("default/");
pub(crate) const NEXT: Item<PolicyId> = Item::new("next");
pub(crate) const ONLY_ME_POLICIES: Map<Vec<u8>, PolicyId> = Map::new("migrated/");
pub(crate) const CONVERTED: Map<(ProgramId, String), PolicyId> = Map::new("converted/");

fn key(party: &Party) -> Vec<u8> {
    abi::encode(party)
}

// Q5: the actor is the account the frame acts as; a run that acts as no account is its program's.
fn actor_of(guard: &Guard) -> Party {
    match &guard.sender {
        Some(abi::Principal::Account(number)) => Party::Account(*number),
        Some(abi::Principal::System) | None => Party::Program(guard.program.clone()),
    }
}

fn me(ctx: &ExecCtx) -> Result<Party, Error> {
    Ok(Party::Account(ctx.sender_account()?))
}

pub(crate) fn guard(ctx: &ExecCtx, guard: Guard) -> Result<(), Error> {
    ctx.env().kernel()?;
    let actor = actor_of(&guard);
    // every check first, then every write, so a refusal writes nothing
    let mut claimed = Vec::new();
    let mut dropped = Vec::new();
    for write in guard.writes {
        let at = (guard.program.clone(), write.key);
        let record = RECORDS.get(ctx, &at)?;
        let created = !write.existed && write.kept;
        match (created, write.claimed, record) {
            (true, Some(kind), stale) => claimed.push((at, stale, kind)),
            (_, _, Some(record)) => {
                let policy = effective(ctx, record.policy)?;
                if !admits(ctx, &policy, &record.owner, &actor)? {
                    let kind = record.kind.as_deref().unwrap_or("key");
                    return Err(unauthorized(format!(
                        "{actor} may not write {}'s {kind} {}: its policy {} (#{}) is {}'s",
                        at.0,
                        design::preview(&at.1),
                        policy.name,
                        policy.id,
                        record.owner
                    )));
                }
                if !write.kept {
                    dropped.push((at, record));
                }
            }
            // no record and no claim: a shared key, or one from before the guard, whose owner is
            // the migration's to record
            (_, _, None) => {}
        }
    }
    for ((program, key), stale, kind) in claimed {
        if let Some(stale) = stale {
            LINKED.remove(ctx, &(stale.policy, program.clone(), key.clone()));
        }
        let policy = default_for(ctx, &actor, Some(&kind), &program)?;
        let record = Record {
            owner: actor.clone(),
            policy,
            kind: Some(kind),
        };
        RECORDS.put(ctx, &(program.clone(), key.clone()), &record);
        LINKED.insert(ctx, &(policy, program, key));
    }
    for ((program, key), record) in dropped {
        LINKED.remove(ctx, &(record.policy, program.clone(), key.clone()));
        RECORDS.remove(ctx, &(program, key));
    }
    Ok(())
}

// The policy that governs a record linked to `id`: `id`'s, or the one a switch moved it to.
fn effective(ctx: &QueryCtx, id: PolicyId) -> Result<Policy, Error> {
    let mut policy = policy(ctx, id)?;
    while let Some(next) = policy.switched_to {
        policy = self::policy(ctx, next)?;
    }
    Ok(policy)
}

fn policy(ctx: &QueryCtx, id: PolicyId) -> Result<Policy, Error> {
    POLICIES
        .get(ctx, &id)?
        .ok_or_else(|| not_found(format!("no policy #{id}")))
}

fn admits(ctx: &QueryCtx, policy: &Policy, owner: &Party, actor: &Party) -> Result<bool, Error> {
    if is(ctx, owner, actor)? {
        return Ok(true);
    }
    match &policy.write {
        Writers::Anyone => Ok(true),
        Writers::Owner => Ok(false),
        Writers::Listed(parties) => {
            for party in parties {
                if is(ctx, party, actor)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
    }
}

// Whether `party` is `actor`: the same party, or a program and the account it acts as (Q5).
fn is(ctx: &QueryCtx, party: &Party, actor: &Party) -> Result<bool, Error> {
    match (party, actor) {
        (party, actor) if party == actor => Ok(true),
        (Party::Program(program), Party::Account(number))
        | (Party::Account(number), Party::Program(program)) => {
            Ok(account_of(ctx, program)? == Some(*number))
        }
        _ => Ok(false),
    }
}

pub(crate) fn account_of(
    ctx: &QueryCtx,
    program: &ProgramId,
) -> Result<Option<AccountNumber>, Error> {
    let identity = ctx.env().role(role::IDENTITY)?.clone();
    let asked = identity::Query::OfProgram(program.clone());
    match ctx.query(identity.clone(), &asked)? {
        identity::Reply::Account(account) => Ok(account),
        other => Err(::program::unexpected_reply(&identity, "OfProgram", &other)),
    }
}

fn exists(ctx: &QueryCtx, party: &Party) -> Result<bool, Error> {
    match party {
        Party::Program(program) => Ok(account_of(ctx, program)?.is_some()),
        Party::Account(number) => {
            let identity = ctx.env().role(role::IDENTITY)?.clone();
            match ctx.query(identity.clone(), &identity::Query::Profile(*number))? {
                identity::Reply::Profile(profile) => Ok(profile.is_some()),
                other => Err(::program::unexpected_reply(&identity, "Profile", &other)),
            }
        }
    }
}

fn default_for(
    ctx: &ExecCtx,
    owner: &Party,
    kind: Option<&str>,
    program: &ProgramId,
) -> Result<PolicyId, Error> {
    let scopes = kind
        .map(|kind| Scope::Kind(kind.to_owned()))
        .into_iter()
        .chain([Scope::Program(program.clone()), Scope::Global]);
    for scope in scopes {
        if let Some(id) = DEFAULTS.get(ctx, &(key(owner), abi::encode(&scope)))? {
            return Ok(id);
        }
    }
    only_me(ctx, owner)
}

// The owner's open policy, made the first time it is needed (Q2: never edited or retired).
fn open_of(ctx: &ExecCtx, owner: &Party) -> Result<PolicyId, Error> {
    if let Some(id) = OPEN.get(ctx, &key(owner))? {
        return Ok(id);
    }
    let id = made(ctx, owner, OPEN_POLICY.into(), Writers::Anyone)?;
    OPEN.put(ctx, &key(owner), &id);
    Ok(id)
}

fn made(ctx: &ExecCtx, owner: &Party, name: String, write: Writers) -> Result<PolicyId, Error> {
    let id = NEXT.update(ctx, |next| *next += 1)?;
    let policy = Policy {
        id,
        owner: owner.clone(),
        name,
        write,
        retired: false,
        switched_to: None,
    };
    POLICIES.put(ctx, &id, &policy);
    OWNED.insert(ctx, &(key(owner), id));
    Ok(id)
}

fn is_built_in(ctx: &QueryCtx, policy: &Policy) -> Result<bool, Error> {
    let owner = key(&policy.owner);
    Ok(OPEN.get(ctx, &owner)? == Some(policy.id)
        || ONLY_ME_POLICIES.get(ctx, &owner)? == Some(policy.id))
}

// `id`, which `me` must own.
fn mine(ctx: &ExecCtx, id: PolicyId, me: &Party) -> Result<Policy, Error> {
    let policy = policy(ctx, id)?;
    if !is(ctx, &policy.owner, me)? {
        return Err(unauthorized(format!(
            "policy {} (#{id}) is {}'s",
            policy.name, policy.owner
        )));
    }
    Ok(policy)
}

fn changeable(ctx: &ExecCtx, id: PolicyId, me: &Party) -> Result<Policy, Error> {
    let policy = mine(ctx, id, me)?;
    if is_built_in(ctx, &policy)? {
        return Err(wrong_state(format!(
            "#{id} is {}'s built-in policy {}, which never changes",
            policy.owner, policy.name
        )));
    }
    Ok(policy)
}

pub(crate) fn create(ctx: &ExecCtx, name: String, write: Writers) -> Result<(), Error> {
    let me = me(ctx)?;
    let id = made(ctx, &me, name, write)?;
    ctx.set_return_data(abi::encode(&id));
    Ok(())
}

pub(crate) fn edit(ctx: &ExecCtx, id: PolicyId, name: String, write: Writers) -> Result<(), Error> {
    let policy = changeable(ctx, id, &me(ctx)?)?;
    POLICIES.put(
        ctx,
        &id,
        &Policy {
            name,
            write,
            ..policy
        },
    );
    Ok(())
}

pub(crate) fn retire(ctx: &ExecCtx, id: PolicyId) -> Result<(), Error> {
    let policy = changeable(ctx, id, &me(ctx)?)?;
    if policy.retired {
        return Err(wrong_state(format!("#{id} is retired already")));
    }
    retired(ctx, policy, None)
}

pub(crate) fn switch(ctx: &ExecCtx, from: PolicyId, to: PolicyId) -> Result<(), Error> {
    if from == to {
        return Err(invalid("a policy is switched to another one"));
    }
    let me = me(ctx)?;
    let policy = changeable(ctx, from, &me)?;
    if let Some(already) = policy.switched_to {
        return Err(wrong_state(format!(
            "#{from}'s items were switched to #{already}"
        )));
    }
    let target = mine(ctx, to, &me)?;
    if target.retired {
        return Err(wrong_state(format!("#{to} is retired")));
    }
    retired(ctx, policy, Some(to))
}

// Retires `policy` (and points it at `to`), dropping every default of its owner that names it.
fn retired(ctx: &ExecCtx, policy: Policy, to: Option<PolicyId>) -> Result<(), Error> {
    let owner = key(&policy.owner);
    for ((holder, scope), id) in DEFAULTS.scan(ctx, DEFAULTS.prefix_of(&owner))? {
        if id == policy.id {
            DEFAULTS.remove(ctx, &(holder, scope));
        }
    }
    POLICIES.put(
        ctx,
        &policy.id,
        &Policy {
            retired: true,
            switched_to: to,
            ..policy
        },
    );
    Ok(())
}

fn owned_record(
    ctx: &ExecCtx,
    program: &ProgramId,
    key: &[u8],
    me: &Party,
) -> Result<Record, Error> {
    let at = (program.clone(), key.to_vec());
    let record = RECORDS
        .get(ctx, &at)?
        .ok_or_else(|| not_found(format!("{program} keeps no owned {}", design::preview(key))))?;
    if !is(ctx, &record.owner, me)? {
        return Err(unauthorized(format!(
            "{program}'s {} is {}'s",
            design::preview(key),
            record.owner
        )));
    }
    Ok(record)
}

pub(crate) fn relink(
    ctx: &ExecCtx,
    program: ProgramId,
    key: Vec<u8>,
    id: PolicyId,
) -> Result<(), Error> {
    let record = owned_record(ctx, &program, &key, &me(ctx)?)?;
    let policy = policy(ctx, id)?;
    if policy.owner != record.owner {
        return Err(invalid(format!(
            "an item links to its owner's policies, and #{id} is {}'s",
            policy.owner
        )));
    }
    if policy.retired {
        return Err(wrong_state(format!("#{id} is retired")));
    }
    LINKED.remove(ctx, &(record.policy, program.clone(), key.clone()));
    LINKED.insert(ctx, &(id, program.clone(), key.clone()));
    RECORDS.put(
        ctx,
        &(program, key),
        &Record {
            policy: id,
            ..record
        },
    );
    Ok(())
}

pub(crate) fn transfer(
    ctx: &ExecCtx,
    program: ProgramId,
    key: Vec<u8>,
    to: Party,
) -> Result<(), Error> {
    let record = owned_record(ctx, &program, &key, &me(ctx)?)?;
    if !exists(ctx, &to)? {
        return Err(not_found(format!("there is no {to}")));
    }
    RECORDS.put(
        ctx,
        &(program, key),
        &Record {
            owner: to,
            ..record
        },
    );
    Ok(())
}

pub(crate) fn moved(ctx: &ExecCtx, from_key: Vec<u8>, to_key: Vec<u8>) -> Result<(), Error> {
    let program = ctx.env().sending_program()?;
    let own = account_of(ctx, &program)?;
    let as_itself =
        matches!(ctx.env().sender, Some(Principal::Account(number)) if Some(number) == own);
    if !as_itself {
        return Err(unauthorized(
            "a program moves its own keys' records, in its own message as its own account",
        ));
    }
    let from = (program.clone(), from_key);
    let to = (program.clone(), to_key);
    let record = RECORDS.get(ctx, &from)?.ok_or_else(|| {
        not_found(format!(
            "{program} keeps no owned {}",
            design::preview(&from.1)
        ))
    })?;
    if RECORDS.has(ctx, &to) {
        return Err(already_exists(format!(
            "{program}'s {} is recorded already",
            design::preview(&to.1)
        )));
    }
    LINKED.remove(ctx, &(record.policy, program.clone(), from.1.clone()));
    LINKED.insert(ctx, &(record.policy, program, to.1.clone()));
    RECORDS.remove(ctx, &from);
    RECORDS.put(ctx, &to, &record);
    Ok(())
}

pub(crate) fn set_default(ctx: &ExecCtx, scope: Scope, id: Option<PolicyId>) -> Result<(), Error> {
    let me = me(ctx)?;
    let at = (key(&me), abi::encode(&scope));
    let Some(id) = id else {
        DEFAULTS.remove(ctx, &at);
        return Ok(());
    };
    let policy = mine(ctx, id, &me)?;
    if policy.retired {
        return Err(wrong_state(format!("#{id} is retired")));
    }
    DEFAULTS.put(ctx, &at, &id);
    Ok(())
}

pub(crate) fn record(
    ctx: &QueryCtx,
    program: ProgramId,
    key: Vec<u8>,
) -> Result<Option<Record>, Error> {
    RECORDS.get(ctx, &(program, key))
}

pub(crate) fn effective_of(
    ctx: &QueryCtx,
    program: ProgramId,
    key: Vec<u8>,
) -> Result<Option<Policy>, Error> {
    record(ctx, program, key)?
        .map(|record| effective(ctx, record.policy))
        .transpose()
}

pub(crate) fn policy_of(ctx: &QueryCtx, id: PolicyId) -> Result<Option<Policy>, Error> {
    POLICIES.get(ctx, &id)
}

pub(crate) fn policies(
    ctx: &QueryCtx,
    owner: &Party,
    page: &PageRequest,
) -> Result<PageResponse<Policy>, Error> {
    OWNED.range_of(ctx, &key(owner), page)?.try_map(|(_, id)| {
        POLICIES.get(ctx, &id)?.ok_or_else(|| {
            corrupt(
                POLICIES.prefix(),
                &POLICIES.key(&id),
                "an owner lists a policy that is not kept",
            )
        })
    })
}

pub(crate) fn linked(
    ctx: &QueryCtx,
    id: PolicyId,
    page: &PageRequest,
) -> Result<PageResponse<Linked>, Error> {
    Ok(LINKED
        .range_of(ctx, &id, page)?
        .map(|(_, program, key)| Linked { program, key }))
}

pub(crate) fn defaults(
    ctx: &QueryCtx,
    owner: &Party,
    page: &PageRequest,
) -> Result<PageResponse<ScopeDefault>, Error> {
    DEFAULTS
        .range_of(ctx, &key(owner), page)?
        .try_map(|((_, scope), policy)| {
            let scope = abi::decode(&scope).map_err(error_from)?;
            Ok(ScopeDefault { scope, policy })
        })
}

pub(crate) fn open(ctx: &QueryCtx, owner: &Party) -> Result<Option<PolicyId>, Error> {
    OPEN.get(ctx, &key(owner))
}

pub(crate) fn only_me_of(ctx: &QueryCtx, owner: &Party) -> Result<Option<PolicyId>, Error> {
    ONLY_ME_POLICIES.get(ctx, &key(owner))
}

// One-time (J-011 ⑥): the sending program's rows from before the guard, each recorded as its
// owner's and linked by its rule. A row recorded already is left as it is, unless the program
// itself holds it (a row it wrote for the migration); so run again, it changes nothing.
pub(crate) fn adopt(ctx: &ExecCtx, items: Vec<Adopted>) -> Result<(), Error> {
    let program = ctx.env().sending_program()?;
    let itself = Party::Program(program.clone());
    for item in items {
        let at = (program.clone(), item.key);
        let stale = match RECORDS.get(ctx, &at)? {
            Some(record) if !is(ctx, &itself, &record.owner)? => continue,
            stale => stale,
        };
        let policy = match item.rule {
            Rule::Open => open_of(ctx, &item.owner)?,
            Rule::OwnerOnly => only_me(ctx, &item.owner)?,
            Rule::List {
                at: listed,
                write,
                by,
            } => converted(ctx, &program, listed, write, &by)?,
        };
        let (program, key) = at.clone();
        if let Some(stale) = stale {
            LINKED.remove(ctx, &(stale.policy, program.clone(), key.clone()));
        }
        LINKED.insert(ctx, &(policy, program, key));
        let record = Record {
            owner: item.owner,
            policy,
            kind: item.kind,
        };
        RECORDS.put(ctx, &at, &record);
    }
    Ok(())
}

fn only_me(ctx: &ExecCtx, owner: &Party) -> Result<PolicyId, Error> {
    if let Some(id) = ONLY_ME_POLICIES.get(ctx, &key(owner))? {
        return Ok(id);
    }
    let id = made(ctx, owner, ONLY_ME.into(), Writers::Owner)?;
    ONLY_ME_POLICIES.put(ctx, &key(owner), &id);
    Ok(id)
}

fn converted(
    ctx: &ExecCtx,
    program: &ProgramId,
    at: String,
    write: Writers,
    by: &Party,
) -> Result<PolicyId, Error> {
    let row = (program.clone(), at);
    if let Some(id) = CONVERTED.get(ctx, &row)? {
        return Ok(id);
    }
    let name = format!("{program} {} (as before)", row.1);
    let id = made(ctx, by, name, write)?;
    CONVERTED.put(ctx, &row, &id);
    Ok(id)
}
