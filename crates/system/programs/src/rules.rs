// The rules: the schedule folded at each block, then the ops and queries.
// A change is made by the validators' vote or by an account
// (`actor`); every cascade a change causes happens in its own op, after
// its last check, never at `fold`: `At(h)` reads `roster` unfolded, so a
// fold-time filter would make what the kernel runs disagree with the state.

use std::collections::{BTreeMap, BTreeSet};

use ::program::{AccountNumber, BlobId, Env, Principal, ProgramId, Roles};
use ::program::{
    Error, ExecCtx, QueryCtx, already_exists, invalid, not_found, unauthorized, unexpected_reply,
    wrong_state,
};
use store::{Item, KeyCodec, Map, PageRequest, PageResponse, Set};

use crate::{
    Built, CODE_KIND, Change, Entry, Genesis, Holder, NOTICE, Provenance, Scheduled, Source, View,
    role_bound,
};

const PROGRAMS: Map<ProgramId, Entry> = Map::new("p/");
const VIEWS: Map<ProgramId, View> = Map::new("v/");
const HOOKS: Set<ProgramId> = Set::new("h/");
// Who holds each name that has a record: written by a name's first change (`schedule`), by
// genesis, and by `own`; a removal leaves it.
pub(crate) const OWNERS: Map<ProgramId, Holder> = Map::new("o/");
pub(crate) const ROLES: Map<String, ProgramId> = Map::new("r/");
type At = (u64, ProgramId);
pub(crate) const SCHEDULE: Map<At, Change> = Map::new("s/");
const FOLDED: Item<u64> = Item::new("folded");
pub(crate) const BY: Map<At, AccountNumber> = Map::new("b/");
pub(crate) const RENAMED: Map<At, ProgramId> = Map::new("n/");

pub(crate) fn init(ctx: &ExecCtx, genesis: Genesis) {
    for entry in genesis.programs {
        OWNERS.put(ctx, &entry.program, &Holder::Vote);
        PROGRAMS.put(ctx, &entry.program, &entry);
    }
    for view in genesis.views {
        OWNERS.put(ctx, &view.name, &Holder::Vote);
        VIEWS.put(ctx, &view.name, &view);
    }
    for hook in genesis.hooks {
        HOOKS.insert(ctx, &hook);
    }
    for (role, program) in genesis.roles.iter() {
        ROLES.put(ctx, &role.to_owned(), program);
    }
    FOLDED.put(ctx, &0);
}

fn held_code(ctx: &QueryCtx, code: BlobId) -> Result<(), Error> {
    let role = ctx.env().role(abi::role::BLOBS)?.clone();
    let holding = match ctx.query(role.clone(), &blob::contract::Query::Held { blob: code })? {
        blob::contract::Reply::Held(holding) => holding,
        other => return Err(unexpected_reply(&role, "Held", &other)),
    };
    let Some(holding) = holding else {
        return Err(not_found(format!(
            "code {code:?} is not held: put it as a {CODE_KIND:?} blob and wait for a quorum of validators to hold it"
        )));
    };
    let program = holding.kind == CODE_KIND;
    if !program {
        return Err(invalid(format!(
            "code {code:?} is held as a {:?} blob, not {CODE_KIND:?}: upload the code using the expected blob kind",
            holding.kind
        )));
    }
    Ok(())
}

fn actor(env: &Env, name: &str) -> Result<Option<AccountNumber>, Error> {
    if env.authority().is_ok() {
        return Ok(None);
    }
    let Some(account) = env.sender.as_ref().and_then(Principal::account) else {
        return Err(unauthorized(format!(
            "only an account, or the validators' vote, may change {name}"
        )));
    };
    Ok(Some(account))
}

fn fills_no_role(ctx: &QueryCtx, name: &str) -> Result<(), Error> {
    if role_bound(&roles_at(ctx, ctx.env().height)?, name) {
        return Err(unauthorized(format!(
            "{name} fills a role the kernel calls: only the validators' vote changes it"
        )));
    }
    Ok(())
}

