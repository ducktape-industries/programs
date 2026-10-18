//! An agent's conversations: a canonical thread that hears every input whose place no other is
//! bound to, and threads anyone binds to a chat channel or thread (`OpenThread`). A canonical
//! thread is renewed or forked by its manager, and closes once idle and no longer current.
use ::program::{Error, ExecCtx, Principal, QueryCtx, already_exists, not_found, unexpected_reply};
use abi::role::identity::AccountNumber;
use store::PageRequest;

use crate::agents::{Agent, agent_named, managed, thread};
use crate::state::{
    BOUND, BY_AGENT, Binding, COUNT, CURRENT, FRAMES, PLACES, THREADS, TOPICS, WITHHELD,
    newest_first,
};
use crate::{Asking, Body, Context, Frame, Place, Role, Said, ThreadRow};

pub(crate) fn current(ctx: &QueryCtx, agent: AccountNumber) -> Result<Option<ThreadRow>, Error> {
    let Some(id) = CURRENT.get(ctx, &agent)? else {
        return adopted(ctx, agent);
    };
    let row = thread(ctx, id)?;
    Ok((!row.closed).then_some(row))
}

pub(crate) fn binding(ctx: &QueryCtx, row: &ThreadRow) -> Result<Option<Place>, Error> {
    let bound = BOUND.get(ctx, &(row.agent, row.place.clone()))?;
    Ok(bound
        .filter(|bound| bound.thread == row.id)
        .map(|bound| bound.place))
}

fn adopted(ctx: &QueryCtx, agent: AccountNumber) -> Result<Option<ThreadRow>, Error> {
    let mut newest: Option<ThreadRow> = None;
    for (_, key) in BY_AGENT.scan(ctx, BY_AGENT.prefix_of(&agent))? {
        let row = thread(ctx, u64::MAX - key)?;
        let canonical = binding(ctx, &row)?.is_none();
        let open_and_idle = !row.closed && row.asking == Asking::Idle;
        let later = newest
            .as_ref()
            .is_none_or(|newest| row.last_height > newest.last_height);
        if canonical && open_and_idle && later {
            newest = Some(row);
        }
    }
    Ok(newest)
}

pub(crate) fn conversation(ctx: &ExecCtx, agent: &Agent, place: &str) -> Result<ThreadRow, Error> {
    match current(ctx, agent.number)? {
        Some(row) => {
            CURRENT.put(ctx, &agent.number, &row.id);
            Ok(row)
        }
        None => open(ctx, agent, place.to_owned(), &Opening::Fresh),
    }
}

pub(crate) fn route(
    ctx: &ExecCtx,
    agent: &Agent,
    places: &[String],
    place: String,
) -> Result<(ThreadRow, String), Error> {
    for key in places {
        let Some(bound) = BOUND.get(ctx, &(agent.number, key.clone()))? else {
            continue;
        };
        let row = thread(ctx, bound.thread)?;
        if !row.closed {
            return Ok((row, key.clone()));
        }
    }
    let row = conversation(ctx, agent, &place)?;
    Ok((row, place))
}

enum Opening {
    Fresh,
    Fork { from: u64 },
    Bound(Place),
}

fn open(
    ctx: &ExecCtx,
    agent: &Agent,
    place: String,
    opening: &Opening,
) -> Result<ThreadRow, Error> {
    let id = COUNT.get(ctx)?.unwrap_or(0) + 1;
    let title = match opening {
        Opening::Fresh => format!("conversation {id}"),
        Opening::Fork { from } => format!("conversation {id}, forked from conversation {from}"),
        Opening::Bound(bound) => format!("conversation {id}, bound to {}", bound.words()),
    };
    let row = ThreadRow {
        id,
        agent: agent.number,
        manager: agent.manager,
        place,
        title,
        opened: ctx.env().height,
        closed: false,
        frames: 0,
        last_input: 0,
        seen: 0,
        last_stop: None,
        asking: Asking::Idle,
        open_calls: 0,
        held: 0,
        sealed: 0,
        base: 1,
        tail_from: 1,
        summary: None,
        context: Context::Measured(0),
        compact_before: None,
        last_height: 0,
        last_time: 0,
    };
    COUNT.put(ctx, &id);
    BY_AGENT.insert(ctx, &(agent.number, newest_first(id)));
    match opening {
        Opening::Fresh | Opening::Fork { .. } => CURRENT.put(ctx, &agent.number, &id),
        Opening::Bound(bound) => {
            let binding = Binding {
                thread: id,
                place: bound.clone(),
            };
            BOUND.put(ctx, &(agent.number, bound.key()), &binding);
        }
    }
    Ok(row)
}

pub(crate) fn retire(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    let mut row = thread(ctx, id)?;
    if binding(ctx, &row)?.is_some() {
        return Ok(());
    }
    let superseded = CURRENT.get(ctx, &row.agent)? != Some(id);
    let idle = row.asking == Asking::Idle;
    if superseded && idle && !row.closed {
        row.closed = true;
        THREADS.put(ctx, &id, &row);
    }
    Ok(())
}

pub(crate) fn new_thread(ctx: &ExecCtx, number: AccountNumber) -> Result<(), Error> {
    let agent = managed(ctx, number)?;
    let previous = current(ctx, number)?;
    let row = open(ctx, &agent, String::new(), &Opening::Fresh)?;
    THREADS.put(ctx, &row.id, &row);
    if let Some(previous) = previous {
        retire(ctx, previous.id)?;
    }
    ctx.set_return_data(abi::encode(&row.id));
    Ok(())
}

