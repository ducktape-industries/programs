//! The change ops: open, edit, close, review and merge. Acceptance reads
//! refs and records only; no object a node holds or lacks decides an op.

use std::collections::BTreeSet;

use ::program::Env;
use ::program::{
    AccountNumber, Error, ExecCtx, QueryCtx, capacity, invalid, stale, unexpected_reply,
    wrong_state,
};
use gitcore::merge::{Clash, Conflict};
use gitcore::{Commit, Hash, Kind, Object, Objects, Oid, Signature, WriteObjects};

use crate::contract::*;
use crate::discussion::{self, Event};
use crate::objects::{ObjectWriter, object_not_held};
use crate::ops::{cap, refusal_of, require_named};
use crate::state::{
    AUTHORSHIP, Authorship, CHANGES, NUMBERS, load_bounds, load_change, load_ref, load_repo, next,
    next_message, next_number, parse_oid, peek_message, protected, repo_hash, resolve, save_change,
    save_review, set_ref,
};

/// What `ChangeOpen` carries besides its repository.
pub(crate) struct Draft {
    pub from: Revision,
    pub into: Vec<u8>,
    pub title: String,
    pub body: String,
    pub reviewers: Vec<Principal>,
}

pub(crate) fn open(
    ctx: &ExecCtx,
    actor: &Principal,
    repo: &str,
    mut draft: Draft,
) -> Result<OpReply, Error> {
    let env = ctx.env();
    let record = load_repo(ctx, repo)?;
    check_title(&draft.title)?;
    check_reviewers(ctx, &draft.reviewers)?;
    check_endpoints(&draft.into, &draft.from)?;
    let hash = repo_hash(&record);
    let source = resolve(ctx, repo, &draft.from, hash)?;
    let target = resolve(ctx, repo, &Revision::Ref(draft.into.clone()), hash)?;
    if source == target {
        return Err(wrong_state("source and target already agree"));
    }
    if matches!(draft.from, Revision::Oid(_)) {
        draft.from = Revision::Oid(source.to_hex());
    }
    let n = next_number(ctx, repo)?;
    let channel = discussion::room(ctx, repo, n, &draft.from)?;
    let change = Change {
        n,
        from: draft.from,
        into: draft.into,
        title: draft.title,
        body: draft.body,
        author: actor.clone(),
        state: ChangeState::Open,
        reviewers: draft.reviewers,
        created_height: env.height,
        updated_height: env.height,
        created_time: env.time,
        updated_time: env.time,
        review_count: 0,
        comment_count: 0,
        verdicts: ReviewCounts::default(),
        merge_oid: None,
        closed_by: None,
        merged_by: None,
        merged_heads: None,
        channel,
        system_seq: 1,
    };
    fits(ctx, &change)?;
    let message = next_message(ctx)?;
    save_change(ctx, repo, &change)?;
    let authorship = Authorship {
        author: actor.clone(),
        rev: 0,
    };
    AUTHORSHIP.put(ctx, &(repo.to_owned(), n), &authorship);
    if !discussion::in_job_room(repo, &change) {
        discussion::create(ctx, repo, &change);
    }
    discussion::post(ctx, repo, &change, message, Event::Opened);
    let opened = Watched::Opened {
        repo: repo.to_owned(),
        n,
        from: change.from.clone(),
        into: change.into.clone(),
        head: source.to_hex(),
        by: actor.clone(),
    };
    crate::watchers::tell(ctx, &opened)?;
    Ok(OpReply::Change {
        height: env.height,
        n,
    })
}

/// What `ChangeEdit` changes; `None` leaves a field as it is.
pub(crate) struct Edit {
    pub title: Option<String>,
    pub body: Option<String>,
    pub reviewers: Option<Vec<Principal>>,
}

