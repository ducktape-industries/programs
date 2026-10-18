//! An agent's `listen` setting kept in chat: at each model call harness starts the agent listening
//! to each channel its configuration names that chat holds, and stops it where it names none.
use ::program::{Error, ExecCtx, QueryCtx, unexpected_reply};
use abi::role::identity::AccountNumber;
use store::PageRequest;

use crate::agents::Agent;
use crate::calls::refusal;
use crate::turns::append;
use crate::{Body, ThreadRow};

pub(crate) fn mirror(ctx: &ExecCtx, thread: &mut ThreadRow, agent: &Agent, listen: &[String]) {
    if let Err(error) = mirrored(ctx, agent.number, listen) {
        let text = format!(
            "the channels the agent listens to were not matched to its configuration: {}",
            refusal(&error)
        );
        append(ctx, thread, Body::Notice { text });
    }
}

fn mirrored(ctx: &ExecCtx, agent: AccountNumber, listen: &[String]) -> Result<(), Error> {
    let held = listened(ctx, agent)?;
    for channel in listen.iter().filter(|channel| !held.contains(channel)) {
        if exists(ctx, channel)? {
            tell(ctx, agent, channel, true);
        }
    }
    for channel in held.iter().filter(|channel| !listen.contains(channel)) {
        tell(ctx, agent, channel, false);
    }
    Ok(())
}

fn tell(ctx: &ExecCtx, agent: AccountNumber, channel: &str, listening: bool) {
    let op = chat::Op::Listen {
        channel_id: channel.to_owned(),
        listening,
    };
    ctx.emit_as(agent, chat::PROGRAM, &op, ::program::Reply::None);
}

fn exists(ctx: &QueryCtx, channel: &str) -> Result<bool, Error> {
    let asked = chat::Query::Channel {
        channel_id: channel.to_owned(),
    };
    match ctx.query(chat::PROGRAM, &asked)? {
        chat::Reply::Channel(found) => Ok(found.is_some()),
        other => Err(unexpected_reply(chat::PROGRAM, "Channel", &other)),
    }
}

fn listened(ctx: &QueryCtx, agent: AccountNumber) -> Result<Vec<String>, Error> {
    let mut channels = Vec::new();
    let mut after = None;
    loop {
        let asked = chat::Query::Listening {
            account: agent,
            page: PageRequest { after, limit: None },
        };
        let page = match ctx.query(chat::PROGRAM, &asked)? {
            chat::Reply::Listening(page) => page,
            other => return Err(unexpected_reply(chat::PROGRAM, "Listening", &other)),
        };
        channels.extend(page.items);
        after = page.next;
        if after.is_none() {
            return Ok(channels);
        }
    }
}
