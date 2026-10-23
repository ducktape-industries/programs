//! What each op checks and writes (README.md): kinds and policies, offers and beats, requests placed
//! in their own frame or queued, queue walks one bounded step per booked frame, and assignments
//! that end, decline, miss or lapse, with each side's count and what it rests on.
use ::program::{
    Error, ExecCtx, MessageId, Origin, Outcome as Answered, QueryCtx, Reply as Wanted, code,
    invalid, not_found, unauthorized, wrong_state,
};
use abi::role::egress::DEFAULT_WAIT;

use crate::ordering::{self, UNITS, admits, classes, group_of, weight_of};
use crate::state::{
    ADMITTED, ASSIGNED, BEATS, CHARGED, CHECKS, DECLINED, DISPUTED, HOLDS, IDLE, KIND_OFFERS,
    KINDS, LAST, LEDGER, NOTARY_OF, OFFERS, POLICIES, PolicyRow, QUEUE, RANKS, RECORDS, REQUESTS,
    STATES, StateRow, USED, WALK_AT, WALKS, WalkRow,
};
use crate::{
    AccountNumber, Admission, AdmittedRow, AssignState, Assignment, BeatRow, BlobId, Class,
    Counter, Event, InForce, Isolation, KindRow, KindSpec, LedgerRow, Notify, Offer, OfferId,
    OfferRow, OfferRule, OfferState, OfferView, Op, Outcome, PageRequest, PageResponse, Passed,
    PassedOverRow, Pin, Placement, Policy, Reason, Record, RequestRow, RequestState, RequestView,
    Selector, Spec, Standing, Usage, Waiting, WhyView,
};

fn now(ctx: &QueryCtx) -> u64 {
    ctx.env().height
}

fn kind_row(ctx: &QueryCtx, name: &str) -> Result<KindRow, Error> {
    KINDS
        .get(ctx, &name.to_owned())?
        .ok_or_else(|| not_found(format!("no kind {name}")))
}

fn administer(ctx: &ExecCtx, name: &str) -> Result<KindRow, Error> {
    let row = kind_row(ctx, name)?;
    KINDS.put(ctx, &row.name, &row);
    Ok(row)
}

fn lapse_of(spec: &KindSpec) -> u64 {
    spec.lapse.unwrap_or(DEFAULT_WAIT)
}

fn key(offer: &OfferId) -> (AccountNumber, String) {
    (offer.provider, offer.name.clone())
}

fn id_of(row: &OfferRow) -> OfferId {
    OfferId {
        provider: row.provider,
        name: row.offer.name.clone(),
    }
}

fn offer_row(ctx: &QueryCtx, offer: &OfferId) -> Result<OfferRow, Error> {
    OFFERS
        .get(ctx, &key(offer))?
        .ok_or_else(|| not_found(format!("no offer {} of {}", offer.name, offer.provider)))
}

fn request_row(ctx: &QueryCtx, id: u64) -> Result<RequestRow, Error> {
    REQUESTS
        .get(ctx, &id)?
        .ok_or_else(|| not_found(format!("no request {id}")))
}

fn state_row(ctx: &QueryCtx, id: u64) -> Result<StateRow, Error> {
    STATES
        .get(ctx, &id)?
        .ok_or_else(|| not_found(format!("no request {id}")))
}

fn assignment_at(ctx: &QueryCtx, request: u64, attempt: u32) -> Result<Assignment, Error> {
    ASSIGNED
        .get(ctx, &(request, attempt))?
        .ok_or_else(|| not_found(format!("request {request} has no attempt {attempt}")))
}

fn effective(ctx: &QueryCtx, kind: &str, height: u64) -> Result<(Policy, u64), Error> {
    Ok(match POLICIES.get(ctx, &kind.to_owned())? {
        None => (Policy::default(), 0),
        Some(row) if height > row.height => (row.policy, row.height),
        Some(row) => row.before.unwrap_or((Policy::default(), 0)),
    })
}

fn schedule(ctx: &ExecCtx, at: u64, op: &Op) -> Result<(), Error> {
    let scheduler = ctx.env().role(abi::role::SCHEDULER)?.clone();
    let schedule = abi::role::scheduler::Op::Schedule {
        height: at,
        target: ctx.env().program.clone(),
        payload: abi::encode(op),
    };
    ctx.emit(scheduler, &schedule, Wanted::None);
    Ok(())
}

fn book_step(ctx: &ExecCtx, kind: &str, at: u64) -> Result<(), Error> {
    let booked = (kind.to_owned(), at);
    if WALK_AT.has(ctx, &booked) {
        return Ok(());
    }
    WALK_AT.insert(ctx, &booked);
    schedule(ctx, at, &Op::Place { kind: kind.into() })
}

fn capacity_changed(ctx: &ExecCtx, kind: &str) -> Result<(), Error> {
    let next = now(ctx) + 1;
    book_step(ctx, kind, next)?;
    for (_, notarized) in NOTARY_OF.scan(ctx, NOTARY_OF.prefix_of(&kind.to_owned()))? {
        book_step(ctx, &notarized, next)?;
    }
    Ok(())
}

fn queued_takes(ctx: &QueryCtx, kind: &KindRow, offer: &OfferRow) -> Result<bool, Error> {
    let (policy, _) = effective(ctx, &kind.name, now(ctx))?;
    let first = QUEUE.prefix_of(&kind.name).limit(kind.spec.walk);
    for (_, _, group, id) in QUEUE.scan(ctx, first)? {
        let request = request_row(ctx, id)?;
        let state = state_row(ctx, id)?;
        let notarized = notarized(ctx, &request, &state)?;
        let held_back = over_quota(ctx, &kind.name, &group, &policy)?.is_some();
        let no_notary = notarized && notary_for(ctx, kind, offer.provider)?.is_none();
        let refused = passed(ctx, &request, notarized, offer)?.is_some();
        let takes = !(held_back || no_notary || refused);
        if takes {
            return Ok(true);
        }
    }
    Ok(false)
}

fn share_used(ctx: &QueryCtx, kind: &str, group: &str, policy: &Policy, since: u64) -> u64 {
    ordering::share_used(ctx, &LEDGER, kind, group, policy, since)
}

fn charge(ctx: &ExecCtx, kind: &str, group: &str, meter: &str, amount: u64) -> Result<(), Error> {
    let (policy, since) = effective(ctx, kind, now(ctx))?;
    ordering::charge(ctx, &LEDGER, kind, group, meter, amount, (&policy, since))
}

fn over_quota(
    ctx: &QueryCtx,
    kind: &str,
    group: &str,
    policy: &Policy,
) -> Result<Option<Waiting>, Error> {
    ordering::over_quota(ctx, &LEDGER, kind, group, policy)
}

fn record(ctx: &ExecCtx, offer: &OfferId, count: impl FnOnce(&mut Record)) -> Result<(), Error> {
    let mut row = RECORDS.get(ctx, &key(offer))?.unwrap_or_default();
    count(&mut row);
    RECORDS.put(ctx, &key(offer), &row);
    Ok(())
}