pub(crate) fn edit(ctx: &ExecCtx, repo: &str, n: u64, fields: Edit) -> Result<OpReply, Error> {
    let env = ctx.env();
    load_repo(ctx, repo)?;
    let mut change = load_change(ctx, repo, n)?;
    require_open(&change)?;
    if let Some(title) = fields.title {
        check_title(&title)?;
        change.title = title;
    }
    if let Some(body) = fields.body {
        change.body = body;
    }
    if let Some(reviewers) = fields.reviewers {
        check_reviewers(ctx, &reviewers)?;
        change.reviewers = reviewers;
    }
    touched(&mut change, &env);
    fits(ctx, &change)?;
    save_change(ctx, repo, &change)?;
    crate::state::reshaped(ctx, repo, n)?;
    Ok(OpReply::Change {
        height: env.height,
        n,
    })
}

/// Closing is terminal: the author or a writer ends an open change.
pub(crate) fn close(
    ctx: &ExecCtx,
    actor: &Principal,
    repo: &str,
    n: u64,
) -> Result<OpReply, Error> {
    let env = ctx.env();
    let mut change = load_change(ctx, repo, n)?;
    require_open(&change)?;
    change.state = ChangeState::Closed;
    change.closed_by = Some(actor.clone());
    touched(&mut change, &env);
    change.system_seq = next(change.system_seq)?;
    let message = next_message(ctx)?;
    save_change(ctx, repo, &change)?;
    discussion::post(ctx, repo, &change, message, Event::Closed);
    discussion::archive(ctx, repo, &change);
    let closed = Watched::Closed {
        repo: repo.to_owned(),
        n,
        by: actor.clone(),
    };
    crate::watchers::tell(ctx, &closed)?;
    Ok(OpReply::Change {
        height: env.height,
        n,
    })
}

/// One immutable review: a verdict, a body and its line comments, pinned
/// at the commits it read. Reviews stay appendable after a change ends.
pub(crate) fn submit_review(
    ctx: &ExecCtx,
    actor: &Principal,
    repo: &str,
    n: u64,
    mut draft: ReviewDraft,
) -> Result<OpReply, Error> {
    let env = ctx.env();
    let record = load_repo(ctx, repo)?;
    let mut change = load_change(ctx, repo, n)?;
    let hash = repo_hash(&record);
    draft.commit_oid = parse_oid(hash, &draft.commit_oid)?.to_hex();
    draft.base_oid = draft
        .base_oid
        .map(|hex| parse_oid(hash, &hex).map(|oid| oid.to_hex()))
        .transpose()?;
    check_comments(&draft)?;
    let id = next(change.review_count)?;
    let review = Review {
        id,
        author: actor.clone(),
        height: env.height,
        time: env.time,
        draft,
        message_id: peek_message(ctx)?,
    };
    fits(ctx, &review)?;
    change.review_count = id;
    change.comment_count = change
        .comment_count
        .checked_add(review.draft.comments.len() as u64)
        .ok_or_else(|| capacity("comment counter exhausted"))?;
    count_verdict(&mut change.verdicts, review.draft.verdict)?;
    touched(&mut change, &env);
    change.system_seq = next(change.system_seq)?;
    fits(ctx, &change)?;
    let message = next_message(ctx)?;
    save_review(ctx, repo, n, &review);
    save_change(ctx, repo, &change)?;
    discussion::post(ctx, repo, &change, message, Event::Reviewed(id));
    Ok(OpReply::Review {
        height: env.height,
        n,
        id,
    })
}

/// What `Merge` carries besides its repository.
pub(crate) struct MergeRequest {
    pub into: Vec<u8>,
    pub from: Revision,
    pub expected_into: String,
    pub expected_from: String,
    pub result: String,
    pub change: Option<u64>,
}

