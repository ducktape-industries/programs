//! Waits: one event an agent names wakes the conversation that waits for it once, and the turn that
//! armed it ends. Each but a chat message's is a wake armed as the agent (`arm`), its end heard back
//! through attribution (`woken`); a chat message's is a filter on what harness hears from chat.
use ::program::{
    Ctx, Error, ExecCtx, Principal, QueryCtx, corrupt, invalid, not_found, unauthorized,
    unexpected_reply, wrong_state,
};
use abi::role::identity::AccountNumber;
use store::{PageRequest, PageResponse};

use crate::agents::{configured, harnessed_agent, thread};
use crate::conversations::current;
use crate::pictures::Picture;
use crate::state::{
    ARMED, ENDING, MESSAGE_WAITS, THREADS, WAIT_COUNT, WAIT_WAKES, WAITING, WAITS, WAKE_WAITS,
    WATCHED,
};
use crate::turns::{converse, say_from};
use crate::woken::{self, Arming, Call};
use crate::{Asking, Body, Ended, Place, ThreadRow, Until, WaitFor, WaitRow, listening};

pub(crate) fn what(event: &WaitFor) -> String {
    match event {
        WaitFor::Height { height } => format!("block {height}"),
        WaitFor::Pod { name } => format!("pod {name} to end"),
        WaitFor::Message {
            channel,
            from,
            mentions_me,
        } => {
            let who = from.map_or("anyone".to_owned(), |from| format!("account {from}"));
            let to_you = if *mentions_me {
                " addressed to you"
            } else {
                ""
            };
            format!("a message{to_you} from {who} in chat channel {channel}")
        }
        WaitFor::Change { repo, n } => format!("change {n} in {repo} to move, close or merge"),
        WaitFor::Ref { repo, reference } => format!("{reference} in {repo} to move"),
        WaitFor::File { path } => format!("{path} to change in files"),
        WaitFor::Settled { repo, n, until } => {
            let how = match until {
                Until::Merged => "be merged",
                Until::Closed => "be closed",
                Until::Either => "be merged or closed",
            };
            format!("change {n} in {repo} to {how}")
        }
        WaitFor::Clear { channel, thread } => {
            let place = match thread {
                Some(root) => Place::Thread {
                    channel: channel.clone(),
                    root: *root,
                },
                None => Place::Channel {
                    channel: channel.clone(),
                },
            };
            format!("{} to clear", place.words())
        }
    }
}

fn kept(ctx: &QueryCtx, wait: u64) -> Result<WaitRow, Error> {
    WAITS
        .get(ctx, &wait)?
        .ok_or_else(|| not_found(format!("no wait {wait}")))
}

fn open(ctx: &QueryCtx, wait: u64) -> Result<Option<WaitRow>, Error> {
    Ok(WAITS.get(ctx, &wait)?.filter(|row| row.ended.is_none()))
}

fn normalized(event: WaitFor) -> WaitFor {
    match event {
        WaitFor::Ref { repo, reference } if !reference.starts_with("refs/") => WaitFor::Ref {
            repo,
            reference: format!("refs/heads/{reference}"),
        },
        other => other,
    }
}

fn waking(event: &WaitFor) -> Option<wake::WaitFor> {
    let until = |until: &Until| match until {
        Until::Merged => wake::Until::Merged,
        Until::Closed => wake::Until::Closed,
        Until::Either => wake::Until::Either,
    };
    Some(match event {
        WaitFor::Height { height } => wake::WaitFor::Height { height: *height },
        WaitFor::Pod { name } => wake::WaitFor::Pod { name: name.clone() },
        WaitFor::Message { .. } => return None,
        WaitFor::Change { repo, n } => wake::WaitFor::Change {
            repo: repo.clone(),
            n: *n,
        },
        WaitFor::Ref { repo, reference } => wake::WaitFor::Ref {
            repo: repo.clone(),
            reference: reference.clone(),
        },
        WaitFor::File { path } => wake::WaitFor::File { path: path.clone() },
        WaitFor::Settled {
            repo,
            n,
            until: how,
        } => wake::WaitFor::Settled {
            repo: repo.clone(),
            n: *n,
            until: until(how),
        },
        WaitFor::Clear { channel, thread } => wake::WaitFor::Clear {
            channel_id: channel.clone(),
            thread: *thread,
        },
    })
}

