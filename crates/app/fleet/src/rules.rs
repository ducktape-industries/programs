//! What each op checks and writes: an Ask queues a run and places it, a ready machine with a free
//! slot takes the queued runs it covers, and a run goes from assigned to running to how its
//! machine says it ended; a machine silent for the egress wait loses its runs.
use ::program::{
    Error, ExecCtx, MessageId, Outcome, QueryCtx, Reply as Wanted, code, invalid, not_found,
    unexpected_reply, wrong_state,
};
use abi::role::egress::DEFAULT_WAIT;
use abi::role::identity;

use crate::state::{
    BEATS, CLAIMS, Claim, DECLINED, HOLDS, LAST, MACHINES, QUEUED, RUNS, TELLING, TOLD, WORKLOADS,
};
use crate::{
    AccountNumber, Affinity, Ended, Kind, MachineId, MachineRow, MachineView, Notify, OnLoss, Op,
    Out, PageRequest, PageResponse, ProgramId, RunRow, Serves, Spec, State, Workload,
};

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
    let by = ctx.sender_account()?;
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
    QUEUED.insert(ctx, &id);
    // placed the block after: the guard records the claim row's owner and policy only once this
    // run returns, and placement reads that policy
    schedule(ctx, height(ctx).saturating_add(1), &Op::Place { run: id })?;
    ctx.set_return_data(abi::encode(&id));
    Ok(())
}

// A run still queued, placed. Open: it acts only on what the chain shows.
pub(crate) fn place_queued(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    if load(ctx, id)?.state != State::Queued {
        return Ok(());
    }
    place(ctx, id)
}

pub(crate) fn ready(
    ctx: &ExecCtx,
    name: String,
    labels: Vec<String>,
    slots: u64,
    serves: Serves,
) -> Result<(), Error> {
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
    row.labels = labels;
    row.slots = slots;
    row.serves = serves;
    row.gone = false;
    settle_idle(ctx, &mut row);
    MACHINES.put(ctx, &key(&id), &row);
    fill(ctx, &id)
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
                "run {id} is not assigned to a machine of account {sender} at attempt {attempt}"
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
    book_check(ctx, &machine)
}

pub(crate) fn beat(ctx: &ExecCtx, name: String) -> Result<(), Error> {
    let id = mine(ctx, name)?;
    let mut row = machine_row(ctx, &id)?;
    BEATS.put(ctx, &key(&id), &height(ctx));
    if row.gone {
        row.gone = false;
        settle_idle(ctx, &mut row);
        MACHINES.put(ctx, &key(&id), &row);
        fill(ctx, &id)?;
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
                "run {id} is {} and not held by a machine of account {sender} at attempt {attempt}",
                run.state.word()
            )));
        }
    };
    let stopped = run.state == State::Stopping;
    if held_now(&run) {
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
    fill(ctx, &machine)
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
                "run {id} is not assigned to a machine of account {sender} at attempt {attempt}"
            )));
        }
    };
    DECLINED.insert(ctx, &(id, machine.account, machine.name.clone()));
    release(ctx, id, &machine)?;
    run.summary = format!("{} declined it: {why}", machine.name);
    requeued(ctx, &mut run)?;
    fill(ctx, &machine)
}

pub(crate) fn cancel(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    let mut run = load(ctx, id)?;
    if run.state.ended() {
        return Err(wrong_state(format!(
            "run {id} already ended {}",
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
            QUEUED.remove(ctx, &id);
            let assigned = state == State::Assigned;
            run.state = State::Cancelled;
            run.finished = Some(height(ctx));
            RUNS.put(ctx, &id, &run);
            tell(ctx, &workload(ctx, id)?, &run)?;
            match machine.filter(|_| assigned) {
                Some(machine) => {
                    release(ctx, id, &machine)?;
                    fill(ctx, &machine)
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
        if row.check_at.is_some_and(|at| at <= now) {
            row.check_at = None;
            MACHINES.put(ctx, &key(&machine), &row);
        }
        return Ok(());
    }
    let beat = BEATS.get(ctx, &key(&machine))?.unwrap_or(0);
    if beat.saturating_add(DEFAULT_WAIT) > now {
        return book_check(ctx, &machine);
    }
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
    row.gone = true;
    row.idle_since = None;
    MACHINES.put(ctx, &key(&machine), &row);
    run.summary = format!("{} did not claim it", machine.name);
    requeued(ctx, &mut run)
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
    QUEUED.insert(ctx, &run.id);
    place(ctx, run.id)
}

// A run its silent machine held: a job that must never run twice is parked as lost, a run someone
// was stopping ends cancelled, and any other is queued again.
fn lose(ctx: &ExecCtx, mut run: RunRow, machine: &MachineId, why: &str) -> Result<(), Error> {
    HOLDS.remove(ctx, &(machine.account, machine.name.clone(), run.id));
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
    settle_idle(ctx, &mut row);
    MACHINES.put(ctx, &key(machine), &row);
    Ok(())
}

// A machine with a free slot has been free since the first block it was; one with none is not.
fn settle_idle(ctx: &QueryCtx, row: &mut MachineRow) {
    let free = !row.gone && row.busy < row.slots;
    row.idle_since = match (free, row.idle_since) {
        (true, Some(since)) => Some(since),
        (true, None) => Some(height(ctx)),
        (false, _) => None,
    };
}

// The queued run, to the ready machine free longest that may take it.
fn place(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    let workload = workload(ctx, id)?;
    let mut best: Option<MachineRow> = None;
    for (_, row) in MACHINES.all(ctx)? {
        if !takes(ctx, &row, &workload)? {
            continue;
        }
        let earlier = best
            .as_ref()
            .is_none_or(|best| (row.idle_since, &row.id) < (best.idle_since, &best.id));
        if earlier {
            best = Some(row);
        }
    }
    match best {
        Some(row) => assign(ctx, id, row),
        None => Ok(()),
    }
}

// The queued runs the machine may take, oldest first, while it has a free slot.
fn fill(ctx: &ExecCtx, machine: &MachineId) -> Result<(), Error> {
    for id in QUEUED.all(ctx)? {
        let row = machine_row(ctx, machine)?;
        if row.gone || row.busy >= row.slots {
            return Ok(());
        }
        if takes(ctx, &row, &workload(ctx, id)?)? {
            assign(ctx, id, row)?;
        }
    }
    Ok(())
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
        program: crate::PROGRAM.into(),
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
        program: crate::PROGRAM.into(),
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
    QUEUED.remove(ctx, &id);
    HOLDS.insert(ctx, &(row.id.account, row.id.name.clone(), id));
    row.busy += 1;
    settle_idle(ctx, &mut row);
    MACHINES.put(ctx, &key(&row.id), &row);
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
            let ended = Ended {
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
            let mut line = format!(
                "**run {} {}** (attempt {}), asked by <@{}>",
                run.id,
                run.state.word(),
                run.attempt,
                workload.by
            );
            if !run.summary.is_empty() {
                line.push_str(&format!(": {}", run.summary));
            }
            if let Some(out) = &run.out {
                line.push_str(&format!(". Out: `{}`", out.path));
            }
            let post = chat::Op::PostMessage {
                channel_id: channel.clone(),
                message_id: format!("fleet-run-{}-{}-{told}", run.id, run.attempt),
                blocks: chat::parse_message(&line),
                thread: None,
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
    QUEUED.range(ctx, page)?.try_map(|id| load(ctx, id))
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
