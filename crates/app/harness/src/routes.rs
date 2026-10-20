//! Which model service and ccs account answers a thread's next call: its route first, as its prompt
//! cache lives there, then every other listed account in ccs's order; an account that refused or
//! failed this call is passed over for it alone, and harness pins the call to the one picked.
use ::program::{Error, ExecCtx, QueryCtx};
use abi::role::egress::http;
use store::{PageRequest, PageResponse};

use crate::budget::{CCS_ACCOUNT, ORGANIZATION};
use crate::config::Pin;
use crate::resume::header;
use crate::state::{ANSWERED, BUDGETS, MODEL_SERVICES, PICKS, Pick, ROUTES, TRIED};
use crate::turns::serves;
use crate::{Answerer, ModelRow, Route, Routed, ThreadRow};

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

fn labels(ctx: &QueryCtx, service: &str) -> Result<Vec<String>, Error> {
    let all = BUDGETS.scan(ctx, BUDGETS.prefix_of(&service.to_owned()))?;
    Ok(all.into_iter().filter_map(|(_, row)| row.label).collect())
}

fn untried_by(ctx: &QueryCtx, thread: u64, (service, account): &Candidate) -> bool {
    !TRIED.has(ctx, &tried(thread, service, account.as_deref()))
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
    let route = ROUTES.get(ctx, &thread.id)?;
    let at = |route: &Route| {
        let routed = (route.service.clone(), route.account.clone());
        candidates.iter().position(|candidate| *candidate == routed)
    };
    let cached = route
        .as_ref()
        .filter(|route| route.summary == thread.summary);
    let sticky = cached.and_then(at);
    let order = sticky.into_iter().chain(0..candidates.len());
    let mut open = order.filter(|index| untried_by(ctx, thread.id, &candidates[*index]));
    let (index, forced) = match open.next() {
        Some(index) => (index, cached.is_some() && Some(index) != sticky),
        None => (route.as_ref().and_then(at).unwrap_or(0), false),
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
    PICKS.put(ctx, &thread, &pick);
}

pub(crate) fn answered(ctx: &ExecCtx, thread: u64, seq: u64) -> Result<(), Error> {
    let Some(pick) = PICKS.get(ctx, &thread)? else {
        return Ok(());
    };
    let account = pick.label.clone().or(pick.account.clone());
    let before = ROUTES.get(ctx, &thread)?;
    let moved = before
        .as_ref()
        .is_some_and(|route| route.service != pick.service || route.account != account);
    let answerer = Answerer {
        seq,
        service: pick.service.clone(),
        account: account.clone(),
        organization: pick.organization.clone(),
        moved,
    };
    ANSWERED.put(ctx, &(thread, seq), &answerer);
    untried(ctx, thread);
    let passed_over = before.as_ref().is_some_and(|route| {
        let routed = (route.service.clone(), route.account.clone());
        let listed = pick.candidates.contains(&routed);
        moved && listed && route.summary == pick.summary
    });
    if passed_over {
        return Ok(());
    }
    let route = Route {
        service: pick.service,
        account,
        summary: pick.summary,
        height: ctx.env().height,
    };
    ROUTES.put(ctx, &thread, &route);
    Ok(())
}

pub(crate) fn failed(ctx: &ExecCtx, thread: u64) -> Result<bool, Error> {
    let Some(pick) = PICKS.get(ctx, &thread)? else {
        return Ok(false);
    };
    TRIED.insert(ctx, &tried(thread, &pick.service, pick.account.as_deref()));
    if pick
        .candidates
        .iter()
        .any(|candidate| untried_by(ctx, thread, candidate))
    {
        return Ok(true);
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
