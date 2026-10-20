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

const ROOM: [&str; 2] = ["5h", "7d"];
const REJECTED: &str = "rejected";
const SIGNED_OUT: &str = "signed_out";

type Candidate = (String, Option<String>);

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

fn rows(ctx: &QueryCtx, service: &str, account: Option<&str>) -> Result<Vec<Budget>, Error> {
    let all = BUDGETS.scan(ctx, BUDGETS.prefix_of(&service.to_owned()))?;
    let own = all
        .into_iter()
        .map(|(_, row)| row)
        .filter(|row| row.label.as_deref() == account);
    Ok(own.collect())
}

fn labels(ctx: &QueryCtx, service: &str) -> Result<Vec<String>, Error> {
    let all = BUDGETS.scan(ctx, BUDGETS.prefix_of(&service.to_owned()))?;
    Ok(all.into_iter().filter_map(|(_, row)| row.label).collect())
}

fn open(reset: u64, now: u64) -> bool {
    now < reset.saturating_mul(1000)
}

fn out(row: &Budget, now: u64) -> bool {
    let signed_out = row.error.as_deref() == Some(SIGNED_OUT);
    let whole = row.windows.iter().any(|window| {
        let full = window.status.as_deref() == Some(REJECTED) || window.used >= WHOLE_WINDOW;
        ROOM.contains(&window.name.as_str()) && open(window.reset, now) && full
    });
    signed_out || whole
}

fn limited(ctx: &QueryCtx, service: &str, account: Option<&str>) -> Result<bool, Error> {
    let now = ctx.env().time;
    let refused = REFUSED.get(ctx, &key(service, account))?;
    if refused.is_some_and(|until| until.is_none_or(|until| now < until)) {
        return Ok(true);
    }
    Ok(rows(ctx, service, account)?.iter().any(|row| out(row, now)))
}

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
    Chosen { pick: Pick, forced: bool },
    Unlisted(String),
    Unserved(Vec<String>),
}

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
    untried(ctx, thread);
    Ok(false)
}

pub(crate) fn untried(ctx: &ExecCtx, thread: u64) {
    let Ok(passed) = TRIED.scan(ctx, TRIED.prefix_of(&thread)) else {
        return;
    };
    for key in passed {
        TRIED.remove(ctx, &key);
    }
}

pub(crate) fn read(ctx: &ExecCtx, service: &str, label: &str, row: &Budget) {
    if !out(row, ctx.env().time) {
        REFUSED.remove(ctx, &key(service, Some(label)));
    }
}
