//! What each op checks and writes: the network's rules, and each change event forge tells, posted
//! as one notice in the room its change was mirrored into when it opened.
use ::program::{
    Error, ExecCtx, Principal, QueryCtx, Reply as Wanted, code, invalid, unexpected_reply,
};
use chat::message::Status;
use forge::{Change, Revision, Verdict, Watched};
use store::{PageRequest, PageResponse};

use crate::state::{POSTED, ROOMS, RULES};
use crate::{Mirror, Rule};

const CAPTURE: &str = "{}";

struct Line {
    word: String,
    status: Status,
    detail: String,
}

pub(crate) fn set_rules(ctx: &ExecCtx, rules: Vec<Rule>) -> Result<(), Error> {
    ctx.sender()?;
    for rule in &rules {
        check(rule)?;
    }
    RULES.put(ctx, &rules);
    Ok(())
}

fn check(rule: &Rule) -> Result<(), Error> {
    let names_both = !rule.branch.is_empty() && !rule.room.is_empty();
    if !names_both {
        return Err(invalid("a rule names a branch and a room"));
    }
    let captures = |text: &str| text.matches(CAPTURE).count();
    let fillable = captures(&rule.branch) <= 1 && captures(&rule.room) <= captures(&rule.branch);
    if !fillable {
        return Err(invalid(
            "a rule's branch holds {} at most once, and its room only where its branch does",
        ));
    }
    Ok(())
}

pub(crate) fn room_for(rules: &[Rule], branch: &str) -> Option<String> {
    rules.iter().find_map(|rule| matched(rule, branch))
}

fn matched(rule: &Rule, branch: &str) -> Option<String> {
    let Some((before, after)) = rule.branch.split_once(CAPTURE) else {
        return branch.starts_with(&rule.branch).then(|| rule.room.clone());
    };
    let rest = branch.strip_prefix(before)?;
    let segment = rest.split_once('/').map_or(rest, |(segment, _)| segment);
    let end = (1..=segment.len())
        .rev()
        .filter(|end| segment.is_char_boundary(*end))
        .find(|end| rest[*end..].starts_with(after))?;
    Some(rule.room.replacen(CAPTURE, &segment[..end], 1))
}

pub(crate) fn watched(ctx: &ExecCtx, event: Watched) -> Result<(), Error> {
    ctx.env().sent_by(forge::PROGRAM)?;
    match event {
        Watched::Opened {
            repo, n, from, by, ..
        } => opened(ctx, &repo, n, &from, &by),
        Watched::Reviewed {
            repo,
            n,
            id,
            verdict,
            by,
        } => {
            let (said, status) = match verdict {
                Verdict::Approve => ("approved", Status::Passed),
                Verdict::RequestChanges => ("changes requested", Status::Failed),
                Verdict::Comment => ("commented", Status::Note),
            };
            tell(ctx, &repo, n, |_| Line {
                word: format!("review {id}"),
                status,
                detail: format!("review {id}: {said} by {}", who(&by)),
            })
        }
        Watched::LeftQueue {
            repo, n, into, why, ..
        } => tell(ctx, &repo, n, |_| Line {
            word: "left queue".into(),
            status: Status::Failed,
            detail: format!("left the merge queue of {}: {why}", branch(&into)),
        }),
        Watched::Merged { repo, n, by, .. } => {
            tell(ctx, &repo, n, |change| Line {
                word: "merged".into(),
                status: Status::Passed,
                detail: format!("merged into {} by {}", branch(&change.into), who(&by)),
            })?;
            ROOMS.remove(ctx, &(repo, n));
            Ok(())
        }
        Watched::Closed { repo, n, by } => {
            tell(ctx, &repo, n, |_| Line {
                word: "closed".into(),
                status: Status::Note,
                detail: format!("closed by {}", who(&by)),
            })?;
            ROOMS.remove(ctx, &(repo, n));
            Ok(())
        }
        Watched::Moved { .. } | Watched::Previewed { .. } | Watched::Dispatched { .. } => Ok(()),
    }
}