fn settle_idle(ctx: &ExecCtx, row: &mut OfferRow) {
    let kind = row.offer.kind.clone();
    if let Some(since) = row.idle_since {
        IDLE.remove(
            ctx,
            &(kind.clone(), since, row.provider, row.offer.name.clone()),
        );
    }
    let ready = row.state == OfferState::Ready;
    let room = row.busy < row.offer.capacity;
    let admitted = ADMITTED.has(ctx, &(kind.clone(), row.provider));
    let free = ready && room && admitted;
    row.idle_since = match free {
        true => {
            let since = row.idle_since.unwrap_or(now(ctx));
            IDLE.insert(ctx, &(kind, since, row.provider, row.offer.name.clone()));
            Some(since)
        }
        false => None,
    };
}

fn book_check(ctx: &ExecCtx, offer: &(AccountNumber, String), lapse: u64) -> Result<(), Error> {
    let height = now(ctx);
    if CHECKS.get(ctx, offer)?.is_some_and(|at| at > height) {
        return Ok(());
    }
    let beat = BEATS.get(ctx, offer)?.map_or(height, |beat| beat.height);
    let at = beat.saturating_add(lapse).max(height + 1);
    CHECKS.put(ctx, offer, &at);
    let check = Op::Lapse {
        offer: OfferId {
            provider: offer.0,
            name: offer.1.clone(),
        },
    };
    schedule(ctx, at, &check)
}

pub(crate) fn register_kind(ctx: &ExecCtx, name: String, spec: KindSpec) -> Result<(), Error> {
    if name.is_empty() {
        return Err(invalid("a kind has a name"));
    }
    if spec.walk == 0 {
        return Err(invalid(
            "walk is how many queued requests a step examines: at least 1",
        ));
    }
    if let Some(looped) = spec
        .meters
        .iter()
        .find(|meter| meter.checked_by == Some(meter.reported_by))
    {
        return Err(invalid(format!("meter {} can't check itself", looped.name)));
    }
    if spec.notary.as_deref() == Some(name.as_str()) {
        return Err(invalid("a kind can't notarize itself"));
    }
    let held = KINDS.get(ctx, &name)?;
    let before = held.as_ref().and_then(|held| held.spec.notary.clone());
    let consent = match (&spec.notary, spec.notary != before) {
        (Some(notary), true) => Some(notary.clone()),
        _ => None,
    };
    if let Some(notary) = consent {
        administer(ctx, &notary)?;
    }
    let row = match held {
        Some(mut held) => {
            if let Some(notary) = &held.spec.notary {
                NOTARY_OF.remove(ctx, &(notary.clone(), name.clone()));
            }
            held.spec = spec;
            held
        }
        None => KindRow {
            name,
            owner: ctx.sender_account()?,
            spec,
            registered: now(ctx),
        },
    };
    if let Some(notary) = &row.spec.notary {
        NOTARY_OF.insert(ctx, &(notary.clone(), row.name.clone()));
    }
    KINDS.put(ctx, &row.name, &row);
    book_step(ctx, &row.name, now(ctx) + 1)
}

pub(crate) fn set_policy(ctx: &ExecCtx, kind: String, policy: Policy) -> Result<(), Error> {
    administer(ctx, &kind)?;
    let mut names: Vec<&str> = policy
        .classes
        .iter()
        .map(|class| class.name.as_str())
        .collect();
    names.sort_unstable();
    if names.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(invalid("two classes share a name"));
    }
    let mut groups: Vec<&str> = policy
        .groups
        .iter()
        .map(|group| group.name.as_str())
        .collect();
    groups.sort_unstable();
    if groups.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(invalid("two groups share a name"));
    }
    if policy
        .groups
        .iter()
        .any(|group| group.name.is_empty() || group.weight == 0)
    {
        return Err(invalid("a group has a name and a weight of at least 1"));
    }
    if policy.quotas.iter().any(|quota| quota.window == 0) {
        return Err(invalid("a quota's window is at least one block"));
    }
    if policy.share.window == Some(0) {
        return Err(invalid("the share window is at least one block"));
    }
    let height = now(ctx);
    let before = effective(ctx, &kind, height)?;
    let row = PolicyRow {
        policy,
        height,
        before: Some(before),
    };
    POLICIES.put(ctx, &kind, &row);
    book_step(ctx, &kind, height + 1)
}

pub(crate) fn admit(ctx: &ExecCtx, kind: String, provider: AccountNumber) -> Result<(), Error> {
    administer(ctx, &kind)?;
    let row = AdmittedRow {
        provider,
        by: ctx.sender_account()?,
        height: now(ctx),
    };
    ADMITTED.put(ctx, &(kind.clone(), provider), &row);
    for (_, _, name) in KIND_OFFERS.scan(ctx, KIND_OFFERS.prefix_of(&(kind.clone(), provider)))? {
        let at = (provider, name);
        if let Some(mut offer) = OFFERS.get(ctx, &at)? {
            settle_idle(ctx, &mut offer);
            OFFERS.put(ctx, &at, &offer);
        }
    }
    capacity_changed(ctx, &kind)
}

pub(crate) fn evict(ctx: &ExecCtx, kind: String, provider: AccountNumber) -> Result<(), Error> {
    administer(ctx, &kind)?;
    ADMITTED.remove(ctx, &(kind.clone(), provider));
    let next = now(ctx) + 1;
    for (_, _, name) in KIND_OFFERS.scan(ctx, KIND_OFFERS.prefix_of(&(kind.clone(), provider)))? {
        let at = (provider, name.clone());
        if let Some(mut offer) = OFFERS.get(ctx, &at)? {
            settle_idle(ctx, &mut offer);
            OFFERS.put(ctx, &at, &offer);
        }
        schedule(
            ctx,
            next,
            &Op::Lapse {
                offer: OfferId { provider, name },
            },
        )?;
    }
    Ok(())
}

pub(crate) fn offer(ctx: &ExecCtx, offer: Offer) -> Result<(), Error> {
    let provider = ctx.sender_account()?;
    let kind = kind_row(ctx, &offer.kind)?;
    if offer.name.is_empty() {
        return Err(invalid("an offer has a name"));
    }
    if offer.capacity == 0 {
        return Err(invalid("an offer takes at least one slot"));
    }
    let height = now(ctx);
    let at = (provider, offer.name.clone());
    let mut row = match OFFERS.get(ctx, &at)? {
        Some(mut held) => {
            let moves = held.offer.kind != offer.kind;
            if moves {
                if held.busy > 0 {
                    return Err(wrong_state("an offer holding work can't change its kind"));
                }
                held.state = OfferState::Gone;
                settle_idle(ctx, &mut held);
                KIND_OFFERS.remove(
                    ctx,
                    &(held.offer.kind.clone(), provider, held.offer.name.clone()),
                );
            }
            held.offer = offer;
            held.state = OfferState::Ready;
            held
        }
        None => OfferRow {
            provider,
            offer,
            state: OfferState::Ready,
            busy: 0,
            idle_since: None,
            since: height,
        },
    };
    KIND_OFFERS.insert(
        ctx,
        &(row.offer.kind.clone(), provider, row.offer.name.clone()),
    );
    settle_idle(ctx, &mut row);
    OFFERS.put(ctx, &at, &row);
    let beat = BeatRow {
        height,
        free: row.offer.capacity.saturating_sub(row.busy),
        readings: Vec::new(),
    };
    BEATS.put(ctx, &at, &beat);
    book_check(ctx, &at, lapse_of(&kind.spec))?;
    capacity_changed(ctx, &kind.name)
}

