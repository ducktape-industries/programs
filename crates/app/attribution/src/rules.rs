//! What each op checks and writes: an object's relations replaced at a later revision, the
//! difference recorded as changes, and each change handed to the program its recipient's
//! script calls, when that recipient is an active program account and the program registered.
use std::collections::{BTreeMap, BTreeSet};

use ::program::{
    Error, ExecCtx, QueryCtx, Range, Renames, Reply, invalid, stale, unexpected_reply,
};
use abi::role::identity::{Kind as Account, Profile, Standing};

use crate::state::{CHANGES, OBJECTS, OF, ObjectKey, REGISTERED, SEQ, TO};
use crate::{
    AccountNumber, Actor, Change, Kind, PageRequest, PageResponse, ProgramId, Reason, Relation,
    Relations, Rows, Source, Transfer, Update,
};

type Held = BTreeMap<(AccountNumber, Reason), Vec<u8>>;

fn object_key(source: &Source) -> ObjectKey {
    (
        source.program.clone(),
        source.object.kind.clone(),
        source.object.id.clone(),
    )
}

fn named(text: &str) -> bool {
    !text.is_empty() && !text.contains('/')
}

fn well_formed(update: &Update) -> Result<(), Error> {
    let object_named = named(&update.object.kind) && named(&update.object.id);
    if !object_named {
        return Err(invalid(
            "an object has a kind and an id, each named and with no '/'",
        ));
    }
    let actor_named = match &update.actor {
        Actor::Account(number) => *number != 0,
        Actor::Key(key) => !key.is_empty(),
        Actor::Program(program) => !program.is_empty(),
        Actor::System | Actor::Foreign(_) => true,
    };
    if !actor_named {
        return Err(invalid(
            "an actor is a live account, a key, a program, the system or a foreign user",
        ));
    }
    let mut seen = BTreeSet::new();
    for relation in &update.relations {
        if relation.recipient == 0 {
            return Err(invalid("a recipient is a live account"));
        }
        let unnamed = matches!(&relation.reason, Reason::Defined(name) if name.is_empty());
        if unnamed {
            return Err(invalid("a defined reason has a name"));
        }
        let distinct = seen.insert((relation.recipient, relation.reason.clone()));
        if !distinct {
            return Err(invalid(format!(
                "account {} relates twice for {:?}: list each account once for each reason",
                relation.recipient, relation.reason
            )));
        }
    }
    Ok(())
}

fn held(relations: &[Relation]) -> Held {
    relations
        .iter()
        .map(|relation| {
            (
                (relation.recipient, relation.reason.clone()),
                relation.detail.clone(),
            )
        })
        .collect()
}

fn kinds(
    before: &Held,
    after: &Held,
    transfers: &[Transfer],
) -> Result<BTreeMap<(AccountNumber, Reason), Kind>, Error> {
    let mut kinds = BTreeMap::new();
    for key in after.keys().filter(|key| !before.contains_key(*key)) {
        kinds.insert(key.clone(), Kind::Added);
    }
    for key in before.keys().filter(|key| !after.contains_key(*key)) {
        kinds.insert(key.clone(), Kind::Withdrawn);
    }
    for Transfer { reason, from, to } in transfers {
        let withdrawn = kinds.get(&(*from, reason.clone())) == Some(&Kind::Withdrawn);
        let added = kinds.get(&(*to, reason.clone())) == Some(&Kind::Added);
        let matches_the_diff = withdrawn && added;
        if !matches_the_diff {
            return Err(invalid(format!(
                "the transfer of {reason:?} from {from} to {to} matches no withdrawal and addition"
            )));
        }
        kinds.insert((*from, reason.clone()), Kind::TransferredOut { to: *to });
        kinds.insert((*to, reason.clone()), Kind::TransferredIn { from: *from });
    }
    Ok(kinds)
}

pub(crate) fn attribute(ctx: &ExecCtx, updates: Vec<Update>) -> Result<(), Error> {
    let program = ctx.env().sending_program()?;
    for update in updates {
        attribute_one(ctx, &program, update)?;
    }
    Ok(())
}