// The holder of `name`: its record, else the vote's for a name that runs, is listed or has a
// change waiting (one from before records were kept), else none: the name is free.
pub(crate) fn holder_of(ctx: &QueryCtx, name: &str) -> Result<Option<Holder>, Error> {
    let key = name.to_owned();
    if let Some(holder) = OWNERS.get(ctx, &key)? {
        return Ok(Some(holder));
    }
    let present = PROGRAMS.has(ctx, &key)
        || VIEWS.has(ctx, &key)
        || matching_change_waits(ctx, name, &[], |_| true)?;
    Ok(present.then_some(Holder::Vote))
}

fn belongs(name: &str, holder: &Holder) -> String {
    match holder {
        Holder::Vote => format!(
            "{name} belongs to the validators' vote: only the vote changes it, and only the vote hands it to an account (`Own`)"
        ),
        Holder::Account(account) => format!(
            "{name} belongs to account {account}: only account {account} or the validators' vote changes it, and either hands it on (`Own`)"
        ),
        Holder::Accounts(accounts) => {
            let listed: Vec<String> = accounts.iter().map(u64::to_string).collect();
            let listed = listed.join(", ");
            format!(
                "{name} belongs to accounts {listed}: only one of them or the validators' vote changes it, and any of them hands it on (`Own`)"
            )
        }
    }
}

// Whether `account` may change `name`: it holds it, or nobody does and its change claims it.
fn held_by(ctx: &QueryCtx, name: &str, account: AccountNumber) -> Result<(), Error> {
    match holder_of(ctx, name)? {
        None => Ok(()),
        Some(holder) if holder.lists(account) => Ok(()),
        Some(holder) => Err(unauthorized(belongs(name, &holder))),
    }
}

pub(crate) fn own(ctx: &ExecCtx, program: ProgramId, to: Holder) -> Result<(), Error> {
    if to == Holder::Accounts(Vec::new()) {
        return Err(invalid(format!(
            "a list of no accounts holds nothing: hand {program} to the validators' vote instead"
        )));
    }
    if let Some(account) = actor(&ctx.env(), &program)? {
        match holder_of(ctx, &program)? {
            Some(holder) if holder.lists(account) => {}
            Some(holder) => return Err(unauthorized(belongs(&program, &holder))),
            None => {
                return Err(not_found(format!(
                    "nobody holds {program}: the first account to set it holds it"
                )));
            }
        }
    }
    OWNERS.put(ctx, &program, &to);
    Ok(())
}

fn by_account(ctx: &QueryCtx, scheduled: &Scheduled, account: AccountNumber) -> Result<(), Error> {
    let name = scheduled.change.program();
    if matches!(scheduled.change, Change::SetHook(_) | Change::RemoveHook(_)) {
        return Err(unauthorized(format!(
            "only the validators' vote sets or removes a hook, as on {name}"
        )));
    }
    let declares = matches!(&scheduled.change, Change::Publish { roles, .. } if !roles.is_empty());
    if declares {
        return Err(unauthorized(format!(
            "only the validators' vote publishes {name} to fill a role"
        )));
    }
    fills_no_role(ctx, name)?;
    held_by(ctx, name, account)?;
    let earliest = ctx.env().height.checked_add(NOTICE);
    if earliest.is_none_or(|earliest| scheduled.height < earliest) {
        let min = earliest.map_or_else(|| "past the last".into(), |h| h.to_string());
        return Err(invalid(format!(
            "a change by an account lands at block {min} or later"
        )));
    }
    if let Some(height) = pending_vote_height(ctx, name)? {
        return Err(wrong_state(format!(
            "the validators' vote changes {name} at {height}: wait until it lands or is cancelled"
        )));
    }
    let frees: Option<fn(&Change) -> bool> = match scheduled.change {
        Change::Set(_) | Change::Publish { .. } => {
            Some(|change| matches!(change, Change::RemoveView(_)))
        }
        Change::SetView(_) => Some(|change| matches!(change, Change::Remove(_))),
        _ => None,
    };
    if let Some(frees) = frees
        && matching_change_waits(ctx, name, &[], frees)?
    {
        return Err(wrong_state(format!(
            "{name} is held until an account's removal lands: wait, or cancel it"
        )));
    }
    Ok(())
}