/// A compare-and-swap of both heads: the client built and published the
/// result; forge only checks that neither endpoint moved since.
pub(crate) fn merge_heads(
    ctx: &ExecCtx,
    actor: &Principal,
    repo: &str,
    merge: MergeRequest,
) -> Result<OpReply, Error> {
    let env = ctx.env();
    let record = load_repo(ctx, repo)?;
    check_endpoints(&merge.into, &merge.from)?;
    let hash = repo_hash(&record);
    let expected_into = parse_oid(hash, &merge.expected_into)?;
    let expected_from = parse_oid(hash, &merge.expected_from)?;
    let result = parse_oid(hash, &merge.result)?;
    let ends = (merge.into.as_slice(), &merge.from);
    still_at(ctx, repo, hash, ends, (expected_into, expected_from))?;
    let heads = (&expected_into, &expected_from);
    crate::checks::gate(ctx, repo, &merge.into, merge.change, heads)?;
    if result == expected_into || expected_from == expected_into {
        return Err(wrong_state("merge does not advance the target"));
    }
    if let Some(n) = merge.change {
        let change = open_on(ctx, repo, hash, n, ends)?;
        let heads = MergedHeads {
            source: expected_from.to_hex(),
            target: expected_into.to_hex(),
        };
        merged(ctx, actor, repo, change, heads, &result)?;
    }
    set_ref(ctx, repo, &merge.into, &result)?;
    Ok(OpReply::Merged {
        height: env.height,
        oid: result.to_hex(),
        change: merge.change,
    })
}

// Both heads still where the merge read them: `into` at the first, `from` at the second.
fn still_at(
    ctx: &QueryCtx,
    repo: &str,
    hash: Hash,
    (into, from): (&[u8], &Revision),
    (expected_into, expected_from): (Oid, Oid),
) -> Result<(), Error> {
    let heads_moved = load_ref(ctx, repo, into, hash)? != Some(expected_into)
        || resolve(ctx, repo, from, hash)? != expected_from;
    if heads_moved {
        return Err(stale("source or target head moved; recompute the merge"));
    }
    Ok(())
}

// Change `n`, open, on the merge's own endpoints.
fn open_on(
    ctx: &QueryCtx,
    repo: &str,
    hash: Hash,
    n: u64,
    (into, from): (&[u8], &Revision),
) -> Result<Change, Error> {
    let change = load_change(ctx, repo, n)?;
    require_open(&change)?;
    let same_source = match (&change.from, from) {
        (Revision::Oid(a), Revision::Oid(b)) => parse_oid(hash, a)? == parse_oid(hash, b)?,
        (a, b) => a == b,
    };
    if change.into != into || !same_source {
        return Err(invalid("merge endpoints differ from the change"));
    }
    Ok(change)
}

// The change merged at `result` by `actor`, and its line in the change's channel.
pub(crate) fn merged(
    ctx: &ExecCtx,
    actor: &Principal,
    repo: &str,
    mut change: Change,
    heads: MergedHeads,
    result: &Oid,
) -> Result<(), Error> {
    let env = ctx.env();
    change.state = ChangeState::Merged;
    change.merge_oid = Some(result.to_hex());
    change.merged_by = Some(actor.clone());
    change.merged_heads = Some(heads);
    touched(&mut change, &env);
    change.system_seq = next(change.system_seq)?;
    fits(ctx, &change)?;
    let message = next_message(ctx)?;
    save_change(ctx, repo, &change)?;
    discussion::post(ctx, repo, &change, message, Event::Merged);
    discussion::archive(ctx, repo, &change);
    let merged = Watched::Merged {
        repo: repo.to_owned(),
        n: change.n,
        oid: result.to_hex(),
        by: actor.clone(),
    };
    crate::watchers::tell(ctx, &merged)?;
    Ok(())
}

// What `SquashMerge` carries besides its repository.
pub(crate) struct SquashRequest {
    pub into: Vec<u8>,
    pub from: Revision,
    pub expected_into: String,
    pub expected_from: String,
    pub message: Option<String>,
    pub change: Option<u64>,
}