fn own_offer(ctx: &ExecCtx, name: &str) -> Result<((AccountNumber, String), OfferRow), Error> {
    let at = (ctx.sender_account()?, name.to_owned());
    let row = OFFERS
        .get(ctx, &at)?
        .ok_or_else(|| not_found(format!("you have no offer {name}")))?;
    Ok((at, row))
}

pub(crate) fn withdraw(ctx: &ExecCtx, name: String) -> Result<(), Error> {
    let (at, mut row) = own_offer(ctx, &name)?;
    row.state = match row.busy {
        0 => OfferState::Gone,
        _ => OfferState::Draining,
    };
    settle_idle(ctx, &mut row);
    OFFERS.put(ctx, &at, &row);
    Ok(())
}

pub(crate) fn beat(
    ctx: &ExecCtx,
    name: String,
    free: u64,
    readings: Vec<(String, u64)>,
) -> Result<(), Error> {
    let (at, mut row) = own_offer(ctx, &name)?;
    let gone = matches!(row.state, OfferState::Gone | OfferState::Draining);
    if gone {
        return Err(wrong_state(format!("offer {name} is withdrawn")));
    }
    let beat = BeatRow {
        height: now(ctx),
        free,
        readings,
    };
    BEATS.put(ctx, &at, &beat);
    let kind = kind_row(ctx, &row.offer.kind)?;
    let back = matches!(row.state, OfferState::Missed | OfferState::Lapsed);
    if back {
        row.state = OfferState::Ready;
        settle_idle(ctx, &mut row);
        OFFERS.put(ctx, &at, &row);
        capacity_changed(ctx, &kind.name)?;
    }
    book_check(ctx, &at, lapse_of(&kind.spec))
}

struct Asked {
    id: u64,
    request: RequestRow,
    class: Class,
    group: String,
}

fn asked(ctx: &ExecCtx, spec: Spec) -> Result<(Asked, KindRow), Error> {
    let kind = kind_row(ctx, &spec.kind)?;
    if spec.needs.units == 0 {
        return Err(invalid("a request needs at least one unit"));
    }
    let by = ctx.sender_account()?;
    let via = match &ctx.env().origin {
        Origin::Program(program) => Some(program.clone()),
        _ => None,
    };
    let height = now(ctx);
    let (policy, _) = effective(ctx, &kind.name, height)?;
    let class = classes(&policy)
        .into_iter()
        .find(|class| class.name == spec.class)
        .ok_or_else(|| invalid(format!("kind {} has no class {:?}", kind.name, spec.class)))?;
    let refused = match &class.may_use {
        Some(may_use) => !admits(ctx, may_use, by)?,
        None => false,
    };
    if refused {
        return Err(unauthorized(format!(
            "class {:?} doesn't admit account {by}",
            class.name
        )));
    }
    let notarized = spec.needs.notarized || class.notarized;
    let unnotarized = notarized && kind.spec.notary.is_none();
    if unnotarized {
        return Err(invalid(format!("kind {} names no notary kind", kind.name)));
    }
    let group = group_of(ctx, &policy, by)?;
    let id = LAST.get(ctx)?.unwrap_or(0) + 1;
    let request = RequestRow {
        id,
        by,
        via,
        spec,
        made: height,
    };
    Ok((
        Asked {
            id,
            request,
            class,
            group,
        },
        kind,
    ))
}

fn keep(ctx: &ExecCtx, asked: &Asked) {
    LAST.put(ctx, &asked.id);
    REQUESTS.put(ctx, &asked.id, &asked.request);
    let state = StateRow {
        attempt: 0,
        state: RequestState::Queued,
        class: asked.class.name.clone(),
        group: asked.group.clone(),
    };
    STATES.put(ctx, &asked.id, &state);
}

pub(crate) fn request(ctx: &ExecCtx, spec: Spec) -> Result<(), Error> {
    let (asked, _) = asked(ctx, spec)?;
    keep(ctx, &asked);
    try_place(ctx, asked.id)?;
    ctx.set_return_data(abi::encode(&asked.id));
    Ok(())
}

pub(crate) fn pick(ctx: &ExecCtx, spec: Spec) -> Result<(), Error> {
    let (asked, kind) = asked(ctx, spec)?;
    let state = StateRow {
        attempt: 0,
        state: RequestState::Queued,
        class: asked.class.name.clone(),
        group: asked.group.clone(),
    };
    let taken = match choose(ctx, &asked.request, &state, &kind)? {
        Ok(taken) => taken,
        Err(waiting) => {
            return Err(wrong_state(format!(
                "no admitted offer takes it now: {waiting:?}"
            )));
        }
    };
    if queued_takes(ctx, &kind, &taken.offer)? {
        return Err(wrong_state(format!(
            "a queued request of {} takes offer {} first: Request joins the queue",
            kind.name, taken.offer.offer.name
        )));
    }
    keep(ctx, &asked);
    let placement = assign(ctx, asked.id, taken, true)?;
    ctx.set_return_data(abi::encode(&placement));
    Ok(())
}

struct Taken {
    offer: OfferRow,
    notary: Option<OfferId>,
    rule: OfferRule,
}

fn covers(offered: Isolation, needed: Isolation) -> bool {
    match (offered, needed) {
        (_, Isolation::None) => true,
        (Isolation::Landlock { abi: has }, Isolation::Landlock { abi: wants }) => has >= wants,
        (offered, needed) => offered == needed,
    }
}

fn holds_label(offer: &Offer, selector: &Selector) -> bool {
    match selector {
        Selector::Is(name, value) => offer
            .labels
            .iter()
            .any(|(held, its)| held == name && its == value),
        Selector::Has(name) => offer.labels.iter().any(|(held, _)| held == name),
    }
}

fn passed(
    ctx: &QueryCtx,
    request: &RequestRow,
    notarized: bool,
    row: &OfferRow,
) -> Result<Option<Passed>, Error> {
    let needs = &request.spec.needs;
    let offer = &row.offer;
    if !ADMITTED.has(ctx, &(offer.kind.clone(), row.provider)) {
        return Ok(Some(Passed::NotAdmitted));
    }
    if row.state != OfferState::Ready {
        return Ok(Some(Passed::NotReady(row.state)));
    }
    if DECLINED.has(ctx, &(request.id, row.provider, offer.name.clone())) {
        return Ok(Some(Passed::Declined));
    }
    if row.busy.saturating_add(needs.units) > offer.capacity {
        return Ok(Some(Passed::Full));
    }
    if !needs
        .labels
        .iter()
        .all(|selector| holds_label(offer, selector))
    {
        return Ok(Some(Passed::Labels));
    }
    if !covers(offer.isolation, needs.isolation) {
        return Ok(Some(Passed::Isolation));
    }
    let unserved = match &offer.terms.serves {
        Some(serves) => !admits(ctx, serves, request.by)?,
        None => false,
    };
    if unserved {
        return Ok(Some(Passed::Terms));
    }
    let unnotarizing = notarized && !offer.terms.notarizes;
    if unnotarizing {
        return Ok(Some(Passed::NotNotarizing));
    }
    let elsewhere = match &needs.pin {
        Some(Pin::Only(only)) => *only != id_of(row),
        _ => false,
    };
    if elsewhere {
        return Ok(Some(Passed::NotPinned));
    }
    Ok(None)
}

