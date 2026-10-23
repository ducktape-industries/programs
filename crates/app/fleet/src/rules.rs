//! What each op checks and writes: an Ask queues a run and places it, a ready machine with a free
//! slot takes the queued runs it covers, and a run goes from assigned to running to how its
//! machine says it ended; a machine silent for the egress wait loses its runs.
use ::program::{
    Ctx, Error, ExecCtx, MessageId, Outcome, QueryCtx, Reply as Wanted, code, invalid, not_found,
    unauthorized, unexpected_reply, wrong_state,
};
use abi::role::egress::{DEFAULT_WAIT, Delivery, Op as Egress};
use abi::role::identity;
use resources::ordering::{self, UNITS, WalkRow};
use resources::{LedgerRow, Policy, Waiting};

use crate::state::{
    BEATS, CALLS, CLAIMS, Claim, DECLINED, FILED, HOLDS, IDLE, LAST, LEDGER, MACHINE_KINDS,
    MACHINES, PIECES, QUEUE, RUNS, TELLING, TOLD, WALK_AT, WALKS, WORKLOADS,
};
use crate::{
    AccountNumber, Affinity, BLOCKS, Event, Filed, Kind, MachineId, MachineRow, MachineView,
    Notify, OnLoss, Op, Out, PageRequest, PageResponse, Piece, PieceRow, ProgramId, Request,
    RunEnd, RunRow, STAMPED, Serves, Spec, State, Workload,
};

const UNTYPED_WALK: u64 = 1;

struct Ruling {
    policy: Policy,
    since: u64,
    walk: u64,
}

type Key = (AccountNumber, String);

fn key(machine: &MachineId) -> Key {
    (machine.account, machine.name.clone())
}

fn mine(ctx: &ExecCtx, name: String) -> Result<MachineId, Error> {
    Ok(MachineId {
        account: ctx.sender_account()?,
        name,
    })
}

fn height(ctx: &QueryCtx) -> u64 {
    ctx.env().height
}

pub(crate) fn ask(ctx: &ExecCtx, spec: Spec) -> Result<(), Error> {
    ask_in(ctx, spec, None, String::new(), None)
}

pub(crate) fn ask_in(
    ctx: &ExecCtx,
    spec: Spec,
    kind: Option<String>,
    class: String,
    limit: Option<u64>,
) -> Result<(), Error> {
    let by = ctx.sender_account()?;
    let kind = kind.filter(|name| !name.is_empty());
    let group = match class_of(ctx, kind.as_deref(), &class, by)? {
        Some(ruling) => ordering::group_of(ctx, &ruling.policy, by)?,
        None => String::new(),
    };
    let id = LAST
        .get(ctx)?
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| Error::new(code::EXHAUSTED, "run ids are exhausted"))?;
    LAST.put(ctx, &id);
    let workload = Workload {
        id,
        by,
        height: height(ctx),
        spec,
    };
    WORKLOADS.put(ctx, &id, &workload);
    let run = RunRow {
        id,
        attempt: 0,
        state: State::Queued,
        machine: None,
        assigned: None,
        started: None,
        finished: None,
        summary: String::new(),
        out: None,
    };
    RUNS.put(ctx, &id, &run);
    CLAIMS.put(ctx, &id, &Claim::default());
    let filed = Filed {
        kind,
        class,
        group,
        limit,
    };
    FILED.put(ctx, &id, &filed);
    enqueue(ctx, id)?;
    ctx.set_return_data(abi::encode(&id));
    Ok(())
}

fn class_of(
    ctx: &QueryCtx,
    kind: Option<&str>,
    class: &str,
    account: AccountNumber,
) -> Result<Option<Ruling>, Error> {
    let Some(kind) = kind else {
        return match class.is_empty() {
            true => Ok(None),
            false => Err(invalid(format!(
                "class {class:?} needs a kind: a run with no kind has no class"
            ))),
        };
    };
    let ruling = ruling(ctx, kind)?
        .ok_or_else(|| not_found(format!("no kind {kind} in resources: register it there")))?;
    let named = ordering::classes(&ruling.policy)
        .into_iter()
        .find(|named| named.name == class)
        .ok_or_else(|| invalid(format!("kind {kind} has no class {class:?}")))?;
    let refused = match &named.may_use {
        Some(may_use) => !ordering::admits(ctx, may_use, account)?,
        None => false,
    };
    if refused {
        return Err(unauthorized(format!(
            "class {class:?} of kind {kind} doesn't admit account {account}"
        )));
    }
    Ok(Some(ruling))
}

fn ruling(ctx: &QueryCtx, kind: &str) -> Result<Option<Ruling>, Error> {
    if kind.is_empty() {
        return Ok(Some(Ruling {
            policy: Policy::default(),
            since: 0,
            walk: UNTYPED_WALK,
        }));
    }
    let controller = ProgramId::from(resources::PROGRAM);
    let asked = resources::Query::InForce {
        kind: kind.to_owned(),
    };
    match ctx.query::<resources::Query, resources::Reply>(controller.clone(), &asked)? {
        resources::Reply::InForce(found) => Ok(found.map(|in_force| Ruling {
            policy: in_force.policy,
            since: in_force.since,
            walk: in_force.kind.spec.walk,
        })),
        other => Err(unexpected_reply(&controller, "InForce", &other)),
    }
}