fn pending_vote_height(ctx: &QueryCtx, name: &str) -> Result<Option<u64>, Error> {
    Ok(SCHEDULE
        .all(ctx)?
        .into_iter()
        .map(|(key, _)| key)
        .find(|key| key.1 == name && !BY.has(ctx, key))
        .map(|key| key.0))
}

fn schedule(ctx: &ExecCtx, scheduled: Scheduled) -> Result<(), Error> {
    let env = ctx.env();
    let by = actor(&env, scheduled.change.program())?;
    let changes = match by {
        Some(account) => {
            by_account(ctx, &scheduled, account)?;
            Vec::new()
        }
        None => overruled(ctx, scheduled.change.program())?,
    };
    let in_the_future = scheduled.height > env.height;
    if !in_the_future {
        return Err(invalid(format!(
            "a change lands at a later block than {}",
            env.height
        )));
    }
    if let Some(code) = scheduled.change.code() {
        held_code(ctx, code)?;
    }
    let name = scheduled.change.program();
    let landing = scheduled.change.lands();
    if let Some(id) = landing.filter(|id| !::program::is_id(id)) {
        return Err(invalid(format!(
            "{id:?} is not a program id: a name or owner/name, each name 1..=64 of [a-z0-9_-]"
        )));
    }
    let free = match landing {
        Some(id) => holder_of(ctx, id)?.is_none(),
        None => false,
    };
    let names_no_owner = landing.is_some_and(|id| !id.contains('/'));
    let a_new_id_without_its_owner = free && names_no_owner;
    if a_new_id_without_its_owner {
        let id = landing.unwrap_or_default();
        return Err(invalid(format!(
            "{id} is a new id, and a new id names its owner: <owner>/{id}"
        )));
    }
    if let Change::Publish { roles, .. } = &scheduled.change {
        declarable(roles)?;
    }
    let before = roster(ctx, scheduled.height, &changes)?;
    let mut after = before.clone();
    apply(&mut after, scheduled.change.clone());
    fillable(&after)?;
    if let Some(id) = landing.filter(|id| !lists(&before, id)) {
        let renamed_from = match &scheduled.change {
            Change::Rename { from, .. } => Some(from.as_str()),
            _ => None,
        };
        let clash = slug_clash(ctx, id, renamed_from, &after)?;
        if let Some(other) = clash {
            return Err(already_exists(format!(
                "{id} writes as {:?} in a channel id, as {other} does: choose an id no other id writes the same",
                ::program::slug(id)
            )));
        }
    }
    let Roster {
        programs,
        views,
        hooks,
        ..
    } = before;
    let gone = by.is_none()
        && matches!(scheduled.change, Change::Remove(_) | Change::RemoveView(_))
        && !programs.contains_key(name)
        && !views.contains_key(name);
    let listed = programs.contains_key(name) || views.contains_key(name);
    let clash = match &scheduled.change {
        Change::Set(_) | Change::Publish { .. } => {
            views.contains_key(name)
                || matching_change_waits(ctx, name, &changes, |c| matches!(c, Change::SetView(_)))?
        }
        Change::SetView(_) => {
            programs.contains_key(name)
                || matching_change_waits(ctx, name, &changes, |c| {
                    matches!(c, Change::Set(_) | Change::Publish { .. })
                })?
        }
        Change::Rename { .. } if !listed => {
            return Err(not_found(format!(
                "no program or view {name} is listed at {}",
                scheduled.height
            )));
        }
        Change::Rename { to, .. } => {
            let taken = holder_of(ctx, to)?.is_some()
                || matching_change_waits(ctx, to, &changes, |_| true)?;
            if taken {
                return Err(already_exists(format!(
                    "{to} is held or has a change waiting: a rename lands on a free id"
                )));
            }
            false
        }
        _ if gone => false,
        Change::Remove(_) | Change::SetHook(_) if !programs.contains_key(name) => {
            return Err(not_found(format!(
                "no program {name} runs at {}",
                scheduled.height
            )));
        }
        Change::RemoveView(_) if !views.contains_key(name) => {
            return Err(not_found(format!(
                "no view {name} is listed at {}",
                scheduled.height
            )));
        }
        Change::RemoveHook(_) if !hooks.contains(name) => {
            return Err(not_found(format!(
                "{name} is no hook at {}",
                scheduled.height
            )));
        }
        Change::Remove(_) | Change::RemoveView(_) | Change::SetHook(_) | Change::RemoveHook(_) => {
            false
        }
    };
    if clash {
        return Err(already_exists(format!(
            "{name} already names a program or a view: choose a different name or change the existing entry"
        )));
    }
    let unhooks = matches!(scheduled.change, Change::Remove(_)) && hooks.contains(name);
    if by.is_some() && unhooks {
        return Err(unauthorized(format!(
            "{name} is a hook at {}: only the validators' vote removes it",
            scheduled.height
        )));
    }
    let key = (scheduled.height, scheduled.change.program().to_owned());
    if !gone && SCHEDULE.has(ctx, &key) && !changes.contains(&key) {
        return Err(already_exists(format!(
            "{} already changes at {}",
            key.1, key.0
        )));
    }
    changes
        .iter()
        .for_each(|at| remove_change_and_maker(ctx, at));
    if gone {
        return Ok(());
    }
    if !OWNERS.has(ctx, &key.1) {
        OWNERS.put(ctx, &key.1, &by.map_or(Holder::Vote, Holder::Account));
    }
    if let Change::Rename { to, .. } = &scheduled.change {
        let holder = holder_of(ctx, &key.1)?.unwrap_or(Holder::Vote);
        OWNERS.put(ctx, to, &holder);
    }
    SCHEDULE.put(ctx, &key, &scheduled.change);
    if let Some(account) = by {
        BY.put(ctx, &key, &account);
    }
    Ok(())
}

