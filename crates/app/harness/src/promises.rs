//! Promises and asks: what an account owes, recorded by the agent that promises or asks it, as it
//! does. harness's own scheduled entry wakes both sides at the due block and again every `REPEAT`
//! blocks, until the account that owes it closes it as kept or dropped (`Op::Keep`).
use ::program::{
    Cause, Ctx, Error, QueryCtx, code, corrupt, invalid, not_found, unauthorized, wrong_state,
};
use abi::role::identity::AccountNumber;
use store::{PageRequest, PageResponse};

use crate::agents::{profile, running, thread};
use crate::bookings::note;
use crate::state::{ASKING, PROMISE_COUNT, PROMISE_ENTRIES, PROMISES, PROMISING, THREADS};
use crate::turns::converse;
use crate::{Op, PromiseClosed, PromiseRow};

pub(crate) const REPEAT: u64 = 3600;

async fn book(ctx: &Ctx, id: u64, height: u64) -> Result<u64, Error> {
    let due = Op::PromiseDue { promise: id };
    ctx.schedule(height, &ctx.env().program, &due).await
}

pub(crate) async fn promise(
    ctx: &Ctx,
    thread_id: u64,
    owed_by: AccountNumber,
    (channel, seq, text): (String, u64, String),
    due: u64,
) -> Result<(), Error> {
    let asked_by = ctx.sender_account()?;
    let row = thread(ctx, thread_id)?;
    if row.agent != asked_by {
        return Err(invalid(format!(
            "conversation {thread_id} is not account {asked_by}'s: choose a conversation owned by the acting account"
        )));
    }
    if row.closed {
        return Err(wrong_state(format!(
            "conversation {thread_id} is closed: promise or ask from an open conversation"
        )));
    }
    if profile(ctx, owed_by)?.is_none() {
        return Err(not_found(format!(
            "no account {owed_by}: name the account that owes it"
        )));
    }
    let made = ctx.env().height;
    if due <= made {
        return Err(invalid(format!(
            "a promise is due at a block after this one, {made}: give a later height, or in"
        )));
    }
    let id = PROMISE_COUNT.get(ctx)?.unwrap_or(0) + 1;
    PROMISE_COUNT.put(ctx, &id);
    let entry = book(ctx, id, due).await?;
    let promised = PromiseRow {
        id,
        owed_by,
        asked_by,
        thread: thread_id,
        channel,
        seq,
        text,
        made,
        due,
        next: due,
        closed: None,
    };
    PROMISES.put(ctx, &id, &promised);
    PROMISING.insert(ctx, &(owed_by, id));
    if owed_by != asked_by {
        ASKING.insert(ctx, &(asked_by, id));
    }
    PROMISE_ENTRIES.put(ctx, &id, &entry);
    ctx.set_return_data(abi::encode(&id));
    Ok(())
}

pub(crate) async fn keep(ctx: &Ctx, id: u64, kept: bool, how: String) -> Result<(), Error> {
    let mut row = PROMISES
        .get(ctx, &id)?
        .ok_or_else(|| not_found(format!("no promise {id}")))?;
    let owes = ctx.sender_account()? == row.owed_by;
    if !owes {
        return Err(unauthorized(format!(
            "promise {id} is owed by account {}, and only it closes it",
            row.owed_by
        )));
    }
    if let Some(closed) = &row.closed {
        return Err(wrong_state(format!(
            "promise {id} was closed at block {}: make a new promise instead of closing it again",
            closed.height
        )));
    }
    if let Some(entry) = PROMISE_ENTRIES.get(ctx, &id)? {
        PROMISE_ENTRIES.remove(ctx, &id);
        match ctx.cancel(entry).await {
            Err(error) if error.code == code::WRONG_STATE => {}
            cancelled => cancelled?,
        }
    }
    row.closed = Some(PromiseClosed {
        height: ctx.env().height,
        kept,
        how,
    });
    PROMISES.put(ctx, &id, &row);
    PROMISING.remove(ctx, &(row.owed_by, id));
    ASKING.remove(ctx, &(row.asked_by, id));
    Ok(())
}

fn told_owed(row: &PromiseRow, list: &str) -> String {
    let PromiseRow {
        id,
        asked_by,
        text,
        made,
        seq,
        channel,
        next,
        ..
    } = row;
    format!(
        "Ask {id} is due: account {asked_by} asked you: {text}\n(Asked at block {made}, answering message {seq} of chat channel {channel}. Close it with promise_keep once it is kept or dropped; while it is open it comes back at block {next}.)\n{list}"
    )
}