fn tier_of(ctx: &QueryCtx, kind: &str, account: AccountNumber) -> Result<Option<u32>, Error> {
    if kind.is_empty() {
        return Ok(Some(0));
    }
    let controller = ProgramId::from(resources::PROGRAM);
    let asked = resources::Query::Admission {
        kind: kind.to_owned(),
        provider: account,
    };
    match ctx.query::<resources::Query, resources::Reply>(controller.clone(), &asked)? {
        resources::Reply::Admission(found) => Ok(found.map(|admission| admission.tier)),
        other => Err(unexpected_reply(&controller, "Admission", &other)),
    }
}

fn filed(ctx: &QueryCtx, id: u64) -> Result<Filed, Error> {
    Ok(FILED.get(ctx, &id)?.unwrap_or_default())
}

fn queue_key(id: u64, filed: &Filed) -> (String, String, String, u64) {
    let kind = filed.kind.clone().unwrap_or_default();
    (kind, filed.class.clone(), filed.group.clone(), id)
}

fn enqueue(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    let filed = filed(ctx, id)?;
    let kind = filed.kind.clone().unwrap_or_default();
    QUEUE.insert(ctx, &queue_key(id, &filed));
    if let Some(mut walk) = WALKS.get(ctx, &kind)? {
        walk.behind = true;
        WALKS.put(ctx, &kind, &walk);
    }
    book_step(ctx, &kind, height(ctx) + 1)
}

fn dequeue(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    let filed = filed(ctx, id)?;
    QUEUE.remove(ctx, &queue_key(id, &filed));
    Ok(())
}

fn book_step(ctx: &ExecCtx, kind: &str, at: u64) -> Result<(), Error> {
    let booked = (kind.to_owned(), at);
    if WALK_AT.has(ctx, &booked) {
        return Ok(());
    }
    WALK_AT.insert(ctx, &booked);
    schedule(ctx, at, &Op::Walk { kind: kind.into() })
}

fn kind_of(ctx: &QueryCtx, machine: &MachineId) -> Result<String, Error> {
    Ok(MACHINE_KINDS.get(ctx, &key(machine))?.unwrap_or_default())
}

fn capacity_changed(ctx: &ExecCtx, machine: &MachineId) -> Result<(), Error> {
    let kind = kind_of(ctx, machine)?;
    book_step(ctx, &kind, height(ctx) + 1)
}

fn charge(ctx: &ExecCtx, id: u64, meter: &str, amount: u64) -> Result<(), Error> {
    let filed = filed(ctx, id)?;
    let kind = filed.kind.unwrap_or_default();
    let Some(ruling) = ruling(ctx, &kind)? else {
        return Ok(());
    };
    let in_force = (&ruling.policy, ruling.since);
    ordering::charge(ctx, &LEDGER, &kind, &filed.group, meter, amount, in_force)
}

fn ran(ctx: &ExecCtx, run: &RunRow) -> Result<(), Error> {
    let Some(started) = run.started else {
        return Ok(());
    };
    let blocks = height(ctx).saturating_sub(started);
    charge(ctx, run.id, BLOCKS, blocks)
}

pub(crate) fn place_queued(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    if load(ctx, id)?.state != State::Queued {
        return Ok(());
    }
    let filed = filed(ctx, id)?;
    QUEUE.insert(ctx, &queue_key(id, &filed));
    walk(ctx, filed.kind.unwrap_or_default())
}