// One squash commit forge builds itself: what `from` changed since its merge base with
// `into`, merged onto `into`'s tree, with `into`'s head its one parent; then `into` moves to
// it under `Merge`'s CAS. A conflict refuses it, naming every path, and writes nothing.
pub(crate) fn squash(
    ctx: &ExecCtx,
    actor: &Principal,
    repo: &str,
    squash: SquashRequest,
) -> Result<OpReply, Error> {
    let env = ctx.env();
    let record = load_repo(ctx, repo)?;
    check_endpoints(&squash.into, &squash.from)?;
    let hash = repo_hash(&record);
    let expected_into = parse_oid(hash, &squash.expected_into)?;
    let expected_from = parse_oid(hash, &squash.expected_from)?;
    let ends = (squash.into.as_slice(), &squash.from);
    still_at(ctx, repo, hash, ends, (expected_into, expected_from))?;
    let heads = (&expected_into, &expected_from);
    crate::checks::gate(ctx, repo, &squash.into, squash.change, heads)?;
    if expected_from == expected_into {
        return Err(wrong_state("merge does not advance the target"));
    }
    let change = match squash.change {
        Some(n) => Some(open_on(ctx, repo, hash, n, ends)?),
        None => None,
    };
    let message = message_of(squash.message, change.as_ref())?;
    let bounds = load_bounds(ctx)?;
    let objects = ObjectWriter::new(ctx, hash);
    let walk = cap(bounds.log_walk);
    let base = gitcore::walk::merge_base(&objects, &expected_from, &expected_into, walk)
        .map_err(refusal_of)?
        .ok_or_else(|| {
            wrong_state("the branches share no history: no merge base to squash from")
        })?;
    if base == expected_from {
        return Err(wrong_state("merge does not advance the target"));
    }
    let committer = signature(ctx, require_named(actor)?)?;
    let author = match change.as_ref().and_then(|change| change.author.account()) {
        Some(author) => signature(ctx, author)?,
        None => committer.clone(),
    };
    let made = Squashed {
        base,
        ours: expected_into,
        theirs: expected_from,
        author,
        committer,
        message,
    };
    let id = write_squash(objects, &bounds, hash, made)?;
    if let Some(change) = change {
        let heads = MergedHeads {
            source: expected_from.to_hex(),
            target: expected_into.to_hex(),
        };
        merged(ctx, actor, repo, change, heads, &id)?;
    }
    set_ref(ctx, repo, &squash.into, &id)?;
    Ok(OpReply::Merged {
        height: env.height,
        oid: id.to_hex(),
        change: squash.change,
    })
}

// What a squash commit is made of: the merge base, the head it lands on (`ours`, its one
// parent), the head whose changes it carries (`theirs`), and whom it names.
struct Squashed {
    base: Oid,
    ours: Oid,
    theirs: Oid,
    author: Signature,
    committer: Signature,
    message: Vec<u8>,
}

// Writes the squash commit: what `theirs` changed since `base`, merged onto `ours`' tree. A
// conflict refuses it, naming every path, and writes nothing.
fn write_squash(
    mut objects: ObjectWriter<'_>,
    bounds: &Bounds,
    hash: Hash,
    squash: Squashed,
) -> Result<Oid, Error> {
    let base = root_tree(&objects, &squash.base)?;
    let ours = root_tree(&objects, &squash.ours)?;
    let theirs = root_tree(&objects, &squash.theirs)?;
    let cost = cap(bounds.merge_cost);
    let merge =
        gitcore::merge::trees(&objects, Some(&base), &ours, &theirs, cost).map_err(refusal_of)?;
    if !merge.conflicts.is_empty() {
        return Err(wrong_state(conflicted(&merge.conflicts)));
    }
    let commit = Commit {
        tree: merge.tree,
        parents: vec![squash.ours],
        author: squash.author,
        committer: squash.committer,
        extra: Vec::new(),
        message: squash.message,
    };
    let body = commit.serialize();
    let id = gitcore::oid_of(hash, Kind::Commit, &body).map_err(refusal_of)?;
    let made = merge
        .objects
        .into_iter()
        .chain([(id, Object::new(Kind::Commit, body))]);
    for (oid, object) in made {
        if object.body.len() as u64 > bounds.max_object_size {
            return Err(capacity(format!(
                "the squash makes an object of {} bytes; forge takes at most {} (Bounds.max_object_size)",
                object.body.len(),
                bounds.max_object_size
            )));
        }
        objects.keep(oid, object).map_err(refusal_of)?;
    }
    objects.flush()?;
    Ok(id)
}

