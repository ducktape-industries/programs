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
    AUTHORSHIP, Authorship, load_bounds, load_change, load_ref, load_repo, next, next_message,
    next_number, parse_oid, peek_message, protected, repo_hash, resolve, save_change, save_review,
    set_ref,
};

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
        return Err(wrong_state(
            "source and target already agree: choose a source with changes absent from the target",
        ));
    }
    if matches!(draft.from, Revision::Oid(_)) {
        draft.from = Revision::Oid(source.to_hex());
    }
    let n = next_number(ctx, repo)?;
    let channel = crate::state::channel_id(repo, n);
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
    enforce_record_size(ctx, &change)?;
    let message = next_message(ctx)?;
    save_change(ctx, repo, &change)?;
    let authorship = Authorship {
        author: actor.clone(),
        rev: 0,
    };
    AUTHORSHIP.put(ctx, &(repo.to_owned(), n), &authorship);
    discussion::create(ctx, repo, &change);
    crate::involved::seat_opened(ctx, &change);
    discussion::post(ctx, repo, &change, message, Event::Opened, Some(actor))?;
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
    let mut asked = Vec::new();
    if let Some(reviewers) = fields.reviewers {
        check_reviewers(ctx, &reviewers)?;
        asked = reviewers
            .iter()
            .filter(|reviewer| !change.reviewers.contains(reviewer))
            .cloned()
            .collect();
        change.reviewers = reviewers;
    }
    touched(&mut change, &env);
    enforce_record_size(ctx, &change)?;
    save_change(ctx, repo, &change)?;
    crate::state::reshaped(ctx, repo, n)?;
    crate::involved::seat(ctx, repo, &change, asked.iter().cloned())?;
    discussion::requested(ctx, repo, &change, asked)?;
    Ok(OpReply::Change {
        height: env.height,
        n,
    })
}

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
    discussion::post(ctx, repo, &change, message, Event::Closed, Some(actor))?;
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
    enforce_record_size(ctx, &review)?;
    change.review_count = id;
    change.comment_count = change
        .comment_count
        .checked_add(review.draft.comments.len() as u64)
        .ok_or_else(|| capacity("comment counter exhausted"))?;
    count_verdict(&mut change.verdicts, review.draft.verdict)?;
    touched(&mut change, &env);
    change.system_seq = next(change.system_seq)?;
    enforce_record_size(ctx, &change)?;
    let message = next_message(ctx)?;
    save_review(ctx, repo, n, &review);
    save_change(ctx, repo, &change)?;
    let reviewed = Event::Reviewed {
        id,
        verdict: review.draft.verdict,
    };
    discussion::post(ctx, repo, &change, message, reviewed, Some(actor))?;
    crate::involved::seat(ctx, repo, &change, [actor.clone()])?;
    let told = Watched::Reviewed {
        repo: repo.to_owned(),
        n,
        id,
        verdict: review.draft.verdict,
        by: actor.clone(),
    };
    crate::watchers::tell(ctx, &told)?;
    Ok(OpReply::Review {
        height: env.height,
        n,
        id,
    })
}

