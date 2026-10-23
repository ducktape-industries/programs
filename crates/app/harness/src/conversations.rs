//! An agent's conversations: a canonical thread that hears every input whose place no other is
//! bound to, and threads anyone binds to a place (`OpenThread`). The agent or its manager renews
//! the canonical one, its manager forks it, and it closes once idle and no longer current.
use ::program::{
    Error, ExecCtx, Principal, QueryCtx, already_exists, not_found, unauthorized, unexpected_reply,
};
use abi::role::identity::AccountNumber;
use forge_review_chat::{Query as MirrorQuery, Reply as MirrorReply};
use store::PageRequest;

use crate::agents::{Agent, agent_named, managed, thread};
use crate::state::{
    BOUND, BY_AGENT, Binding, CONVERSATIONS, COUNT, CURRENT, Conversations, FRAMES, HEIRS, OPENERS,
    PLACES, THREADS, TOPICS, VACATED, WITHHELD, newest_first,
};
use crate::{Asking, Body, Context, Frame, Place, Said, ThreadRow};

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
    if let Some(found) = bound_at(ctx, agent.number, places)? {
        return Ok(found);
    }
    if let Some(found) = opener(ctx, agent.number, places)? {
        return Ok(found);
    }
    if let Some((row, _)) = bound_at(ctx, agent.number, &mirrored(ctx, &place)?)? {
        return Ok((row, place));
    }
    let row = conversation(ctx, agent, &place)?;
    Ok((row, place))
}

pub(crate) fn opened_change(ctx: &ExecCtx, agent: AccountNumber, thread: u64, repo: &str, n: u64) {
    let room = Place::Channel {
        channel: forge::room_of(repo, n),
    };
    OPENERS.put(ctx, &(agent, room.key()), &thread);
}

fn opener(
    ctx: &QueryCtx,
    agent: AccountNumber,
    places: &[String],
) -> Result<Option<(ThreadRow, String)>, Error> {
    for key in places {
        let Some(id) = OPENERS.get(ctx, &(agent, key.clone()))? else {
            continue;
        };
        if let Some(row) = reaching(ctx, id)? {
            return Ok(Some((row, key.clone())));
        }
    }
    Ok(None)
}

fn mirrored(ctx: &QueryCtx, place: &str) -> Result<Vec<String>, Error> {
    let room = place
        .strip_prefix(chat::PROGRAM)
        .and_then(|rest| rest.strip_prefix('/'))
        .and_then(forge::change_of_room);
    let Some((repo, n)) = room else {
        return Ok(Vec::new());
    };
    let asked = MirrorQuery::Room {
        repo: repo.to_owned(),
        n,
    };
    let into = match ctx.query(forge_review_chat::PROGRAM, &asked) {
        Ok(MirrorReply::Room(into)) => into,
        Ok(_) | Err(_) => None,
    };
    Ok(into
        .map(|channel| Place::Channel { channel }.key())
        .into_iter()
        .collect())
}

pub(crate) fn bound_at(
    ctx: &QueryCtx,
    agent: AccountNumber,
    places: &[String],
) -> Result<Option<(ThreadRow, String)>, Error> {
    for key in places {
        let Some(bound) = BOUND.get(ctx, &(agent, key.clone()))? else {
            continue;
        };
        let row = thread(ctx, bound.thread)?;
        if !row.closed {
            return Ok(Some((row, key.clone())));
        }
    }
    Ok(None)
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
    crate::kept::opened(ctx, agent, id)?;
    BY_AGENT.insert(ctx, &(agent.number, newest_first(id)));
    crate::sessions::opened(ctx, agent.number, id);
    let predecessor = match opening {
        Opening::Fresh | Opening::Fork { .. } => {
            let replaced = CURRENT.get(ctx, &agent.number)?;
            CURRENT.put(ctx, &agent.number, &id);
            replaced
        }
        Opening::Bound(bound) => {
            let binding = Binding {
                thread: id,
                place: bound.clone(),
            };
            let key = (agent.number, bound.key());
            BOUND.put(ctx, &key, &binding);
            let vacated = VACATED.get(ctx, &key)?;
            VACATED.remove(ctx, &key);
            vacated
        }
    };
    if let Some(from) = predecessor {
        took_over(ctx, from, id)?;
    }
    Ok(row)
}

fn took_over(ctx: &ExecCtx, from: u64, to: u64) -> Result<(), Error> {
    HEIRS.put(ctx, &from, &to);
    crate::waits::carried(ctx, from, to)
}

pub(crate) fn heir(ctx: &QueryCtx, id: u64) -> Result<Option<ThreadRow>, Error> {
    let mut from = id;
    while let Some(next) = HEIRS.get(ctx, &from)? {
        let row = thread(ctx, next)?;
        if !row.closed {
            return Ok(Some(row));
        }
        from = next;
    }
    Ok(None)
}

pub(crate) fn reaching(ctx: &QueryCtx, id: u64) -> Result<Option<ThreadRow>, Error> {
    match THREADS.get(ctx, &id)? {
        Some(row) if !row.closed => Ok(Some(row)),
        Some(_) => heir(ctx, id),
        None => Ok(None),
    }
}

pub(crate) fn carry_to_heir(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    let Some(taken_by) = heir(ctx, id)? else {
        return Ok(());
    };
    crate::waits::carried(ctx, id, taken_by.id)
}

pub(crate) fn retire(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    let mut row = thread(ctx, id)?;
    if binding(ctx, &row)?.is_some() {
        return Ok(());
    }
    let superseded = CURRENT.get(ctx, &row.agent)? != Some(id);
    let idle = row.asking == Asking::Idle && row.open_calls == 0;
    if superseded && idle && !row.closed {
        row.closed = true;
        crate::sessions::closed(ctx, &row);
        crate::unanswered::closing(ctx, id)?;
        crate::turns::let_go(ctx, &mut row)?;
        THREADS.put(ctx, &id, &row);
    }
    Ok(())
}

