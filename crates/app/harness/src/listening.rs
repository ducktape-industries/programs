//! What an agent hears of each chat room is its choice there, in chat (J-084): its `listen` seeds
//! each room it names once with every message (`j-*` every open room whose id starts `j-`), and its
//! message waits watch a room (`Op::Watch`) without moving that choice.
use ::program::{Error, ExecCtx, QueryCtx, unexpected_reply};
use abi::role::identity::AccountNumber;
use store::PageRequest;

use crate::agents::Agent;
use crate::calls::refusal;
use crate::state::SEEDED;
use crate::turns::append;
use crate::{Body, ThreadRow};

pub(crate) fn seed(ctx: &ExecCtx, thread: &mut ThreadRow, agent: &Agent, listen: &[String]) {
    if let Err(error) = seeded(ctx, agent.number, listen) {
        let text = format!(
            "the channels the agent's configuration names to listen to were not seeded: {}",
            refusal(&error)
        );
        append(ctx, thread, Body::Notice { text });
    }
}

// each room `listen` names that chat holds and harness never seeded for the agent starts it
// listening, unless something is chosen there already; from then on chat's choice stands
fn seeded(ctx: &ExecCtx, agent: AccountNumber, listen: &[String]) -> Result<(), Error> {
    let unseeded = |channel: &String| !SEEDED.has(ctx, &(agent, channel.clone()));
    let mut rooms = Vec::new();
    for channel in listen.iter().filter(|entry| !entry.ends_with('*')) {
        if unseeded(channel) && exists(ctx, channel)? {
            rooms.push(channel.clone());
        }
    }
    if listen.iter().any(|entry| entry.ends_with('*')) {
        for info in channels(ctx)? {
            let id = info.channel.id;
            let named = listen.iter().any(|entry| names(entry, &id));
            if named && !info.channel.archived && !rooms.contains(&id) && unseeded(&id) {
                rooms.push(id);
            }
        }
    }
    for channel in rooms {
        if chosen(ctx, agent, &channel)? == (false, None) {
            tell(ctx, agent, &channel, true);
        }
        SEEDED.insert(ctx, &(agent, channel));
    }
    Ok(())
}

// whether a `listen` entry names `channel`: one ending in `*` every room whose id starts with what
// comes before it, any other the room of that id
pub(crate) fn names(entry: &str, channel: &str) -> bool {
    match entry.strip_suffix('*') {
        Some(prefix) => channel.starts_with(prefix),
        None => entry == channel,
    }
}

// every room chat holds, a page at a time
fn channels(ctx: &QueryCtx) -> Result<Vec<chat::ChannelInfo>, Error> {
    let mut rooms = Vec::new();
    let mut after = None;
    loop {
        let asked = chat::Query::Channels {
            page: PageRequest { after, limit: None },
        };
        let page = match ctx.query(chat::PROGRAM, &asked)? {
            chat::Reply::Channels(page) => page,
            other => return Err(unexpected_reply(chat::PROGRAM, "Channels", &other)),
        };
        rooms.extend(page.items);
        after = page.next;
        if after.is_none() {
            return Ok(rooms);
        }
    }
}

pub(crate) fn tell(ctx: &ExecCtx, agent: AccountNumber, channel: &str, listening: bool) {
    let op = chat::Op::Listen {
        channel_id: channel.to_owned(),
        listening,
    };
    ctx.emit_as(agent, chat::PROGRAM, &op, ::program::Reply::None);
}

pub(crate) fn watch(ctx: &ExecCtx, agent: AccountNumber, channel: &str, watching: bool) {
    let op = chat::Op::Watch {
        channel_id: channel.to_owned(),
        watching,
    };
    ctx.emit_as(agent, chat::PROGRAM, &op, ::program::Reply::None);
}

pub(crate) fn exists(ctx: &QueryCtx, channel: &str) -> Result<bool, Error> {
    let asked = chat::Query::Channel {
        channel_id: channel.to_owned(),
    };
    match ctx.query(chat::PROGRAM, &asked)? {
        chat::Reply::Channel(found) => Ok(found.is_some()),
        other => Err(unexpected_reply(chat::PROGRAM, "Channel", &other)),
    }
}

// what the agent chose for `channel` in chat: whether it listens there, and what of it still tells
// it when cut
pub(crate) fn chosen(
    ctx: &QueryCtx,
    agent: AccountNumber,
    channel: &str,
) -> Result<(bool, Option<chat::Notices>), Error> {
    let asked = chat::Query::Chosen {
        account: agent,
        channel_id: channel.to_owned(),
    };
    match ctx.query(chat::PROGRAM, &asked)? {
        chat::Reply::Chosen { listening, notices } => Ok((listening, notices)),
        other => Err(unexpected_reply(chat::PROGRAM, "Chosen", &other)),
    }
}