// Change `n`'s squash on its target's head, as `SquashMerge` would land it, under
// `refs/changes/<n>/merge`; the same two heads answer the Preview already made.
pub(crate) fn preview(
    ctx: &ExecCtx,
    actor: &Principal,
    repo: &str,
    n: u64,
) -> Result<OpReply, Error> {
    let made = previewed(ctx, require_named(actor)?, repo, n)?;
    Ok(OpReply::Previewed {
        height: ctx.env().height,
        n,
        oid: made.merge,
    })
}

// Change `n`'s Preview on its target's head now: the one made already while both heads stay,
// else a new squash `committer` commits, which watchers hear of.
pub(crate) fn previewed(
    ctx: &ExecCtx,
    committer: AccountNumber,
    repo: &str,
    n: u64,
) -> Result<PreviewInfo, Error> {
    let env = ctx.env();
    let mut record = load_repo(ctx, repo)?;
    let hash = repo_hash(&record);
    let change = load_change(ctx, repo, n)?;
    require_open(&change)?;
    let from = resolve(ctx, repo, &change.from, hash)?;
    let into = resolve(ctx, repo, &Revision::Ref(change.into.clone()), hash)?;
    let key = (repo.to_owned(), n);
    if let Some(made) = crate::state::PREVIEWS.get(ctx, &key)?
        && made.into_head == into.to_hex()
        && made.from_head == from.to_hex()
    {
        return Ok(made);
    }
    let message = message_of(None, Some(&change))?;
    let bounds = load_bounds(ctx)?;
    let objects = ObjectWriter::new(ctx, hash);
    let walk = cap(bounds.log_walk);
    let base = gitcore::walk::merge_base(&objects, &from, &into, walk)
        .map_err(refusal_of)?
        .ok_or_else(|| wrong_state("the change and its target share no history: no merge base"))?;
    if base == from {
        return Err(wrong_state(
            "the target already holds the change: nothing to preview",
        ));
    }
    let by = Principal::Account(committer);
    let committer = signature(ctx, committer)?;
    let author = match change.author.account() {
        Some(author) => signature(ctx, author)?,
        None => committer.clone(),
    };
    let made = Squashed {
        base,
        ours: into,
        theirs: from,
        author,
        committer,
        message,
    };
    let id = write_squash(objects, &bounds, hash, made)?;
    let reference = format!("refs/changes/{n}/merge").into_bytes();
    if !crate::state::REFS.has(ctx, &crate::state::ref_key(repo, &reference)) {
        record.refs_count += 1;
        crate::state::save_repo(ctx, repo, &record)?;
    }
    set_ref(ctx, repo, &reference, &id)?;
    let made = PreviewInfo {
        n,
        into_head: into.to_hex(),
        from_head: from.to_hex(),
        merge: id.to_hex(),
        height: env.height,
    };
    crate::state::PREVIEWS.put(ctx, &key, &made);
    let previewed = Watched::Previewed {
        repo: repo.to_owned(),
        n,
        merge: id.to_hex(),
        by,
    };
    crate::watchers::tell(ctx, &previewed)?;
    Ok(made)
}

// What `Rebase` carries besides its repository.
pub(crate) struct RebaseRequest {
    pub n: u64,
    pub onto: Revision,
    pub expected_head: String,
}

