//! What each files op needs of the access program, and who owns each key: a path is owned by
//! the account that created it, each `/home/<name>` by the one account named `<name>`, and a
//! pin (`pins/<name>`) by the account that made it, which alone drops it unless it lists others.
use ::program::{Error, Need, Ownership, Party, QueryCtx, Resource};
use store::PageRequest;

use crate::commit::needs as commit_needs;
use crate::paths::{change_path, is_home};
use crate::state::{HEAD, PINS};
use crate::tree::TreeEdit;
use crate::{AccountNumber, Op};

pub fn pin_key(name: &str) -> String {
    format!("pins/{name}")
}

pub(crate) fn needs(op: &Op) -> Vec<Need> {
    match op {
        Op::Commit { changes, .. } => commit_needs(changes),
        Op::Unpin { name } => vec![Need::delete(pin_key(name))],
        Op::Stage { .. } | Op::Pin { .. } => Vec::new(),
    }
}

pub(crate) fn resource(ctx: &QueryCtx, key: &str) -> Result<Resource, Error> {
    if let Some(name) = key.strip_prefix("pins/") {
        let owner = PINS
            .get(ctx, &name.to_owned())?
            .and_then(|row| row.owner.account());
        return Ok(Resource {
            owner: owner.map(Party::Account),
            default: Some(Ownership::Owner),
        });
    }
    let Ok(segments) = change_path(key) else {
        return Ok(Resource::default());
    };
    let owner = match is_home(&segments) {
        Some(name) => named(ctx, name)?,
        None => created(ctx, &segments)?,
    };
    Ok(Resource {
        owner: owner.map(Party::Account),
        default: None,
    })
}

fn created(ctx: &QueryCtx, segments: &[String]) -> Result<Option<AccountNumber>, Error> {
    let head = HEAD.get(ctx)?.map(|row| row.root);
    let along = TreeEdit::over(head).access_along(ctx, segments)?;
    Ok(along.last().cloned().flatten().and_then(|row| row.creator))
}

fn named(ctx: &QueryCtx, name: &str) -> Result<Option<AccountNumber>, Error> {
    use abi::role::identity::{Query, Reply};
    let identity = ctx.env().role(abi::role::IDENTITY)?.clone();
    let limit = PageRequest::default().limit() as u32;
    let mut holders = Vec::new();
    let mut after = None;
    loop {
        let asked = Query::Profiles { after, limit };
        let (profiles, next) = match ctx.query::<Query, Reply>(&identity, &asked)? {
            Reply::Profiles { profiles, next } => (profiles, next),
            other => return Err(::program::unexpected_reply(&identity, "Profiles", &other)),
        };
        holders.extend(
            profiles
                .into_iter()
                .filter(|profile| profile.name == name)
                .map(|profile| profile.number),
        );
        let Some(next) = next else {
            break;
        };
        after = Some(next);
    }
    Ok(match holders.as_slice() {
        [one] => Some(*one),
        _ => None,
    })
}