fn told_asker(row: &PromiseRow, list: &str) -> String {
    let PromiseRow {
        id,
        owed_by,
        text,
        made,
        seq,
        channel,
        next,
        ..
    } = row;
    match *owed_by == row.asked_by {
        true => format!(
            "Promise {id} is due: {text}\n(Made at block {made}, answering message {seq} of chat channel {channel}. Close it with promise_keep once it is kept or dropped; while it is open it comes back at block {next}.)\n{list}"
        ),
        false => format!(
            "Ask {id} of account {owed_by} is due: {text}\n(Asked at block {made}, answering message {seq} of chat channel {channel}. Only account {owed_by} closes it; while it is open it comes back at block {next}.)\n{list}"
        ),
    }
}

pub(crate) async fn due(ctx: Ctx, id: u64) -> Result<(), Error> {
    let entry = PROMISE_ENTRIES.get(&ctx, &id)?;
    let env = ctx.env();
    let by_harness = env
        .sending_program()
        .is_ok_and(|program| program == env.program);
    let its_own = matches!(env.cause, Cause::Scheduled { entry: ran } if Some(ran) == entry);
    let its_turn = by_harness && its_own;
    if !its_turn {
        return Err(unauthorized(
            "a promise comes due only as harness's own scheduled entry, at its block",
        ));
    }
    let Some(mut row) = PROMISES.get(&ctx, &id)?.filter(|row| row.closed.is_none()) else {
        PROMISE_ENTRIES.remove(&ctx, &id);
        return Ok(());
    };
    let next = env.height.saturating_add(REPEAT);
    let entry = book(&ctx, id, next).await?;
    PROMISE_ENTRIES.put(&ctx, &id, &entry);
    row.next = next;
    PROMISES.put(&ctx, &id, &row);
    let mut woken = Vec::new();
    let asked = row.owed_by != row.asked_by;
    let owed_runs = asked && running(&ctx, row.owed_by)?.is_some();
    if owed_runs {
        let list = open_list(&ctx, row.owed_by)?.unwrap_or_default();
        let text = told_owed(&row, &list);
        woken.extend(note(&ctx, row.owed_by, None, text)?);
    }
    let list = open_list(&ctx, row.asked_by)?.unwrap_or_default();
    let text = told_asker(&row, &list);
    let alive = THREADS
        .get(&ctx, &row.thread)?
        .is_some_and(|told| !told.closed);
    if let Some(told) = note(&ctx, row.asked_by, alive.then_some(row.thread), text)? {
        row.thread = told;
        PROMISES.put(&ctx, &id, &row);
        woken.push(told);
    }
    for thread in woken {
        converse(&ctx, thread).await;
    }
    Ok(())
}

fn line(row: &PromiseRow) -> String {
    let what = match row.owed_by == row.asked_by {
        true => format!("promise {}", row.id),
        false => format!(
            "ask {} of account {} by account {}",
            row.id, row.owed_by, row.asked_by
        ),
    };
    format!(
        "- {what}, due next at block {}: {} (conversation {}, answering message {} of chat channel {})",
        row.next, row.text, row.thread, row.seq, row.channel
    )
}

fn lines(ctx: &QueryCtx, ids: Vec<(AccountNumber, u64)>) -> Result<Vec<String>, Error> {
    let mut lines = Vec::new();
    for (_, id) in ids {
        if let Some(row) = PROMISES.get(ctx, &id)? {
            lines.push(line(&row));
        }
    }
    Ok(lines)
}

pub(crate) fn open_list(ctx: &QueryCtx, agent: AccountNumber) -> Result<Option<String>, Error> {
    let owed = lines(ctx, PROMISING.scan(ctx, PROMISING.prefix_of(&agent))?)?;
    let asked = lines(ctx, ASKING.scan(ctx, ASKING.prefix_of(&agent))?)?;
    let mut sections = Vec::new();
    if !owed.is_empty() {
        sections.push(format!("Your open promises:\n{}", owed.join("\n")));
    }
    if !asked.is_empty() {
        sections.push(format!(
            "What you asked of others, still open:\n{}",
            asked.join("\n")
        ));
    }
    Ok((!sections.is_empty()).then(|| sections.join("\n")))
}

pub(crate) fn open(
    ctx: &QueryCtx,
    agent: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<PromiseRow>, Error> {
    PROMISING.range_of(ctx, &agent, page)?.try_map(|(_, id)| {
        PROMISES.get(ctx, &id)?.ok_or_else(|| {
            corrupt(
                "promising",
                &id.to_be_bytes(),
                "names a promise that is gone",
            )
        })
    })
}

pub(crate) fn asked(
    ctx: &QueryCtx,
    agent: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<PromiseRow>, Error> {
    ASKING.range_of(ctx, &agent, page)?.try_map(|(_, id)| {
        PROMISES
            .get(ctx, &id)?
            .ok_or_else(|| corrupt("asking", &id.to_be_bytes(), "names an ask that is gone"))
    })
}

pub(crate) fn row(ctx: &QueryCtx, id: u64) -> Result<Option<PromiseRow>, Error> {
    PROMISES.get(ctx, &id)
}
