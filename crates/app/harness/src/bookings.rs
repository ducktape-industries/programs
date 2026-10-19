//! Runs an account books for a later block: harness holds the scheduler's entry and, when it is
//! due, sends the booked op as that account; what it did is kept and told to the conversation it
//! was booked from, which is how an agent wakes itself. Only who booked a run cancels it.
use ::program::{
    Cause, Ctx, Error, ExecCtx, Principal, QueryCtx, Range, corrupt, invalid, not_found,
    unauthorized, unexpected_reply, wrong_state,
};
use abi::role::identity::AccountNumber;
use abi::role::scheduler;
use store::{PageRequest, PageResponse};

use crate::calls::{refusal, relayable};
use crate::conversations::current;
use crate::state::{BOOKED, BOOKINGS, THREADS};
use crate::tools::interface::answered;
use crate::turns::{append, converse, say};
use crate::{Body, Booking, Op, PROGRAM, Ran, ThreadRow};

fn bound_scheduler(ctx: &QueryCtx) -> Result<String, Error> {
    Ok(ctx.env().role(abi::role::SCHEDULER)?.clone())
}

fn runs(ctx: &QueryCtx, account: AccountNumber) -> Result<(), Error> {
    let role = ctx.env().role(abi::role::IDENTITY)?.clone();
    let asked = identity::Query::Runs {
        program: PROGRAM.into(),
        account,
    };
    match ctx.query::<identity::Query, identity::Reply>(&role, &asked)? {
        identity::Reply::Number(Some(number)) if number == account => Ok(()),
        identity::Reply::Number(_) => Err(unauthorized(format!(
            "harness does not run account {account}"
        ))),
        other => Err(unexpected_reply(&role, "Runs", &other)),
    }
}

fn kept(ctx: &QueryCtx, entry: u64) -> Result<Booking, Error> {
    BOOKINGS
        .get(ctx, &entry)?
        .ok_or_else(|| not_found(format!("no booked run {entry}")))
}

fn unran(booking: &Booking) -> Result<(), Error> {
    let Some(ran) = &booking.ran else {
        return Ok(());
    };
    Err(wrong_state(format!(
        "run {} ran at block {}",
        booking.entry, ran.height
    )))
}

pub(crate) async fn book(
    ctx: &Ctx,
    height: u64,
    target: String,
    payload: Vec<u8>,
    thread: Option<u64>,
) -> Result<(), Error> {
    let agent = ctx.sender_account()?;
    if let Some(id) = thread {
        let theirs = crate::agents::thread(ctx, id)?.agent == agent;
        if !theirs {
            return Err(invalid(format!(
                "conversation {id} is not account {agent}'s"
            )));
        }
    }
    runs(ctx, agent)?;
    relayable(ctx, &target, &payload)?;
    let schedule = scheduler::Op::Schedule {
        height,
        target: PROGRAM.into(),
        payload: abi::encode(&Op::RunBooked),
    };
    let bound = bound_scheduler(ctx)?;
    let answer = ctx.call_raw(bound.clone(), abi::encode(&schedule)).await?;
    let entry: u64 = abi::decode(&answer)
        .map_err(|fault| unexpected_reply(&bound, "Schedule", &fault.sentence))?;
    let booking = Booking {
        entry,
        agent,
        thread,
        booked: ctx.env().height,
        height,
        target,
        payload,
        ran: None,
    };
    BOOKINGS.put(ctx, &entry, &booking);
    BOOKED.insert(ctx, &(agent, entry));
    ctx.set_return_data(abi::encode(&entry));
    Ok(())
}

pub(crate) async fn unbook(ctx: &Ctx, entry: u64) -> Result<(), Error> {
    let booking = kept(ctx, entry)?;
    let theirs = ctx.sender_account()? == booking.agent;
    if !theirs {
        return Err(unauthorized(format!(
            "run {entry} is account {}'s to cancel",
            booking.agent
        )));
    }
    unran(&booking)?;
    let cancel = scheduler::Op::Cancel { entry };
    ctx.call_raw(bound_scheduler(ctx)?, abi::encode(&cancel))
        .await?;
    BOOKINGS.remove(ctx, &entry);
    BOOKED.remove(ctx, &(booking.agent, entry));
    Ok(())
}

pub(crate) async fn run_booked(ctx: &Ctx) -> Result<(), Error> {
    let cause = ctx.env().cause.clone();
    let by_harness = ctx
        .env()
        .sending_program()
        .is_ok_and(|program| program == PROGRAM);
    let (Cause::Scheduled { entry }, true) = (cause, by_harness) else {
        return Err(unauthorized(
            "a booked run runs only as harness's own scheduled entry, at its block",
        ));
    };
    let booking = kept(ctx, entry)?;
    unran(&booking)?;
    let sent = match relayable(ctx, &booking.target, &booking.payload) {
        Ok(()) => {
            let (target, payload) = (booking.target.clone(), booking.payload.clone());
            ctx.call_raw_as(booking.agent, target, payload).await
        }
        Err(refused) => Err(refused),
    };
    let height = ctx.env().height;
    let ran = match sent {
        Ok(output) => Ran {
            height,
            failed: false,
            text: answered(ctx, &booking.target, &booking.payload, &output).to_string(),
        },
        Err(error) => Ran {
            height,
            failed: true,
            text: refusal(&error),
        },
    };
    let mut booking = kept(ctx, entry)?;
    let text = report(&booking, &ran);
    booking.ran = Some(ran);
    BOOKINGS.put(ctx, &entry, &booking);
    tell(ctx, booking.agent, booking.thread, text).await
}