async fn arm(ctx: &Ctx, row: &WaitRow, event: wake::WaitFor) -> Result<(), Error> {
    let call = format!("wait {}", row.id);
    let arming = Call {
        agent: row.agent,
        thread: row.thread,
        call: &call,
    };
    woken::sending(ctx, &arming, &Some(Arming::On));
    let on = wake::Op::WakeOn { event };
    let armed = ctx
        .call_raw_as(row.agent, wake::PROGRAM, abi::encode(&on))
        .await;
    let woke = armed.and_then(|output| {
        abi::decode::<u64>(&output)
            .map_err(|fault| unexpected_reply(wake::PROGRAM, "WakeOn", &fault.sentence))
    });
    if let Ok(woke) = &woke {
        WAIT_WAKES.put(ctx, &row.id, woke);
        WAKE_WAITS.put(ctx, woke, &row.id);
    }
    woken::ended(ctx, &arming)?;
    woke.map(|_| ())
}

fn listen_for(ctx: &ExecCtx, row: &WaitRow) -> Result<(), Error> {
    let WaitFor::Message { channel, .. } = &row.event else {
        return Ok(());
    };
    if !listening::exists(ctx, channel)? {
        return Err(not_found(format!("chat holds no channel {channel}")));
    }
    MESSAGE_WAITS.insert(ctx, &(row.agent, (channel.clone(), row.id)));
    WATCHED.insert(ctx, &row.id);
    listening::watch(ctx, row.agent, channel, true);
    Ok(())
}

pub(crate) async fn wait(ctx: &Ctx, thread_id: u64, event: WaitFor) -> Result<(), Error> {
    let agent = ctx.sender_account()?;
    let row = thread(ctx, thread_id)?;
    if row.agent != agent {
        return Err(invalid(format!(
            "conversation {thread_id} is not account {agent}'s: choose a conversation owned by the acting account"
        )));
    }
    if row.closed {
        return Err(wrong_state(format!(
            "conversation {thread_id} is closed: open a new conversation before waiting"
        )));
    }
    let event = normalized(event);
    let id = WAIT_COUNT.get(ctx)?.unwrap_or(0) + 1;
    WAIT_COUNT.put(ctx, &id);
    let waiting = WaitRow {
        id,
        agent,
        thread: thread_id,
        made: ctx.env().height,
        event,
        ended: None,
    };
    WAITS.put(ctx, &id, &waiting);
    WAITING.insert(ctx, &(agent, id));
    ARMED.insert(ctx, &(thread_id, id));
    match waking(&waiting.event) {
        Some(event) => arm(ctx, &waiting, event).await?,
        None => listen_for(ctx, &waiting)?,
    }
    let in_turn = thread(ctx, thread_id)?.asking != Asking::Idle;
    if in_turn {
        ENDING.insert(ctx, &thread_id);
    }
    ctx.set_return_data(abi::encode(&id));
    Ok(())
}

pub(crate) async fn unwait(ctx: &Ctx, wait: u64) -> Result<(), Error> {
    let row = kept(ctx, wait)?;
    let theirs = ctx.sender_account()? == row.agent;
    if !theirs {
        return Err(unauthorized(format!(
            "wait {wait} is account {}'s to cancel",
            row.agent
        )));
    }
    if let Some(ended) = &row.ended {
        return Err(wrong_state(format!(
            "wait {wait} ended at block {}: create a new wait instead of cancelling the ended one",
            ended.height
        )));
    }
    if let Some(woke) = WAIT_WAKES.get(ctx, &wait)? {
        match wake_row(ctx, woke)? {
            Some(wake::WakeRow { ended: None, .. }) => {
                let cancel = wake::Op::Cancel { id: woke };
                ctx.call_raw_as(row.agent, wake::PROGRAM, abi::encode(&cancel))
                    .await?;
            }
            Some(wake::WakeRow {
                ended: Some(ended), ..
            }) => return fire(ctx, wait, ended.text).await,
            None => {}
        }
    }
    disarm(ctx, &row)?;
    WAITS.remove(ctx, &wait);
    WAITING.remove(ctx, &(row.agent, wait));
    Ok(())
}

