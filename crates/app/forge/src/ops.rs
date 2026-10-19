//! The execute path: who acts, which op, what it needs of the access program, and the
//! repository ops (create, configure, push, stage, protect, delete a branch). Change ops live
//! in `changes`.

use ::program::{AccountNumber, BlobId, Error, HashKind, Need, Party, Resource};
use ::program::{
    ExecCtx, QueryCtx, already_exists, capacity, decoded, invalid, not_found, stale, unauthorized,
    wrong_state,
};
use gitcore::server::{Policy, RefUpdate};
use gitcore::{Error as GitError, Limits, server};

use crate::changes::check_branch;
use crate::contract::{
    Bounds, Op, PACK_KIND, Principal, Repo, Settings, change_key, repo_key, settings_key,
    valid_repo_name,
};
use crate::objects::{ObjectWriter, object_not_held};
use crate::state::{
    PROTECTED, delete_ref, load_bounds, load_change, load_ref, load_refs, load_repo, parse_oid,
    protected, ref_key, repo_exists, repo_hash, save_bounds, save_repo, set_ref, sourced_by,
    storage,
};

pub const PROGRAM: &str = "forge";

pub(crate) fn init(ctx: &ExecCtx, params: &[u8]) -> Result<(), Error> {
    let bounds: Bounds = decoded(PROGRAM, "Bounds", params)?;
    let usable = bounds.page_size > 0
        && bounds.log_walk > 0
        && bounds.tree_walk > 0
        && bounds.blob_bytes > 0
        && bounds.record_bytes > 0
        && bounds.diff_bytes >= bounds.blob_bytes;
    if !usable {
        return Err(invalid(
            "bounds need a positive page size and positive read/record budgets; diff_bytes >= blob_bytes",
        ));
    }
    save_bounds(ctx, &bounds);
    Ok(())
}

/// Every accepted op marks its repository active at this height and counts
/// as a write, which every listing cursor is pinned to.
pub(crate) fn touch(ctx: &ExecCtx, name: &str, height: u64) -> Result<(), Error> {
    let mut repo = load_repo(ctx, name)?;
    repo.last_activity = height;
    save_repo(ctx, name, &repo)?;
    crate::state::wrote(ctx)
}

pub(crate) fn create(
    ctx: &ExecCtx,
    actor: &Principal,
    name: &str,
    hash: HashKind,
) -> Result<(), Error> {
    if !valid_repo_name(name) {
        return Err(invalid(format!("{name:?} is not a repository name")));
    }
    if repo_exists(ctx, name) {
        return Err(already_exists(format!("a repository named {name} exists")));
    }
    let repo = Repo {
        hash,
        owner: actor.clone(),
        settings: Settings::default(),
        refs_count: 0,
        last_activity: 0,
    };
    let custody = crate::state::Custody {
        owner: actor.clone(),
        rev: 0,
    };
    crate::state::CUSTODY.put(ctx, &name.to_owned(), &custody);
    save_repo(ctx, name, &repo)
}

pub(crate) fn configure(ctx: &ExecCtx, name: &str, settings: Settings) -> Result<(), Error> {
    let mut repo = load_repo(ctx, name)?;
    let head_is_a_ref = server::valid_ref_name(&settings.head);
    if !head_is_a_ref {
        return Err(invalid("head names a ref under refs/"));
    }
    repo.settings = settings;
    save_repo(ctx, name, &repo)
}

pub(crate) fn retired() -> Error {
    unauthorized(
        "a repository's writers are its list in the access program: set it there, as the owner",
    )
}

// The repository whose policy decides `op`, if any: the ops `needs` asks a repository's list for
// (its `repo_key` or `settings_key`), so that each writes the repository's custody.
pub(crate) fn governed<'o>(ctx: &QueryCtx, op: &'o Op) -> Option<&'o str> {
    match op {
        Op::Configure { repo, .. }
        | Op::Protect { repo, .. }
        | Op::RequireChecks { repo, .. }
        | Op::Push { repo, .. }
        | Op::Merge { repo, .. }
        | Op::Stage { repo, .. }
        | Op::SquashMerge { repo, .. }
        | Op::DeleteBranch { repo, .. }
        | Op::Enqueue { repo, .. }
        | Op::Dequeue { repo, .. }
        | Op::Rebase { repo, .. } => Some(repo),
        Op::ChangeClose { repo, n } => {
            let sender = ctx.env().sender.clone();
            let authored = load_change(ctx, repo, *n)
                .is_ok_and(|change| sender.as_ref() == Some(&change.author));
            (!authored).then_some(repo.as_str())
        }
        Op::Create { .. }
        | Op::Grant { .. }
        | Op::Revoke { .. }
        | Op::ChangeOpen { .. }
        | Op::ChangeEdit { .. }
        | Op::ChangeRepoint { .. }
        | Op::ReviewSubmit { .. }
        | Op::Watch { .. }
        | Op::ArchiveEnded { .. }
        | Op::Preview { .. }
        | Op::Status { .. } => None,
    }
}

