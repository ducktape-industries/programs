//! A chat room's rules (its `rules` meta: the text itself, or a files path whose file holds them),
//! told to a conversation ahead of the first message it hears from the room, and again once they
//! change or a summary may have left them out. Nothing enforces them: the agent reads them.
use ::program::{BlobId, Error, ExecCtx, Principal, QueryCtx, code, unexpected_reply};
use borsh::{BorshDeserialize, BorshSerialize};

use crate::state::RULES_TOLD;
use crate::turns::say;
use crate::{Body, ThreadRow};

// what a conversation was last told of a room's rules: the meta's text, or the file its path named
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Told {
    Text(String),
    File { path: String, file: BlobId },
}

// the room's rules, written into `thread` ahead of a message from the room, when they are not what
// the conversation was last told of them; rules removed since are told as gone
pub(crate) fn tell(ctx: &ExecCtx, thread: &mut ThreadRow, channel: &str) -> Result<(), Error> {
    let asked = chat::Query::Channel {
        channel_id: channel.to_owned(),
    };
    let info = match ctx.query(chat::PROGRAM, &asked)? {
        chat::Reply::Channel(info) => info,
        other => return Err(unexpected_reply(chat::PROGRAM, "Channel", &other)),
    };
    let Some(info) = info else {
        return Ok(());
    };
    let rules = info.meta.get(chat::RULES).map(|rules| rules.trim());
    let now = match rules.filter(|rules| !rules.is_empty()) {
        Some(rules) => Some(told(ctx, rules)?),
        None => None,
    };
    let key = (thread.id, channel.to_owned());
    if RULES_TOLD.get(ctx, &key)? == now {
        return Ok(());
    }
    let room = format!("chat channel {} ({channel})", info.channel.name);
    let text = match &now {
        Some(Told::Text(text)) => format!("Rules of {room}, for how to talk there:\n{text}"),
        Some(Told::File { path, .. }) => {
            let (_, bytes) = crate::agents::read(ctx, path, None)?;
            let text = String::from_utf8_lossy(&bytes);
            format!("Rules of {room}, for how to talk there (from {path}):\n{text}")
        }
        None => format!("The {room} has no rules now."),
    };
    match &now {
        Some(told) => RULES_TOLD.put(ctx, &key, told),
        None => RULES_TOLD.remove(ctx, &key),
    }
    say(
        ctx,
        thread,
        Body::Post {
            author: Principal::Root,
            text,
        },
    );
    Ok(())
}

// a value that starts with `/` and names a file files holds is that file; any other value, a path
// naming no file or no path files takes included, is the text itself
fn told(ctx: &QueryCtx, rules: &str) -> Result<Told, Error> {
    let text = || Told::Text(rules.to_owned());
    if !rules.starts_with('/') {
        return Ok(text());
    }
    let asked = files::Query::Stat {
        path: rules.to_owned(),
        at: None,
    };
    let entry = match ctx.query(files::PROGRAM, &asked) {
        Ok(files::Reply::Stat { entry, .. }) => entry,
        Ok(other) => return Err(unexpected_reply(files::PROGRAM, "Stat", &other)),
        Err(refused) if refused.code == code::INVALID_INPUT => return Ok(text()),
        Err(refused) => return Err(refused),
    };
    Ok(match entry.map(|entry| entry.node) {
        Some(files::Node::File { file, .. }) => Told::File {
            path: rules.to_owned(),
            file,
        },
        Some(files::Node::Dir { .. } | files::Node::Symlink { .. }) | None => text(),
    })
}

// a summary may leave the rules out: the next message from each room tells them again
pub(crate) fn forget(ctx: &ExecCtx, thread: u64) -> Result<(), Error> {
    for (key, _) in RULES_TOLD.scan(ctx, RULES_TOLD.prefix_of(&thread))? {
        RULES_TOLD.remove(ctx, &key);
    }
    Ok(())
}