fn notary_for(
    ctx: &QueryCtx,
    kind: &KindRow,
    besides: AccountNumber,
) -> Result<Option<OfferId>, Error> {
    let Some(notary) = &kind.spec.notary else {
        return Ok(None);
    };
    for (_, _, provider, name) in IDLE.scan(ctx, IDLE.prefix_of(notary))? {
        if provider != besides {
            return Ok(Some(OfferId { provider, name }));
        }
    }
    Ok(None)
}

fn notarized(ctx: &QueryCtx, request: &RequestRow, state: &StateRow) -> Result<bool, Error> {
    if request.spec.needs.notarized {
        return Ok(true);
    }
    let (policy, _) = effective(ctx, &request.spec.kind, now(ctx))?;
    Ok(classes(&policy)
        .iter()
        .any(|class| class.name == state.class && class.notarized))
}

fn choose(
    ctx: &QueryCtx,
    request: &RequestRow,
    state: &StateRow,
    kind: &KindRow,
) -> Result<Result<Taken, Vec<Waiting>>, Error> {
    let (policy, _) = effective(ctx, &kind.name, now(ctx))?;
    if let Some(over) = over_quota(ctx, &kind.name, &state.group, &policy)? {
        return Ok(Err(vec![over]));
    }
    let notarized = notarized(ctx, request, state)?;
    let mut candidates: Vec<(OfferId, OfferRule)> = Vec::new();
    match &request.spec.needs.pin {
        Some(Pin::Only(only)) => candidates.push((only.clone(), OfferRule::Only)),
        Some(Pin::Prefer(preferred)) => candidates.push((preferred.clone(), OfferRule::Preferred)),
        None => {}
    }
    let pinned_only = matches!(request.spec.needs.pin, Some(Pin::Only(_)));
    if !pinned_only {
        for (_, _, provider, name) in IDLE.scan(ctx, IDLE.prefix_of(&kind.name))? {
            candidates.push((OfferId { provider, name }, OfferRule::FreeLongest));
        }
    }
    let mut no_notary = false;
    for (candidate, rule) in candidates {
        let Some(row) = OFFERS.get(ctx, &key(&candidate))? else {
            continue;
        };
        if row.offer.kind != kind.name {
            continue;
        }
        if passed(ctx, request, notarized, &row)?.is_some() {
            continue;
        }
        let notary = match notarized {
            true => match notary_for(ctx, kind, row.provider)? {
                Some(notary) => Some(notary),
                None => {
                    no_notary = true;
                    continue;
                }
            },
            false => None,
        };
        return Ok(Ok(Taken {
            offer: row,
            notary,
            rule,
        }));
    }
    let waiting = match (no_notary, &kind.spec.notary) {
        (true, Some(notary)) => Waiting::NoNotary {
            kind: notary.clone(),
        },
        _ => Waiting::NoOfferTakes,
    };
    Ok(Err(vec![waiting]))
}

fn assign(ctx: &ExecCtx, id: u64, taken: Taken, claimed: bool) -> Result<Placement, Error> {
    let request = request_row(ctx, id)?;
    let mut state = state_row(ctx, id)?;
    let kind = kind_row(ctx, &request.spec.kind)?;
    let height = now(ctx);
    let (policy, since) = effective(ctx, &kind.name, height)?;
    state.attempt = state
        .attempt
        .checked_add(1)
        .ok_or_else(|| Error::new(code::EXHAUSTED, "attempts are exhausted"))?;
    state.state = RequestState::Placed;
    let offer = id_of(&taken.offer);
    let reason = Reason {
        class: state.class.clone(),
        group: state.group.clone(),
        group_used: share_used(ctx, &kind.name, &state.group, &policy, since),
        group_weight: weight_of(&policy, &state.group),
        rule: taken.rule,
    };
    let assignment = Assignment {
        request: id,
        attempt: state.attempt,
        offer: offer.clone(),
        notary: taken.notary.clone(),
        height,
        reason,
        state: match claimed {
            true => AssignState::Claimed,
            false => AssignState::Assigned,
        },
    };
    ASSIGNED.put(ctx, &(id, state.attempt), &assignment);
    QUEUE.remove(
        ctx,
        &(
            kind.name.clone(),
            state.class.clone(),
            state.group.clone(),
            id,
        ),
    );
    STATES.put(ctx, &id, &state);
    HOLDS.insert(ctx, &(offer.provider, offer.name.clone(), id));
    let mut row = taken.offer;
    row.busy = row.busy.saturating_add(request.spec.needs.units);
    settle_idle(ctx, &mut row);
    OFFERS.put(ctx, &key(&offer), &row);
    record(ctx, &offer, |counts| {
        counts.assigned += 1;
        counts.claimed += u64::from(claimed);
    })?;
    charge(
        ctx,
        &kind.name,
        &state.group,
        UNITS,
        request.spec.needs.units,
    )?;
    if !claimed {
        let check = Op::Unclaimed {
            request: id,
            attempt: state.attempt,
        };
        schedule(ctx, height.saturating_add(lapse_of(&kind.spec)), &check)?;
    }
    let placement = Placement {
        request: id,
        attempt: state.attempt,
        offer,
        reach: row.offer.reach.clone(),
        notary: taken.notary,
    };
    tell(ctx, &request, &Event::Assigned(placement.clone()))?;
    Ok(placement)
}

fn enqueue(ctx: &ExecCtx, id: u64, kind: &str) -> Result<(), Error> {
    let mut state = state_row(ctx, id)?;
    state.state = RequestState::Queued;
    STATES.put(ctx, &id, &state);
    QUEUE.insert(ctx, &(kind.to_owned(), state.class, state.group, id));
    if let Some(mut walk) = WALKS.get(ctx, &kind.to_owned())? {
        walk.behind = true;
        WALKS.put(ctx, &kind.to_owned(), &walk);
    }
    Ok(())
}

fn try_place(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    let request = request_row(ctx, id)?;
    let kind = kind_row(ctx, &request.spec.kind)?;
    let state = state_row(ctx, id)?;
    match choose(ctx, &request, &state, &kind)? {
        Ok(taken) => {
            if queued_takes(ctx, &kind, &taken.offer)? {
                enqueue(ctx, id, &kind.name)?;
                return book_step(ctx, &kind.name, now(ctx) + 1);
            }
            assign(ctx, id, taken, false).map(drop)
        }
        Err(waiting) => {
            enqueue(ctx, id, &kind.name)?;
            for wait in waiting {
                if let Waiting::OverQuota { until, .. } = wait {
                    book_step(ctx, &kind.name, until)?;
                }
            }
            Ok(())
        }
    }
}

fn order(ctx: &QueryCtx, kind: &str, policy: &Policy, since: u64) -> Vec<(String, String)> {
    ordering::order(ctx, &LEDGER, kind, policy, since)
}

