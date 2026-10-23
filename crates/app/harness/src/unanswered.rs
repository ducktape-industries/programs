//! Each agent's inputs said to it (a mention, a direct message, a thread reply) until it posts or
//! reacts in their place: what a conversation still holds when it closes goes to the agent and reaches
//! its live conversation for each place, in one input naming the conversation, its closer and block.
use std::collections::BTreeMap;

use ::program::{Error, ExecCtx, Principal, unexpected_reply};
use abi::role::identity::AccountNumber;

use crate::agents::{chat_places, running, sender_words, thread};
use crate::conversations::route;
use crate::resume;
use crate::state::{
    Closing, PLACES, THREADS, UNANSWERED, UNANSWERED_CLOSED, UNANSWERED_HELD, UNANSWERED_UNHEARD,
    Unanswered,
};
use crate::turns::say;
use crate::{Body, Op, ThreadRow};

pub(crate) fn kept(
    ctx: &ExecCtx,
    agent: AccountNumber,
    seq: u64,
    row: &chat::MsgRow,
    text: &str,
    at: u64,
) {
    let queued = Unanswered {
        channel: row.channel_id.clone(),
        thread: row.thread,
        text: text.to_owned(),
        held_by: Some(at),
    };
    UNANSWERED.put(ctx, &(agent, seq), &queued);
    UNANSWERED_HELD.insert(ctx, &(at, agent, seq));
}

pub(crate) fn closing(ctx: &ExecCtx, closed: u64) -> Result<(), Error> {
    let by = sender_words(ctx);
    let height = ctx.env().height;
    for (_, agent, seq) in UNANSWERED_HELD.scan(ctx, UNANSWERED_HELD.prefix_of(&closed))? {
        let closing = Closing {
            thread: closed,
            by: by.clone(),
            height,
        };
        UNANSWERED_CLOSED.put(ctx, &(agent, seq), &closing);
    }
    Ok(())
}

pub(crate) fn answered(
    ctx: &ExecCtx,
    agent: AccountNumber,
    channel: &str,
    thread: Option<u64>,
) -> Result<(), Error> {
    for ((_, seq), queued) in UNANSWERED.scan(ctx, UNANSWERED.prefix_of(&agent))? {
        let in_room = queued.channel == channel;
        let in_place = queued.thread.is_none_or(|root| Some(root) == thread);
        let here = in_room && in_place;
        if !here {
            continue;
        }
        UNANSWERED.remove(ctx, &(agent, seq));
        UNANSWERED_UNHEARD.remove(ctx, &(agent, seq));
        UNANSWERED_CLOSED.remove(ctx, &(agent, seq));
        if let Some(at) = queued.held_by {
            UNANSWERED_HELD.remove(ctx, &(at, agent, seq));
        }
    }
    Ok(())
}

pub(crate) fn reacted(
    ctx: &ExecCtx,
    agent: AccountNumber,
    channel: &str,
    seq: u64,
) -> Result<(), Error> {
    let asked = chat::Query::MessagesAround {
        channel_id: channel.to_owned(),
        seq,
        viewer: Vec::new(),
        page: store::PageRequest {
            after: None,
            limit: Some(0),
        },
    };
    let rows = match crate::reading::chat_for(ctx, agent, &asked)? {
        chat::Reply::Messages(rows) => rows,
        other => return Err(unexpected_reply(chat::PROGRAM, "MessagesAround", &other)),
    };
    let Some(row) = rows.into_iter().find(|row| row.seq == seq) else {
        return Ok(());
    };
    answered(ctx, agent, channel, row.thread)
}

pub(crate) fn moved(ctx: &ExecCtx, from: u64, to: u64) -> Result<(), Error> {
    for (_, agent, seq) in UNANSWERED_HELD.scan(ctx, UNANSWERED_HELD.prefix_of(&from))? {
        UNANSWERED_HELD.remove(ctx, &(from, agent, seq));
        UNANSWERED_HELD.insert(ctx, &(to, agent, seq));
        if let Some(mut queued) = UNANSWERED.get(ctx, &(agent, seq))? {
            queued.held_by = Some(to);
            UNANSWERED.put(ctx, &(agent, seq), &queued);
        }
    }
    Ok(())
}

pub(crate) fn returned(ctx: &ExecCtx, closed: &ThreadRow) -> Result<(), Error> {
    for (_, agent, seq) in UNANSWERED_HELD.scan(ctx, UNANSWERED_HELD.prefix_of(&closed.id))? {
        UNANSWERED_HELD.remove(ctx, &(closed.id, agent, seq));
        if let Some(mut queued) = UNANSWERED.get(ctx, &(agent, seq))? {
            queued.held_by = None;
            UNANSWERED.put(ctx, &(agent, seq), &queued);
            UNANSWERED_UNHEARD.insert(ctx, &(agent, seq));
        }
    }
    deliver(ctx, closed.agent)
}

pub(crate) fn deliver(ctx: &ExecCtx, agent: AccountNumber) -> Result<(), Error> {
    let unheard = UNANSWERED_UNHEARD.scan(ctx, UNANSWERED_UNHEARD.prefix_of(&agent))?;
    if unheard.is_empty() {
        return Ok(());
    }
    let Some(runs) = running(ctx, agent)? else {
        return Ok(());
    };
    let mut reached: BTreeMap<u64, Vec<(u64, Unanswered)>> = BTreeMap::new();
    for (_, seq) in unheard {
        let Some(queued) = UNANSWERED.get(ctx, &(agent, seq))? else {
            UNANSWERED_UNHEARD.remove(ctx, &(agent, seq));
            UNANSWERED_CLOSED.remove(ctx, &(agent, seq));
            continue;
        };
        let places = chat_places(&queued.channel, queued.thread);
        let place = format!("{}/{}", chat::PROGRAM, queued.channel);
        let (row, at) = route(ctx, &runs, &places, place)?;
        THREADS.put(ctx, &row.id, &row);
        PLACES.put(ctx, &(agent, at), &row.id);
        reached.entry(row.id).or_default().push((seq, queued));
    }
    for (id, items) in reached {
        let mut row = thread(ctx, id)?;
        let mut texts = Vec::new();
        for (seq, queued) in &items {
            let text = match UNANSWERED_CLOSED.get(ctx, &(agent, *seq))? {
                Some(closing) => format!(
                    "Heard in conversation {}, which {} closed at block {} before it answered:\n{}",
                    closing.thread, closing.by, closing.height, queued.text
                ),
                None => queued.text.clone(),
            };
            texts.push(text);
        }
        let text = format!(
            "notice queue Note: {} unanswered\n\n{}",
            items.len(),
            texts.join("\n\n")
        );
        let author = Principal::Root;
        say(ctx, &mut row, Body::Post { author, text });
        THREADS.put(ctx, &id, &row);
        resume::heard(ctx, id)?;
        for (seq, mut queued) in items {
            queued.held_by = Some(id);
            UNANSWERED.put(ctx, &(agent, seq), &queued);
            UNANSWERED_HELD.insert(ctx, &(id, agent, seq));
            UNANSWERED_UNHEARD.remove(ctx, &(agent, seq));
            UNANSWERED_CLOSED.remove(ctx, &(agent, seq));
        }
        let resume = Op::Resume { thread: id };
        ctx.emit(&ctx.env().program, &resume, ::program::Reply::None);
    }
    Ok(())
}
