//! A mention's run: once an account's post that mentions an agent reaches its conversation, its
//! author's very next posts in that chat room reach the agent too, until a post by anyone else ends
//! it. A room the agent listens to, or a direct room, keeps none: every post there reaches it.
use ::program::{Error, ExecCtx, Principal, QueryCtx};
use abi::role::identity::AccountNumber;
use chat::MsgRow;

use crate::RunRow;
use crate::listening;
use crate::state::{RUNS, Run, THREADS};

pub(crate) fn open(
    ctx: &ExecCtx,
    agent: AccountNumber,
    row: &MsgRow,
    author: AccountNumber,
    thread: u64,
) -> Result<(), Error> {
    let room = &row.channel_id;
    let (listened, _) = listening::chosen(ctx, agent, room)?;
    let heard_whole = listened || chat::dm_peers(room).is_some();
    if heard_whole {
        return Ok(());
    }
    let run = Run {
        author,
        mentioned: row.seq,
        last: row.seq,
        thread,
    };
    RUNS.put(ctx, &(agent, room.clone()), &run);
    listening::watch(ctx, agent, room, true);
    Ok(())
}

pub(crate) fn step(
    ctx: &ExecCtx,
    agent: AccountNumber,
    row: &MsgRow,
    actor: &Principal,
    listened: bool,
) -> Result<Option<u64>, Error> {
    let key = (agent, row.channel_id.clone());
    let Some(run) = RUNS.get(ctx, &key)? else {
        return Ok(None);
    };
    if row.seq <= run.last {
        return Ok(None);
    }
    let next = *actor == Principal::Account(run.author) && row.seq == run.last + 1 && !listened;
    if next {
        RUNS.put(
            ctx,
            &key,
            &Run {
                last: row.seq,
                ..run
            },
        );
        return Ok(Some(run.mentioned));
    }
    end(ctx, agent, &row.channel_id)?;
    Ok(None)
}

pub(crate) fn reached(
    ctx: &ExecCtx,
    agent: AccountNumber,
    room: &str,
    thread: u64,
) -> Result<(), Error> {
    let key = (agent, room.to_owned());
    if let Some(run) = RUNS.get(ctx, &key)?.filter(|run| run.thread != thread) {
        RUNS.put(ctx, &key, &Run { thread, ..run });
    }
    Ok(())
}

pub(crate) fn answered(ctx: &ExecCtx, agent: AccountNumber, thread: u64) -> Result<(), Error> {
    let open = RUNS.scan(ctx, RUNS.prefix_of(&agent))?;
    for ((_, room), run) in open {
        if run.thread == thread {
            end(ctx, agent, &room)?;
        }
    }
    Ok(())
}

pub(crate) fn open_in(ctx: &QueryCtx, agent: AccountNumber, room: &str) -> Result<bool, Error> {
    Ok(RUNS.has(ctx, &(agent, room.to_owned())))
}

pub(crate) fn runs(ctx: &QueryCtx, thread: u64) -> Result<Vec<RunRow>, Error> {
    let Some(row) = THREADS.get(ctx, &thread)? else {
        return Ok(Vec::new());
    };
    let open = RUNS.scan(ctx, RUNS.prefix_of(&row.agent))?;
    Ok(open
        .into_iter()
        .filter(|(_, run)| run.thread == thread)
        .map(|((_, channel), run)| RunRow {
            channel,
            author: run.author,
            mentioned: run.mentioned,
            last: run.last,
        })
        .collect())
}

fn end(ctx: &ExecCtx, agent: AccountNumber, room: &str) -> Result<(), Error> {
    RUNS.remove(ctx, &(agent, room.to_owned()));
    let waits = crate::waits::message_waits(ctx, agent)?;
    if !waits.iter().any(|(waited, _)| waited == room) {
        listening::watch(ctx, agent, room, false);
    }
    Ok(())
}