pub(crate) fn walk(ctx: &ExecCtx, kind: String) -> Result<(), Error> {
    let now = height(ctx);
    WALK_AT.remove(ctx, &(kind.clone(), now));
    let Some(ruling) = ruling(ctx, &kind)? else {
        return Ok(());
    };
    let mut walk = match WALKS.get(ctx, &kind)? {
        Some(walk) => walk,
        None => WalkRow {
            order: ordering::order(ctx, &LEDGER, &kind, &ruling.policy, ruling.since),
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
        let Some((class, group, id)) = ordering::next_queued(ctx, &QUEUE, &kind, &mut walk)? else {
            WALKS.remove(ctx, &kind);
            if walk.behind {
                book_step(ctx, &kind, now + 1)?;
            }
            return Ok(());
        };
        examined += 1;
        walk.after = Some((class.clone(), group.clone(), id));
        match ordering::over_quota(ctx, &LEDGER, &kind, &group, &ruling.policy)? {
            Some(Waiting::OverQuota { until, .. }) => {
                book_step(ctx, &kind, until)?;
                walk.after = Some((class, group, u64::MAX));
            }
            _ => {
                if let Some(row) = choose(ctx, &kind, id)? {
                    assign(ctx, id, row)?;
                }
            }
        }
        if examined >= ruling.walk {
            if any_free(ctx, &kind) {
                WALKS.put(ctx, &kind, &walk);
                book_step(ctx, &kind, now + 1)?;
            } else {
                WALKS.remove(ctx, &kind);
            }
            return Ok(());
        }
    }
}

fn any_free(ctx: &QueryCtx, kind: &str) -> bool {
    !ctx.scan(IDLE.prefix_of(&kind.to_owned()).limit(1))
        .is_empty()
}

fn choose(ctx: &ExecCtx, kind: &str, id: u64) -> Result<Option<MachineRow>, Error> {
    let workload = workload(ctx, id)?;
    let mut best: Option<(u32, MachineRow)> = None;
    for (_, since, account, name) in IDLE.scan(ctx, IDLE.prefix_of(&kind.to_owned()))? {
        let machine = MachineId { account, name };
        let row = machine_row(ctx, &machine)?;
        let current = row.idle_since == Some(since);
        if !current {
            IDLE.remove(ctx, &(kind.to_owned(), since, account, machine.name));
            continue;
        }
        if !takes(ctx, &row, &workload)? {
            continue;
        }
        let Some(tier) = tier_of(ctx, kind, account)? else {
            continue;
        };
        let lower = best.as_ref().is_none_or(|(held, _)| tier < *held);
        if lower {
            best = Some((tier, row));
        }
        if tier == 0 {
            break;
        }
    }
    Ok(best.map(|(_, row)| row))
}

pub(crate) fn promote(ctx: &ExecCtx, id: u64, class: String) -> Result<(), Error> {
    let run = load(ctx, id)?;
    if run.state.ended() {
        return Err(wrong_state(format!(
            "run {id} already ended {}: a class rules only a run still to be placed or run again",
            run.state.word()
        )));
    }
    let sender = ctx.sender_account()?;
    let mut filed = filed(ctx, id)?;
    class_of(ctx, filed.kind.as_deref(), &class, sender)?;
    let claim = CLAIMS.get(ctx, &id)?.unwrap_or_default();
    CLAIMS.put(ctx, &id, &claim);
    let queued = QUEUE.has(ctx, &queue_key(id, &filed));
    if queued {
        QUEUE.remove(ctx, &queue_key(id, &filed));
    }
    filed.class = class;
    FILED.put(ctx, &id, &filed);
    match queued {
        true => enqueue(ctx, id),
        false => Ok(()),
    }
}

pub(crate) fn expire(ctx: &ExecCtx, id: u64, attempt: u32) -> Result<(), Error> {
    let mut run = load(ctx, id)?;
    let Some(limit) = filed(ctx, id)?.limit else {
        return Ok(());
    };
    let due = run
        .started
        .is_some_and(|at| at.saturating_add(limit) <= height(ctx));
    let current = run.state == State::Running && run.attempt == attempt;
    let expired = current && due;
    if !expired {
        return Ok(());
    }
    run.state = State::Stopping;
    run.summary = format!("past its limit of {limit} blocks");
    RUNS.put(ctx, &id, &run);
    Ok(())
}

pub(crate) fn ready(
    ctx: &ExecCtx,
    name: String,
    labels: Vec<String>,
    slots: u64,
    serves: Serves,
) -> Result<(), Error> {
    let kind = MACHINE_KINDS.get(ctx, &key(&mine(ctx, name.clone())?))?;
    ready_in(ctx, name, labels, slots, serves, kind)
}

pub(crate) fn ready_in(
    ctx: &ExecCtx,
    name: String,
    labels: Vec<String>,
    slots: u64,
    serves: Serves,
    kind: Option<String>,
) -> Result<(), Error> {
    let kind = kind.unwrap_or_default();
    if ruling(ctx, &kind)?.is_none() {
        return Err(not_found(format!(
            "no kind {kind} in resources: register it there"
        )));
    }
    let id = mine(ctx, name)?;
    let mut row = MACHINES.get(ctx, &key(&id))?.unwrap_or(MachineRow {
        id: id.clone(),
        labels: Vec::new(),
        slots: 0,
        serves: Serves::Anyone,
        busy: 0,
        idle_since: None,
        gone: false,
        check_at: None,
    });
    unindex(ctx, &row)?;
    match kind.is_empty() {
        true => MACHINE_KINDS.remove(ctx, &key(&id)),
        false => MACHINE_KINDS.put(ctx, &key(&id), &kind),
    }
    row.labels = labels;
    row.slots = slots;
    row.serves = serves;
    row.gone = false;
    settle_idle(ctx, &mut row)?;
    MACHINES.put(ctx, &key(&id), &row);
    capacity_changed(ctx, &id)
}

pub(crate) fn claim(ctx: &ExecCtx, id: u64, attempt: u32) -> Result<(), Error> {
    let sender = ctx.sender_account()?;
    let mut run = load(ctx, id)?;
    let machine = match &run.machine {
        Some(machine)
            if run.state == State::Assigned
                && run.attempt == attempt
                && machine.account == sender =>
        {
            machine.clone()
        }
        _ => {
            return Err(wrong_state(format!(
                "run {id} is not assigned to a machine of account {sender} at attempt {attempt}: read the current assignment and report from its assigned machine"
            )));
        }
    };
    let claim = Claim {
        machine: Some(machine.clone()),
        attempt,
        cancelled: false,
    };
    CLAIMS.put(ctx, &id, &claim);
    run.state = State::Running;
    run.started = Some(height(ctx));
    RUNS.put(ctx, &id, &run);
    BEATS.put(ctx, &key(&machine), &height(ctx));
    if let Some(limit) = filed(ctx, id)?.limit {
        let at = height(ctx).saturating_add(limit).max(height(ctx) + 1);
        schedule(ctx, at, &Op::Expire { run: id, attempt })?;
    }
    book_check(ctx, &machine)
}

pub(crate) fn beat(ctx: &ExecCtx, name: String) -> Result<(), Error> {
    let id = mine(ctx, name)?;
    let mut row = machine_row(ctx, &id)?;
    BEATS.put(ctx, &key(&id), &height(ctx));
    let back = row.gone;
    let unindexed = index_missing(ctx, &row)?;
    row.gone = false;
    let settle = back || unindexed;
    if settle {
        settle_idle(ctx, &mut row)?;
        MACHINES.put(ctx, &key(&id), &row);
        capacity_changed(ctx, &id)?;
    }
    book_check(ctx, &id)
}

pub(crate) fn report(
    ctx: &ExecCtx,
    id: u64,
    attempt: u32,
    state: State,
    summary: String,
    out: Option<Out>,
) -> Result<(), Error> {
    let sender = ctx.sender_account()?;
    if !state.ended() {
        return Err(invalid(
            "a report says how a run ended: passed, failed, errored or cancelled",
        ));
    }
    let mut run = load(ctx, id)?;
    let reportable = held_now(&run) || run.state == State::Lost;
    let machine = match &run.machine {
        Some(machine) if reportable && run.attempt == attempt && machine.account == sender => {
            machine.clone()
        }
        _ => {
            return Err(wrong_state(format!(
                "run {id} is {} and not held by a machine of account {sender} at attempt {attempt}: read the current assignment and report from its assigned machine",
                run.state.word()
            )));
        }
    };
    let stopped = run.state == State::Stopping;
    if held_now(&run) {
        ran(ctx, &run)?;
        release(ctx, id, &machine)?;
        BEATS.put(ctx, &key(&machine), &height(ctx));
    }
    run.finished = Some(height(ctx));
    run.summary = summary;
    run.out = out;
    let workload = workload(ctx, id)?;
    if workload.spec.kind == Kind::Service && !stopped && state != State::Cancelled {
        requeued(ctx, &mut run)?;
    } else {
        run.state = state;
        RUNS.put(ctx, &id, &run);
        tell(ctx, &workload, &run)?;
    }
    capacity_changed(ctx, &machine)
}

pub(crate) fn decline(ctx: &ExecCtx, id: u64, attempt: u32, why: String) -> Result<(), Error> {
    let sender = ctx.sender_account()?;
    let mut run = load(ctx, id)?;
    let machine = match &run.machine {
        Some(machine)
            if run.state == State::Assigned
                && run.attempt == attempt
                && machine.account == sender =>
        {
            machine.clone()
        }
        _ => {
            return Err(wrong_state(format!(
                "run {id} is not assigned to a machine of account {sender} at attempt {attempt}: read the current assignment and report from its assigned machine"
            )));
        }
    };
    DECLINED.insert(ctx, &(id, machine.account, machine.name.clone()));
    release(ctx, id, &machine)?;
    run.summary = format!("{} declined it: {why}", machine.name);
    requeued(ctx, &mut run)?;
    capacity_changed(ctx, &machine)
}

pub(crate) fn cancel(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    let mut run = load(ctx, id)?;
    if run.state.ended() {
        return Err(wrong_state(format!(
            "run {id} already ended {}: ask for a rerun to start another attempt",
            run.state.word()
        )));
    }
    // the claim row is the asker's: its policy decides whether this sender may cancel
    let mut claim = CLAIMS.get(ctx, &id)?.unwrap_or_default();
    claim.cancelled = true;
    CLAIMS.put(ctx, &id, &claim);
    match (run.state, run.machine.clone()) {
        (State::Running, Some(_)) => {
            run.state = State::Stopping;
            RUNS.put(ctx, &id, &run);
            Ok(())
        }
        (State::Stopping, _) => Ok(()),
        (state, machine) => {
            dequeue(ctx, id)?;
            let assigned = state == State::Assigned;
            run.state = State::Cancelled;
            run.finished = Some(height(ctx));
            RUNS.put(ctx, &id, &run);
            tell(ctx, &workload(ctx, id)?, &run)?;
            match machine.filter(|_| assigned) {
                Some(machine) => {
                    release(ctx, id, &machine)?;
                    capacity_changed(ctx, &machine)
                }
                None => Ok(()),
            }
        }
    }
}

pub(crate) fn requeue(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    let mut run = load(ctx, id)?;
    if !(run.state.ended() || run.state == State::Lost) {
        return Err(wrong_state(format!(
            "run {id} is {}: only a lost or ended run is queued again",
            run.state.word()
        )));
    }
    // the claim row is the asker's: its policy decides whether this sender may requeue
    let claim = Claim {
        machine: None,
        attempt: run.attempt,
        cancelled: false,
    };
    CLAIMS.put(ctx, &id, &claim);
    requeued(ctx, &mut run)
}

// A machine silent for the egress wait loses every run it holds; one that beat since books the
// next check from its beat. Open: it acts only on what the chain shows.
pub(crate) fn lapse(ctx: &ExecCtx, machine: MachineId) -> Result<(), Error> {
    let mut row = machine_row(ctx, &machine)?;
    let now = height(ctx);
    let running: Vec<RunRow> = held(ctx, &machine)?
        .into_iter()
        .filter(|run| matches!(run.state, State::Running | State::Stopping))
        .collect();
    if running.is_empty() {
        let checked = row.check_at.is_some_and(|at| at <= now);
        if checked {
            row.check_at = None;
        }
        let unindexed = index_missing(ctx, &row)?;
        if unindexed {
            settle_idle(ctx, &mut row)?;
        }
        let changed = checked || unindexed;
        if changed {
            MACHINES.put(ctx, &key(&machine), &row);
        }
        if unindexed {
            capacity_changed(ctx, &machine)?;
        }
        return Ok(());
    }
    let beat = BEATS.get(ctx, &key(&machine))?.unwrap_or(0);
    if beat.saturating_add(DEFAULT_WAIT) > now {
        return book_check(ctx, &machine);
    }
    unindex(ctx, &row)?;
    row.gone = true;
    row.idle_since = None;
    row.check_at = None;
    row.busy = row.busy.saturating_sub(running.len() as u64);
    MACHINES.put(ctx, &key(&machine), &row);
    let why = format!("{} went silent after block {beat}", machine.name);
    for run in running {
        lose(ctx, run, &machine, &why)?;
    }
    Ok(())
}

// An assignment its machine did not claim for the egress wait: the machine is taken as gone and
// the run goes elsewhere. Open: it acts only on what the chain shows.
pub(crate) fn unclaimed(ctx: &ExecCtx, id: u64, attempt: u32) -> Result<(), Error> {
    let mut run = load(ctx, id)?;
    let due = run
        .assigned
        .is_some_and(|at| at.saturating_add(DEFAULT_WAIT) <= height(ctx));
    let Some(machine) = run.machine.clone() else {
        return Ok(());
    };
    if run.state != State::Assigned || run.attempt != attempt || !due {
        return Ok(());
    }
    release(ctx, id, &machine)?;
    let mut row = machine_row(ctx, &machine)?;
    unindex(ctx, &row)?;
    row.gone = true;
    row.idle_since = None;
    MACHINES.put(ctx, &key(&machine), &row);
    run.summary = format!("{} did not claim it", machine.name);
    requeued(ctx, &mut run)
}

pub(crate) async fn relay(
    ctx: &Ctx,
    id: u64,
    attempt: u32,
    call: u64,
    request: Request,
    stream: bool,
    wait: Option<u64>,
) -> Result<(), Error> {
    let sender = ctx.sender_account()?;
    let run = load(ctx, id)?;
    let holder = run.machine.as_ref().map(|machine| machine.account);
    if holder != Some(sender) {
        return Err(unauthorized(format!(
            "run {id} is not held by a machine of account {sender}: only the machine holding a run calls for it"
        )));
    }
    let running = held_now(&run) && run.attempt == attempt;
    if !running {
        return Err(wrong_state(format!(
            "run {id} is {} at attempt {}, not running at attempt {attempt}: call only for the attempt your machine runs",
            run.state.word(),
            run.attempt
        )));
    }
    let at = (id, attempt, call);
    if CALLS.has(ctx, &at) {
        return Err(wrong_state(format!(
            "call {call} of run {id} at attempt {attempt} was sent already: give each call of an attempt its own number"
        )));
    }
    let asker = workload(ctx, id)?.by;
    let request = stamped(request, id, attempt, asker, sender)?;
    CALLS.put(ctx, &at, &0);
    let delivery = if stream {
        Delivery::Events {
            deliver: heard_as(id, attempt, call),
        }
    } else {
        Delivery::Whole
    };
    let egress = ctx.env().role(abi::role::EGRESS)?.clone();
    let asked = Egress::Request {
        request,
        delivery,
        wait,
    };
    let piece = match ctx.call_raw(egress, abi::encode(&asked)).await {
        Ok(output) => Piece::Answer(output),
        Err(refused) => Piece::Failed(refused.message),
    };
    landed(ctx, at, piece)
}

fn stamped(
    request: Request,
    run: u64,
    attempt: u32,
    asker: AccountNumber,
    machine: AccountNumber,
) -> Result<Request, Error> {
    let Request::Http {
        service,
        mut head,
        body,
    } = request
    else {
        return Err(invalid(
            "fleet relays an Http request only: the service learns the run from the headers fleet sets on it",
        ));
    };
    let ours = |name: &str| STAMPED.iter().any(|stamp| name.eq_ignore_ascii_case(stamp));
    head.headers.retain(|(name, _)| !ours(name));
    let values = [
        run.to_string(),
        attempt.to_string(),
        asker.to_string(),
        machine.to_string(),
    ];
    let stamps = STAMPED.iter().zip(values);
    head.headers
        .extend(stamps.map(|(name, value)| (name.to_string(), value)));
    Ok(Request::Http {
        service,
        head,
        body,
    })
}

fn heard_as(run: u64, attempt: u32, call: u64) -> Vec<u8> {
    let empty = Event {
        id: 0,
        seq: 0,
        data: Vec::new(),
    };
    let tail = abi::encode(&empty).len();
    let heard = Op::Heard {
        run,
        attempt,
        call,
        event: empty,
    };
    let mut deliver = abi::encode(&heard);
    deliver.truncate(deliver.len() - tail);
    deliver
}

pub(crate) fn hear(
    ctx: &ExecCtx,
    run: u64,
    attempt: u32,
    call: u64,
    event: Event,
) -> Result<(), Error> {
    let egress = ctx.env().role(abi::role::EGRESS)?.clone();
    let from_egress = ctx
        .env()
        .sending_program()
        .is_ok_and(|program| program == egress);
    if !from_egress {
        return Err(unauthorized(
            "only the program bound to the egress role hands fleet a call's events",
        ));
    }
    let at = (run, attempt, call);
    if !CALLS.has(ctx, &at) {
        return Ok(());
    }
    let piece = Piece::Event {
        seq: event.seq,
        data: event.data,
    };
    landed(ctx, at, piece)
}

fn landed(ctx: &ExecCtx, at: (u64, u32, u64), piece: Piece) -> Result<(), Error> {
    let n = CALLS.get(ctx, &at)?.unwrap_or(0);
    let (run, attempt, call) = at;
    PIECES.put(ctx, &(run, attempt, call, n), &piece);
    let next = n
        .checked_add(1)
        .ok_or_else(|| Error::new(code::EXHAUSTED, "a call's pieces are exhausted"))?;
    CALLS.put(ctx, &at, &next);
    Ok(())
}

// Whether a run sits in its machine's holds as claimed: running, or being stopped.
fn held_now(run: &RunRow) -> bool {
    matches!(run.state, State::Running | State::Stopping)
}

// The run back in the queue as its next attempt, and placed.
fn requeued(ctx: &ExecCtx, run: &mut RunRow) -> Result<(), Error> {
    run.state = State::Queued;
    run.machine = None;
    run.assigned = None;
    run.started = None;
    RUNS.put(ctx, &run.id, run);
    enqueue(ctx, run.id)
}

// A run its silent machine held: a job that must never run twice is parked as lost, a run someone
// was stopping ends cancelled, and any other is queued again.
fn lose(ctx: &ExecCtx, mut run: RunRow, machine: &MachineId, why: &str) -> Result<(), Error> {
    HOLDS.remove(ctx, &(machine.account, machine.name.clone(), run.id));
    ran(ctx, &run)?;
    let workload = workload(ctx, run.id)?;
    run.summary = why.to_owned();
    let park = workload.spec.kind == Kind::Job && workload.spec.on_loss == OnLoss::Park;
    let stopping = run.state == State::Stopping;
    match (park, stopping) {
        (true, _) => run.state = State::Lost,
        (false, true) => {
            run.state = State::Cancelled;
            run.finished = Some(height(ctx));
        }
        (false, false) => return requeued(ctx, &mut run),
    }
    RUNS.put(ctx, &run.id, &run);
    tell(ctx, &workload, &run)
}

// The run leaves its machine's holds, and the machine's slot is free again.
fn release(ctx: &ExecCtx, id: u64, machine: &MachineId) -> Result<(), Error> {
    HOLDS.remove(ctx, &(machine.account, machine.name.clone(), id));
    let mut row = machine_row(ctx, machine)?;
    row.busy = row.busy.saturating_sub(1);
    settle_idle(ctx, &mut row)?;
    MACHINES.put(ctx, &key(machine), &row);
    Ok(())
}

// A machine with a free slot has been free since the first block it was; one with none is not.
fn settle_idle(ctx: &ExecCtx, row: &mut MachineRow) -> Result<(), Error> {
    let kind = kind_of(ctx, &row.id)?;
    unindex_as(ctx, row, &kind);
    let free = !row.gone && row.busy < row.slots;
    row.idle_since = match (free, row.idle_since) {
        (true, Some(since)) => Some(since),
        (true, None) => Some(height(ctx)),
        (false, _) => None,
    };
    if let Some(since) = row.idle_since {
        IDLE.insert(ctx, &(kind, since, row.id.account, row.id.name.clone()));
    }
    Ok(())
}

fn unindex(ctx: &ExecCtx, row: &MachineRow) -> Result<(), Error> {
    let kind = kind_of(ctx, &row.id)?;
    unindex_as(ctx, row, &kind);
    Ok(())
}

fn unindex_as(ctx: &ExecCtx, row: &MachineRow, kind: &str) {
    if let Some(since) = row.idle_since {
        let entry = (kind.to_owned(), since, row.id.account, row.id.name.clone());
        IDLE.remove(ctx, &entry);
    }
}

fn index_missing(ctx: &QueryCtx, row: &MachineRow) -> Result<bool, Error> {
    let Some(since) = row.idle_since else {
        return Ok(false);
    };
    let kind = kind_of(ctx, &row.id)?;
    let entry = (kind, since, row.id.account, row.id.name.clone());
    Ok(!IDLE.has(ctx, &entry))
}

fn takes(ctx: &QueryCtx, row: &MachineRow, workload: &Workload) -> Result<bool, Error> {
    if row.gone || row.busy >= row.slots {
        return Ok(false);
    }
    let covers = match &workload.spec.on {
        None => true,
        Some(Affinity::Machine(machine)) => *machine == row.id,
        Some(Affinity::Labels(labels)) => labels.iter().all(|label| row.labels.contains(label)),
    };
    let serves = match &row.serves {
        Serves::Anyone => true,
        Serves::Listed(accounts) => accounts.contains(&workload.by),
    };
    let declined = DECLINED.has(ctx, &(workload.id, row.id.account, row.id.name.clone()));
    if !covers || !serves || declined {
        return Ok(false);
    }
    admitted(ctx, workload.id, row.id.account)
}

// Whether the run's claim policy, its asker's, lets the machine's account claim it: what the
// guard holds the Claim to, read first so a run never goes to a machine it would refuse.
fn admitted(ctx: &QueryCtx, id: u64, account: AccountNumber) -> Result<bool, Error> {
    let Ok(guardian) = ctx.env().role(abi::role::OWNERSHIP).cloned() else {
        return Ok(true);
    };
    let at = CLAIMS.key(&id);
    let record = ownership::Query::Record {
        program: ctx.env().program.clone(),
        key: at.clone(),
    };
    let asked = ctx.query::<ownership::Query, ownership::Reply>(guardian.clone(), &record)?;
    let ownership::Reply::Record(Some(record)) = asked else {
        return Ok(true);
    };
    if is(ctx, &record.owner, account)? {
        return Ok(true);
    }
    let effective = ownership::Query::Effective {
        program: ctx.env().program.clone(),
        key: at,
    };
    let asked = ctx.query::<ownership::Query, ownership::Reply>(guardian, &effective)?;
    let ownership::Reply::Policy(Some(policy)) = asked else {
        return Ok(true);
    };
    match policy.write {
        ownership::Writers::Anyone => Ok(true),
        ownership::Writers::Owner => Ok(false),
        ownership::Writers::Listed(parties) => {
            for party in &parties {
                if is(ctx, party, account)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
    }
}

// Whether a party is the machine's account as ownership resolves it: that account, or a program
// that acts as it.
fn is(ctx: &QueryCtx, party: &ownership::Party, account: AccountNumber) -> Result<bool, Error> {
    match party {
        ownership::Party::Account(number) => Ok(*number == account),
        ownership::Party::Program(program) => Ok(account_of(ctx, program)? == Some(account)),
        ownership::Party::Foreign(_) => Ok(false),
    }
}

fn account_of(ctx: &QueryCtx, program: &ProgramId) -> Result<Option<AccountNumber>, Error> {
    let registry = ctx.env().role(abi::role::IDENTITY)?.clone();
    let asked = identity::Query::OfProgram(program.clone());
    match ctx.query::<identity::Query, identity::Reply>(registry.clone(), &asked)? {
        identity::Reply::Account(account) => Ok(account),
        other => Err(unexpected_reply(&registry, "OfProgram", &other)),
    }
}

fn assign(ctx: &ExecCtx, id: u64, mut row: MachineRow) -> Result<(), Error> {
    let mut run = load(ctx, id)?;
    run.attempt = run
        .attempt
        .checked_add(1)
        .ok_or_else(|| Error::new(code::EXHAUSTED, "attempts are exhausted"))?;
    run.state = State::Assigned;
    run.machine = Some(row.id.clone());
    run.assigned = Some(height(ctx));
    run.started = None;
    RUNS.put(ctx, &id, &run);
    dequeue(ctx, id)?;
    HOLDS.insert(ctx, &(row.id.account, row.id.name.clone(), id));
    row.busy += 1;
    settle_idle(ctx, &mut row)?;
    MACHINES.put(ctx, &key(&row.id), &row);
    charge(ctx, id, UNITS, 1)?;
    let check = Op::Unclaimed {
        run: id,
        attempt: run.attempt,
    };
    schedule(ctx, height(ctx).saturating_add(DEFAULT_WAIT), &check)
}

// The machine's next lapse check, at its last beat plus the egress wait, unless one is still to
// come: a beat never cancels a check, the check books the next.
fn book_check(ctx: &ExecCtx, machine: &MachineId) -> Result<(), Error> {
    let mut row = machine_row(ctx, machine)?;
    let now = height(ctx);
    if row.check_at.is_some_and(|at| at > now) {
        return Ok(());
    }
    let beat = BEATS.get(ctx, &key(machine))?.unwrap_or(now);
    let at = beat.saturating_add(DEFAULT_WAIT).max(now + 1);
    row.check_at = Some(at);
    MACHINES.put(ctx, &key(machine), &row);
    let check = Op::Lapse {
        machine: machine.clone(),
    };
    schedule(ctx, at, &check)
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

// A run that ended or was lost, told to whom its workload names. Each chat line has a message id
// of its own, and each message is kept by its seq until its answer comes (`replied`).
fn tell(ctx: &ExecCtx, workload: &Workload, run: &RunRow) -> Result<(), Error> {
    let sent = match &workload.spec.notify {
        None => return Ok(()),
        Some(Notify::Program { program, deliver }) => {
            let ended = RunEnd {
                run: run.id,
                attempt: run.attempt,
                state: run.state,
            };
            let mut payload = deliver.clone();
            payload.extend(abi::encode(&ended));
            ctx.emit_raw(program.clone(), payload, Wanted::Wanted)
        }
        Some(Notify::Chat { channel }) => {
            let told = TOLD
                .get(ctx)?
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| Error::new(code::EXHAUSTED, "told ends are exhausted"))?;
            TOLD.put(ctx, &told);
            let mut detail = format!("{} (attempt {})", run.state.word(), run.attempt);
            if !run.summary.is_empty() {
                detail.push_str(&format!(": {}", run.summary));
            }
            let out = run.out.as_ref().map(|out| {
                let path: Vec<&str> = out
                    .path
                    .split('/')
                    .filter(|part| !part.is_empty())
                    .collect();
                ducklink::path("files", &path)
            });
            let status = match run.state {
                State::Passed => chat::message::Status::Passed,
                State::Cancelled => chat::message::Status::Note,
                State::Lost | State::Failed | State::Errored => chat::message::Status::Failed,
                State::Queued | State::Assigned | State::Running | State::Stopping => {
                    chat::message::Status::Running
                }
            };
            let post = chat::Op::Notify {
                channel_id: channel.clone(),
                message_id: format!("fleet-run-{}-{}-{told}", run.id, run.attempt),
                thread: None,
                notice: chat::Notice {
                    kind: "run".into(),
                    group: format!("run {}", run.id),
                    subject: format!("run {}", run.id),
                    link: out.flatten(),
                    status,
                    detail,
                    concerns: vec![workload.by],
                },
            };
            ctx.emit(chat::PROGRAM, &post, Wanted::Wanted)
        }
    };
    TELLING.put(ctx, &sent.seq, &(run.id, run.attempt));
    Ok(())
}

// The answer to an end fleet told: a refusal goes into the run's summary while the run is still at
// that attempt, and the run stays as it ended.
pub(crate) fn replied(ctx: &ExecCtx, id: &MessageId, outcome: &Outcome) -> Result<(), Error> {
    let Some((run, attempt)) = TELLING.get(ctx, &id.seq)? else {
        return Ok(());
    };
    TELLING.remove(ctx, &id.seq);
    let Outcome::Rejected(refused) = outcome else {
        return Ok(());
    };
    let Some(mut row) = RUNS.get(ctx, &run)? else {
        return Ok(());
    };
    if row.attempt != attempt {
        return Ok(());
    }
    row.summary = match row.summary.is_empty() {
        true => format!("not told: {}", refused.message),
        false => format!("{}; not told: {}", row.summary, refused.message),
    };
    RUNS.put(ctx, &run, &row);
    Ok(())
}

fn load(ctx: &QueryCtx, id: u64) -> Result<RunRow, Error> {
    RUNS.get(ctx, &id)?
        .ok_or_else(|| not_found(format!("no run {id}")))
}

fn workload(ctx: &QueryCtx, id: u64) -> Result<Workload, Error> {
    WORKLOADS
        .get(ctx, &id)?
        .ok_or_else(|| not_found(format!("no workload {id}")))
}

fn machine_row(ctx: &QueryCtx, machine: &MachineId) -> Result<MachineRow, Error> {
    MACHINES.get(ctx, &key(machine))?.ok_or_else(|| {
        not_found(format!(
            "no machine {} of account {}",
            machine.name, machine.account
        ))
    })
}

fn held(ctx: &QueryCtx, machine: &MachineId) -> Result<Vec<RunRow>, Error> {
    let head = (machine.account, machine.name.clone());
    HOLDS
        .scan(ctx, HOLDS.prefix_of(&head))?
        .into_iter()
        .map(|(_, _, id)| load(ctx, id))
        .collect()
}

pub(crate) fn workload_row(ctx: &QueryCtx, id: u64) -> Result<Option<Workload>, Error> {
    WORKLOADS.get(ctx, &id)
}

pub(crate) fn run_row(ctx: &QueryCtx, id: u64) -> Result<Option<RunRow>, Error> {
    RUNS.get(ctx, &id)
}

pub(crate) fn queued(ctx: &QueryCtx, page: &PageRequest) -> Result<PageResponse<RunRow>, Error> {
    QUEUE
        .range(ctx, page)?
        .try_map(|(_, _, _, id)| load(ctx, id))
}

pub(crate) fn filed_row(ctx: &QueryCtx, id: u64) -> Result<Option<Filed>, Error> {
    FILED.get(ctx, &id)
}

pub(crate) fn machine_kind(ctx: &QueryCtx, id: &MachineId) -> Result<Option<String>, Error> {
    MACHINE_KINDS.get(ctx, &key(id))
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

fn view(ctx: &QueryCtx, machine: MachineRow) -> Result<MachineView, Error> {
    let beat = BEATS.get(ctx, &key(&machine.id))?;
    Ok(MachineView { machine, beat })
}

pub(crate) fn machine(ctx: &QueryCtx, id: &MachineId) -> Result<Option<MachineView>, Error> {
    MACHINES
        .get(ctx, &key(id))?
        .map(|row| view(ctx, row))
        .transpose()
}

pub(crate) fn machines(
    ctx: &QueryCtx,
    page: &PageRequest,
) -> Result<PageResponse<MachineView>, Error> {
    MACHINES
        .range(ctx, page)?
        .try_map(|(_, row)| view(ctx, row))
}

pub(crate) fn pieces(
    ctx: &QueryCtx,
    run: u64,
    attempt: u32,
    call: u64,
    page: &PageRequest,
) -> Result<PageResponse<PieceRow>, Error> {
    let rows = PIECES.range_of(ctx, &(run, attempt, call), page)?;
    Ok(rows.map(|((_, _, _, n), piece)| PieceRow { n, piece }))
}

pub(crate) fn holds(
    ctx: &QueryCtx,
    machine: &MachineId,
    page: &PageRequest,
) -> Result<PageResponse<RunRow>, Error> {
    let head = (machine.account, machine.name.clone());
    HOLDS
        .range_of(ctx, &head, page)?
        .try_map(|(_, _, id)| load(ctx, id))
}
