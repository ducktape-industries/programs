//! Which model service and ccs account answers a thread's next call (J-090): its route while that
//! account has room, as its prompt cache lives there, else the listed account with the least used;
//! an account that refused or failed the call is passed over, and harness pins the call to it.
use ::program::{Error, ExecCtx, QueryCtx};
use abi::role::egress::http;
use store::{PageRequest, PageResponse};

use crate::budget::{CCS_ACCOUNT, ORGANIZATION};
use crate::config::Pin;
use crate::resume::header;
use crate::state::{
    ANSWERED, BUDGETS, Budget, MODEL_SERVICES, PICKS, Pick, REFUSED, ROUTES, TRIED,
};
use crate::turns::serves;
use crate::{Answerer, ModelRow, Route, Routed, ThreadRow, WHOLE_WINDOW};

// the windows an account's room is measured by: its 5-hour and 7-day ones, as the headers name them
const ROOM: [&str; 2] = ["5h", "7d"];
// a window's status once the account is out of it, and ccs's word for an account it can't use
const REJECTED: &str = "rejected";
const SIGNED_OUT: &str = "signed_out";

// a service, and the ccs account on it the call is pinned to; none lets the service choose
type Candidate = (String, Option<String>);

// every service that reaches a model, its record's first
pub(crate) fn services_of(ctx: &QueryCtx, id: &str, first: &str) -> Result<Vec<String>, Error> {
    let mut services = vec![first.to_owned()];
    for service in MODEL_SERVICES.get(ctx, &id.to_owned())?.unwrap_or_default() {
        if !services.contains(&service) {
            services.push(service);
        }
    }
    Ok(services)
}

pub(crate) fn services(ctx: &QueryCtx, model: &str) -> Result<Option<Vec<String>>, Error> {
    let Some(row) = crate::models::model(ctx, model)? else {
        return Ok(None);
    };
    services_of(ctx, &row.id, &row.service).map(Some)
}

pub(crate) fn route(ctx: &QueryCtx, thread: u64) -> Result<Option<Route>, Error> {
    ROUTES.get(ctx, &thread)
}

pub(crate) fn routes(ctx: &QueryCtx, page: &PageRequest) -> Result<PageResponse<Routed>, Error> {
    Ok(ROUTES
        .range(ctx, page)?
        .map(|(thread, route)| Routed { thread, route }))
}

pub(crate) fn answerers(
    ctx: &QueryCtx,
    thread: u64,
    page: &PageRequest,
) -> Result<PageResponse<Answerer>, Error> {
    Ok(ANSWERED
        .range_of(ctx, &thread, page)?
        .map(|(_, answerer)| answerer))
}

fn key(service: &str, account: Option<&str>) -> (String, String) {
    (service.to_owned(), account.unwrap_or_default().to_owned())
}

fn tried(thread: u64, service: &str, account: Option<&str>) -> (u64, (String, String)) {
    (thread, key(service, account))
}

// the rows an account's room reads: its own, by ccs's name for it, else its service's unnamed ones
fn rows(ctx: &QueryCtx, service: &str, account: Option<&str>) -> Result<Vec<Budget>, Error> {
    let all = BUDGETS.scan(ctx, BUDGETS.prefix_of(&service.to_owned()))?;
    let own = all
        .into_iter()
        .map(|(_, row)| row)
        .filter(|row| row.label.as_deref() == account);
    Ok(own.collect())
}

// the accounts ccs names on a service, in their order
fn labels(ctx: &QueryCtx, service: &str) -> Result<Vec<String>, Error> {
    let all = BUDGETS.scan(ctx, BUDGETS.prefix_of(&service.to_owned()))?;
    Ok(all.into_iter().filter_map(|(_, row)| row.label).collect())
}

fn open(reset: u64, now: u64) -> bool {
    now < reset.saturating_mul(1000)
}

// whether a row says its account has no room now: ccs can't use it, or a 5-hour or 7-day window
// is rejected or whole before its reset
fn out(row: &Budget, now: u64) -> bool {
    let signed_out = row.error.as_deref() == Some(SIGNED_OUT);
    let whole = row.windows.iter().any(|window| {
        let full = window.status.as_deref() == Some(REJECTED) || window.used >= WHOLE_WINDOW;
        ROOM.contains(&window.name.as_str()) && open(window.reset, now) && full
    });
    signed_out || whole
}

