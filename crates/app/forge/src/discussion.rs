//! Chat owns conversations: forge opens a change's channel, posts its
//! system lines into it, and asks chat about the replies a review drew.

use ::program::{Error, ExecCtx, QueryCtx, unexpected_reply};
use ::program::{MessageId, Outcome, Principal};
use chat::{Block, MsgRow, Op, PostPolicy, Query, Reply};

use crate::Change;
use crate::state::{ARCHIVING, LOGGING, channel_id, next_message};

/// What a system line marks in a change's timeline. The line carries this
/// event's code and nothing else: who acted and what they concluded live
/// on forge's records, which the view words in the reader's language.
pub enum Event {
    Opened,
    Closed,
    Merged,
    /// The review of this id; its line is the root its discussion hangs on.
    Reviewed(u64),
    // chat refused to archive the channel: its reason and sentence.
    ArchiveRefused(String),
}

impl Event {
    /// The code a `forge` block carries: `opened`, `closed`, `merged` or
    /// `review <id>`.
    fn code(&self) -> String {
        match self {
            Event::Opened => "opened".into(),
            Event::Closed => "closed".into(),
            Event::Merged => "merged".into(),
            Event::Reviewed(id) => format!("review {id}"),
            Event::ArchiveRefused(why) => format!("archive refused: {why}"),
        }
    }
}

/// Opens the change's channel: chat creates it in this frame, and its
/// refusal fails the op.
pub fn create(ctx: &ExecCtx, repo: &str, change: &Change) {
    emit(
        ctx,
        Op::CreateChannel {
            channel_id: change.channel.clone(),
            name: format!("{repo}#{}", change.n),
            post_policy: PostPolicy::Open,
        },
    );
}

/// Posts one system line into the change's channel, in this frame: the
/// event's code in a `forge` block, no name and no sentence.
pub fn post(ctx: &ExecCtx, change: &Change, message_id: String, event: Event) {
    emit(
        ctx,
        Op::PostMessage {
            channel_id: change.channel.clone(),
            message_id,
            blocks: vec![Block::Code {
                lang: Some(crate::PROGRAM.into()),
                text: event.code(),
            }],
            thread: None,
        },
    );
}

// Archives the change's channel. chat's answer comes back to `replied`, so its refusal never
// fails the op that ended the change.
pub fn archive(ctx: &ExecCtx, repo: &str, change: &Change) {
    let archive = Op::SetChannelArchived {
        channel_id: change.channel.clone(),
        archived: true,
    };
    let id = ctx.emit(chat::PROGRAM, &archive, ::program::Reply::Wanted);
    ARCHIVING.put(ctx, &id.seq, &(repo.to_owned(), change.n));
}

// Whether chat holds the change's channel and shows it unarchived.
pub fn unarchived(ctx: &QueryCtx, change: &Change) -> Result<bool, Error> {
    let query = Query::Channel {
        channel_id: change.channel.clone(),
    };
    match ctx.query::<Query, Reply>(chat::PROGRAM, &query)? {
        Reply::Channel(info) => Ok(info.is_some_and(|info| !info.channel.archived)),
        other => Err(unexpected_reply(chat::PROGRAM, "Channel", &other)),
    }
}

// The answer to a message forge asked one of. chat's refusal of an archive is absorbed and said
// in the change's channel; chat's answer to that line is absorbed whatever it is (the channel
// may be gone); and a watcher's answer is absorbed.
pub fn replied(ctx: &ExecCtx, id: &MessageId, outcome: &Outcome) -> Result<(), Error> {
    if LOGGING.has(ctx, &id.seq) {
        LOGGING.remove(ctx, &id.seq);
        return Ok(());
    }
    let Some((repo, n)) = ARCHIVING.get(ctx, &id.seq)? else {
        return Ok(());
    };
    ARCHIVING.remove(ctx, &id.seq);
    if let Outcome::Rejected(refusal) = outcome {
        let why = format!("{}: {}", refusal.code, refusal.message);
        let line = Op::PostMessage {
            channel_id: channel_id(&repo, n),
            message_id: next_message(ctx)?,
            blocks: vec![Block::Code {
                lang: Some(crate::PROGRAM.into()),
                text: Event::ArchiveRefused(why).code(),
            }],
            thread: None,
        };
        let posted = ctx.emit(chat::PROGRAM, &line, ::program::Reply::Wanted);
        LOGGING.insert(ctx, &posted.seq);
    }
    Ok(())
}

fn emit(ctx: &ExecCtx, message: Op) {
    ctx.emit(chat::PROGRAM, &message, ::program::Reply::None);
}

/// The chat root a review posted, by its message id.
pub fn message(ctx: &QueryCtx, id: &str) -> Result<Option<MsgRow>, Error> {
    let query = Query::MessageById {
        message_id: id.into(),
    };
    match ctx.query::<Query, Reply>(chat::PROGRAM, &query)? {
        Reply::Message(row) => Ok(row),
        other => Err(unexpected_reply(chat::PROGRAM, "MessageById", &other)),
    }
}

/// The newest thread in `channel` that `principal` started and someone answered.
pub fn attention(
    ctx: &QueryCtx,
    channel: &str,
    principal: &Principal,
) -> Result<Option<MsgRow>, Error> {
    let query = Query::ThreadAttention {
        channel_id: channel.into(),
        author: principal.clone(),
    };
    match ctx.query::<Query, Reply>(chat::PROGRAM, &query)? {
        Reply::Attention(row) => Ok(row),
        other => Err(unexpected_reply(chat::PROGRAM, "ThreadAttention", &other)),
    }
}