fn next_queued(
    ctx: &QueryCtx,
    kind: &str,
    walk: &mut WalkRow,
) -> Result<Option<(String, String, u64)>, Error> {
    ordering::next_queued(ctx, &QUEUE, kind, walk)
}

fn any_free(ctx: &QueryCtx, kind: &str) -> bool {
    !ctx.scan(IDLE.prefix_of(&kind.to_owned()).limit(1))
        .is_empty()
}

pub(crate) fn place(ctx: &ExecCtx, kind: String) -> Result<(), Error> {
    let height = now(ctx);
    WALK_AT.remove(ctx, &(kind.clone(), height));
    let Some(row) = KINDS.get(ctx, &kind)? else {
        return Ok(());
    };
    let (policy, since) = effective(ctx, &kind, height)?;
    let mut walk = match WALKS.get(ctx, &kind)? {
        Some(walk) => walk,
        None => WalkRow {
            order: order(ctx, &kind, &policy, since),
            at: 0,
            after: None,
            behind: false,
        },
    };
    let mut examined = 0;
    loop {
        if !any_free(ctx, &kind) {
            WALKS.remove(ctx, &kind);
            return Ok(());
        }
        let Some((class, group, id)) = next_queued(ctx, &kind, &mut walk)? else {
            WALKS.remove(ctx, &kind);
            if walk.behind {
                book_step(ctx, &kind, height + 1)?;
            }
            return Ok(());
        };
        examined += 1;
        walk.after = Some((class.clone(), group.clone(), id));
        let request = request_row(ctx, id)?;
        let state = state_row(ctx, id)?;
        match choose(ctx, &request, &state, &row)? {
            Ok(taken) => {
                assign(ctx, id, taken, false)?;
            }
            Err(waiting) => {
                for wait in waiting {
                    if let Waiting::OverQuota { until, .. } = wait {
                        book_step(ctx, &kind, until)?;
                        walk.after = Some((class.clone(), group.clone(), u64::MAX));
                    }
                }
            }
        }
        if examined == row.spec.walk {
            if any_free(ctx, &kind) {
                WALKS.put(ctx, &kind, &walk);
                book_step(ctx, &kind, height + 1)?;
            } else {
                WALKS.remove(ctx, &kind);
            }
            return Ok(());
        }
    }
}

fn release(ctx: &ExecCtx, assignment: &Assignment, units: u64) -> Result<OfferRow, Error> {
    let offer = &assignment.offer;
    HOLDS.remove(
        ctx,
        &(offer.provider, offer.name.clone(), assignment.request),
    );
    let mut row = offer_row(ctx, offer)?;
    row.busy = row.busy.saturating_sub(units);
    let drained = row.state == OfferState::Draining && row.busy == 0;
    if drained {
        row.state = OfferState::Gone;
    }
    settle_idle(ctx, &mut row);
    OFFERS.put(ctx, &key(offer), &row);
    Ok(row)
}

fn miss_offer(ctx: &ExecCtx, offer: &OfferId) -> Result<(), Error> {
    let mut row = offer_row(ctx, offer)?;
    if row.state == OfferState::Ready {
        row.state = OfferState::Missed;
        settle_idle(ctx, &mut row);
        OFFERS.put(ctx, &key(offer), &row);
    }
    Ok(())
}

fn again(ctx: &ExecCtx, request: &RequestRow, stopping: bool, retry: bool) -> Result<(), Error> {
    let mut state = state_row(ctx, request.id)?;
    let lost = Event::Lost {
        request: request.id,
        attempt: state.attempt,
    };
    match (stopping, retry) {
        (true, _) => {
            state.state = RequestState::Cancelled;
            STATES.put(ctx, &request.id, &state);
            Ok(())
        }
        (false, true) => {
            state.state = RequestState::Queued;
            STATES.put(ctx, &request.id, &state);
            try_place(ctx, request.id)
        }
        (false, false) => {
            state.state = RequestState::Lost;
            STATES.put(ctx, &request.id, &state);
            tell(ctx, request, &lost)
        }
    }
}

fn current(ctx: &QueryCtx, request: u64, attempt: u32) -> Result<(RequestRow, Assignment), Error> {
    let row = request_row(ctx, request)?;
    let state = state_row(ctx, request)?;
    if state.attempt != attempt {
        return Err(wrong_state(format!(
            "request {request} is at attempt {}, not {attempt}",
            state.attempt
        )));
    }
    Ok((row, assignment_at(ctx, request, attempt)?))
}

fn provider_of(ctx: &ExecCtx, assignment: &Assignment) -> Result<(), Error> {
    let sender = ctx.sender_account()?;
    match sender == assignment.offer.provider {
        true => Ok(()),
        false => Err(wrong_state(format!(
            "request {} at attempt {} is assigned to another offer",
            assignment.request, assignment.attempt
        ))),
    }
}

fn via_of(ctx: &ExecCtx, request: &RequestRow) -> Result<(), Error> {
    let through = match &ctx.env().origin {
        Origin::Program(program) => request.via.as_ref() == Some(program),
        _ => false,
    };
    match through {
        true => Ok(()),
        false => Err(unauthorized(format!(
            "only the program request {} came through sends this",
            request.id
        ))),
    }
}

fn live(assignment: &Assignment) -> bool {
    matches!(
        assignment.state,
        AssignState::Assigned | AssignState::Claimed | AssignState::Stopping
    )
}

pub(crate) fn claim(ctx: &ExecCtx, request: u64, attempt: u32) -> Result<(), Error> {
    let (_, mut assignment) = current(ctx, request, attempt)?;
    provider_of(ctx, &assignment)?;
    if assignment.state != AssignState::Assigned {
        return Err(wrong_state(format!(
            "request {request} at {attempt} isn't waiting for a claim"
        )));
    }
    assignment.state = AssignState::Claimed;
    ASSIGNED.put(ctx, &(request, attempt), &assignment);
    record(ctx, &assignment.offer, |counts| counts.claimed += 1)
}

pub(crate) fn decline(ctx: &ExecCtx, request: u64, attempt: u32, why: String) -> Result<(), Error> {
    let (row, mut assignment) = current(ctx, request, attempt)?;
    provider_of(ctx, &assignment)?;
    if !live(&assignment) {
        return Err(wrong_state(format!(
            "request {request} at {attempt} has ended: {why}"
        )));
    }
    let stopping = assignment.state == AssignState::Stopping;
    assignment.state = AssignState::Declined;
    ASSIGNED.put(ctx, &(request, attempt), &assignment);
    let offer = assignment.offer.clone();
    DECLINED.insert(ctx, &(request, offer.provider, offer.name.clone()));
    release(ctx, &assignment, row.spec.needs.units)?;
    record(ctx, &offer, |counts| counts.declined += 1)?;
    again(ctx, &row, stopping, true)?;
    capacity_changed(ctx, &row.spec.kind)
}

fn usage_of(ctx: &QueryCtx, request: u64, attempt: u32) -> Result<Usage, Error> {
    Ok(USED.get(ctx, &(request, attempt))?.unwrap_or(Usage {
        provider: Vec::new(),
        provider_at: None,
        evidence: None,
        requester: Vec::new(),
        requester_at: None,
        chain: Vec::new(),
        standing: Standing::Counted,
    }))
}

