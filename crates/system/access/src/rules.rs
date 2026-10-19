//! The rules: a key's writers are the nearest list at or above it, else the default its owner
//! declares, else anyone; the owner of a key above may also delete it. The key's owner or the
//! program that names it sets a list (a program's own list as it sends it, its owner unasked),
//! and a delete drops every list at and below its key.

use ::program::{
    Action, Error, ExecCtx, Need, Origin, Ownership, Party, ProgramId, QueryCtx, Range, Resource,
    Writers, not_found, unauthorized, unexpected_reply,
};
use abi::role::access::{ancestors, writers};
use store::{KeyCodec, Map, PageRequest, PageResponse};

use crate::Listed;

const LISTS: Map<(ProgramId, String), Ownership> = Map::new("l/");

pub(crate) fn check(
    ctx: &ExecCtx,
    program: ProgramId,
    actor: Vec<Party>,
    needs: Vec<Need>,
) -> Result<(), Error> {
    ctx.env().kernel()?;
    for need in &needs {
        judge(ctx, &program, &actor, need)?;
    }
    let deleted = needs.iter().filter(|need| need.action == Action::Delete);
    for need in deleted {
        drop_below(ctx, &program, &need.key)?;
    }
    Ok(())
}

fn judge(ctx: &QueryCtx, program: &ProgramId, actor: &[Party], need: &Need) -> Result<(), Error> {
    let writers = writers_of(ctx, program, &need.key)?;
    if writers.allows(actor) {
        return Ok(());
    }
    let deletes = need.action == Action::Delete;
    if deletes && owns_above(ctx, program, &need.key, actor)? {
        return Ok(());
    }
    Err(unauthorized(format!(
        "{} may not {} {} in {program}: {}",
        who(actor),
        verb(need.action),
        need.key,
        allowed(&writers)
    )))
}

fn owns_above(
    ctx: &QueryCtx,
    program: &ProgramId,
    key: &str,
    actor: &[Party],
) -> Result<bool, Error> {
    for at in ancestors(key).skip(1) {
        let owner = resource_of(ctx, program, at)?.owner;
        let owns = owner.is_some_and(|owner| actor.contains(&owner));
        if owns {
            return Ok(true);
        }
    }
    Ok(false)
}

fn verb(action: Action) -> &'static str {
    match action {
        Action::Write => "write",
        Action::Delete => "delete",
    }
}

fn who(actor: &[Party]) -> String {
    match actor {
        [] => "no one".into(),
        parties => parties
            .iter()
            .map(party)
            .collect::<Vec<_>>()
            .join(", sent by "),
    }
}

fn party(party: &Party) -> String {
    match party {
        Party::Account(number) => format!("account {number}"),
        Party::Program(program) => format!("program {program}"),
    }
}

fn allowed(writers: &Writers) -> String {
    let Writers::Listed { at, owner, list } = writers else {
        return "anyone may".into();
    };
    let owner = owner.as_ref().map(party);
    let listed = list
        .listed()
        .iter()
        .map(party)
        .collect::<Vec<_>>()
        .join(", ");
    let allows = match (list, owner) {
        (Ownership::Anyone, _) => "anyone".to_owned(),
        (Ownership::Owner, Some(owner)) => format!("only its owner, {owner}"),
        (Ownership::Owner, None) => "no one: it has no owner".to_owned(),
        (Ownership::Listed(_), Some(owner)) => format!("its owner, {owner}, and {listed}"),
        (Ownership::Listed(_), None) => listed,
    };
    format!("the list on {at} allows {allows}")
}

fn drop_below(ctx: &ExecCtx, program: &ProgramId, key: &str) -> Result<(), Error> {
    for (held, _) in at_and_below(ctx, program, key)? {
        LISTS.remove(ctx, &held);
    }
    Ok(())
}

type Row = ((ProgramId, String), Ownership);