pub(crate) fn fork_thread(ctx: &ExecCtx, number: AccountNumber) -> Result<(), Error> {
    let agent = managed(ctx, number)?;
    let Some(from) = current(ctx, number)? else {
        return Err(not_found(format!(
            "agent {number} has no conversation to fork"
        )));
    };
    let opening = Opening::Fork { from: from.id };
    let mut row = open(ctx, &agent, from.place.clone(), &opening)?;
    copied(ctx, &from, &mut row)?;
    THREADS.put(ctx, &row.id, &row);
    retire(ctx, from.id)?;
    ctx.set_return_data(abi::encode(&row.id));
    Ok(())
}

pub(crate) fn open_thread(ctx: &ExecCtx, number: AccountNumber, place: Place) -> Result<(), Error> {
    let agent = agent_named(ctx, number)?;
    located(ctx, &agent, &place)?;
    let key = place.key();
    if let Some(bound) = BOUND.get(ctx, &(number, key.clone()))? {
        return Err(already_exists(format!(
            "agent {number} already has conversation {} open in {}; close it first",
            bound.thread,
            place.words()
        )));
    }
    let row = open(ctx, &agent, key.clone(), &Opening::Bound(place))?;
    THREADS.put(ctx, &row.id, &row);
    PLACES.put(ctx, &(number, key), &row.id);
    ctx.set_return_data(abi::encode(&row.id));
    Ok(())
}

fn located(ctx: &QueryCtx, agent: &Agent, place: &Place) -> Result<(), Error> {
    let channel = place.channel().to_owned();
    let asked = chat::Query::Channel {
        channel_id: channel.clone(),
    };
    let found = match ctx.query(chat::PROGRAM, &asked)? {
        chat::Reply::Channel(found) => found,
        other => return Err(unexpected_reply(chat::PROGRAM, "Channel", &other)),
    };
    if found.is_none() {
        return Err(not_found(format!("there is no chat channel {channel}")));
    }
    let Place::Thread { root, .. } = place else {
        return Ok(());
    };
    let asked = chat::Query::Thread {
        channel_id: channel.clone(),
        root_seq: *root,
        viewer: vec![Principal::Account(agent.number)],
        page: PageRequest::default(),
    };
    let opened = match ctx.query(chat::PROGRAM, &asked)? {
        chat::Reply::Thread { root, .. } => root,
        other => return Err(unexpected_reply(chat::PROGRAM, "Thread", &other)),
    };
    let is_root = opened.is_some_and(|row| row.thread.is_none());
    if !is_root {
        return Err(not_found(format!(
            "message {root} is no thread's first message in chat channel {channel}"
        )));
    }
    Ok(())
}

fn whole(ctx: &ExecCtx, id: u64) -> Result<Vec<Frame>, Error> {
    let frames = FRAMES.scan(ctx, FRAMES.prefix_of(&id))?;
    Ok(frames.into_iter().map(|(_, frame)| frame).collect())
}

fn answered(frames: &[Frame]) -> usize {
    let Some(at) = frames
        .iter()
        .rposition(|frame| matches!(frame.body, Body::Turn { .. }))
    else {
        return frames.len();
    };
    let Body::Turn { said, .. } = &frames[at].body else {
        return frames.len();
    };
    let results: Vec<&str> = frames[at + 1..]
        .iter()
        .filter_map(|frame| match &frame.body {
            Body::Result { call, .. } | Body::Refused { call, .. } => Some(call.as_str()),
            Body::Post { .. }
            | Body::Turn { .. }
            | Body::Notice { .. }
            | Body::Compaction { .. } => None,
        })
        .collect();
    let every_call_back = said.iter().all(|said| match said {
        Said::ToolUse { id, .. } => results.contains(&id.as_str()),
        Said::Text(_)
        | Said::Thinking { .. }
        | Said::RedactedThinking(_)
        | Said::ServerToolUse { .. }
        | Said::ServerToolResult(_)
        | Said::Cited { .. } => true,
    });
    match every_call_back {
        true => frames.len(),
        false => at,
    }
}

fn copied(ctx: &ExecCtx, from: &ThreadRow, row: &mut ThreadRow) -> Result<(), Error> {
    let mut frames = whole(ctx, from.id)?;
    frames.truncate(answered(&frames));
    let cut = frames.last().map_or(0, |frame| frame.seq);
    for frame in &frames {
        FRAMES.put(ctx, &(row.id, frame.seq), frame);
        if WITHHELD.has(ctx, &(from.id, frame.seq)) {
            WITHHELD.insert(ctx, &(row.id, frame.seq));
        }
    }
    let inputs = frames
        .iter()
        .filter(|frame| frame.body.role() == Some(Role::User));
    let mut stops = frames.iter().filter_map(|frame| match &frame.body {
        Body::Turn { stop, .. } => Some(*stop),
        Body::Post { .. }
        | Body::Refused { .. }
        | Body::Result { .. }
        | Body::Notice { .. }
        | Body::Compaction { .. } => None,
    });
    // a fork goes on with what it was about, until its agent says otherwise
    if let Some(topic) = TOPICS.get(ctx, &from.id)? {
        TOPICS.put(ctx, &row.id, &topic);
    }
    let summary = from.summary.filter(|seq| *seq <= cut);
    row.frames = cut;
    row.last_input = inputs.map(|frame| frame.seq).next_back().unwrap_or(0);
    row.seen = from.seen.min(cut);
    row.last_stop = stops.next_back();
    row.summary = summary;
    row.base = summary.map_or(1, |_| from.base);
    row.tail_from = row.base;
    row.context = from.context;
    row.last_height = ctx.env().height;
    row.last_time = ctx.env().time;
    Ok(())
}