fn wake_row(ctx: &QueryCtx, woke: u64) -> Result<Option<wake::WakeRow>, Error> {
    let asked = wake::Query::Wake { id: woke };
    match ctx.query(wake::PROGRAM, &asked)? {
        wake::Reply::Wake(row) => Ok(row),
        other => Err(unexpected_reply(wake::PROGRAM, "Wake", &other)),
    }
}

fn disarm(ctx: &ExecCtx, row: &WaitRow) -> Result<(), Error> {
    ARMED.remove(ctx, &(row.thread, row.id));
    if let Some(woke) = WAIT_WAKES.get(ctx, &row.id)? {
        WAIT_WAKES.remove(ctx, &row.id);
        WAKE_WAITS.remove(ctx, &woke);
        return Ok(());
    }
    let WaitFor::Message { channel, .. } = &row.event else {
        return Ok(());
    };
    MESSAGE_WAITS.remove(ctx, &(row.agent, (channel.clone(), row.id)));
    let watched = WATCHED.has(ctx, &row.id);
    WATCHED.remove(ctx, &row.id);
    unwatch(ctx, row.agent, channel, watched)
}

pub(crate) fn end(ctx: &ExecCtx, wait: u64, what: String) -> Result<Option<u64>, Error> {
    ended(ctx, wait, what, Vec::new(), None, None)
}

pub(crate) fn end_seen(
    ctx: &ExecCtx,
    wait: u64,
    what: String,
    pictures: Vec<Picture>,
    room: &str,
    place: Option<String>,
) -> Result<Option<u64>, Error> {
    ended(ctx, wait, what, pictures, Some(room), place)
}

fn ended(
    ctx: &ExecCtx,
    wait: u64,
    what: String,
    pictures: Vec<Picture>,
    room: Option<&str>,
    place: Option<String>,
) -> Result<Option<u64>, Error> {
    let Some(mut row) = open(ctx, wait)? else {
        return Ok(None);
    };
    row.ended = Some(Ended {
        height: ctx.env().height,
        text: what.clone(),
    });
    WAITS.put(ctx, &wait, &row);
    disarm(ctx, &row)?;
    if let Some(mut closed) = THREADS.get(ctx, &row.thread)?.filter(|told| told.closed) {
        let text = format!("Wait {wait} is over: {what}");
        crate::bookings::undelivered(ctx, &mut closed, text);
        return Ok(None);
    }
    let Some(mut told) = told(ctx, &row)? else {
        return Ok(None);
    };
    let still = still_armed(ctx, told.id)?;
    let stop = if what.contains('\n') { "" } else { "." };
    let text = format!("Wait {wait} is over: {what}{stop}{still}");
    if let Some(room) = room {
        crate::room_rules::tell(ctx, &mut told, room)?;
    }
    let author = Principal::Root;
    say_from(ctx, &mut told, Body::Post { author, text }, pictures, place);
    THREADS.put(ctx, &told.id, &told);
    Ok(Some(told.id))
}

pub(crate) fn woken(
    ctx: &ExecCtx,
    woke: u64,
    wait: u64,
    change: &attribution::Change,
) -> Result<Option<u64>, Error> {
    if change.kind != attribution::Kind::Added {
        return Ok(None);
    }
    let asked = wake::Query::Wake { id: woke };
    let what = match ctx.query(wake::PROGRAM, &asked)? {
        wake::Reply::Wake(Some(wake::WakeRow {
            ended: Some(ended), ..
        })) => ended.text,
        wake::Reply::Wake(_) => String::from_utf8_lossy(&change.detail).into_owned(),
        other => return Err(unexpected_reply(wake::PROGRAM, "Wake", &other)),
    };
    end(ctx, wait, what)
}

