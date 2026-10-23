//! Each call goes unpinned to an eligible service in the provider route. The service chooses its
//! account; answerer history records placement without constraining the next call.
use ::program::{Cause, Error, ExecCtx, QueryCtx, Range};
use abi::role::egress::http;
use store::{PageRequest, PageResponse};

use crate::budget::ORGANIZATION;
use crate::providers::Reach;
use crate::resume::header;
use crate::state::{ANSWERED, PICKS, PROVIDER_ROUTES, Pick, ROUTES, TRIED};
use crate::turns::serves;
use crate::{Answerer, Route, Routed, ThreadRow, Wire};

type Candidate = (String, Option<String>);

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
    let scope = ANSWERED.key(&thread);
    let whole = ANSWERED.prefix_of(&thread);
    let below = page.open(&scope)?.map(|cursor| cursor.after);
    let end = [below, whole.end].into_iter().flatten().min();
    let limit = page.limit();
    let newest = Range::new(whole.start, end).reverse();
    let mut rows = ANSWERED.scan(ctx, newest.limit(limit.saturating_add(1)))?;
    let more = rows.len() as u64 > limit;
    rows.truncate(limit as usize);
    let height = ctx.env().height;
    let next = rows.last().filter(|_| more).map(|(key, _)| {
        abi::encode(&store::Cursor {
            height,
            scope: scope.clone(),
            after: ANSWERED.key(key),
        })
    });
    Ok(PageResponse {
        height,
        items: rows.into_iter().map(|(_, answerer)| answerer).collect(),
        next,
    })
}

fn key(service: &str, account: Option<&str>) -> (String, String) {
    (service.to_owned(), account.unwrap_or_default().to_owned())
}

fn tried(thread: u64, service: &str, account: Option<&str>) -> (u64, (String, String)) {
    (thread, key(service, account))
}

fn untried_by(ctx: &QueryCtx, thread: u64, (service, account): &Candidate) -> bool {
    !TRIED.has(ctx, &tried(thread, service, account.as_deref()))
}

pub(crate) fn reaches(ctx: &QueryCtx, provider: &str, service: &str) -> bool {
    let row = PROVIDER_ROUTES
        .get(ctx, &provider.to_owned())
        .ok()
        .flatten();
    row.is_some_and(|row| row.services.iter().any(|entry| entry.service == service))
}

pub(crate) enum Picked {
    Chosen(Pick, Wire),
    Rejected(Option<u64>),
    Unserved(Vec<String>),
}

pub(crate) fn pick(ctx: &ExecCtx, thread: &ThreadRow, reach: &Reach) -> Result<Picked, Error> {
    let mut spoken = Vec::new();
    for entry in &reach.services {
        if serves(ctx, &entry.service)? {
            spoken.push(entry.clone());
        }
    }
    if spoken.is_empty() {
        let services = reach
            .services
            .iter()
            .map(|entry| entry.service.clone())
            .collect();
        return Ok(Picked::Unserved(services));
    }
    let mut eligible = Vec::new();
    let mut resets = Vec::new();
    for entry in &spoken {
        match crate::budget::rejected_until(ctx, &entry.service, &reach.provider)? {
            Some(reset) => resets.push(reset),
            None => eligible.push(entry.clone()),
        }
    }
    let retrying = matches!(ctx.env().cause, Cause::Scheduled { .. });
    let asked_anyway = eligible.is_empty() && !retrying;
    if asked_anyway {
        eligible = spoken;
    }
    let Some(first) = eligible.first() else {
        return Ok(Picked::Rejected(resets.into_iter().min()));
    };
    let candidates: Vec<Candidate> = eligible
        .iter()
        .map(|entry| (entry.service.clone(), None))
        .collect();
    let open = eligible
        .iter()
        .find(|entry| untried_by(ctx, thread.id, &(entry.service.clone(), None)));
    let entry = open.unwrap_or(first);
    let pick = Pick {
        service: entry.service.clone(),
        account: None,
        summary: thread.summary,
        candidates,
        label: None,
        organization: None,
    };
    PICKS.put(ctx, &thread.id, &pick);
    Ok(Picked::Chosen(pick, entry.wire))
}

pub(crate) fn heard(ctx: &ExecCtx, thread: u64, response: &http::Response) {
    let Some(mut pick) = PICKS.get(ctx, &thread).ok().flatten() else {
        return;
    };
    let named = |name: &str| {
        let value = header(response, name).filter(|value| !value.is_empty());
        value.map(str::to_owned)
    };
    pick.label = named(model_wire::budget::ACCOUNT);
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
