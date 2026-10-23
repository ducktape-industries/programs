//! A kind's policy as a placer reads it, shared by resources and fleet: classes in order, the group
//! an account files under, fair share and quotas over the placer's own ledger, and a queue walk.
use ::program::{Error, ExecCtx, QueryCtx, unexpected_reply};
use abi::role::identity;
use borsh::{BorshDeserialize, BorshSerialize};
use store::{Map, Set};

use crate::{AccountNumber, Class, Policy, PolicyId, ProgramId, Waiting};

pub const UNITS: &str = "";

pub type Ledger = Map<(String, String, String, u64), u64>;

pub type Queue = Set<(String, String, String, u64)>;

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct WalkRow {
    pub order: Vec<(String, String)>,
    pub at: u64,
    pub after: Option<(String, String, u64)>,
    pub behind: bool,
}

fn now(ctx: &QueryCtx) -> u64 {
    ctx.env().height
}

pub fn classes(policy: &Policy) -> Vec<Class> {
    match policy.classes.is_empty() {
        true => vec![Class {
            name: String::new(),
            may_use: None,
            notarized: false,
        }],
        false => policy.classes.clone(),
    }
}

pub fn admits(ctx: &QueryCtx, policy: &PolicyId, account: AccountNumber) -> Result<bool, Error> {
    let guardian = ctx.env().role(abi::role::OWNERSHIP)?.clone();
    let mut asked = *policy;
    loop {
        let query = ownership::Query::Policy { id: asked };
        let row = match ctx.query::<ownership::Query, ownership::Reply>(guardian.clone(), &query)? {
            ownership::Reply::Policy(row) => row,
            other => return Err(unexpected_reply(&guardian, "Policy", &other)),
        };
        let Some(row) = row else {
            return Ok(false);
        };
        if let Some(next) = row.switched_to {
            asked = next;
            continue;
        }
        if is(ctx, &row.owner, account)? {
            return Ok(true);
        }
        return match row.write {
            ownership::Writers::Anyone => Ok(true),
            ownership::Writers::Owner => Ok(false),
            ownership::Writers::Listed(parties) => {
                for party in &parties {
                    if is(ctx, party, account)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        };
    }
}

pub fn is(ctx: &QueryCtx, party: &ownership::Party, account: AccountNumber) -> Result<bool, Error> {
    match party {
        ownership::Party::Account(number) => Ok(*number == account),
        ownership::Party::Program(program) => Ok(account_of(ctx, program)? == Some(account)),
        ownership::Party::Foreign(_) => Ok(false),
    }
}

pub fn account_of(ctx: &QueryCtx, program: &ProgramId) -> Result<Option<AccountNumber>, Error> {
    let registry = ctx.env().role(abi::role::IDENTITY)?.clone();
    let asked = identity::Query::OfProgram(program.clone());
    match ctx.query::<identity::Query, identity::Reply>(registry.clone(), &asked)? {
        identity::Reply::Account(account) => Ok(account),
        other => Err(unexpected_reply(&registry, "OfProgram", &other)),
    }
}

pub fn group_of(ctx: &QueryCtx, policy: &Policy, account: AccountNumber) -> Result<String, Error> {
    for group in &policy.groups {
        if admits(ctx, &group.members, account)? {
            return Ok(group.name.clone());
        }
    }
    Ok(String::new())
}

pub fn window_start(height: u64, window: Option<u64>, since: u64) -> u64 {
    match window {
        Some(window) => height - height % window,
        None => since,
    }
}

pub fn share_used(
    ctx: &QueryCtx,
    ledger: &Ledger,
    kind: &str,
    group: &str,
    policy: &Policy,
    since: u64,
) -> u64 {
    let meter = policy.share.meter.clone().unwrap_or_default();
    let start = window_start(now(ctx), policy.share.window, since);
    let at = (kind.to_owned(), group.to_owned(), meter, start);
    ledger.get(ctx, &at).ok().flatten().unwrap_or(0)
}

pub fn weight_of(policy: &Policy, group: &str) -> u32 {
    policy
        .groups
        .iter()
        .find(|named| named.name == group)
        .map_or(1, |named| named.weight)
}

pub fn charge(
    ctx: &ExecCtx,
    ledger: &Ledger,
    kind: &str,
    group: &str,
    meter: &str,
    amount: u64,
    (policy, since): (&Policy, u64),
) -> Result<(), Error> {
    if amount == 0 {
        return Ok(());
    }
    let height = now(ctx);
    let mut starts = Vec::new();
    let shared = policy.share.meter.as_deref().unwrap_or(UNITS) == meter;
    if shared {
        starts.push(window_start(height, policy.share.window, since));
    }
    for quota in &policy.quotas {
        let counts = quota.group == group && quota.meter == meter;
        if counts {
            starts.push(window_start(height, Some(quota.window), since));
        }
    }
    starts.sort_unstable();
    starts.dedup();
    for start in starts {
        let at = (kind.to_owned(), group.to_owned(), meter.to_owned(), start);
        let used = ledger.get(ctx, &at)?.unwrap_or(0).saturating_add(amount);
        ledger.put(ctx, &at, &used);
    }
    Ok(())
}

pub fn over_quota(
    ctx: &QueryCtx,
    ledger: &Ledger,
    kind: &str,
    group: &str,
    policy: &Policy,
) -> Result<Option<Waiting>, Error> {
    let height = now(ctx);
    for quota in policy.quotas.iter().filter(|quota| quota.group == group) {
        let start = window_start(height, Some(quota.window), 0);
        let at = (
            kind.to_owned(),
            group.to_owned(),
            quota.meter.clone(),
            start,
        );
        let used = ledger.get(ctx, &at)?.unwrap_or(0);
        if used >= quota.most {
            return Ok(Some(Waiting::OverQuota {
                group: group.to_owned(),
                meter: quota.meter.clone(),
                until: start + quota.window,
            }));
        }
    }
    Ok(None)
}

pub fn order(
    ctx: &QueryCtx,
    ledger: &Ledger,
    kind: &str,
    policy: &Policy,
    since: u64,
) -> Vec<(String, String)> {
    let mut groups: Vec<(u64, u32, usize, String)> = policy
        .groups
        .iter()
        .enumerate()
        .map(|(at, group)| {
            let used = share_used(ctx, ledger, kind, &group.name, policy, since);
            (used, group.weight, at, group.name.clone())
        })
        .collect();
    groups.sort_by(|a, b| {
        let left = u128::from(a.0) * u128::from(b.1);
        let right = u128::from(b.0) * u128::from(a.1);
        left.cmp(&right).then(a.2.cmp(&b.2))
    });
    let mut names: Vec<String> = groups.into_iter().map(|group| group.3).collect();
    names.push(String::new());
    classes(policy)
        .into_iter()
        .flat_map(|class| {
            names
                .iter()
                .map(move |group| (class.name.clone(), group.clone()))
        })
        .collect()
}

pub fn next_queued(
    ctx: &QueryCtx,
    queue: &Queue,
    kind: &str,
    walk: &mut WalkRow,
) -> Result<Option<(String, String, u64)>, Error> {
    while let Some((class, group)) = walk.order.get(walk.at as usize).cloned() {
        let mut range = queue.prefix_of(&(kind.to_owned(), class.clone(), group.clone()));
        if let Some((after_class, after_group, after)) = &walk.after {
            range = range.after(queue.key(&(
                kind.to_owned(),
                after_class.clone(),
                after_group.clone(),
                *after,
            )));
        }
        if let Some((_, class, group, id)) = queue.scan(ctx, range.limit(1))?.into_iter().next() {
            return Ok(Some((class, group, id)));
        }
        walk.at += 1;
        walk.after = None;
    }
    loop {
        let mut range = queue.prefix_of(&kind.to_owned());
        if let Some((after_class, after_group, after)) = &walk.after {
            range = range.after(queue.key(&(
                kind.to_owned(),
                after_class.clone(),
                after_group.clone(),
                *after,
            )));
        }
        let Some((_, class, group, id)) = queue.scan(ctx, range.limit(1))?.into_iter().next()
        else {
            return Ok(None);
        };
        let named = walk
            .order
            .iter()
            .any(|pair| pair.0 == class && pair.1 == group);
        if !named {
            return Ok(Some((class, group, id)));
        }
        walk.after = Some((class, group, u64::MAX));
    }
}