pub(crate) fn cancelled(ctx: &ExecCtx, woke: u64) -> Result<(), Error> {
    let Some(wait) = WAKE_WAITS.get(ctx, &woke)? else {
        return Ok(());
    };
    let Some(row) = open(ctx, wait)? else {
        return Ok(());
    };
    disarm(ctx, &row)?;
    WAITS.remove(ctx, &wait);
    WAITING.remove(ctx, &(row.agent, wait));
    Ok(())
}

async fn fire(ctx: &Ctx, wait: u64, what: String) -> Result<(), Error> {
    if let Some(thread) = end(ctx, wait, what)? {
        converse(ctx, thread).await;
    }
    Ok(())
}

fn told(ctx: &QueryCtx, row: &WaitRow) -> Result<Option<ThreadRow>, Error> {
    match THREADS.get(ctx, &row.thread)?.filter(|told| !told.closed) {
        Some(told) => Ok(Some(told)),
        None => current(ctx, row.agent),
    }
}

pub(crate) fn message_waits(
    ctx: &QueryCtx,
    agent: AccountNumber,
) -> Result<Vec<(String, u64)>, Error> {
    let waits = MESSAGE_WAITS.scan(ctx, MESSAGE_WAITS.prefix_of(&agent))?;
    Ok(waits.into_iter().map(|(_, waited)| waited).collect())
}

fn unwatch(ctx: &ExecCtx, agent: AccountNumber, channel: &str, watched: bool) -> Result<(), Error> {
    let needed = message_waits(ctx, agent)?
        .iter()
        .any(|(waited, _)| waited == channel)
        || crate::runs::open_in(ctx, agent, channel)?;
    if needed {
        return Ok(());
    }
    if watched {
        listening::watch(ctx, agent, channel, false);
        return Ok(());
    }
    let Some(harnessed) = harnessed_agent(ctx, agent)? else {
        return Ok(());
    };
    let configured = configured(ctx, &harnessed.path)?;
    let named = configured.config().is_some_and(|(_, config)| {
        config
            .listen
            .iter()
            .any(|entry| listening::names(entry, channel))
    });
    if !named {
        listening::tell(ctx, agent, channel, false);
    }
    Ok(())
}

pub(crate) fn still_armed(ctx: &QueryCtx, thread: u64) -> Result<String, Error> {
    let mut armed = Vec::new();
    for (_, id) in ARMED.scan(ctx, ARMED.prefix_of(&thread))? {
        if let Some(row) = open(ctx, id)? {
            armed.push(format!("wait {id} for {}", what(&row.event)));
        }
    }
    Ok(match armed.is_empty() {
        true => String::new(),
        false => format!("\n(This conversation still waits: {}.)", armed.join("; ")),
    })
}

pub(crate) fn waits(
    ctx: &QueryCtx,
    agent: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<WaitRow>, Error> {
    WAITING.range_of(ctx, &agent, page)?.try_map(|(_, id)| {
        WAITS
            .get(ctx, &id)?
            .ok_or_else(|| corrupt("waiting", &id.to_be_bytes(), "names a wait that is gone"))
    })
}

pub(crate) fn armed(
    ctx: &QueryCtx,
    thread: u64,
    page: &PageRequest,
) -> Result<PageResponse<WaitRow>, Error> {
    ARMED.range_of(ctx, &thread, page)?.try_map(|(_, id)| {
        WAITS
            .get(ctx, &id)?
            .ok_or_else(|| corrupt("armed", &id.to_be_bytes(), "names a wait that is gone"))
    })
}

pub(crate) fn wait_row(ctx: &QueryCtx, wait: u64) -> Result<Option<WaitRow>, Error> {
    WAITS.get(ctx, &wait)
}
