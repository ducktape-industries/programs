//! Chat owns conversations: forge opens a change's channel, or talks in its job's room, posts
//! its notices there, and asks chat about the replies a review drew.

use std::collections::BTreeSet;

use ::program::{AccountNumber, Error, ExecCtx, QueryCtx, unexpected_reply};
use ::program::{MessageId, Outcome, Principal};
use chat::message::Status;
use chat::{MsgRow, Notice, Op, PostPolicy, Query, Reply};

use crate::Change;
use crate::contract::{Revision, Verdict};
use crate::state::{
    ARCHIVING, LOGGING, OPEN_CHANGES_BY_SOURCE, channel_id, load_change, load_repo, next_message,
};

pub const NOTICE: &str = "change";

pub fn notice_link(repo: &str, n: u64) -> Option<String> {
    ducklink::path(crate::PROGRAM, &[repo, &n.to_string()])
}

pub enum Event {
    Opened,
    Closed,
    Merged,
    Reviewed {
        id: u64,
        verdict: Verdict,
    },
    ArchiveRefused(String),
    LeftQueue {
        into: Vec<u8>,
        why: String,
        by: Principal,
    },
    NewHead {
        head: String,
    },
    Requested {
        reviewers: Vec<Principal>,
    },
}

impl Event {
    fn word(&self) -> String {
        match self {
            Event::Opened => "opened".into(),
            Event::Closed => "closed".into(),
            Event::Merged => "merged".into(),
            Event::Reviewed { id, .. } => format!("review {id}"),
            Event::ArchiveRefused(_) => "archive refused".into(),
            Event::LeftQueue { .. } => "left queue".into(),
            Event::NewHead { .. } => "head".into(),
            Event::Requested { .. } => "requested".into(),
        }
    }

    fn status(&self) -> Status {
        match self {
            Event::Opened => Status::Running,
            Event::Merged => Status::Passed,
            Event::Closed => Status::Note,
            Event::Reviewed { verdict, .. } => match verdict {
                Verdict::Approve => Status::Passed,
                Verdict::RequestChanges => Status::Failed,
                Verdict::Comment => Status::Note,
            },
            Event::ArchiveRefused(_) | Event::LeftQueue { .. } => Status::Failed,
            Event::NewHead { .. } | Event::Requested { .. } => Status::Running,
        }
    }

    fn detail(&self, change: &Change) -> String {
        match self {
            Event::Opened => "opened".into(),
            Event::Closed => "closed".into(),
            Event::Merged => format!("merged into {}", branch(&change.into)),
            Event::Reviewed { id, verdict } => {
                let said = match verdict {
                    Verdict::Approve => "approved",
                    Verdict::RequestChanges => "changes requested",
                    Verdict::Comment => "commented",
                };
                format!("review {id}: {said}")
            }
            Event::ArchiveRefused(why) => format!("archive refused: {why}"),
            Event::LeftQueue { into, why, .. } => {
                format!("left the merge queue of {}: {why}", branch(into))
            }
            Event::NewHead { head } => format!("new head {head}"),
            Event::Requested { .. } => "review requested".into(),
        }
    }

    fn concerns(
        &self,
        ctx: &QueryCtx,
        repo: &str,
        change: &Change,
    ) -> Result<Vec<Principal>, Error> {
        Ok(match self {
            Event::Opened if change.reviewers.is_empty() => vec![load_repo(ctx, repo)?.owner],
            Event::Opened => change.reviewers.clone(),
            Event::Closed | Event::Merged | Event::Reviewed { .. } => {
                std::iter::once(&change.author)
                    .chain(&change.reviewers)
                    .cloned()
                    .collect()
            }
            Event::ArchiveRefused(_) => change
                .closed_by
                .iter()
                .chain(&change.merged_by)
                .cloned()
                .collect(),
            Event::LeftQueue { by, .. } => vec![by.clone()],
            Event::NewHead { .. } => change.reviewers.clone(),
            Event::Requested { reviewers } => reviewers.clone(),
        })
    }
}

fn branch(name: &[u8]) -> String {
    let name = name.strip_prefix(b"refs/heads/").unwrap_or(name);
    String::from_utf8_lossy(name).into_owned()
}