fn at_and_below(ctx: &QueryCtx, program: &ProgramId, key: &str) -> Result<Vec<Row>, Error> {
    let row = (program.clone(), key.to_owned());
    let at = LISTS.get(ctx, &row)?.map(|list| (row, list));
    let mut below = LISTS.key(program);
    let mut nested = Vec::new();
    format!("{key}/").encode_key(&mut nested);
    nested.truncate(nested.len().saturating_sub(2));
    below.extend_from_slice(&nested);
    let nested = LISTS.scan(ctx, Range::prefix(below))?;
    Ok(at.into_iter().chain(nested).collect())
}

pub(crate) fn moved(ctx: &ExecCtx, program: ProgramId, from: &str, to: &str) -> Result<(), Error> {
    let by_program = ctx.env().origin == Origin::Program(program.clone());
    if !by_program {
        return Err(unauthorized(format!(
            "only {program} moves the lists of its keys"
        )));
    }
    let rows = at_and_below(ctx, &program, from)?;
    for (held, list) in rows {
        LISTS.remove(ctx, &held);
        let rest = &held.1[from.len()..];
        LISTS.put(ctx, &(program.clone(), format!("{to}{rest}")), &list);
    }
    Ok(())
}

pub(crate) fn set(
    ctx: &ExecCtx,
    program: ProgramId,
    key: String,
    list: Option<Ownership>,
) -> Result<(), Error> {
    let env = ctx.env();
    let by_program = env.origin == Origin::Program(program.clone());
    let owner = match by_program {
        true => None,
        false => owner_setting(ctx, &env.actor(), &program, &key)?,
    };
    let row = (program, key);
    let Some(list) = list else {
        LISTS.remove(ctx, &row);
        return Ok(());
    };
    let list = list.settled(owner.as_ref());
    known(ctx, &list)?;
    LISTS.put(ctx, &row, &list);
    Ok(())
}

fn owner_setting(
    ctx: &QueryCtx,
    actor: &[Party],
    program: &ProgramId,
    key: &str,
) -> Result<Option<Party>, Error> {
    let owner = resource_of(ctx, program, key)?.owner;
    let owns = owner.as_ref().is_some_and(|owner| actor.contains(owner));
    if !owns {
        return Err(unauthorized(format!(
            "only the owner of {key} in {program}, or {program} itself, sets its list"
        )));
    }
    Ok(owner)
}

fn known(ctx: &QueryCtx, list: &Ownership) -> Result<(), Error> {
    use abi::role::identity::{Query, Reply};
    let identity = ctx.env().role(abi::role::IDENTITY)?.clone();
    for number in list.accounts() {
        match ctx.query::<Query, Reply>(&identity, &Query::Profile(number))? {
            Reply::Profile(Some(_)) => {}
            Reply::Profile(None) => return Err(not_found(format!("account {number}"))),
            other => return Err(unexpected_reply(&identity, "Profile", &other)),
        }
    }
    for program in list.programs() {
        match ctx.query::<Query, Reply>(&identity, &Query::OfProgram(program.clone()))? {
            Reply::Account(Some(_)) => {}
            Reply::Account(None) => return Err(not_found(format!("program {program}"))),
            other => return Err(unexpected_reply(&identity, "OfProgram", &other)),
        }
    }
    Ok(())
}

pub(crate) fn list(
    ctx: &QueryCtx,
    program: &ProgramId,
    key: &str,
) -> Result<Option<Ownership>, Error> {
    LISTS.get(ctx, &(program.clone(), key.to_owned()))
}

pub(crate) fn resource_of(
    ctx: &QueryCtx,
    program: &ProgramId,
    key: &str,
) -> Result<Resource, Error> {
    ctx.resource(program.clone(), key)
}

pub(crate) fn writers_of(ctx: &QueryCtx, program: &ProgramId, key: &str) -> Result<Writers, Error> {
    writers(
        key,
        |at| list(ctx, program, at),
        |at| resource_of(ctx, program, at),
    )
}

pub(crate) fn lists(
    ctx: &QueryCtx,
    program: &ProgramId,
    page: &PageRequest,
) -> Result<PageResponse<Listed>, Error> {
    Ok(LISTS
        .range_of(ctx, program, page)?
        .map(|((_, key), list)| Listed { key, list }))
}