fn readable(kind: &KindRow, counter: Counter, readings: &[(String, u64)]) -> Result<(), Error> {
    for (name, _) in readings {
        let counts = kind.spec.meters.iter().any(|meter| {
            let reports = meter.reported_by == counter;
            let checks = meter.checked_by == Some(counter);
            meter.name == *name && (reports || checks)
        });
        if !counts {
            return Err(invalid(format!(
                "{counter:?} doesn't count meter {name} of {}",
                kind.name
            )));
        }
    }
    Ok(())
}

fn side(usage: &Usage, counter: Counter, meter: &str) -> Option<u64> {
    let (counts, given) = match counter {
        Counter::Provider => (&usage.provider, usage.provider_at.is_some()),
        Counter::Requester => (&usage.requester, usage.requester_at.is_some()),
        Counter::Chain => (&usage.chain, !usage.chain.is_empty()),
    };
    let found = counts
        .iter()
        .find(|(name, _)| name == meter)
        .map(|(_, count)| *count);
    match given {
        true => Some(found.unwrap_or(0)),
        false => None,
    }
}

fn agree(a: u64, b: u64, tolerance_ppm: u32) -> bool {
    let gap = u128::from(a.abs_diff(b)) * 1_000_000;
    gap <= u128::from(tolerance_ppm) * u128::from(a.max(b))
}

fn standing(kind: &KindRow, policy: &Policy, usage: &Usage) -> Standing {
    let mut disputed = Vec::new();
    let mut checked = 0;
    let mut both = 0;
    for meter in &kind.spec.meters {
        let Some(checker) = meter.checked_by else {
            continue;
        };
        checked += 1;
        let pair = (
            side(usage, meter.reported_by, &meter.name),
            side(usage, checker, &meter.name),
        );
        if let (Some(reported), Some(check)) = pair {
            both += 1;
            if !agree(reported, check, policy.tolerance_ppm) {
                disputed.push(meter.name.clone());
            }
        }
    }
    if !disputed.is_empty() {
        return Standing::Disputed { meters: disputed };
    }
    let confirmed = checked > 0 && both == checked;
    match (confirmed, usage.provider_at, usage.requester_at) {
        (true, _, _) => Standing::Confirmed,
        (false, Some(_), _) => Standing::Attested,
        (false, None, Some(_)) => Standing::Claimed,
        (false, None, None) => Standing::Counted,
    }
}

fn counted(
    ctx: &ExecCtx,
    row: &RequestRow,
    attempt: u32,
    counter: Counter,
    readings: Vec<(String, u64)>,
    evidence: Option<BlobId>,
) -> Result<(), Error> {
    let kind = kind_row(ctx, &row.spec.kind)?;
    readable(&kind, counter, &readings)?;
    let state = state_row(ctx, row.id)?;
    let mut usage = usage_of(ctx, row.id, attempt)?;
    for meter in kind
        .spec
        .meters
        .iter()
        .filter(|meter| meter.reported_by == counter)
    {
        let at = (row.id, attempt, meter.name.clone());
        let charged = CHARGED.get(ctx, &at)?.unwrap_or(0);
        let after = readings
            .iter()
            .find(|(name, _)| *name == meter.name)
            .map_or(0, |(_, count)| *count);
        if after > charged {
            CHARGED.put(ctx, &at, &after);
            charge(ctx, &kind.name, &state.group, &meter.name, after - charged)?;
        }
    }
    let height = now(ctx);
    match counter {
        Counter::Provider => {
            usage.provider = readings;
            usage.provider_at = Some(height);
            if evidence.is_some() {
                usage.evidence = evidence;
            }
        }
        Counter::Requester => {
            usage.requester = readings;
            usage.requester_at = Some(height);
        }
        Counter::Chain => usage.chain = readings,
    }
    let (policy, _) = effective(ctx, &kind.name, height)?;
    usage.standing = standing(&kind, &policy, &usage);
    let disputed = matches!(usage.standing, Standing::Disputed { .. });
    USED.put(ctx, &(row.id, attempt), &usage);
    let first = disputed && !DISPUTED.has(ctx, &(row.id, attempt));
    if first {
        DISPUTED.insert(ctx, &(row.id, attempt));
        let assignment = assignment_at(ctx, row.id, attempt)?;
        record(ctx, &assignment.offer, |counts| counts.disputed += 1)?;
    }
    Ok(())
}

fn finish(
    ctx: &ExecCtx,
    row: &RequestRow,
    mut assignment: Assignment,
    outcome: Outcome,
) -> Result<(), Error> {
    let stopping = assignment.state == AssignState::Stopping;
    assignment.state = AssignState::Ended(outcome);
    ASSIGNED.put(ctx, &(assignment.request, assignment.attempt), &assignment);
    release(ctx, &assignment, row.spec.needs.units)?;
    record(ctx, &assignment.offer, |counts| counts.ended += 1)?;
    let mut state = state_row(ctx, row.id)?;
    state.state = match stopping {
        true => RequestState::Cancelled,
        false => RequestState::Ended,
    };
    STATES.put(ctx, &row.id, &state);
    let ended = Event::Ended {
        request: row.id,
        attempt: assignment.attempt,
        outcome,
    };
    tell(ctx, row, &ended)?;
    capacity_changed(ctx, &row.spec.kind)
}

pub(crate) fn report(
    ctx: &ExecCtx,
    request: u64,
    attempt: u32,
    outcome: Option<Outcome>,
    readings: Vec<(String, u64)>,
    evidence: Option<BlobId>,
) -> Result<(), Error> {
    let (row, assignment) = current(ctx, request, attempt)?;
    provider_of(ctx, &assignment)?;
    let running = matches!(
        assignment.state,
        AssignState::Claimed | AssignState::Stopping
    );
    if !running {
        return Err(wrong_state(format!(
            "request {request} at {attempt} isn't running"
        )));
    }
    counted(ctx, &row, attempt, Counter::Provider, readings, evidence)?;
    match outcome {
        Some(outcome) => finish(ctx, &row, assignment, outcome),
        None => Ok(()),
    }
}

pub(crate) fn count(
    ctx: &ExecCtx,
    request: u64,
    attempt: u32,
    outcome: Option<Outcome>,
    readings: Vec<(String, u64)>,
) -> Result<(), Error> {
    let (row, assignment) = current(ctx, request, attempt)?;
    via_of(ctx, &row)?;
    if !live(&assignment) {
        return Err(wrong_state(format!(
            "request {request} at {attempt} has ended"
        )));
    }
    counted(ctx, &row, attempt, Counter::Chain, readings, None)?;
    match outcome {
        Some(outcome) => finish(ctx, &row, assignment, outcome),
        None => Ok(()),
    }
}

