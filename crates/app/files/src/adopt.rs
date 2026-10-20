//! One-time (J-011 ⑥): `AdoptOwners` records an owner for each path the access program keeps a
//! write list for, each file beneath a chat message, and each pin, linked to the policy matching
//! what the access program enforces for it today. Goes once duckpond has run it.
use std::collections::BTreeSet;

use ::program::{Error, ExecCtx, QueryCtx, Reply, invalid, unexpected_reply};
use ownership::{Adopted, adopt, by_itself, owner_of, rule_of, writers_of};
use store::PageRequest;

use crate::gate::{pin_key, resource};
use crate::guards::carried;
use crate::paths::change_path;
use crate::state::{GUARDED, PINS, PathGuard};
use crate::{Op, PROGRAM};

const PATH: &str = "files.path";
const PIN: &str = "files.pin";

pub(crate) fn adopt_owners(
    ctx: &ExecCtx,
    kind: &str,
    after: Option<Vec<u8>>,
    limit: u64,
) -> Result<(), Error> {
    let at = after.as_deref();
    if !by_itself(ctx)? {
        let next = match kind {
            PATH => paths(ctx, at, limit)?.1,
            PIN => PINS.batch(ctx, at, limit)?.next,
            other => return Err(invalid(format!("files keeps no owned kind {other}"))),
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
        PATH => {
            for path in paths(ctx, at, limit)?.0 {
                let Some(owner) = resource(ctx, &path)?.owner else {
                    continue;
                };
                let owner = ownership::Party::from(owner);
                let rule = rule_of(&owner, &today(&path)?);
                if !GUARDED.has(ctx, &path) {
                    ctx.set(GUARDED.key(&path), abi::encode(&PathGuard { rev: 0 }));
                }
                items.push(Adopted {
                    key: GUARDED.key(&path),
                    owner,
                    kind: Some(PATH.to_owned()),
                    rule,
                });
            }
        }
        PIN => {
            for (name, row) in PINS.batch(ctx, at, limit)?.rows {
                let owner = owner_of(&row.owner, PROGRAM);
                let rule = rule_of(&owner, &today(&pin_key(&name))?);
                items.push(Adopted {
                    key: PINS.key(&name),
                    owner,
                    kind: Some(PIN.to_owned()),
                    rule,
                });
            }
        }
        other => return Err(invalid(format!("files keeps no owned kind {other}"))),
    }
    adopt(ctx, items)
}

fn paths(
    ctx: &QueryCtx,
    after: Option<&[u8]>,
    limit: u64,
) -> Result<(Vec<String>, Option<Vec<u8>>), Error> {
    let every: BTreeSet<String> = listed(ctx)?.into_iter().chain(attached(ctx)?).collect();
    let batch: Vec<String> = every
        .into_iter()
        .filter(|path| after.is_none_or(|after| path.as_bytes() > after))
        .take(limit as usize)
        .collect();
    let full = limit > 0 && batch.len() as u64 == limit;
    let next = batch
        .last()
        .filter(|_| full)
        .map(|path| path.as_bytes().to_vec());
    Ok((batch, next))
}

fn listed(ctx: &QueryCtx) -> Result<Vec<String>, Error> {
    let checker = ctx.env().role(abi::role::ACCESS)?.clone();
    let mut keys = Vec::new();
    let mut page = PageRequest::default();
    loop {
        let asked = access::Query::Lists {
            program: PROGRAM.into(),
            page,
        };
        let lists = match ctx.query::<access::Query, access::Reply>(&checker, &asked)? {
            access::Reply::Lists(lists) => lists,
            other => return Err(unexpected_reply(&checker, "Lists", &other)),
        };
        let paths = lists.items.into_iter().map(|listed| listed.key);
        keys.extend(paths.filter(|key| change_path(key).is_ok()));
        let Some(after) = lists.next else {
            return Ok(keys);
        };
        page = PageRequest {
            after: Some(after),
            ..PageRequest::default()
        };
    }
}

fn attached(ctx: &QueryCtx) -> Result<Vec<String>, Error> {
    let mut paths = Vec::new();
    let mut page = PageRequest::default();
    loop {
        let (_, found) = crate::queries::find(ctx, "/chat/", None, &page)?;
        let beneath = found.items.into_iter().map(|entry| entry.path);
        paths.extend(beneath.filter(|path| change_path(path).is_ok_and(|at| carried(&at))));
        let Some(after) = found.next else {
            return Ok(paths);
        };
        page = PageRequest {
            after: Some(after),
            ..PageRequest::default()
        };
    }
}