pub(crate) fn needs(ctx: &QueryCtx, op: &Op) -> Result<Vec<Need>, Error> {
    let needs = match op {
        Op::Configure { repo, .. } | Op::Protect { repo, .. } | Op::RequireChecks { repo, .. } => {
            vec![Need::write(settings_key(repo))]
        }
        Op::Push { repo, .. }
        | Op::Merge { repo, .. }
        | Op::Stage { repo, .. }
        | Op::SquashMerge { repo, .. }
        | Op::DeleteBranch { repo, .. }
        | Op::Enqueue { repo, .. }
        | Op::Dequeue { repo, .. } => {
            vec![Need::write(repo_key(repo))]
        }
        Op::ChangeClose { repo, n } => {
            let sender = ctx.env().sender.clone();
            let authored = load_change(ctx, repo, *n)
                .is_ok_and(|change| sender.as_ref() == Some(&change.author));
            match authored {
                true => Vec::new(),
                false => vec![Need::write(repo_key(repo))],
            }
        }
        Op::ChangeEdit { repo, n, .. } | Op::ChangeRepoint { repo, n, .. } => {
            match load_change(ctx, repo, *n) {
                Ok(_) => vec![Need::write(change_key(repo, *n))],
                Err(_) => Vec::new(),
            }
        }
        // a rebase rewrites the change, as an edit does, and moves a ref, as a push does
        Op::Rebase { repo, n, .. } => {
            let mut needs = vec![Need::write(repo_key(repo))];
            if load_change(ctx, repo, *n).is_ok() {
                needs.insert(0, Need::write(change_key(repo, *n)));
            }
            needs
        }
        Op::Create { .. }
        | Op::Grant { .. }
        | Op::Revoke { .. }
        | Op::ChangeOpen { .. }
        | Op::ReviewSubmit { .. }
        | Op::Watch { .. }
        | Op::ArchiveEnded { .. }
        | Op::Preview { .. }
        | Op::Status { .. } => Vec::new(),
    };
    Ok(needs)
}

pub(crate) fn resource(ctx: &QueryCtx, key: &str) -> Result<Resource, Error> {
    if let Some((repo, n)) = change_of(key) {
        return change_resource(ctx, repo, n);
    }
    let (name, default) = match key.strip_suffix("/settings") {
        Some(name) => (name, None),
        None => (key, Some(::program::Ownership::Anyone)),
    };
    if !repo_exists(ctx, name) {
        return Ok(Resource::default());
    }
    let owner = load_repo(ctx, name)?.owner.account().map(Party::Account);
    Ok(Resource { owner, default })
}

/// A git receive-pack: once the pack unpacks, its objects land as blobs and
/// each accepted ref moves; git's own report is the op's output. The
/// objects land even when every ref is refused, as git's receive-pack keeps
/// what it unpacked: a stage stores objects no ref reaches as well, and a
/// client that pushes again after a refusal (a branch moved meanwhile)
/// sends less. A pack that does not unpack stores nothing.
pub(crate) fn push(
    ctx: &ExecCtx,
    name: &str,
    request: &[u8],
    pack: Option<BlobId>,
) -> Result<(), Error> {
    let mut repo = load_repo(ctx, name)?;
    let pack = match pack {
        Some(id) => held_pack(ctx, id)?,
        None => Vec::new(),
    };
    let bounds = load_bounds(ctx)?;
    let hash = repo_hash(&repo);
    let refs = load_refs(ctx, name, hash)?;
    let policy = Policy {
        allow_force: repo.settings.allow_force,
        allow_delete: repo.settings.allow_delete,
    };
    let mut objects = ObjectWriter::new(ctx, hash);
    let outcome = server::push(
        &mut objects,
        &refs,
        request,
        &pack,
        hash,
        &limits_of(&bounds),
        &policy,
        cap(bounds.push_walk),
    )
    .map_err(refusal_of)?;
    if outcome.unpack_error.is_none() {
        objects.flush()?;
    }
    for (reference, _) in &outcome.moves {
        crate::checks::refuse_push(ctx, name, reference)?;
    }
    for (reference, update) in &outcome.moves {
        match update {
            RefUpdate::Set(target) => {
                if !refs.contains_key(reference) {
                    repo.refs_count += 1;
                }
                set_ref(ctx, name, reference, target)?;
            }
            RefUpdate::Delete => {
                repo.refs_count -= 1;
                delete_ref(ctx, name, reference)?;
            }
        }
    }
    save_repo(ctx, name, &repo)?;
    ctx.set_return_data(outcome.report);
    Ok(())
}