pub(crate) fn missed(ctx: &ExecCtx, request: u64, attempt: u32) -> Result<(), Error> {
    let (row, mut assignment) = current(ctx, request, attempt)?;
    via_of(ctx, &row)?;
    if !live(&assignment) {
        return Err(wrong_state(format!(
            "request {request} at {attempt} has ended"
        )));
    }
    let stopping = assignment.state == AssignState::Stopping;
    assignment.state = AssignState::Missed;
    ASSIGNED.put(ctx, &(request, attempt), &assignment);
    release(ctx, &assignment, row.spec.needs.units)?;
    miss_offer(ctx, &assignment.offer)?;
    record(ctx, &assignment.offer, |counts| counts.missed += 1)?;
    let retry = row.spec.on_loss == crate::OnLoss::Retry;
    again(ctx, &row, stopping, retry)?;
    capacity_changed(ctx, &row.spec.kind)
}

pub(crate) fn confirm(
    ctx: &ExecCtx,
    request: u64,
    attempt: u32,
    readings: Vec<(String, u64)>,
) -> Result<(), Error> {
    let row = request_row(ctx, request)?;
    if ctx.sender_account()? != row.by {
        return Err(unauthorized(format!(
            "request {request} isn't yours to confirm"
        )));
    }
    assignment_at(ctx, request, attempt)?;
    counted(ctx, &row, attempt, Counter::Requester, readings, None)
}

pub(crate) fn cancel(ctx: &ExecCtx, request: u64) -> Result<(), Error> {
    let row = request_row(ctx, request)?;
    REQUESTS.put(ctx, &request, &row);
    let mut state = state_row(ctx, request)?;
    match state.state {
        RequestState::Queued => {
            QUEUE.remove(
                ctx,
                &(
                    row.spec.kind.clone(),
                    state.class.clone(),
                    state.group.clone(),
                    request,
                ),
            );
            state.state = RequestState::Cancelled;
            STATES.put(ctx, &request, &state);
            Ok(())
        }
        RequestState::Placed => {
            let mut assignment = assignment_at(ctx, request, state.attempt)?;
            if !live(&assignment) {
                return Err(wrong_state(format!("request {request} has ended")));
            }
            assignment.state = AssignState::Stopping;
            ASSIGNED.put(ctx, &(request, state.attempt), &assignment);
            Ok(())
        }
        RequestState::Lost | RequestState::Ended | RequestState::Cancelled => {
            Err(wrong_state(format!("request {request} has ended")))
        }
    }
}

pub(crate) fn lapse(ctx: &ExecCtx, offer: OfferId) -> Result<(), Error> {
    let at = key(&offer);
    let Some(mut row) = OFFERS.get(ctx, &at)? else {
        return Ok(());
    };
    let height = now(ctx);
    if CHECKS.get(ctx, &at)?.is_some_and(|check| check <= height) {
        CHECKS.remove(ctx, &at);
    }
    let kind = kind_row(ctx, &row.offer.kind)?;
    let lapse = lapse_of(&kind.spec);
    let last = BEATS.get(ctx, &at)?.map_or(row.since, |beat| beat.height);
    let silent = height >= last.saturating_add(lapse);
    let admitted = ADMITTED.has(ctx, &(kind.name.clone(), offer.provider));
    if row.state == OfferState::Gone {
        return Ok(());
    }
    let alive = !silent && admitted;
    if alive {
        return book_check(ctx, &at, lapse);
    }
    if silent {
        row.state = OfferState::Lapsed;
    }
    settle_idle(ctx, &mut row);
    OFFERS.put(ctx, &at, &row);
    let held = HOLDS.scan(ctx, HOLDS.prefix_of(&at))?;
    for (_, _, request) in &held {
        let row = request_row(ctx, *request)?;
        let state = state_row(ctx, *request)?;
        let mut assignment = assignment_at(ctx, *request, state.attempt)?;
        let stopping = assignment.state == AssignState::Stopping;
        assignment.state = AssignState::Lost;
        ASSIGNED.put(ctx, &(*request, state.attempt), &assignment);
        release(ctx, &assignment, row.spec.needs.units)?;
        record(ctx, &offer, |counts| counts.lost += 1)?;
        let retry = row.spec.on_loss == crate::OnLoss::Retry;
        again(ctx, &row, stopping, retry)?;
    }
    if !held.is_empty() {
        capacity_changed(ctx, &kind.name)?;
    }
    match silent {
        true => Ok(()),
        false => book_check(ctx, &at, lapse),
    }
}

pub(crate) fn unclaimed(ctx: &ExecCtx, request: u64, attempt: u32) -> Result<(), Error> {
    let Some(mut assignment) = ASSIGNED.get(ctx, &(request, attempt))? else {
        return Ok(());
    };
    let waiting = matches!(
        assignment.state,
        AssignState::Assigned | AssignState::Stopping
    );
    if !waiting {
        return Ok(());
    }
    let row = request_row(ctx, request)?;
    let kind = kind_row(ctx, &row.spec.kind)?;
    if now(ctx) < assignment.height.saturating_add(lapse_of(&kind.spec)) {
        return Ok(());
    }
    let stopping = assignment.state == AssignState::Stopping;
    assignment.state = AssignState::Missed;
    ASSIGNED.put(ctx, &(request, attempt), &assignment);
    release(ctx, &assignment, row.spec.needs.units)?;
    miss_offer(ctx, &assignment.offer)?;
    record(ctx, &assignment.offer, |counts| counts.missed += 1)?;
    again(ctx, &row, stopping, true)?;
    capacity_changed(ctx, &kind.name)
}

fn tell(ctx: &ExecCtx, request: &RequestRow, event: &Event) -> Result<(), Error> {
    match &request.spec.notify {
        None => Ok(()),
        Some(Notify::Program { program, deliver }) => {
            let mut payload = deliver.clone();
            payload.extend(abi::encode(event));
            ctx.emit_raw(program.clone(), payload, Wanted::Wanted);
            Ok(())
        }
        Some(Notify::Chat { channel }) => {
            let (attempt, word, status, detail) = match event {
                Event::Assigned(placement) => (
                    placement.attempt,
                    "assigned",
                    chat::message::Status::Running,
                    format!(
                        "placed on {} of {}",
                        placement.offer.name, placement.offer.provider
                    ),
                ),
                Event::Lost { attempt, .. } => (
                    *attempt,
                    "lost",
                    chat::message::Status::Failed,
                    "lost, and parked by its on_loss".to_owned(),
                ),
                Event::Ended {
                    attempt, outcome, ..
                } => {
                    let status = match outcome {
                        Outcome::Passed => chat::message::Status::Passed,
                        Outcome::Cancelled => chat::message::Status::Note,
                        Outcome::Failed | Outcome::Errored => chat::message::Status::Failed,
                    };
                    (*attempt, "ended", status, format!("ended {outcome:?}"))
                }
            };
            let post = chat::Op::Notify {
                channel_id: channel.clone(),
                message_id: format!("resources-{}-{attempt}-{word}", request.id),
                thread: None,
                notice: chat::Notice {
                    kind: "resource".into(),
                    group: format!("request {}", request.id),
                    subject: format!("request {} of {}", request.id, request.spec.kind),
                    link: None,
                    status,
                    detail,
                    concerns: vec![request.by],
                },
            };
            ctx.emit(chat::PROGRAM, &post, Wanted::Wanted);
            Ok(())
        }
    }
}