fn profile(ctx: &QueryCtx, number: AccountNumber) -> Result<Option<Profile>, Error> {
    use abi::role::identity::{Query, Reply};
    let identity = &ctx.env().role(abi::role::IDENTITY)?.clone();
    match ctx.query::<Query, Reply>(identity, &Query::Profile(number))? {
        Reply::Profile(profile) => Ok(profile),
        other => Err(unexpected_reply(identity, "Profile", &other)),
    }
}

fn scripted(profile: &Profile) -> Option<&ProgramId> {
    let Account::Program {
        standing: Standing::Active,
        script: Some(script),
        ..
    } = &profile.kind
    else {
        return None;
    };
    Some(&script.calls)
}

fn handed_to(
    ctx: &QueryCtx,
    recipient: AccountNumber,
) -> Result<Option<(ProgramId, Vec<u8>)>, Error> {
    let Some(profile) = profile(ctx, recipient)? else {
        return Ok(None);
    };
    let Some(calls) = scripted(&profile) else {
        return Ok(None);
    };
    Ok(REGISTERED
        .get(ctx, calls)?
        .map(|deliver| (calls.clone(), deliver)))
}

fn attribute_one(ctx: &ExecCtx, program: &ProgramId, update: Update) -> Result<(), Error> {
    well_formed(&update)?;
    let source = Source {
        program: program.clone(),
        object: update.object.clone(),
    };
    let key = object_key(&source);
    let previous = OBJECTS.get(ctx, &key)?;
    let reported = previous
        .as_ref()
        .is_some_and(|previous| update.revision <= previous.revision);
    if reported {
        return Err(stale(format!(
            "{program}/{}/{} was already reported at revision {}: read the latest revision before reporting a newer one",
            source.object.kind, source.object.id, update.revision
        )));
    }
    let before = previous
        .map(|previous| held(&previous.relations))
        .unwrap_or_default();
    let after = held(&update.relations);
    let kinds = kinds(&before, &after, &update.transfers)?;
    let mut relations = update.relations;
    relations.sort_by(|a, b| (a.recipient, &a.reason).cmp(&(b.recipient, &b.reason)));
    let row = Relations {
        source: source.clone(),
        revision: update.revision,
        relations,
    };
    OBJECTS.put(ctx, &key, &row);
    let mut seq = SEQ.get(ctx)?.unwrap_or(0);
    for ((recipient, reason), kind) in kinds {
        seq += 1;
        let detail = after
            .get(&(recipient, reason.clone()))
            .cloned()
            .unwrap_or_default();
        let change = Change {
            seq,
            source: source.clone(),
            revision: update.revision,
            recipient,
            reason,
            kind,
            detail,
            actor: update.actor.clone(),
            cause: ctx.env().cause.clone(),
            height: ctx.env().height,
        };
        CHANGES.put(ctx, &seq, &change);
        TO.insert(ctx, &(recipient, seq));
        OF.insert(ctx, &(key.0.clone(), key.1.clone(), key.2.clone(), seq));
        if let Some((calls, deliver)) = handed_to(ctx, recipient)? {
            ctx.emit_raw(calls, Change::payload(&deliver, &change), Reply::Wanted);
        }
    }
    SEQ.put(ctx, &seq);
    Ok(())
}

pub(crate) fn register(ctx: &ExecCtx, deliver: Vec<u8>) -> Result<(), Error> {
    let program = ctx.env().sending_program()?;
    REGISTERED.put(ctx, &program, &deliver);
    Ok(())
}