// an account with no room: by its rows, or by a 429 it answered that nothing since has lifted
fn limited(ctx: &QueryCtx, service: &str, account: Option<&str>) -> Result<bool, Error> {
    let now = ctx.env().time;
    let refused = REFUSED.get(ctx, &key(service, account))?;
    if refused.is_some_and(|until| until.is_none_or(|until| now < until)) {
        return Ok(true);
    }
    Ok(rows(ctx, service, account)?.iter().any(|row| out(row, now)))
}

// how much of its room an account has used: the most of its 5-hour and 7-day windows before their
// resets; an account harness knows nothing of has used none (chief's J-090 ruling 5)
fn used(ctx: &QueryCtx, service: &str, account: Option<&str>) -> Result<u64, Error> {
    let now = ctx.env().time;
    let rows = rows(ctx, service, account)?;
    let windows = rows.iter().flat_map(|row| &row.windows);
    let counted =
        windows.filter(|window| ROOM.contains(&window.name.as_str()) && open(window.reset, now));
    Ok(counted.map(|window| window.used).max().unwrap_or(0))
}

fn free(ctx: &QueryCtx, thread: u64, service: &str, account: Option<&str>) -> Result<bool, Error> {
    let passed = TRIED.has(ctx, &tried(thread, service, account));
    Ok(!passed && !limited(ctx, service, account)?)
}

// the services an agent's call may go to, and on each the accounts ccs names; a pinned account
// goes on the listed service that names it, else on the first
fn candidates(
    ctx: &QueryCtx,
    served: &[String],
    pinned: Option<&str>,
) -> Result<Vec<Candidate>, Error> {
    if let Some(account) = pinned {
        let mut on = served[0].clone();
        for service in served {
            if labels(ctx, service)?.iter().any(|label| label == account) {
                on = service.clone();
                break;
            }
        }
        return Ok(vec![(on, Some(account.to_owned()))]);
    }
    let mut found = Vec::new();
    for service in served {
        let labels = labels(ctx, service)?;
        if labels.is_empty() {
            found.push((service.clone(), None));
        }
        found.extend(
            labels
                .into_iter()
                .map(|label| (service.clone(), Some(label))),
        );
    }
    Ok(found)
}

pub(crate) enum Picked {
    // the call goes there; `forced` when the thread's cache lives on another account it left
    Chosen { pick: Pick, forced: bool },
    // the agent pins a service its model doesn't list: why
    Unlisted(String),
    // no member node serves any service the call may go to
    Unserved(Vec<String>),
}

// the account a thread's next call goes to, kept as its pick: the route while its account is free
// and no compaction restarted the cache since, else the free account with the least used, else
// (none free) the route's, whose wait the retry keeps
pub(crate) fn pick(
    ctx: &ExecCtx,
    thread: &ThreadRow,
    model: &ModelRow,
    pin: &Pin,
) -> Result<Picked, Error> {
    let mut services = services_of(ctx, &model.id, &model.service)?;
    if let Some(pinned) = &pin.service {
        if !services.contains(pinned) {
            return Ok(Picked::Unlisted(format!(
                "the agent's configuration pins service {pinned}, which model {} is not reached through ({})",
                model.id,
                services.join(", ")
            )));
        }
        services = vec![pinned.clone()];
    }
    let mut served = Vec::new();
    for service in &services {
        if serves(ctx, service)? {
            served.push(service.clone());
        }
    }
    if served.is_empty() {
        return Ok(Picked::Unserved(services));
    }
    let candidates = candidates(ctx, &served, pin.account.as_deref())?;
    let mut room = Vec::new();
    for (index, (service, account)) in candidates.iter().enumerate() {
        if free(ctx, thread.id, service, account.as_deref())? {
            room.push((used(ctx, service, account.as_deref())?, index));
        }
    }
    let route = ROUTES.get(ctx, &thread.id)?;
    let at = |route: &Route| {
        let routed = (route.service.clone(), route.account.clone());
        candidates.iter().position(|candidate| *candidate == routed)
    };
    let cached = route
        .as_ref()
        .filter(|route| route.summary == thread.summary);
    let sticky = cached.and_then(at);
    let free_at = |index: usize| room.iter().any(|(_, free)| *free == index);
    let (index, forced) = match sticky.filter(|index| free_at(*index)) {
        Some(index) => (index, false),
        None => match room.iter().min() {
            Some((_, index)) => (*index, cached.is_some()),
            None => (route.as_ref().and_then(at).unwrap_or(0), false),
        },
    };
    let (service, account) = candidates[index].clone();
    let pick = Pick {
        service,
        account,
        summary: thread.summary,
        candidates,
        label: None,
        organization: None,
    };
    PICKS.put(ctx, &thread.id, &pick);
    Ok(Picked::Chosen { pick, forced })
}

