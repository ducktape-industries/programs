//! A wake's end, heard: wake tells an agent a wake is over as a `Result` on `wake/<id>`. One that backs
//! a wait ends it (`waits::woken`); another reaches the conversation whose turn armed it, even inside
//! its own `WakeOn`, a closed one is told it was not delivered, and one never seen armed the canonical.
use ::program::{Error, ExecCtx};
use abi::role::identity::AccountNumber;
use attribution::{Change, Kind, Reason};

use crate::agents::{hear_again, running};
use crate::bookings::note;
use crate::state::{ARMING, UNCLAIMED, WAKE_THREADS, WAKE_WAITS};
use crate::tools::Write;

pub(crate) enum Arming {
    On,
    Off(u64),
}

pub(crate) struct Call<'a> {
    pub agent: AccountNumber,
    pub thread: u64,
    pub call: &'a str,
}

pub(crate) fn arming(writes: &[Write]) -> Option<Arming> {
    let write = writes.last()?;
    if write.target != wake::PROGRAM {
        return None;
    }
    match abi::decode::<wake::Op>(&write.payload) {
        Ok(wake::Op::WakeOn { .. }) => Some(Arming::On),
        Ok(wake::Op::Cancel { id }) => Some(Arming::Off(id)),
        _ => None,
    }
}

fn key(call: &Call) -> (AccountNumber, u64, String) {
    (call.agent, call.thread, call.call.to_owned())
}

pub(crate) fn sending(ctx: &ExecCtx, call: &Call, arming: &Option<Arming>) {
    if let Some(Arming::On) = arming {
        ARMING.insert(ctx, &key(call));
    }
}

pub(crate) fn sent(
    ctx: &ExecCtx,
    call: &Call<'_>,
    arming: Option<Arming>,
    output: Option<&[u8]>,
) -> Result<(), Error> {
    match arming {
        Some(Arming::On) => {}
        Some(Arming::Off(id)) => {
            WAKE_THREADS.remove(ctx, &id);
            if output.is_some() {
                crate::waits::cancelled(ctx, id)?;
            }
            return Ok(());
        }
        None => return Ok(()),
    }
    let id = output.and_then(|output| abi::decode::<u64>(output).ok());
    if let Some(id) = id {
        let unclaimed = (call.agent, id);
        match UNCLAIMED.get(ctx, &unclaimed)? {
            Some(change) => {
                UNCLAIMED.remove(ctx, &unclaimed);
                note(ctx, call.agent, Some(call.thread), detail(&change))?;
            }
            None => WAKE_THREADS.put(ctx, &id, &call.thread),
        }
    }
    ended(ctx, call)
}

pub(crate) fn ended(ctx: &ExecCtx, call: &Call<'_>) -> Result<(), Error> {
    ARMING.remove(ctx, &key(call));
    flush(ctx, call.agent)
}

pub(crate) fn let_go(ctx: &ExecCtx, agent: AccountNumber, thread: u64) -> Result<(), Error> {
    for arming in ARMING.scan(ctx, ARMING.prefix_of(&agent))? {
        if arming.1 == thread {
            ARMING.remove(ctx, &arming);
        }
    }
    flush(ctx, agent)
}

fn flush(ctx: &ExecCtx, agent: AccountNumber) -> Result<(), Error> {
    if arming_now(ctx, agent)? {
        return Ok(());
    }
    let mut changes = Vec::new();
    for ((agent, id), change) in UNCLAIMED.scan(ctx, UNCLAIMED.prefix_of(&agent))? {
        UNCLAIMED.remove(ctx, &(agent, id));
        changes.push(change);
    }
    hear_again(ctx, changes);
    Ok(())
}

fn detail(change: &Change) -> String {
    String::from_utf8_lossy(&change.detail).into_owned()
}

fn arming_now(ctx: &ExecCtx, agent: AccountNumber) -> Result<bool, Error> {
    Ok(!ARMING.scan(ctx, ARMING.prefix_of(&agent))?.is_empty())
}

pub(crate) fn from_wake(change: &Change) -> bool {
    change.source.program == wake::PROGRAM
        && change.source.object.kind == wake::KIND
        && change.reason == Reason::Result
}

pub(crate) fn told(ctx: &ExecCtx, change: &Change) -> Result<Option<u64>, Error> {
    let Ok(id) = change.source.object.id.parse::<u64>() else {
        return Ok(None);
    };
    if let Some(wait) = WAKE_WAITS.get(ctx, &id)? {
        return crate::waits::woken(ctx, id, wait, change);
    }
    let thread = WAKE_THREADS.get(ctx, &id)?;
    WAKE_THREADS.remove(ctx, &id);
    let agent = change.recipient;
    let heeded = change.kind == Kind::Added && running(ctx, agent)?.is_some();
    if !heeded {
        return Ok(None);
    }
    let unclaimed = thread.is_none() && arming_now(ctx, agent)?;
    if unclaimed {
        UNCLAIMED.put(ctx, &(agent, id), change);
        return Ok(None);
    }
    let told = note(ctx, agent, thread, detail(change))?;
    if let Some(thread) = told {
        crate::resume::heard(ctx, thread)?;
    }
    Ok(told)
}