// Change `n` rebuilt on `onto`: what its head changed since their merge base, merged onto
// `onto`'s tree as one squash commit whose one parent is `onto`, and its source moved there
// under a CAS on its head. Its number, reviews and channel stay.
pub(crate) fn rebase(
    ctx: &ExecCtx,
    actor: &Principal,
    repo: &str,
    request: RebaseRequest,
) -> Result<OpReply, Error> {
    let env = ctx.env();
    let record = load_repo(ctx, repo)?;
    let hash = repo_hash(&record);
    let mut change = load_change(ctx, repo, request.n)?;
    require_open(&change)?;
    let head = parse_oid(hash, &request.expected_head)?;
    if resolve(ctx, repo, &change.from, hash)? != head {
        return Err(stale("the change's head moved; read it again"));
    }
    let moves_protected = match &change.from {
        Revision::Ref(branch) => protected(ctx, repo, &record, branch),
        Revision::Oid(_) => false,
    };
    if moves_protected {
        return Err(wrong_state(
            "the change's source is a protected branch, which no rebase moves",
        ));
    }
    let onto = resolve(ctx, repo, &request.onto, hash)?;
    let bounds = load_bounds(ctx)?;
    let objects = ObjectWriter::new(ctx, hash);
    let walk = cap(bounds.log_walk);
    let base = gitcore::walk::merge_base(&objects, &head, &onto, walk)
        .map_err(refusal_of)?
        .ok_or_else(|| wrong_state("the change and onto share no history: no merge base"))?;
    if base == head {
        return Err(wrong_state(
            "onto already holds the change: nothing to rebase",
        ));
    }
    if base == onto {
        return Err(wrong_state(
            "the change already sits on onto: nothing to rebase",
        ));
    }
    let committer = signature(ctx, require_named(actor)?)?;
    let author = match change.author.account() {
        Some(author) => signature(ctx, author)?,
        None => committer.clone(),
    };
    let made = Squashed {
        base,
        ours: onto,
        theirs: head,
        author,
        committer,
        message: rebased_message(&change),
    };
    let id = write_squash(objects, &bounds, hash, made)?;
    match &change.from {
        Revision::Ref(branch) => set_ref(ctx, repo, branch, &id)?,
        Revision::Oid(_) => change.from = Revision::Oid(id.to_hex()),
    }
    touched(&mut change, &env);
    fits(ctx, &change)?;
    save_change(ctx, repo, &change)?;
    crate::state::reshaped(ctx, repo, request.n)?;
    Ok(OpReply::Rebased {
        height: env.height,
        n: request.n,
        oid: id.to_hex(),
    })
}

// A rebase's message: the change's title, a blank line and its body.
fn rebased_message(change: &Change) -> Vec<u8> {
    let message = match change.body.trim() {
        "" => change.title.trim().to_owned(),
        body => format!("{}\n\n{body}", change.title.trim()),
    };
    let mut message = message.into_bytes();
    message.push(b'\n');
    message
}

// Change `n` takes `from` as its source, checked as `ChangeOpen` checks it.
pub(crate) fn repoint(ctx: &ExecCtx, repo: &str, n: u64, from: Revision) -> Result<OpReply, Error> {
    let env = ctx.env();
    let record = load_repo(ctx, repo)?;
    let mut change = load_change(ctx, repo, n)?;
    require_open(&change)?;
    check_endpoints(&change.into, &from)?;
    let hash = repo_hash(&record);
    let source = resolve(ctx, repo, &from, hash)?;
    let target = resolve(ctx, repo, &Revision::Ref(change.into.clone()), hash)?;
    if source == target {
        return Err(wrong_state("source and target already agree"));
    }
    change.from = match from {
        Revision::Oid(_) => Revision::Oid(source.to_hex()),
        from => from,
    };
    touched(&mut change, &env);
    fits(ctx, &change)?;
    save_change(ctx, repo, &change)?;
    crate::state::reshaped(ctx, repo, n)?;
    Ok(OpReply::Change {
        height: env.height,
        n,
    })
}