fn code_under(roster: &Roster, name: &str) -> Option<BlobId> {
    let program = roster.programs.get(name).map(|entry| entry.code);
    program.or_else(|| roster.views.get(name).map(|view| view.view))
}

fn named(code: Option<BlobId>) -> String {
    code.map_or_else(|| "nothing".into(), |code| format!("code {code:?}"))
}

pub(crate) fn found_under(
    ctx: &ExecCtx,
    scheduled: &Scheduled,
) -> Result<[Option<BlobId>; 2], Error> {
    let name = scheduled.change.program();
    let overruled = match actor(&ctx.env(), name)? {
        Some(_) => Vec::new(),
        None => overruled(ctx, name)?,
    };
    let before = code_under(&roster(ctx, scheduled.height, &overruled)?, name);
    let mut last = stored(ctx)?;
    let waiting = SCHEDULE.all(ctx)?.into_iter();
    for (_, change) in waiting.filter(|(at, _)| !overruled.contains(at)) {
        apply(&mut last, change);
    }
    Ok([before, code_under(&last, name)])
}

pub(crate) fn schedule_over(
    ctx: &ExecCtx,
    scheduled: Scheduled,
    expected: Option<BlobId>,
) -> Result<(), Error> {
    let [before, after] = found_under(ctx, &scheduled)?;
    let name = scheduled.change.program();
    let found = [
        (before, format!("just before block {}", scheduled.height)),
        (
            after,
            "once every change waiting for it has landed".to_owned(),
        ),
    ];
    if let Some((code, when)) = found.into_iter().find(|(code, _)| *code != expected) {
        return Err(wrong_state(format!(
            "{name} has {} {when}, not the expected {}: read Program and Scheduled again",
            named(code),
            named(expected)
        )));
    }
    schedule(ctx, scheduled)
}

fn overruled(ctx: &QueryCtx, name: &str) -> Result<Vec<At>, Error> {
    Ok(BY
        .all(ctx)?
        .into_iter()
        .map(|(at, _)| at)
        .filter(|at| at.1 == name)
        .collect())
}

fn remove_change_and_maker(ctx: &ExecCtx, at: &At) {
    SCHEDULE.remove(ctx, at);
    BY.remove(ctx, at);
}

