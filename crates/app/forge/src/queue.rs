//! The merge queue: changes wait in line for a branch with required checks, and the first, or a
//! train of the first few each previewed on the one ahead, lands in the op that marks its Preview
//! green, as the very commit its checks ran on.
use ::program::{Error, ExecCtx, QueryCtx, invalid, not_found, stale, wrong_state};
use gitcore::{Hash, Oid};
use store::Listing;

use crate::changes::{merged, previewed};
use crate::contract::{
    Change, ChangeState, CheckState, Formed, MergedHeads, PageRequest, PageResponse, PreviewInfo,
    Principal, QueueEntry, Required, Train, TrainMember,
};
use crate::state::{
    FORMED, HeldMember, HeldTrain, PREVIEWS, QUEUE, QUEUE_SEQ, QUEUED, REQUIRED, STATUSES, TRAINS,
    load_change, load_repo, next, next_message, parse_oid, ref_key, repo_hash, resolve, set_ref,
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

pub(crate) fn set_train(
    ctx: &ExecCtx,
    repo: &str,
    branch: &[u8],
    train: Option<Train>,
) -> Result<(), Error> {
    load_repo(ctx, repo)?;
    let key = ref_key(repo, branch);
    let Some(train) = train else {
        TRAINS.remove(ctx, &key);
        forget(ctx, &key);
        return Ok(());
    };
    let Some(required) = REQUIRED.get(ctx, &key)? else {
        return Err(wrong_state(format!(
            "{} requires no checks: a train lands only what required checks passed, so RequireChecks first",
            String::from_utf8_lossy(branch)
        )));
    };
    if train.length == 0 {
        return Err(invalid(
            "a train takes at least one change: length 1 lands one change at a time",
        ));
    }
    let unrequired = train
        .tip
        .iter()
        .find(|context| !required.contexts.contains(*context));
    if let Some(context) = unrequired {
        return Err(invalid(format!(
            "{context} is no required context of {}: tip names only contexts RequireChecks named",
            String::from_utf8_lossy(branch)
        )));
    }
    TRAINS.put(ctx, &key, &train);
    forget(ctx, &key);
    Ok(())
}

pub(crate) fn kept_preview(
    ctx: &QueryCtx,
    repo: &str,
    n: u64,
) -> Result<Option<PreviewInfo>, Error> {
    let Some((into, seq)) = QUEUED.get(ctx, &(repo.to_owned(), n))? else {
        return Ok(None);
    };
    let key = ref_key(repo, &into);
    let Some(train) = TRAINS.get(ctx, &key)? else {
        return Ok(None);
    };
    let window = QUEUE.range_of(ctx, &key, &PageRequest::first(train.length))?;
    let in_window = window.items.iter().any(|((_, _, each), _)| *each == seq);
    if !in_window {
        return Ok(None);
    }
    PREVIEWS.get(ctx, &(repo.to_owned(), n))
}

enum Verdict {
    Green,
    Red(String),
    Waiting,
}

struct Line {
    key: (String, Vec<u8>),
    hash: Hash,
    required: Required,
    tip: Vec<String>,
}

struct Member {
    seq: u64,
    entry: QueueEntry,
    change: Change,
    made: PreviewInfo,
}

impl Member {
    fn held(&self) -> HeldMember {
        HeldMember {
            seq: self.seq,
            n: self.entry.n,
            merge: self.made.merge.clone(),
        }
    }
}

fn settle_branch(ctx: &ExecCtx, repo: &str, into: &[u8]) -> Result<(), Error> {
    let key = ref_key(repo, into);
    let Some(required) = REQUIRED.get(ctx, &key)? else {
        return Ok(());
    };
    let train = TRAINS.get(ctx, &key)?;
    let hash = repo_hash(&load_repo(ctx, repo)?);
    let tip = train
        .as_ref()
        .map(|train| train.tip.clone())
        .unwrap_or_default();
    let line = Line {
        key,
        hash,
        required,
        tip,
    };
    loop {
        let front = QUEUE.range_of(ctx, &line.key, &PageRequest::first(1))?;
        let Some(((_, _, seq), entry)) = front.items.into_iter().next() else {
            forget(ctx, &line.key);
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
        let made = match previewed(ctx, committer, repo, entry.n, None) {
            Ok(made) => made,
            Err(refused) => {
                let why = format!("it does not preview: {}", refused.message);
                left(ctx, repo, into, seq, &entry, &why)?;
                continue;
            }
        };
        let waiting = match verdict(ctx, &line, &made.merge, false)? {
            Verdict::Red(why) => {
                left(ctx, repo, into, seq, &entry, &why)?;
                forget(ctx, &line.key);
                continue;
            }
            Verdict::Waiting => true,
            Verdict::Green => false,
        };
        let front = Member {
            seq,
            entry,
            change,
            made,
        };
        let window = match &train {
            Some(train) => window(ctx, &line, train.length, front)?,
            None => vec![front],
        };
        if waiting {
            forget(ctx, &line.key);
            return Ok(());
        }
        let members = match &train {
            Some(_) => formed(ctx, &line, window)?,
            None => window,
        };
        let Some(tip) = members.last().map(|last| last.made.merge.clone()) else {
            return Ok(());
        };
        match verdict(ctx, &line, &tip, true)? {
            Verdict::Waiting => return Ok(()),
            Verdict::Red(why) => {
                let behind = members.len() > 1;
                let Some(front) = members.into_iter().next() else {
                    return Ok(());
                };
                if behind {
                    let alone = HeldTrain {
                        members: vec![front.held()],
                        alone: true,
                        height: ctx.env().height,
                    };
                    FORMED.put(ctx, &line.key, &alone);
                    continue;
                }
                left(ctx, repo, into, front.seq, &front.entry, &why)?;
                forget(ctx, &line.key);
            }
            Verdict::Green => {
                let landed = parse_oid(hash, &tip)?;
                for member in members {
                    leave(ctx, repo, into, member.seq, member.entry.n);
                    let merge = parse_oid(hash, &member.made.merge)?;
                    let heads = MergedHeads {
                        source: member.made.from_head,
                        target: member.made.into_head,
                    };
                    merged(ctx, &member.entry.by, repo, member.change, heads, &merge)?;
                }
                set_ref(ctx, repo, into, &landed)?;
                forget(ctx, &line.key);
            }
        }
    }
}

fn window(ctx: &ExecCtx, line: &Line, length: u64, front: Member) -> Result<Vec<Member>, Error> {
    let queued = QUEUE.range_of(ctx, &line.key, &PageRequest::first(length))?;
    let mut window = vec![front];
    for ((_, _, seq), entry) in queued.items.into_iter().skip(1) {
        let Some(ahead) = window.last() else {
            break;
        };
        let onto = parse_oid(line.hash, &ahead.made.merge)?;
        let Some(member) = stacked(ctx, line, seq, entry, onto)? else {
            break;
        };
        window.push(member);
    }
    Ok(window)
}

fn stacked(
    ctx: &ExecCtx,
    line: &Line,
    seq: u64,
    entry: QueueEntry,
    onto: Oid,
) -> Result<Option<Member>, Error> {
    let repo = line.key.0.as_str();
    let change = load_change(ctx, repo, entry.n)?;
    if change.state != ChangeState::Open {
        return Ok(None);
    }
    let moved = resolve(ctx, repo, &change.from, line.hash)?.to_hex() != entry.head;
    if moved {
        return Ok(None);
    }
    let Some(committer) = entry.by.account() else {
        return Ok(None);
    };
    let Ok(made) = previewed(ctx, committer, repo, entry.n, Some(onto)) else {
        return Ok(None);
    };
    Ok(Some(Member {
        seq,
        entry,
        change,
        made,
    }))
}

fn formed(ctx: &ExecCtx, line: &Line, mut window: Vec<Member>) -> Result<Vec<Member>, Error> {
    let height = ctx.env().height;
    let Some(front_seq) = window.first().map(|front| front.seq) else {
        return Ok(window);
    };
    let held = FORMED.get(ctx, &line.key)?.filter(|held| {
        held.members
            .first()
            .is_some_and(|first| first.seq == front_seq)
    });
    if held.as_ref().is_some_and(|held| held.alone) {
        window.truncate(1);
        return Ok(window);
    }
    let mut eligible = 1;
    for member in window.iter().skip(1) {
        match verdict(ctx, line, &member.made.merge, false)? {
            Verdict::Green => eligible += 1,
            Verdict::Red(_) | Verdict::Waiting => break,
        }
    }
    let frozen = held.filter(|held| held.height < height);
    let (length, since) = match frozen {
        Some(held) if still_holds(&held, &window[..eligible]) => (held.members.len(), held.height),
        _ => (eligible, height),
    };
    window.truncate(length);
    let row = HeldTrain {
        members: window.iter().map(Member::held).collect(),
        alone: false,
        height: since,
    };
    if FORMED.get(ctx, &line.key)?.as_ref() != Some(&row) {
        FORMED.put(ctx, &line.key, &row);
    }
    Ok(window)
}

fn still_holds(held: &HeldTrain, eligible: &[Member]) -> bool {
    let fits = held.members.len() <= eligible.len();
    fits && held
        .members
        .iter()
        .zip(eligible)
        .all(|(kept, now)| kept.seq == now.seq && kept.merge == now.made.merge)
}

fn forget(ctx: &ExecCtx, key: &(String, Vec<u8>)) {
    if FORMED.has(ctx, key) {
        FORMED.remove(ctx, key);
    }
}

fn verdict(ctx: &QueryCtx, line: &Line, merge: &str, on_tip: bool) -> Result<Verdict, Error> {
    let mut waiting = false;
    let judged = line
        .required
        .contexts
        .iter()
        .filter(|context| line.tip.contains(*context) == on_tip);
    for context in judged {
        let key = (
            line.key.0.clone(),
            merge.to_owned(),
            line.required.reporter.clone(),
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

pub(crate) fn train(
    ctx: &QueryCtx,
    repo: &str,
    branch: &[u8],
) -> Result<(Option<Train>, Option<Formed>), Error> {
    load_repo(ctx, repo)?;
    let key = ref_key(repo, branch);
    let formed = FORMED.get(ctx, &key)?.map(|held| Formed {
        members: held
            .members
            .into_iter()
            .map(|member| TrainMember {
                n: member.n,
                merge: member.merge,
            })
            .collect(),
        alone: held.alone,
        height: held.height,
    });
    Ok((TRAINS.get(ctx, &key)?, formed))
}