// The channels left unarchived before forge archived them as their changes ended: each merged
// or closed change of `repo` numbered above `after`, `limit` numbers of them, has its channel
// archived unless chat shows it archived already or holds none; a job room is left as it is. Run
// again, it archives nothing.
pub(crate) fn archive_ended(
    ctx: &ExecCtx,
    repo: &str,
    after: Option<u64>,
    limit: u64,
) -> Result<OpReply, Error> {
    let env = ctx.env();
    load_repo(ctx, repo)?;
    let page_size = u64::from(load_bounds(ctx)?.page_size);
    if !(1..=page_size).contains(&limit) {
        return Err(invalid(format!(
            "limit is 1..={page_size} (Bounds.page_size)"
        )));
    }
    let last = NUMBERS.get(ctx, &repo.to_owned())?.unwrap_or(0);
    let first = after.unwrap_or(0).saturating_add(1);
    let end = first.saturating_add(limit - 1).min(last);
    let mut archived = Vec::new();
    for n in first..=end {
        let Some(change) = CHANGES.get(ctx, &(repo.to_owned(), n))? else {
            continue;
        };
        let ended = change.state != ChangeState::Open;
        let own_room = !discussion::in_job_room(repo, &change);
        if ended && own_room && discussion::unarchived(ctx, &change)? {
            discussion::archive(ctx, repo, &change);
            archived.push(n);
        }
    }
    Ok(OpReply::ArchivedEnded {
        height: env.height,
        archived,
        next: (end < last).then_some(end),
    })
}

// The root tree of the commit `id`.
fn root_tree(objects: &impl Objects, id: &Oid) -> Result<Oid, Error> {
    let object = objects
        .get(id)
        .map_err(refusal_of)?
        .ok_or_else(|| object_not_held(id))?;
    if object.kind != Kind::Commit {
        return Err(invalid(format!("{id} is not a commit")));
    }
    Ok(Commit::parse(&object.body, id.hash())
        .map_err(refusal_of)?
        .tree)
}

// The squash's message: the one given, or the change's title and number, a blank line and
// its body.
fn message_of(given: Option<String>, change: Option<&Change>) -> Result<Vec<u8>, Error> {
    let message = match (given, change) {
        (Some(given), _) if given.trim().is_empty() => {
            return Err(invalid("a squash's message is nonblank"));
        }
        (Some(given), _) => given,
        (None, Some(change)) => match change.body.trim() {
            "" => format!("{} (change {})", change.title.trim(), change.n),
            body => format!("{} (change {})\n\n{body}", change.title.trim(), change.n),
        },
        (None, None) => {
            return Err(invalid(
                "a squash without a change needs a message to name it by",
            ));
        }
    };
    let mut message = message.into_bytes();
    if !message.ends_with(b"\n") {
        message.push(b'\n');
    }
    Ok(message)
}

// Who a squash names, as `forge_commit` signs an agent's commit: the account by its identity
// name (git's `<`, `>` and control characters left out) at `<number>@<chain id>`, at the
// block's time, so every validator writes the same bytes.
fn signature(ctx: &QueryCtx, account: AccountNumber) -> Result<Signature, Error> {
    use abi::role::identity::{Query, Reply};
    let identity = &ctx.env().role(abi::role::IDENTITY)?.clone();
    let profile = match ctx.query::<Query, Reply>(identity, &Query::Profile(account))? {
        Reply::Profile(profile) => profile,
        other => return Err(unexpected_reply(identity, "Profile", &other)),
    };
    let name: String = profile
        .map(|profile| profile.name)
        .unwrap_or_default()
        .chars()
        .filter(|c| !matches!(c, '<' | '>') && !c.is_control())
        .collect();
    let name = match name.trim() {
        "" => format!("account {account}"),
        name => name.to_owned(),
    };
    let env = ctx.env();
    Ok(Signature {
        name: name.into_bytes(),
        email: format!("{account}@{}", String::from_utf8_lossy(&env.chain_id)).into_bytes(),
        time: (env.time / 1000) as i64,
        offset_minutes: 0,
    })
}

// A conflict's refusal: every path, and why.
fn conflicted(conflicts: &[Conflict]) -> String {
    let named: Vec<String> = conflicts
        .iter()
        .map(|conflict| {
            let why = match conflict.clash {
                Clash::Lines => "both sides changed the same or adjacent lines",
                Clash::Searched => "edits too interleaved to merge within Bounds.merge_cost",
                Clash::Binary => "a binary file both sides changed",
                Clash::Mode => "both sides changed its mode",
                Clash::Deleted => "deleted on one side, changed on the other",
                Clash::Kind => "changed on both sides, and not as two files",
            };
            format!("{} ({why})", String::from_utf8_lossy(&conflict.path))
        })
        .collect();
    format!(
        "the squash conflicts, so nothing was written: {}",
        named.join("; ")
    )
}