pub(crate) fn cancel(ctx: &ExecCtx, height: u64, program: ProgramId) -> Result<(), Error> {
    let by = actor(&ctx.env(), &program)?;
    if by.is_some() {
        fills_no_role(ctx, &program)?;
    }
    let key = (height, program);
    let Some(change) = SCHEDULE.get(ctx, &key)? else {
        if by.is_none() {
            return Ok(());
        }
        return Err(not_found(format!("{} does not change at {height}", key.1)));
    };
    if by.is_some() && !BY.has(ctx, &key) {
        return Err(unauthorized(format!(
            "the validators' vote changes {} at {height}: only the vote cancels it",
            key.1
        )));
    }
    if let Some(account) = by {
        held_by(ctx, &key.1, account)?;
    }
    // A removal cancelled keeps its name held, which a pending set of the
    // other kind may since have claimed.
    let reinstated: Option<fn(&Change) -> bool> = match change {
        Change::Remove(_) => Some(|change| matches!(change, Change::SetView(_))),
        Change::RemoveView(_) => {
            Some(|change| matches!(change, Change::Set(_) | Change::Publish { .. }))
        }
        Change::Set(_)
        | Change::SetView(_)
        | Change::SetHook(_)
        | Change::RemoveHook(_)
        | Change::Publish { .. }
        | Change::Rename { .. } => None,
    };
    if let Some(other) = reinstated
        && matching_change_waits(ctx, &key.1, &[], other)?
    {
        return Err(already_exists(format!(
            "{} is claimed by a pending change of the other kind: wait for or cancel the pending change before reusing the name",
            key.1
        )));
    }
    remove_change_and_maker(ctx, &key);
    Ok(())
}

pub(crate) fn fold(ctx: &ExecCtx, height: u64) -> Result<(), Error> {
    let folded = FOLDED.get(ctx)?.unwrap_or(0);
    let nothing_new = folded >= height;
    if nothing_new {
        return Ok(());
    }
    let due = due(ctx, height)?;
    if due.is_empty() {
        FOLDED.put(ctx, &height);
        return Ok(());
    }
    let before = stored(ctx)?;
    let mut after = before.clone();
    for (key, change) in due {
        if let Change::Rename { to, .. } = &change {
            RENAMED.put(ctx, &key, to);
        }
        apply(&mut after, change);
        remove_change_and_maker(ctx, &key);
    }
    write(ctx, &before, &after);
    FOLDED.put(ctx, &height);
    Ok(())
}

fn write(ctx: &ExecCtx, before: &Roster, after: &Roster) {
    write_map(ctx, PROGRAMS, &before.programs, &after.programs);
    write_map(ctx, VIEWS, &before.views, &after.views);
    write_map(ctx, ROLES, &before.roles, &after.roles);
    before
        .hooks
        .difference(&after.hooks)
        .for_each(|hook| HOOKS.remove(ctx, hook));
    after
        .hooks
        .difference(&before.hooks)
        .for_each(|hook| HOOKS.insert(ctx, hook));
}

fn write_map<K, V>(ctx: &ExecCtx, table: Map<K, V>, before: &BTreeMap<K, V>, after: &BTreeMap<K, V>)
where
    K: KeyCodec + Ord,
    V: borsh::BorshSerialize + borsh::BorshDeserialize + PartialEq,
{
    before
        .keys()
        .filter(|key| !after.contains_key(*key))
        .for_each(|key| table.remove(ctx, key));
    after
        .iter()
        .filter(|(key, value)| before.get(*key) != Some(*value))
        .for_each(|(key, value)| table.put(ctx, key, value));
}

fn declarable(roles: &[String]) -> Result<(), Error> {
    let named = roles.iter().find(|role| !::program::is_name(role));
    if let Some(role) = named {
        return Err(invalid(format!(
            "{role:?} is not a role name: 1..=64 of [a-z0-9_-]"
        )));
    }
    let unique: BTreeSet<&String> = roles.iter().collect();
    if unique.len() != roles.len() {
        return Err(invalid("a change declares each role once"));
    }
    Ok(())
}