pub(crate) fn migrate(ctx: &ExecCtx, height: u64, rows: Rows) -> Result<(), Error> {
    let renamed = ctx.renames(height)?;
    match rows {
        Rows::Objects { first, last } => {
            let at = |at: ObjectKey| OBJECTS.key(&at);
            let range = within(OBJECTS.prefix_of(&()), first.map(at), last.map(at));
            objects_renamed(ctx, &renamed, range)
        }
        Rows::Of { first, last } => {
            let at = |at: (ProgramId, String, String, u64)| OF.key(&at);
            let range = within(OF.prefix_of(&()), first.map(at), last.map(at));
            of_renamed(ctx, &renamed, range)
        }
        Rows::Registered { first, last } => {
            let at = |at: ProgramId| REGISTERED.key(&at);
            let range = within(REGISTERED.prefix_of(&()), first.map(at), last.map(at));
            registered_renamed(ctx, &renamed, range)
        }
    }
}

fn within(mut range: Range, first: Option<Vec<u8>>, last: Option<Vec<u8>>) -> Range {
    if let Some(first) = first {
        range.start = first;
    }
    if let Some(last) = last {
        range.end = Some([last, vec![0]].concat());
    }
    range
}

fn objects_renamed(ctx: &ExecCtx, renamed: &Renames, range: Range) -> Result<(), Error> {
    for ((program, kind, id), row) in OBJECTS.scan(ctx, range)? {
        let Some(to) = renamed.moved(&program).cloned() else {
            continue;
        };
        OBJECTS.remove(ctx, &(program, kind.clone(), id.clone()));
        let at = (to.clone(), kind, id);
        if OBJECTS.has(ctx, &at) {
            continue;
        }
        let source = Source {
            program: to,
            ..row.source
        };
        OBJECTS.put(ctx, &at, &Relations { source, ..row });
    }
    Ok(())
}

fn of_renamed(ctx: &ExecCtx, renamed: &Renames, range: Range) -> Result<(), Error> {
    for (program, kind, id, seq) in OF.scan(ctx, range)? {
        let Some(to) = renamed.moved(&program).cloned() else {
            continue;
        };
        OF.remove(ctx, &(program, kind.clone(), id.clone(), seq));
        OF.insert(ctx, &(to, kind, id, seq));
    }
    Ok(())
}

fn registered_renamed(ctx: &ExecCtx, renamed: &Renames, range: Range) -> Result<(), Error> {
    for (program, deliver) in REGISTERED.scan(ctx, range)? {
        let Some(to) = renamed.moved(&program) else {
            continue;
        };
        REGISTERED.remove(ctx, &program);
        if !REGISTERED.has(ctx, to) {
            REGISTERED.put(ctx, to, &deliver);
        }
    }
    Ok(())
}

pub(crate) fn relations(ctx: &QueryCtx, source: &Source) -> Result<Option<Relations>, Error> {
    OBJECTS.get(ctx, &object_key(source))
}

pub(crate) fn changes(ctx: &QueryCtx, page: &PageRequest) -> Result<PageResponse<Change>, Error> {
    Ok(CHANGES.range(ctx, page)?.map(|(_, change)| change))
}

fn change(ctx: &QueryCtx, seq: u64) -> Result<Change, Error> {
    CHANGES
        .get(ctx, &seq)?
        .ok_or_else(|| ::program::corrupt("change/", &seq.to_be_bytes(), "indexed but missing"))
}

pub(crate) fn change_at(ctx: &QueryCtx, seq: u64) -> Result<Option<Change>, Error> {
    CHANGES.get(ctx, &seq)
}

pub(crate) fn changes_to(
    ctx: &QueryCtx,
    recipient: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<Change>, Error> {
    TO.range_of(ctx, &recipient, page)?
        .try_map(|(_, seq)| change(ctx, seq))
}

pub(crate) fn changes_of(
    ctx: &QueryCtx,
    source: &Source,
    page: &PageRequest,
) -> Result<PageResponse<Change>, Error> {
    OF.range_of(ctx, &object_key(source), page)?
        .try_map(|(_, _, _, seq)| change(ctx, seq))
}

pub(crate) fn registered(ctx: &QueryCtx) -> Result<Vec<ProgramId>, Error> {
    Ok(REGISTERED
        .all(ctx)?
        .into_iter()
        .map(|(program, _)| program)
        .collect())
}