fn report(booking: &Booking, ran: &Ran) -> String {
    let Booking {
        entry,
        booked,
        height,
        target,
        ..
    } = booking;
    match ran.failed {
        false => format!(
            "Your run {entry}, booked at block {booked} for block {height}, ran as you: {target} answered {}",
            ran.text
        ),
        true => format!(
            "Your run {entry}, booked at block {booked} for block {height}, was refused: {}. It does not run again.",
            ran.text
        ),
    }
}

fn told(
    ctx: &QueryCtx,
    agent: AccountNumber,
    thread: Option<u64>,
) -> Result<Option<ThreadRow>, Error> {
    let from = match thread {
        Some(id) => THREADS.get(ctx, &id)?.filter(|row| !row.closed),
        None => None,
    };
    match from {
        Some(row) => Ok(Some(row)),
        None => current(ctx, agent),
    }
}

// a line from the network in the conversation `thread` names, else the agent's current one: the
// conversation it went to
pub(crate) fn note(
    ctx: &ExecCtx,
    agent: AccountNumber,
    thread: Option<u64>,
    text: String,
) -> Result<Option<u64>, Error> {
    if let Some(id) = thread
        && let Some(mut row) = THREADS.get(ctx, &id)?.filter(|row| row.closed)
    {
        undelivered(ctx, &mut row, text);
        return Ok(None);
    }
    let Some(mut row) = told(ctx, agent, thread)? else {
        return Ok(None);
    };
    let author = Principal::Root;
    say(ctx, &mut row, Body::Post { author, text });
    THREADS.put(ctx, &row.id, &row);
    Ok(Some(row.id))
}

// what was meant to wake a conversation that is closed: a notice on it, which wakes nothing, and
// never a line in another conversation, which knows nothing of what was asked
pub(crate) fn undelivered(ctx: &ExecCtx, row: &mut ThreadRow, text: String) {
    let text = format!("Not delivered, as this conversation is closed: {text}");
    append(ctx, row, Body::Notice { text });
    THREADS.put(ctx, &row.id, row);
}

// that line, which wakes the conversation
pub(crate) async fn tell(
    ctx: &Ctx,
    agent: AccountNumber,
    thread: Option<u64>,
    text: String,
) -> Result<(), Error> {
    if let Some(id) = note(ctx, agent, thread, text)? {
        converse(ctx, id).await;
    }
    Ok(())
}

pub(crate) fn bookings(
    ctx: &QueryCtx,
    agent: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<Booking>, Error> {
    BOOKED.range_of(ctx, &agent, page)?.try_map(|(_, entry)| {
        BOOKINGS
            .get(ctx, &entry)?
            .ok_or_else(|| corrupt("booked", &entry.to_be_bytes(), "names a run that is gone"))
    })
}

// an account's runs newest first: `limit` of them booked before the run `below`, only those
// still waiting where `pending` asks so; and the last run read where older ones follow it
pub(crate) fn newest(
    ctx: &QueryCtx,
    agent: AccountNumber,
    below: Option<u64>,
    limit: u64,
    pending: bool,
) -> Result<(Vec<Booking>, Option<u64>), Error> {
    let whole = BOOKED.prefix_of(&agent);
    let before = |entry: Option<u64>| match entry {
        Some(entry) => Some(BOOKED.key(&(agent, entry))),
        None => whole.end.clone(),
    };
    let mut found = Vec::new();
    let mut last = below;
    loop {
        let wanted = limit - found.len() as u64;
        let scan = Range::new(whole.start.clone(), before(last));
        let keys = BOOKED.scan(ctx, scan.reverse().limit(wanted))?;
        for (_, entry) in &keys {
            let booking = BOOKINGS.get(ctx, entry)?.ok_or_else(|| {
                corrupt("booked", &entry.to_be_bytes(), "names a run that is gone")
            })?;
            if !pending || booking.ran.is_none() {
                found.push(booking);
            }
        }
        let Some((_, read)) = keys.last() else {
            return Ok((found, None));
        };
        last = Some(*read);
        if (keys.len() as u64) < wanted {
            return Ok((found, None));
        }
        if found.len() as u64 == limit {
            break;
        }
    }
    let older = Range::new(whole.start.clone(), before(last))
        .reverse()
        .limit(1);
    let more = !BOOKED.scan(ctx, older)?.is_empty();
    Ok((found, last.filter(|_| more)))
}

pub(crate) fn booking(ctx: &QueryCtx, entry: u64) -> Result<Option<Booking>, Error> {
    BOOKINGS.get(ctx, &entry)
}