fn touched(change: &mut Change, env: &Env) {
    change.updated_height = env.height;
    change.updated_time = env.time;
}

fn count_verdict(counts: &mut ReviewCounts, verdict: Verdict) -> Result<(), Error> {
    let count = match verdict {
        Verdict::Approve => &mut counts.approve,
        Verdict::RequestChanges => &mut counts.request_changes,
        Verdict::Comment => &mut counts.comment,
    };
    *count = next(*count)?;
    Ok(())
}

fn require_open(change: &Change) -> Result<(), Error> {
    if change.state != ChangeState::Open {
        return Err(wrong_state("change is not open"));
    }
    Ok(())
}

/// A record over `Bounds.record_bytes` is refused whole.
fn fits(ctx: &QueryCtx, record: &impl borsh::BorshSerialize) -> Result<(), Error> {
    let bound = load_bounds(ctx)?.record_bytes;
    if abi::encode(record).len() as u64 > bound {
        return Err(capacity(format!(
            "a record is at most {bound} bytes (Bounds.record_bytes)"
        )));
    }
    Ok(())
}

fn check_title(title: &str) -> Result<(), Error> {
    if title.trim().is_empty() {
        return Err(invalid("a title is nonblank"));
    }
    Ok(())
}

fn check_reviewers(ctx: &QueryCtx, reviewers: &[Principal]) -> Result<(), Error> {
    let mut seen = BTreeSet::new();
    for reviewer in reviewers {
        ctx.require_account(require_named(reviewer)?)?;
        if !seen.insert(reviewer) {
            return Err(invalid("each reviewer is asked once"));
        }
    }
    Ok(())
}

/// Both ends of a change or a merge are branches under `refs/heads/`.
fn check_endpoints(into: &[u8], from: &Revision) -> Result<(), Error> {
    check_branch(into)?;
    if let Revision::Ref(name) = from {
        check_branch(name)?;
    }
    Ok(())
}

pub(crate) fn check_branch(name: &[u8]) -> Result<(), Error> {
    if !name.starts_with(b"refs/heads/") || !gitcore::server::valid_ref_name(name) {
        return Err(invalid(
            "change endpoints must name branches under refs/heads/",
        ));
    }
    Ok(())
}

fn check_comments(draft: &ReviewDraft) -> Result<(), Error> {
    let mut anchors = BTreeSet::new();
    for comment in &draft.comments {
        check_path(&comment.path, false)?;
        let anchored = comment.line > 0
            && !comment.body.trim().is_empty()
            && (comment.side == Side::New || draft.base_oid.is_some());
        if !anchored {
            return Err(invalid(
                "comments need a positive line, nonblank body, and an old-side base",
            ));
        }
        if !anchors.insert((&comment.path, comment.side, comment.line)) {
            return Err(invalid("duplicate line anchor in one review"));
        }
    }
    let says_nothing = draft.verdict == Verdict::Comment
        && draft.body.trim().is_empty()
        && draft.comments.is_empty();
    if says_nothing {
        return Err(invalid("a comment review needs text or line comments"));
    }
    Ok(())
}

/// A relative git path with no empty, `.` or `..` component; `root` admits
/// the empty path (a tree's root).
pub fn check_path(path: &[u8], root: bool) -> Result<(), Error> {
    if root && path.is_empty() {
        return Ok(());
    }
    let well_formed = !path.contains(&0)
        && path
            .split(|b| *b == b'/')
            .all(|part| !part.is_empty() && part != b"." && part != b"..");
    if !well_formed {
        return Err(invalid(
            "path must be a relative Git path without empty, dot or dot-dot components",
        ));
    }
    Ok(())
}