pub(crate) fn replied(_: &ExecCtx, _: &MessageId, _: &Answered) -> Result<(), Error> {
    Ok(())
}

pub(crate) fn offer_view(ctx: &QueryCtx, offer: &OfferId) -> Result<Option<OfferView>, Error> {
    let Some(row) = OFFERS.get(ctx, &key(offer))? else {
        return Ok(None);
    };
    let beat = BEATS.get(ctx, &key(offer))?;
    let admitted = ADMITTED.has(ctx, &(row.offer.kind.clone(), row.provider));
    Ok(Some(OfferView {
        offer: row,
        beat,
        admitted,
    }))
}

pub(crate) fn offers(
    ctx: &QueryCtx,
    kind: String,
    page: &PageRequest,
) -> Result<PageResponse<OfferView>, Error> {
    KIND_OFFERS
        .range_of(ctx, &kind, page)?
        .try_map(|(_, provider, name)| {
            offer_view(ctx, &OfferId { provider, name })?
                .ok_or_else(|| not_found("an indexed offer is gone"))
        })
}

pub(crate) fn request_view(ctx: &QueryCtx, id: u64) -> Result<Option<RequestView>, Error> {
    let Some(request) = REQUESTS.get(ctx, &id)? else {
        return Ok(None);
    };
    let state = state_row(ctx, id)?;
    Ok(Some(RequestView {
        request,
        attempt: state.attempt,
        state: state.state,
    }))
}

pub(crate) fn queued(
    ctx: &QueryCtx,
    kind: String,
    page: &PageRequest,
) -> Result<PageResponse<RequestView>, Error> {
    QUEUE.range_of(ctx, &kind, page)?.try_map(|(_, _, _, id)| {
        request_view(ctx, id)?.ok_or_else(|| not_found(format!("queued request {id} is gone")))
    })
}

pub(crate) fn holds(
    ctx: &QueryCtx,
    offer: OfferId,
    page: &PageRequest,
) -> Result<PageResponse<Assignment>, Error> {
    HOLDS
        .range_of(ctx, &key(&offer), page)?
        .try_map(|(_, _, request)| {
            let state = state_row(ctx, request)?;
            assignment_at(ctx, request, state.attempt)
        })
}

pub(crate) fn ledger(
    ctx: &QueryCtx,
    kind: String,
    group: String,
    page: &PageRequest,
) -> Result<PageResponse<LedgerRow>, Error> {
    Ok(LEDGER
        .range_of(ctx, &(kind, group), page)?
        .map(|((_, _, meter, window_start), used)| LedgerRow {
            meter,
            window_start,
            used,
        }))
}

pub(crate) fn rank(
    ctx: &ExecCtx,
    kind: String,
    provider: AccountNumber,
    tier: u32,
) -> Result<(), Error> {
    administer(ctx, &kind)?;
    match tier {
        0 => RANKS.remove(ctx, &(kind, provider)),
        tier => RANKS.put(ctx, &(kind, provider), &tier),
    }
    Ok(())
}

pub(crate) fn admission(
    ctx: &QueryCtx,
    kind: String,
    provider: AccountNumber,
) -> Result<Option<Admission>, Error> {
    let at = (kind, provider);
    let Some(admitted) = ADMITTED.get(ctx, &at)? else {
        return Ok(None);
    };
    let tier = RANKS.get(ctx, &at)?.unwrap_or(0);
    Ok(Some(Admission { admitted, tier }))
}

pub(crate) fn in_force(ctx: &QueryCtx, kind: &str) -> Result<Option<InForce>, Error> {
    let Some(row) = KINDS.get(ctx, &kind.to_owned())? else {
        return Ok(None);
    };
    let (policy, since) = effective(ctx, kind, now(ctx))?;
    Ok(Some(InForce {
        kind: row,
        policy,
        since,
    }))
}

pub(crate) fn why(ctx: &QueryCtx, id: u64) -> Result<Option<WhyView>, Error> {
    let Some(request) = REQUESTS.get(ctx, &id)? else {
        return Ok(None);
    };
    let state = state_row(ctx, id)?;
    let assignment = match state.attempt {
        0 => None,
        attempt => ASSIGNED.get(ctx, &(id, attempt))?,
    };
    let waiting = match state.state {
        RequestState::Queued => waiting(ctx, &request, &state)?,
        _ => Vec::new(),
    };
    Ok(Some(WhyView {
        state: state.state,
        assignment,
        waiting,
    }))
}

fn waiting(ctx: &QueryCtx, request: &RequestRow, state: &StateRow) -> Result<Vec<Waiting>, Error> {
    let kind = kind_row(ctx, &request.spec.kind)?;
    let (policy, since) = effective(ctx, &kind.name, now(ctx))?;
    let mut waiting = Vec::new();
    let class = classes(&policy)
        .into_iter()
        .find(|class| class.name == state.class);
    let refuses = match class.and_then(|class| class.may_use) {
        Some(may_use) => !admits(ctx, &may_use, request.by)?,
        None => false,
    };
    if refuses {
        waiting.push(Waiting::ClassRefuses);
    }
    for ahead in classes(&policy)
        .iter()
        .take_while(|class| class.name != state.class)
    {
        let queued = QUEUE.prefix_of(&(kind.name.clone(), ahead.name.clone()));
        if !ctx.scan(queued.limit(1)).is_empty() {
            waiting.push(Waiting::ClassAhead {
                class: ahead.name.clone(),
            });
            break;
        }
    }
    for (class, group) in order(ctx, &kind.name, &policy, since) {
        if class != state.class {
            continue;
        }
        if group == state.group {
            break;
        }
        let queued = QUEUE.prefix_of(&(kind.name.clone(), class, group.clone()));
        if !ctx.scan(queued.limit(1)).is_empty() {
            waiting.push(Waiting::GroupAhead { group });
            break;
        }
    }
    if let Err(held) = choose(ctx, request, state, &kind)? {
        waiting.extend(held);
    }
    Ok(waiting)
}

pub(crate) fn passed_over(
    ctx: &QueryCtx,
    id: u64,
    page: &PageRequest,
) -> Result<PageResponse<PassedOverRow>, Error> {
    let request = request_row(ctx, id)?;
    let state = state_row(ctx, id)?;
    let kind = kind_row(ctx, &request.spec.kind)?;
    let notarized = notarized(ctx, &request, &state)?;
    let notary_missing = notarized && kind.spec.notary.is_some();
    let page = KIND_OFFERS.range_of(ctx, &kind.name, page)?;
    let mut items = Vec::new();
    for (_, provider, name) in &page.items {
        let offer = OfferId {
            provider: *provider,
            name: name.clone(),
        };
        let Some(row) = OFFERS.get(ctx, &key(&offer))? else {
            continue;
        };
        let reason = match (passed(ctx, &request, notarized, &row)?, notary_missing) {
            (Some(reason), _) => Some(reason),
            (None, true) => notary_for(ctx, &kind, *provider)?
                .is_none()
                .then_some(Passed::NoNotary),
            (None, false) => None,
        };
        if let Some(passed) = reason {
            items.push(PassedOverRow { offer, passed });
        }
    }
    Ok(PageResponse {
        height: page.height,
        items,
        next: page.next,
    })
}