fn fillable(roster: &Roster) -> Result<(), Error> {
    if let Some(role) = table(&roster.roles).unfilled() {
        return Err(invalid(format!(
            "the change leaves the {role} role, which the kernel calls, unfilled: assign a running program to that role"
        )));
    }
    let unlisted = roster
        .roles
        .iter()
        .find(|(_, program)| !roster.programs.contains_key(*program));
    if let Some((role, program)) = unlisted {
        return Err(invalid(format!(
            "the {role} role names {program}, which no entry runs: publish that program or choose a running program for the role"
        )));
    }
    Ok(())
}

fn table(roles: &BTreeMap<String, ProgramId>) -> Roles {
    roles
        .iter()
        .map(|(role, program)| (role.clone(), program.clone()))
        .collect()
}

pub(crate) fn roles_at(ctx: &QueryCtx, height: u64) -> Result<Roles, Error> {
    Ok(table(&roster(ctx, height, &[])?.roles))
}

pub(crate) fn to_land(
    ctx: &QueryCtx,
    page: &PageRequest,
) -> Result<PageResponse<(At, Change)>, Error> {
    let table = SCHEDULE.key(&());
    let listing = page.listing(ctx, table.clone())?;
    let mut scan = listing.scan_ahead(&table);
    let unlanded = SCHEDULE.key(&ctx.env().height.saturating_add(1));
    scan.start = scan.start.max(unlanded);
    let rows = SCHEDULE.scan(ctx, scan)?;
    Ok(listing.reply(rows.into_iter().map(|row| (SCHEDULE.key(&row.0), row))))
}

fn due(ctx: &QueryCtx, height: u64) -> Result<Vec<(At, Change)>, Error> {
    SCHEDULE.scan(ctx, SCHEDULE.below(&(height + 1)))
}

pub(crate) fn at(ctx: &QueryCtx, height: u64) -> Result<Vec<Entry>, Error> {
    Ok(roster(ctx, height, &[])?.programs.into_values().collect())
}

pub(crate) fn sources(ctx: &QueryCtx, height: u64) -> Result<Vec<Built>, Error> {
    let built = at(ctx, height)?.into_iter().map(|entry| Built {
        source: provenance(ctx, entry.code),
        program: entry.program,
        code: entry.code,
    });
    Ok(built.collect())
}

fn provenance(ctx: &QueryCtx, code: BlobId) -> Provenance {
    use ::program::source::SECTION;
    match ::program::section::read(ctx, code, SECTION) {
        Ok(Some(section)) => match abi::decode::<Source>(&section) {
            Ok(source) => Provenance::Commit(source),
            Err(_) => {
                Provenance::Unread(format!("its {SECTION} section is no repository and commit"))
            }
        },
        Ok(None) => Provenance::Unstamped,
        Err(error) => Provenance::Unread(error.message),
    }
}

pub(crate) fn views_at(ctx: &QueryCtx, height: u64) -> Result<Vec<View>, Error> {
    Ok(roster(ctx, height, &[])?.views.into_values().collect())
}

pub(crate) fn hooks_at(ctx: &QueryCtx, height: u64) -> Result<Vec<ProgramId>, Error> {
    Ok(roster(ctx, height, &[])?.hooks.into_iter().collect())
}

#[derive(Clone)]
pub(crate) struct Roster {
    pub(crate) programs: BTreeMap<ProgramId, Entry>,
    pub(crate) views: BTreeMap<ProgramId, View>,
    pub(crate) hooks: BTreeSet<ProgramId>,
    pub(crate) roles: BTreeMap<String, ProgramId>,
}

fn stored(ctx: &QueryCtx) -> Result<Roster, Error> {
    Ok(Roster {
        programs: PROGRAMS.all(ctx)?.into_iter().collect(),
        views: VIEWS.all(ctx)?.into_iter().collect(),
        hooks: HOOKS.all(ctx)?.into_iter().collect(),
        roles: ROLES.all(ctx)?.into_iter().collect(),
    })
}