// an answer to a thread's call: which account answered it, and a 2xx lifts that account's 429
pub(crate) fn heard(ctx: &ExecCtx, thread: u64, response: &http::Response) {
    let Some(mut pick) = PICKS.get(ctx, &thread).ok().flatten() else {
        return;
    };
    let named = |name: &str| {
        let value = header(response, name).filter(|value| !value.is_empty());
        value.map(str::to_owned)
    };
    pick.label = named(CCS_ACCOUNT);
    pick.organization = named(ORGANIZATION);
    if (200..300).contains(&response.status) {
        REFUSED.remove(ctx, &key(&pick.service, pick.account.as_deref()));
        if let Some(label) = &pick.label {
            REFUSED.remove(ctx, &key(&pick.service, Some(label)));
        }
    }
    PICKS.put(ctx, &thread, &pick);
}

// the thread's turn or summary at frame `seq` was answered by its pick: recorded, and the route is
// now the account that answered, under the summary the prompt was written with
pub(crate) fn answered(ctx: &ExecCtx, thread: u64, seq: u64) -> Result<(), Error> {
    let Some(pick) = PICKS.get(ctx, &thread)? else {
        return Ok(());
    };
    let account = pick.label.clone().or(pick.account.clone());
    let before = ROUTES.get(ctx, &thread)?;
    let moved =
        before.is_some_and(|route| route.service != pick.service || route.account != account);
    let answerer = Answerer {
        seq,
        service: pick.service.clone(),
        account: account.clone(),
        organization: pick.organization.clone(),
        moved,
    };
    ANSWERED.put(ctx, &(thread, seq), &answerer);
    let route = Route {
        service: pick.service,
        account,
        summary: pick.summary,
        height: ctx.env().height,
    };
    ROUTES.put(ctx, &thread, &route);
    Ok(())
}

// the thread's call failed on its pick: passed over until the thread is answered, and, refused
// with a 429, for every thread until `until` or until it has room again; whether another account
// the call may go to is free now
pub(crate) fn failed(
    ctx: &ExecCtx,
    thread: u64,
    refused: bool,
    until: Option<u64>,
) -> Result<bool, Error> {
    let Some(pick) = PICKS.get(ctx, &thread)? else {
        return Ok(false);
    };
    TRIED.insert(ctx, &tried(thread, &pick.service, pick.account.as_deref()));
    if refused {
        REFUSED.put(ctx, &key(&pick.service, pick.account.as_deref()), &until);
        if let Some(label) = pick.label.as_deref() {
            REFUSED.put(ctx, &key(&pick.service, Some(label)), &until);
        }
    }
    for (service, account) in &pick.candidates {
        if free(ctx, thread, service, account.as_deref())? {
            return Ok(true);
        }
    }
    // none is: the next attempt, after the retry's wait, tries them all afresh
    untried(ctx, thread);
    Ok(false)
}

// a thread answered or given up: every account it passed over is tried afresh
pub(crate) fn untried(ctx: &ExecCtx, thread: u64) {
    let Ok(passed) = TRIED.scan(ctx, TRIED.prefix_of(&thread)) else {
        return;
    };
    for key in passed {
        TRIED.remove(ctx, &key);
    }
}

// a status reading of an account: once it shows room, a 429 the account answered no longer holds
pub(crate) fn read(ctx: &ExecCtx, service: &str, label: &str, row: &Budget) {
    if !out(row, ctx.env().time) {
        REFUSED.remove(ctx, &key(service, Some(label)));
    }
}