pub(crate) struct MergeRequest {
    pub into: Vec<u8>,
    pub from: Revision,
    pub expected_into: String,
    pub expected_from: String,
    pub result: String,
    pub change: Option<u64>,
}

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
    heads_still_at_merge_inputs(ctx, repo, hash, ends, (expected_into, expected_from))?;
    let heads = (&expected_into, &expected_from);
    crate::checks::gate(ctx, repo, &merge.into, merge.change, heads)?;
    if result == expected_into || expected_from == expected_into {
        return Err(wrong_state("merge does not advance the target"));
    }
    if let Some(n) = merge.change {
        let change = open_change_on_endpoints(ctx, repo, hash, n, ends)?;
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

fn heads_still_at_merge_inputs(
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

fn open_change_on_endpoints(
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
    enforce_record_size(ctx, &change)?;
    let message = next_message(ctx)?;
    save_change(ctx, repo, &change)?;
    discussion::post(ctx, repo, &change, message, Event::Merged, Some(actor))?;
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

pub(crate) struct SquashRequest {
    pub into: Vec<u8>,
    pub from: Revision,
    pub expected_into: String,
    pub expected_from: String,
    pub message: Option<String>,
    pub change: Option<u64>,
}

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
    heads_still_at_merge_inputs(ctx, repo, hash, ends, (expected_into, expected_from))?;
    let heads = (&expected_into, &expected_from);
    crate::checks::gate(ctx, repo, &squash.into, squash.change, heads)?;
    if expected_from == expected_into {
        return Err(wrong_state("merge does not advance the target"));
    }
    let change = match squash.change {
        Some(n) => Some(open_change_on_endpoints(ctx, repo, hash, n, ends)?),
        None => None,
    };
    let message = squash_message(squash.message, change.as_ref())?;
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
    let committer = squash_signature_at_block_time(ctx, require_named(actor)?)?;
    let author = match change.as_ref().and_then(|change| change.author.account()) {
        Some(author) => squash_signature_at_block_time(ctx, author)?,
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

struct Squashed {
    base: Oid,
    ours: Oid,
    theirs: Oid,
    author: Signature,
    committer: Signature,
    message: Vec<u8>,
}

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
        return Err(wrong_state(conflict_paths_and_reasons(&merge.conflicts)));
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
    let message = squash_message(None, Some(&change))?;
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
    let committer = squash_signature_at_block_time(ctx, committer)?;
    let author = match change.author.account() {
        Some(author) => squash_signature_at_block_time(ctx, author)?,
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

pub(crate) struct RebaseRequest {
    pub n: u64,
    pub onto: Revision,
    pub expected_head: String,
}

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
            "the change's source is a protected branch, which no rebase moves: choose an unprotected source branch",
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
    let committer = squash_signature_at_block_time(ctx, require_named(actor)?)?;
    let author = match change.author.account() {
        Some(author) => squash_signature_at_block_time(ctx, author)?,
        None => committer.clone(),
    };
    let made = Squashed {
        base,
        ours: onto,
        theirs: head,
        author,
        committer,
        message: rebase_title_and_body(&change),
    };
    let id = write_squash(objects, &bounds, hash, made)?;
    match &change.from {
        Revision::Ref(branch) => set_ref(ctx, repo, branch, &id)?,
        Revision::Oid(_) => change.from = Revision::Oid(id.to_hex()),
    }
    touched(&mut change, &env);
    enforce_record_size(ctx, &change)?;
    save_change(ctx, repo, &change)?;
    crate::state::reshaped(ctx, repo, request.n)?;
    if let Revision::Oid(_) = &change.from {
        discussion::head_moved(ctx, repo, &change, &id.to_hex())?;
    }
    Ok(OpReply::Rebased {
        height: env.height,
        n: request.n,
        oid: id.to_hex(),
    })
}

fn rebase_title_and_body(change: &Change) -> Vec<u8> {
    let message = match change.body.trim() {
        "" => change.title.trim().to_owned(),
        body => format!("{}\n\n{body}", change.title.trim()),
    };
    let mut message = message.into_bytes();
    message.push(b'\n');
    message
}

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
        return Err(wrong_state(
            "source and target already agree: choose a source with changes absent from the target",
        ));
    }
    let old_head = resolve(ctx, repo, &change.from, hash).ok();
    change.from = match from {
        Revision::Oid(_) => Revision::Oid(source.to_hex()),
        from => from,
    };
    touched(&mut change, &env);
    enforce_record_size(ctx, &change)?;
    save_change(ctx, repo, &change)?;
    crate::state::reshaped(ctx, repo, n)?;
    if old_head.as_ref() != Some(&source) {
        discussion::head_moved(ctx, repo, &change, &source.to_hex())?;
    }
    Ok(OpReply::Change {
        height: env.height,
        n,
    })
}

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

fn squash_message(given: Option<String>, change: Option<&Change>) -> Result<Vec<u8>, Error> {
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

fn squash_signature_at_block_time(
    ctx: &QueryCtx,
    account: AccountNumber,
) -> Result<Signature, Error> {
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

fn conflict_paths_and_reasons(conflicts: &[Conflict]) -> String {
    let named: Vec<String> = conflicts
        .iter()
        .map(|conflict| {
            let why = match conflict.clash {
                Clash::Lines => "both sides changed the same or adjacent lines",
                Clash::Searched => "edits too interleaved to merge within Bounds.merge_cost: resolve the interleaved edits with git and push the result",
                Clash::Binary => "a binary file both sides changed: choose the desired binary version and push the resolved merge",
                Clash::Mode => "both sides changed its mode",
                Clash::Deleted => "deleted on one side, changed on the other: choose whether to keep the file and push the resolved merge",
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
        return Err(wrong_state(
            "change is not open: open a new change for further work",
        ));
    }
    Ok(())
}

fn enforce_record_size(ctx: &QueryCtx, record: &impl borsh::BorshSerialize) -> Result<(), Error> {
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
            return Err(invalid(
                "duplicate line anchor in one review: combine comments at the same line anchor",
            ));
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