fn apply(roster: &mut Roster, change: Change) {
    match change {
        Change::Set(entry) => {
            roster.programs.insert(entry.program.clone(), entry);
        }
        Change::Publish { entry, roles } => {
            let program = entry.program.clone();
            roster.roles.retain(|_, filled| *filled != program);
            for role in roles {
                roster.roles.insert(role, program.clone());
            }
            roster.programs.insert(program, entry);
        }
        Change::Remove(program) => {
            roster.programs.remove(&program);
            roster.hooks.remove(&program);
            roster.roles.retain(|_, filled| *filled != program);
        }
        Change::SetView(view) => {
            roster.views.insert(view.name.clone(), view);
        }
        Change::RemoveView(name) => {
            roster.views.remove(&name);
        }
        Change::SetHook(program) => {
            if roster.programs.contains_key(&program) {
                roster.hooks.insert(program);
            }
        }
        Change::RemoveHook(program) => {
            roster.hooks.remove(&program);
        }
        Change::Rename { from, to } => {
            if let Some(mut entry) = roster.programs.remove(&from) {
                entry.program = to.clone();
                roster.programs.entry(to.clone()).or_insert(entry);
            }
            if let Some(mut view) = roster.views.remove(&from) {
                view.name = to.clone();
                roster.views.entry(to.clone()).or_insert(view);
            }
            if roster.hooks.remove(&from) {
                roster.hooks.insert(to.clone());
            }
            let bound = roster.roles.values_mut();
            for filled in bound.filter(|filled| **filled == from) {
                *filled = to.clone();
            }
        }
    }
}

pub(crate) fn roster(ctx: &QueryCtx, height: u64, overruled: &[At]) -> Result<Roster, Error> {
    let mut roster = stored(ctx)?;
    let due = due(ctx, height)?;
    let due = due.into_iter().filter(|(at, _)| !overruled.contains(at));
    for (_, change) in due {
        apply(&mut roster, change);
    }
    Ok(roster)
}

fn matching_change_waits(
    ctx: &QueryCtx,
    name: &str,
    overruled: &[At],
    this: fn(&Change) -> bool,
) -> Result<bool, Error> {
    Ok(SCHEDULE
        .all(ctx)?
        .into_iter()
        .any(|(at, change)| at.1 == name && !overruled.contains(&at) && this(&change)))
}

fn lists(roster: &Roster, id: &str) -> bool {
    roster.programs.contains_key(id) || roster.views.contains_key(id)
}

fn slug_clash(
    ctx: &QueryCtx,
    id: &str,
    renamed_from: Option<&str>,
    after: &Roster,
) -> Result<Option<ProgramId>, Error> {
    let slug = ::program::slug(id);
    let listed = after.programs.keys().chain(after.views.keys()).cloned();
    let waiting = SCHEDULE.all(ctx)?.into_iter();
    let landing = waiting.filter_map(|(_, change)| change.lands().map(str::to_owned));
    let held = OWNERS.all(ctx)?.into_iter().map(|(name, _)| name);
    let itself = |other: &str| other == id || Some(other) == renamed_from;
    Ok(listed
        .chain(landing)
        .chain(held)
        .find(|other| !itself(other) && ::program::slug(other) == slug))
}

pub(crate) fn renames_at(
    ctx: &QueryCtx,
    height: u64,
) -> Result<Vec<(ProgramId, ProgramId)>, Error> {
    let roster = roster(ctx, height, &[])?;
    let landed = RENAMED.scan(ctx, RENAMED.below(&(height + 1)))?;
    let due = due(ctx, height)?;
    let due = due.into_iter().filter_map(|(at, change)| match change {
        Change::Rename { to, .. } => Some((at, to)),
        _ => None,
    });
    let hops: Vec<(ProgramId, ProgramId)> = landed
        .into_iter()
        .chain(due)
        .map(|((_, from), to)| (from, to))
        .collect();
    let through_later_hops = |at: usize, to: &ProgramId| {
        let later = hops[at + 1..].iter();
        later.fold(to.clone(), |id, (from, to)| match *from == id {
            true => to.clone(),
            false => id,
        })
    };
    Ok(hops
        .iter()
        .enumerate()
        .map(|(at, (from, to))| (from.clone(), through_later_hops(at, to)))
        .filter(|(from, to)| lists(&roster, to) && !lists(&roster, from))
        .collect())
}
