//! The merge queue: changes wait in line for a branch with required checks, and the first lands
//! in the op that marks its Preview green, as the very commit its checks ran on.
use ::program::{Error, ExecCtx, QueryCtx, not_found, stale, wrong_state};
use store::Listing;

use crate::changes::{merged, previewed};
use crate::contract::{
    ChangeState, CheckState, MergedHeads, PageRequest, PageResponse, Principal, QueueEntry,
    Required,
};
use crate::state::{
    QUEUE, QUEUE_SEQ, QUEUED, REQUIRED, STATUSES, load_change, load_repo, next, next_message,
    parse_oid, ref_key, repo_hash, resolve, set_ref,
};

pub(crate) fn enqueue(
    ctx: &ExecCtx,
    actor: &Principal,
    repo: &str,
    n: u64,
    expected_head: &str,
) -> Result<(), Error> {
    let hash = repo_hash(&load_repo(ctx, repo)?);
    let change = load_change(ctx, repo, n)?;
    if change.state != ChangeState::Open {
        return Err(wrong_state(
            "change is not open: open a new change for further work",
        ));
    }
    if resolve(ctx, repo, &change.from, hash)? != parse_oid(hash, expected_head)? {
        return Err(stale("the change's head moved; read it again"));
    }
    if !REQUIRED.has(ctx, &ref_key(repo, &change.into)) {
        return Err(wrong_state(format!(
            "{} requires no checks: SquashMerge the change instead",
            String::from_utf8_lossy(&change.into)
        )));
    }
    let key = (repo.to_owned(), n);
    if QUEUED.has(ctx, &key) {
        return Err(wrong_state(format!("change {n} is in the queue already")));
    }
    let seq = next(QUEUE_SEQ.get(ctx)?.unwrap_or(0))?;
    QUEUE_SEQ.put(ctx, &seq);
    let entry = QueueEntry {
        n,
        head: parse_oid(hash, expected_head)?.to_hex(),
        by: actor.clone(),
        height: ctx.env().height,
    };
    QUEUE.put(ctx, &(repo.to_owned(), change.into.clone(), seq), &entry);
    QUEUED.put(ctx, &key, &(change.into.clone(), seq));
    settle_branch(ctx, repo, &change.into)
}

pub(crate) fn dequeue(ctx: &ExecCtx, repo: &str, n: u64) -> Result<(), Error> {
    let key = (repo.to_owned(), n);
    let Some((into, seq)) = QUEUED.get(ctx, &key)? else {
        return Err(not_found(format!("change {n} is in no queue")));
    };
    leave(ctx, repo, &into, seq, n);
    settle_branch(ctx, repo, &into)
}

pub(crate) fn settle(ctx: &ExecCtx, repo: &str) -> Result<(), Error> {
    let branches = REQUIRED.range_of(ctx, &repo.to_owned(), &PageRequest::default())?;
    for ((_, branch), _) in branches.items {
        settle_branch(ctx, repo, &branch)?;
    }
    Ok(())
}

enum Verdict {
    Green,
    Red(String),
    Waiting,
}

fn settle_branch(ctx: &ExecCtx, repo: &str, into: &[u8]) -> Result<(), Error> {
    let Some(required) = REQUIRED.get(ctx, &ref_key(repo, into))? else {
        return Ok(());
    };
    let hash = repo_hash(&load_repo(ctx, repo)?);
    let line = (repo.to_owned(), into.to_vec());
    loop {
        let front = QUEUE.range_of(ctx, &line, &PageRequest::first(1))?;
        let Some(((_, _, seq), entry)) = front.items.into_iter().next() else {
            return Ok(());
        };
        let change = load_change(ctx, repo, entry.n)?;
        if change.state != ChangeState::Open {
            leave(ctx, repo, into, seq, entry.n);
            continue;
        }
        if resolve(ctx, repo, &change.from, hash)?.to_hex() != entry.head {
            let why = "its head moved since it joined; queue it again";
            left(ctx, repo, into, seq, &entry, why)?;
            continue;
        }
        let Some(committer) = entry.by.account() else {
            leave(ctx, repo, into, seq, entry.n);
            continue;
        };
        let made = match previewed(ctx, committer, repo, entry.n) {
            Ok(made) => made,
            Err(refused) => {
                let why = format!("it does not preview: {}", refused.message);
                left(ctx, repo, into, seq, &entry, &why)?;
                continue;
            }
        };
        match verdict(ctx, repo, &required, &made.merge)? {
            Verdict::Waiting => return Ok(()),
            Verdict::Red(why) => left(ctx, repo, into, seq, &entry, &why)?,
            Verdict::Green => {
                leave(ctx, repo, into, seq, entry.n);
                let merge = parse_oid(hash, &made.merge)?;
                let heads = MergedHeads {
                    source: made.from_head,
                    target: made.into_head,
                };
                merged(ctx, &entry.by, repo, change, heads, &merge)?;
                set_ref(ctx, repo, into, &merge)?;
            }
        }
    }
}

fn verdict(ctx: &QueryCtx, repo: &str, required: &Required, merge: &str) -> Result<Verdict, Error> {
    let mut waiting = false;
    for context in &required.contexts {
        let key = (
            repo.to_owned(),
            merge.to_owned(),
            required.reporter.clone(),
            context.clone(),
        );
        match STATUSES.get(ctx, &key)? {
            Some(status) if status.state == CheckState::Passed => {}
            Some(status) if matches!(status.state, CheckState::Failed | CheckState::Errored) => {
                let url = status
                    .url
                    .map(|url| format!(" ({url})"))
                    .unwrap_or_default();
                let why = format!("{context} {}{url}", status.state.word());
                return Ok(Verdict::Red(why));
            }
            _ => waiting = true,
        }
    }
    Ok(match waiting {
        true => Verdict::Waiting,
        false => Verdict::Green,
    })
}

fn leave(ctx: &ExecCtx, repo: &str, into: &[u8], seq: u64, n: u64) {
    QUEUE.remove(ctx, &(repo.to_owned(), into.to_vec(), seq));
    QUEUED.remove(ctx, &(repo.to_owned(), n));
}

fn left(
    ctx: &ExecCtx,
    repo: &str,
    into: &[u8],
    seq: u64,
    entry: &QueueEntry,
    why: &str,
) -> Result<(), Error> {
    leave(ctx, repo, into, seq, entry.n);
    let change = load_change(ctx, repo, entry.n)?;
    let event = crate::discussion::Event::LeftQueue {
        into: into.to_vec(),
        why: why.to_owned(),
        by: entry.by.clone(),
    };
    let env = ctx.env();
    let message = next_message(ctx)?;
    crate::discussion::post(ctx, repo, &change, message, event, env.sender.as_ref())?;
    let told = crate::contract::Watched::LeftQueue {
        repo: repo.to_owned(),
        n: entry.n,
        into: into.to_vec(),
        why: why.to_owned(),
        queued_by: entry.by.clone(),
    };
    crate::watchers::tell(ctx, &told)
}

pub(crate) fn queue(
    ctx: &QueryCtx,
    repo: &str,
    branch: &[u8],
    listing: &Listing,
) -> Result<PageResponse<QueueEntry>, Error> {
    load_repo(ctx, repo)?;
    Ok(QUEUE
        .page_of(ctx, &(repo.to_owned(), branch.to_vec()), listing)?
        .map(|(_, entry)| entry))
}