fn accounts(who: Vec<Principal>, actor: Option<&Principal>) -> Vec<AccountNumber> {
    let acted = actor.and_then(Principal::account);
    let told: BTreeSet<AccountNumber> = who
        .iter()
        .filter_map(Principal::account)
        .filter(|number| Some(*number) != acted)
        .collect();
    told.into_iter().collect()
}

fn room_name(repo: &str, change: &Change) -> String {
    format!("pr/{repo}#{} {}", change.n, change.title)
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
            name: room_name(repo, change),
            post_policy: PostPolicy::Open,
        },
    );
}

pub fn post(
    ctx: &ExecCtx,
    repo: &str,
    change: &Change,
    message_id: String,
    event: Event,
    actor: Option<&Principal>,
) -> Result<(), Error> {
    let job_room = in_job_room(repo, change);
    let notice = Notice {
        kind: NOTICE.into(),
        group: format!("{} {repo} {}", event.word(), change.n),
        subject: format!("{repo} #{}: {}", change.n, change.title),
        link: notice_link(repo, change.n),
        status: event.status(),
        detail: event.detail(change),
        concerns: accounts(event.concerns(ctx, repo, change)?, actor),
    };
    let line = Op::Notify {
        channel_id: change.channel.clone(),
        message_id,
        thread: None,
        notice,
    };
    if !job_room && !matches!(event, Event::Opened | Event::ArchiveRefused(_)) {
        named(ctx, repo, change)?;
    }
    if job_room || matches!(event, Event::ArchiveRefused(_) | Event::LeftQueue { .. }) {
        let posted = ctx.emit(chat::PROGRAM, &line, ::program::Reply::Wanted);
        LOGGING.insert(ctx, &posted.seq);
        return Ok(());
    }
    emit(ctx, line);
    Ok(())
}

pub fn heads_moved(ctx: &ExecCtx, repo: &str, branch: &[u8], head: &str) -> Result<(), Error> {
    crate::state::index_open_changes_by_source_once(ctx, repo)?;
    let sourced = OPEN_CHANGES_BY_SOURCE.prefix_of(&(repo.to_owned(), branch.to_vec()));
    for (_, _, n) in OPEN_CHANGES_BY_SOURCE.scan(ctx, sourced)? {
        let change = load_change(ctx, repo, n)?;
        head_moved(ctx, repo, &change, head)?;
    }
    Ok(())
}

pub fn head_moved(ctx: &ExecCtx, repo: &str, change: &Change, head: &str) -> Result<(), Error> {
    let event = Event::NewHead {
        head: head.to_owned(),
    };
    post_if_it_concerns_anyone(ctx, repo, change, event)
}

pub fn requested(
    ctx: &ExecCtx,
    repo: &str,
    change: &Change,
    reviewers: Vec<Principal>,
) -> Result<(), Error> {
    post_if_it_concerns_anyone(ctx, repo, change, Event::Requested { reviewers })
}

fn post_if_it_concerns_anyone(
    ctx: &ExecCtx,
    repo: &str,
    change: &Change,
    event: Event,
) -> Result<(), Error> {
    let actor = ctx.env().sender.clone();
    let told = accounts(event.concerns(ctx, repo, change)?, actor.as_ref());
    if told.is_empty() {
        return Ok(());
    }
    post(ctx, repo, change, next_message(ctx)?, event, actor.as_ref())
}

fn named(ctx: &ExecCtx, repo: &str, change: &Change) -> Result<(), Error> {
    let name = room_name(repo, change);
    let query = Query::Channel {
        channel_id: change.channel.clone(),
    };
    let held = match ctx.query::<Query, Reply>(chat::PROGRAM, &query)? {
        Reply::Channel(info) => info.map(|info| info.channel.name),
        other => return Err(unexpected_reply(chat::PROGRAM, "Channel", &other)),
    };
    if held.is_some_and(|held| held != name) {
        let rename = Op::RenameChannel {
            channel_id: change.channel.clone(),
            name,
        };
        ctx.emit(chat::PROGRAM, &rename, ::program::Reply::Wanted);
    }
    Ok(())
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
        let change = load_change(ctx, &repo, n)?;
        let event = Event::ArchiveRefused(why);
        post(ctx, &repo, &change, next_message(ctx)?, event, None)?;
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
