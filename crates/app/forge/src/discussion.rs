//! Chat owns conversations: forge opens a change's own room, posts its notices there, names it
//! after the change and archives it once the change ends.

use std::collections::BTreeSet;

use ::program::{AccountNumber, Error, ExecCtx, QueryCtx, unexpected_reply};
use ::program::{MessageId, Outcome, Principal};
use chat::message::Status;
use chat::{Notice, Op, PostPolicy, Query, Reply};

use crate::Change;
use crate::contract::{ReviewDraft, Side, Verdict};
use crate::state::{
    ARCHIVING, LOGGING, OPEN_CHANGES_BY_SOURCE, channel_id, load_change, load_repo, next_message,
};

pub const NOTICE: &str = "change";

pub fn notice_link(repo: &str, n: u64) -> Option<String> {
    ducklink::path(crate::PROGRAM, &[repo, &n.to_string()])
}

pub fn change_of_room(channel: &str) -> Option<(&str, u64)> {
    let (program, name) = channel.split_once(':')?;
    if program != crate::PROGRAM {
        return None;
    }
    let (repo, n) = name.rsplit_once(':')?;
    let named = !repo.is_empty() && n.bytes().all(|byte| byte.is_ascii_digit());
    if !named {
        return None;
    }
    Some((repo, n.parse().ok()?))
}

pub fn room_of(repo: &str, n: u64) -> String {
    crate::state::channel_id(repo, n)
}

pub enum Event {
    Opened,
    Closed,
    Merged,
    Reviewed {
        id: u64,
        verdict: Verdict,
        text: Option<String>,
    },
    ArchiveRefused(String),
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
            Event::NewHead { .. } => "head".into(),
            Event::Requested { .. } => "requested".into(),
        }
    }

    fn text(&self) -> Option<String> {
        match self {
            Event::Reviewed { text, .. } => text.clone(),
            Event::Opened
            | Event::Closed
            | Event::Merged
            | Event::ArchiveRefused(_)
            | Event::NewHead { .. }
            | Event::Requested { .. } => None,
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
            Event::ArchiveRefused(_) => Status::Failed,
            Event::NewHead { .. } | Event::Requested { .. } => Status::Running,
        }
    }

    fn detail(&self, change: &Change) -> String {
        match self {
            Event::Opened if change.reviewers.is_empty() => "opened".into(),
            Event::Opened => "opened, review requested".into(),
            Event::Closed => "closed".into(),
            Event::Merged => format!("merged into {}", branch(&change.into)),
            Event::Reviewed { id, verdict, .. } => {
                let said = match verdict {
                    Verdict::Approve => "approved",
                    Verdict::RequestChanges => "changes requested",
                    Verdict::Comment => "commented",
                };
                format!("review {id}: {said}")
            }
            Event::ArchiveRefused(why) => format!("archive refused: {why}"),
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
            Event::NewHead { .. } => change.reviewers.clone(),
            Event::Requested { reviewers } => reviewers.clone(),
        })
    }
}

pub fn review_text(draft: &ReviewDraft) -> Option<String> {
    let body = draft.body.trim();
    let said = (!body.is_empty()).then(|| body.to_owned());
    let comments = draft.comments.iter().map(|comment| {
        let side = match comment.side {
            Side::New => "",
            Side::Old => " (old)",
        };
        let path = String::from_utf8_lossy(&comment.path);
        format!("`{path}:{}`{side}\n{}", comment.line, comment.body.trim())
    });
    let parts: Vec<String> = said.into_iter().chain(comments).collect();
    (!parts.is_empty()).then(|| parts.join("\n"))
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

pub fn talks_elsewhere(repo: &str, change: &Change) -> bool {
    change.channel != channel_id(repo, change.n)
}

/// Opens the change's channel: chat creates it in this frame, and its
/// refusal fails the op.
pub fn create(ctx: &ExecCtx, repo: &str, change: &Change) {
    emit(
        ctx,
        Op::CreateChannelWithMeta {
            channel_id: change.channel.clone(),
            name: room_name(repo, change),
            post_policy: PostPolicy::Open,
            meta: crate::involved::meta(),
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
    let elsewhere = talks_elsewhere(repo, change);
    let notice = Notice {
        kind: NOTICE.into(),
        group: format!("{} {repo} {}", event.word(), change.n),
        subject: format!("{repo} #{}: {}", change.n, change.title),
        link: notice_link(repo, change.n),
        status: event.status(),
        detail: event.detail(change),
        concerns: accounts(event.concerns(ctx, repo, change)?, actor),
    };
    let channel_id = change.channel.clone();
    let by = actor.filter(|actor| matches!(actor, Principal::Account(_)));
    let line = Op::NotifyWith {
        channel_id,
        message_id,
        thread: None,
        notice,
        by: by.cloned(),
        text: event.text(),
    };
    let renames_its_room = !elsewhere && !matches!(event, Event::Opened | Event::ArchiveRefused(_));
    if renames_its_room {
        named(ctx, repo, change)?;
    }
    let absorbs_the_answer = elsewhere || matches!(event, Event::ArchiveRefused(_));
    if absorbs_the_answer {
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

pub fn archive(ctx: &ExecCtx, repo: &str, change: &Change) {
    if talks_elsewhere(repo, change) {
        return;
    }
    let archive = Op::SetChannelArchived {
        channel_id: change.channel.clone(),
        archived: true,
    };
    let id = ctx.emit(chat::PROGRAM, &archive, ::program::Reply::Wanted);
    ARCHIVING.put(ctx, &id.seq, &(repo.to_owned(), change.n));
}

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

#[test]
fn a_change_room_names_its_change_back() {
    for (repo, n) in [("notes", 7), ("ada/notes", 12), ("a:b", 3)] {
        assert_eq!(change_of_room(&channel_id(repo, n)), Some((repo, n)));
    }
    let others = [
        "j-375",
        "forge:notes",
        "forge::7",
        "forge:notes:x",
        "forge:notes:+7",
        "forger:notes:7",
        "chat:notes:7",
    ];
    for other in others {
        assert_eq!(change_of_room(other), None, "{other}");
    }
}
