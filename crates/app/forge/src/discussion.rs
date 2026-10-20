//! Chat owns conversations: forge opens a change's channel, or talks in its job's room, posts
//! its system lines there, and asks chat about the replies a review drew.

use ::program::{Error, ExecCtx, QueryCtx, unexpected_reply};
use ::program::{MessageId, Outcome, Principal};
use chat::{Block, MsgRow, Op, PostPolicy, Query, Reply};

use crate::Change;
use crate::contract::Revision;
use crate::state::{ARCHIVING, LOGGING, channel_id, load_change, next_message};

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

// Change `n`'s room: the job room `j-NNN` its `j-NNN/…` branch names while chat holds that room
// unarchived and open to post, so forge's lines there are not refused as it opens; else its own
// `forge:<repo>:<n>`. The change keeps the room it opened in.
pub fn room(ctx: &QueryCtx, repo: &str, n: u64, from: &Revision) -> Result<String, Error> {
    let Some(job) = job_of(from) else {
        return Ok(channel_id(repo, n));
    };
    let query = Query::Channel {
        channel_id: job.clone(),
    };
    let open = match ctx.query::<Query, Reply>(chat::PROGRAM, &query)? {
        Reply::Channel(info) => {
            info.is_some_and(|info| !info.channel.archived && !info.channel.members_only())
        }
        other => return Err(unexpected_reply(chat::PROGRAM, "Channel", &other)),
    };
    Ok(match open {
        true => job,
        false => channel_id(repo, n),
    })
}

// The job a branch is for: `j-<digits>`, the first segment of a `refs/heads/` name with more
// after it (`refs/heads/j-101/folders`).
fn job_of(from: &Revision) -> Option<String> {
    let Revision::Ref(name) = from else {
        return None;
    };
    let name = std::str::from_utf8(name.strip_prefix(b"refs/heads/")?).ok()?;
    let (job, _) = name.split_once('/')?;
    let digits = job.strip_prefix("j-")?;
    let numbered = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
    numbered.then(|| job.to_owned())
}

// Whether the change talks in its job's room rather than a room of its own.
pub fn in_job_room(repo: &str, change: &Change) -> bool {
    change.channel != channel_id(repo, change.n)
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

// Posts one system line into the change's room, in this frame: the event's code in a `forge`
// block, no name and no sentence. In a job room the code names the change after it (`opened
// programs 213`), and chat's answer is absorbed (`replied`): the room may have been archived
// since the change opened, which never fails the op.
pub fn post(ctx: &ExecCtx, repo: &str, change: &Change, message_id: String, event: Event) {
    let job_room = in_job_room(repo, change);
    let text = match job_room {
        true => format!("{} {repo} {}", event.code(), change.n),
        false => event.code(),
    };
    let line = Op::PostMessage {
        channel_id: change.channel.clone(),
        message_id,
        blocks: vec![Block::Code {
            lang: Some(crate::PROGRAM.into()),
            text,
        }],
        thread: None,
    };
    if job_room {
        let posted = ctx.emit(chat::PROGRAM, &line, ::program::Reply::Wanted);
        LOGGING.insert(ctx, &posted.seq);
        return;
    }
    emit(ctx, line);
}

// Archives the change's channel. chat's answer comes back to `replied`, so its refusal never
// fails the op that ended the change. A job room outlives its changes: it is left as it is.
pub fn archive(ctx: &ExecCtx, repo: &str, change: &Change) {
    if in_job_room(repo, change) {
        return;
    }
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
// in the change's channel; chat's answer to that line, or to any line in a job room, is absorbed
// whatever it is (the room may be gone or archived); and a watcher's answer is absorbed.
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
            channel_id: load_change(ctx, &repo, n)?.channel,
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