fn opened(ctx: &ExecCtx, repo: &str, n: u64, from: &Revision, by: &Principal) -> Result<(), Error> {
    let Revision::Ref(name) = from else {
        return Ok(());
    };
    let Some(name) = name
        .strip_prefix(b"refs/heads/")
        .and_then(|name| std::str::from_utf8(name).ok())
    else {
        return Ok(());
    };
    let rules = RULES.get(ctx)?.unwrap_or_default();
    let Some(room) = room_for(&rules, name) else {
        return Ok(());
    };
    if !open(ctx, &room)? {
        return Ok(());
    }
    ROOMS.put(ctx, &(repo.to_owned(), n), &room);
    let change = change(ctx, repo, n)?;
    post(ctx, &room, repo, &change, |change| Line {
        word: "opened".into(),
        status: Status::Running,
        detail: format!(
            "opened by {} from {name}; its conversation is in {}",
            who(by),
            change.channel
        ),
    })
}

fn tell(
    ctx: &ExecCtx,
    repo: &str,
    n: u64,
    line: impl FnOnce(&Change) -> Line,
) -> Result<(), Error> {
    let Some(room) = ROOMS.get(ctx, &(repo.to_owned(), n))? else {
        return Ok(());
    };
    let change = change(ctx, repo, n)?;
    post(ctx, &room, repo, &change, line)
}

fn post(
    ctx: &ExecCtx,
    room: &str,
    repo: &str,
    change: &Change,
    line: impl FnOnce(&Change) -> Line,
) -> Result<(), Error> {
    let line = line(change);
    let posted = POSTED
        .get(ctx)?
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| Error::new(code::EXHAUSTED, "the posted count is exhausted"))?;
    POSTED.put(ctx, &posted);
    let notify = chat::Op::Notify {
        channel_id: room.to_owned(),
        message_id: chat::namespace::id(crate::PROGRAM, &format!("{posted:016x}")),
        thread: None,
        notice: chat::Notice {
            kind: forge::NOTICE.into(),
            group: format!("{} {repo} {}", line.word, change.n),
            subject: format!("{repo} #{}: {}", change.n, change.title),
            link: forge::notice_link(repo, change.n),
            status: line.status,
            detail: line.detail,
            concerns: Vec::new(),
        },
    };
    ctx.emit(chat::PROGRAM, &notify, Wanted::Wanted);
    Ok(())
}

fn open(ctx: &QueryCtx, room: &str) -> Result<bool, Error> {
    let query = chat::Query::Channel {
        channel_id: room.to_owned(),
    };
    match ctx.query::<chat::Query, chat::Reply>(chat::PROGRAM, &query)? {
        chat::Reply::Channel(info) => {
            Ok(info.is_some_and(|info| !info.channel.archived && !info.channel.members_only()))
        }
        other => Err(unexpected_reply(chat::PROGRAM, "Channel", &other)),
    }
}

fn change(ctx: &QueryCtx, repo: &str, n: u64) -> Result<Change, Error> {
    let query = forge::Query::Change {
        repo: repo.to_owned(),
        n,
        page: PageRequest::first(1),
    };
    match ctx.query::<forge::Query, forge::Reply>(forge::PROGRAM, &query)? {
        forge::Reply::Change { change, .. } => Ok(change),
        other => Err(unexpected_reply(forge::PROGRAM, "Change", &other)),
    }
}

fn who(by: &Principal) -> String {
    match by {
        Principal::Account(number) => format!("account {number}"),
        Principal::Root => "the chain".into(),
    }
}

fn branch(name: &[u8]) -> String {
    let name = name.strip_prefix(b"refs/heads/").unwrap_or(name);
    String::from_utf8_lossy(name).into_owned()
}

pub(crate) fn rules_of(ctx: &QueryCtx) -> Result<Vec<Rule>, Error> {
    Ok(RULES.get(ctx)?.unwrap_or_default())
}

pub(crate) fn room_of(ctx: &QueryCtx, repo: String, n: u64) -> Result<Option<String>, Error> {
    ROOMS.get(ctx, &(repo, n))
}

pub(crate) fn mirrored(ctx: &QueryCtx, page: &PageRequest) -> Result<PageResponse<Mirror>, Error> {
    Ok(ROOMS
        .range(ctx, page)?
        .map(|((repo, n), room)| Mirror { repo, n, room }))
}