pub(crate) fn claim(ctx: &ExecCtx, number: AccountNumber) -> Result<(), Error> {
    let agent = agent_named(ctx, number)?;
    let by = ctx.sender_account()?;
    if by != agent.manager {
        return Err(unauthorized(format!(
            "only agent {number}'s manager, account {}, claims who starts its conversations",
            agent.manager
        )));
    }
    if CONVERSATIONS.has(ctx, &number) {
        return Ok(());
    }
    let row = Conversations {
        manager: by,
        rev: 0,
    };
    CONVERSATIONS.put(ctx, &number, &row);
    Ok(())
}

fn touch_claimed_conversations(ctx: &ExecCtx, number: AccountNumber) -> Result<(), Error> {
    if let Some(mut row) = CONVERSATIONS.get(ctx, &number)? {
        row.rev += 1;
        CONVERSATIONS.put(ctx, &number, &row);
    }
    Ok(())
}

pub(crate) fn new_thread(ctx: &ExecCtx, number: AccountNumber) -> Result<(), Error> {
    let agent = managed(ctx, number)?;
    if !crate::gate::sent_by_agent(ctx, number) {
        touch_claimed_conversations(ctx, number)?;
    }
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
    touch_claimed_conversations(ctx, number)?;
    let Some(from) = current(ctx, number)? else {
        return Err(not_found(format!(
            "agent {number} has no conversation to fork: open a conversation before forking it"
        )));
    };
    let opening = Opening::Fork { from: from.id };
    let mut row = open(ctx, &agent, from.place.clone(), &opening)?;
    copied(ctx, &from, &mut row)?;
    crate::tallies::kept(ctx, row.id)?;
    THREADS.put(ctx, &row.id, &row);
    crate::unanswered::moved(ctx, from.id, row.id)?;
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

pub(crate) fn rebound(ctx: &ExecCtx, old: &ThreadRow, place: Place) -> Result<ThreadRow, Error> {
    let agent = agent_named(ctx, old.agent)?;
    let key = (old.agent, place.key());
    BOUND.remove(ctx, &key);
    VACATED.put(ctx, &key, &old.id);
    let row = open(ctx, &agent, place.key(), &Opening::Bound(place))?;
    THREADS.put(ctx, &row.id, &row);
    PLACES.put(ctx, &key, &row.id);
    crate::unanswered::moved(ctx, old.id, row.id)?;
    Ok(row)
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
    let opened = match crate::reading::chat_for(ctx, agent.number, &asked)? {
        chat::Reply::Thread { root, .. } => root,
        other => return Err(unexpected_reply(chat::PROGRAM, "Thread", &other)),
    };
    let is_root = opened.is_some_and(|row| row.thread.is_none());
    if !is_root {
        return Err(not_found(format!(
            "message {root} is no thread's first message in chat channel {channel}: read the thread and use its root message"
        )));
    }
    Ok(())
}

fn whole(ctx: &ExecCtx, id: u64) -> Result<Vec<Frame>, Error> {
    let frames = FRAMES.scan(ctx, FRAMES.prefix_of(&id))?;
    Ok(frames.into_iter().map(|(_, frame)| frame).collect())
}

fn answered(frames: &[Frame]) -> usize {
    let back: Vec<&str> = frames
        .iter()
        .filter_map(|frame| match &frame.body {
            Body::Result { call, .. } | Body::Refused { call, .. } | Body::Arrived { call, .. } => {
                Some(call.as_str())
            }
            Body::Post { .. }
            | Body::Light { .. }
            | Body::Turn { .. }
            | Body::Receipt { .. }
            | Body::Notice { .. }
            | Body::Compaction { .. } => None,
        })
        .collect();
    let still_out = |said: &Said| match said {
        Said::ToolUse { id, .. } => !back.contains(&id.as_str()),
        Said::Text(_)
        | Said::Thinking { .. }
        | Said::RedactedThinking(_)
        | Said::ServerToolUse { .. }
        | Said::ServerToolResult(_)
        | Said::Cited { .. } => false,
    };
    let waits = |frame: &Frame| match &frame.body {
        Body::Turn { said, .. } => said.iter().any(still_out),
        Body::Post { .. }
        | Body::Light { .. }
        | Body::Refused { .. }
        | Body::Result { .. }
        | Body::Receipt { .. }
        | Body::Arrived { .. }
        | Body::Notice { .. }
        | Body::Compaction { .. } => false,
    };
    frames.iter().position(waits).unwrap_or(frames.len())
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
    crate::pictures::forked(ctx, from.id, row.id, &frames)?;
    let inputs = frames.iter().filter(|frame| frame.body.input());
    let mut stops = frames.iter().filter_map(|frame| match &frame.body {
        Body::Turn { stop, .. } => Some(*stop),
        Body::Post { .. }
        | Body::Light { .. }
        | Body::Refused { .. }
        | Body::Result { .. }
        | Body::Receipt { .. }
        | Body::Arrived { .. }
        | Body::Notice { .. }
        | Body::Compaction { .. } => None,
    });
    if let Some(topic) = TOPICS.get(ctx, &from.id)? {
        TOPICS.put(ctx, &row.id, &topic);
    }
    let turned = frames
        .iter()
        .rev()
        .find(|frame| matches!(frame.body, Body::Turn { .. }));
    if let Some(turn) = turned {
        crate::state::LAST_TURNS.put(ctx, &row.id, &turn.height);
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