// A branch goes when it is still at `expected`, no protection covers it and no open change
// names it as its source; `allow_delete` governs only what a git push deletes. Its objects
// stay, so it can be pushed again.
pub(crate) fn delete_branch(
    ctx: &ExecCtx,
    name: &str,
    branch: &[u8],
    expected: &str,
) -> Result<(), Error> {
    let mut repo = load_repo(ctx, name)?;
    check_branch(branch)?;
    let hash = repo_hash(&repo);
    let expected = parse_oid(hash, expected)?;
    let shown = String::from_utf8_lossy(branch);
    match load_ref(ctx, name, branch, hash)? {
        None => return Err(not_found(format!("no branch {shown}"))),
        Some(at) if at != expected => {
            return Err(stale(format!("{shown} moved; read it again")));
        }
        Some(_) => {}
    }
    if protected(ctx, name, &repo, branch) {
        return Err(wrong_state(format!("{shown} is protected")));
    }
    let named = sourced_by(ctx, name, branch)?;
    if !named.is_empty() {
        let changes: Vec<String> = named.iter().map(|n| format!("#{n}")).collect();
        return Err(wrong_state(format!(
            "open change {} names {shown} as its source",
            changes.join(", ")
        )));
    }
    repo.refs_count -= 1;
    delete_ref(ctx, name, branch)?;
    save_repo(ctx, name, &repo)
}

// Marks `branch` protected or not. The head is protected whatever the set holds, so it is
// never unprotected.
pub(crate) fn protect(
    ctx: &ExecCtx,
    name: &str,
    branch: &[u8],
    protected: bool,
) -> Result<(), Error> {
    let repo = load_repo(ctx, name)?;
    check_branch(branch)?;
    let key = ref_key(name, branch);
    match protected {
        true => PROTECTED.insert(ctx, &key),
        false if repo.settings.head == branch => {
            return Err(wrong_state("the repository's head is always protected"));
        }
        false => PROTECTED.remove(ctx, &key),
    }
    Ok(())
}

/// The objects half of a push: the pack's objects land as blobs, checked as
/// a push checks them (bounds, deltas against what is held, every object
/// they name held), and no ref moves.
pub(crate) fn stage(ctx: &ExecCtx, name: &str, pack: BlobId) -> Result<(), Error> {
    let repo = load_repo(ctx, name)?;
    let pack = held_pack(ctx, pack)?;
    let bounds = load_bounds(ctx)?;
    let hash = repo_hash(&repo);
    let mut objects = ObjectWriter::new(ctx, hash);
    server::admit_pack(&mut objects, &pack, hash, &limits_of(&bounds)).map_err(refusal_of)?;
    objects.flush()
}

fn held_pack(ctx: &ExecCtx, id: BlobId) -> Result<Vec<u8>, Error> {
    let Some(blob) = ctx.blob_get(id) else {
        return Err(not_found(format!(
            "pack {id:?} is not held: put it as a {PACK_KIND:?} blob and wait for a quorum of validators to hold it"
        )));
    };
    if blob.kind != PACK_KIND {
        return Err(invalid(format!(
            "pack {id:?} is a {:?} blob, not {PACK_KIND:?}",
            blob.kind
        )));
    }
    Ok(blob.body)
}

/// Whom an op names (a writer, a reviewer): an account, never the system.
pub(crate) fn require_named(principal: &Principal) -> Result<AccountNumber, Error> {
    principal
        .account()
        .ok_or_else(|| invalid("only an account is named here"))
}

pub fn limits_of(bounds: &Bounds) -> Limits {
    Limits {
        max_objects: cap(bounds.max_objects),
        max_delta_depth: cap(bounds.max_delta_depth),
        max_object_size: cap(bounds.max_object_size),
    }
}

pub fn cap(bound: u64) -> usize {
    usize::try_from(bound).unwrap_or(usize::MAX)
}

pub fn refusal_of(error: GitError) -> Error {
    match error {
        GitError::Storage => storage("the blob ctx refused a write"),
        GitError::CapReached | GitError::ObjectTooLarge => {
            capacity("query or operation exceeds its configured work/byte bound")
        }
        GitError::MissingObject(id) | GitError::MissingBase(id) => object_not_held(id),
        other => invalid(other.to_string()),
    }
}

fn change_of(key: &str) -> Option<(&str, u64)> {
    let (repo, n) = key.split_once("/changes/")?;
    Some((repo, n.parse().ok()?))
}

fn change_resource(ctx: &QueryCtx, repo: &str, n: u64) -> Result<Resource, Error> {
    let Ok(change) = load_change(ctx, repo, n) else {
        return Ok(Resource::default());
    };
    let owner = match change.author {
        Principal::Account(number) => Some(Party::Account(number)),
        Principal::Root => None,
    };
    Ok(Resource {
        owner,
        default: Some(::program::Ownership::Owner),
    })
}
