//! Every table forge keeps, declared once: the records, the indexes over
//! them and the counters. Git objects are not here: an object's blob id is
//! its oid (`objects`). People are [`Principal`]s: accounts.

use std::collections::BTreeMap;

use ::program::{Error, ProgramId, code};
use ::program::{ExecCtx, QueryCtx, invalid, not_found};
use gitcore::{Hash, Oid};
use store::{Item, Kind, Map, Owned, Set};

use crate::contract::{
    Bounds, Change, ChangeState, CommitStatus, PreviewInfo, Principal, Repo, Required, Review,
    Revision, Watched, valid_repo_name,
};
use crate::objects::hash_of;

/// The bounds forge was founded with.
pub const BOUNDS: Item<Bounds> = Item::new("bounds").shared();
/// How many ops forge has accepted. Every listing cursor is pinned to it:
/// only a forge op rewrites a listing, and two ops in one block share a
/// height but not a count.
pub const WRITES: Item<u64> = Item::new("writes").shared();
/// One record per repository, by name.
pub const REPOS: Map<String, Repo> = Map::new("p/").shared();
/// Index: every repository by its last activity, newest first.
pub const ACTIVITY: Set<(u64, String)> = Set::new("a/").shared();
/// Each repository's refs and the oid bytes each points at.
pub const REFS: Map<(String, Vec<u8>), Vec<u8>> = Map::new("r/").shared();
/// The last change number each repository gave out.
pub const NUMBERS: Map<String, u64> = Map::new("n/").shared();
/// Changes by repository and number; a scan across repositories lists them
/// by name (Judgment pages this table whole).
pub const CHANGES: Map<(String, u64), Change> = Map::new("c/").shared();
/// Reviews by repository, change number and review id.
// Each review is its author's, of the kind `forge.review`: no op rewrites or deletes one.
pub const REVIEWS: Map<(String, u64, u64), Owned<Review>> = Map::new("v/");
/// Index: the reviews one principal submitted on one change, oldest first.
pub const AUTHORED: Set<(String, u64, Principal, u64)> = Set::new("u/").shared();
/// Index: the changes a principal authored, is asked to review, or reviewed.
pub const INVOLVED: Set<(Principal, String, u64)> = Set::new("i/").shared();
/// The last system message id forge posted into chat.
pub const MESSAGES: Item<u64> = Item::new("system-message-seq").shared();
// Each repository's protected branches besides its head, which always is: no rebase moves
// one and no `DeleteBranch` deletes one.
pub const PROTECTED: Set<(String, Vec<u8>)> = Set::new("b/").shared();
// The programs that watch forge, each with the prefix it hears events under.
pub const WATCHERS: Map<ProgramId, Vec<u8>> = Map::new("w/").shared();
// The archives forge asked chat for, by the message's seq, each naming its change. chat
// answers in the frame that asked, which takes the row away again.
pub const ARCHIVING: Map<u64, (String, u64)> = Map::new("x/").shared();
// The lines forge posted to say chat refused an archive, by the message's seq, until chat
// answers them (in the same frame).
pub const LOGGING: Set<u64> = Set::new("l/").shared();
// Each change's latest Preview.
pub const PREVIEWS: Map<(String, u64), PreviewInfo> = Map::new("m/").shared();
// Every status a reporter gave a commit: by repository, commit, reporter and context.
pub const STATUSES: Map<(String, String, Principal, String), CommitStatus> =
    Map::new("s/").shared();
// The branches that move only by a merge whose Preview is green.
pub const REQUIRED: Map<(String, Vec<u8>), Required> = Map::new("q/").shared();

/// Stored state that is not what forge wrote: an operator's problem.
pub(crate) fn storage(sentence: impl Into<String>) -> Error {
    Error::new(code::CORRUPT, sentence)
}

pub fn save_bounds(ctx: &ExecCtx, bounds: &Bounds) {
    BOUNDS.put(ctx, bounds);
}

pub fn load_bounds(ctx: &QueryCtx) -> Result<Bounds, Error> {
    BOUNDS
        .get(ctx)?
        .ok_or_else(|| Error::new(code::PROTOCOL, "the program was founded without bounds"))
}

pub fn repo_exists(ctx: &QueryCtx, name: &str) -> bool {
    REPOS.has(ctx, &name.to_owned())
}

pub fn load_repo(ctx: &QueryCtx, name: &str) -> Result<Repo, Error> {
    if !valid_repo_name(name) {
        return Err(invalid(format!("{name:?} is not a repository name")));
    }
    REPOS
        .get(ctx, &name.to_owned())?
        .ok_or_else(|| not_found(format!("no repository named {name}")))
}

/// Stores the record and moves it in the activity index, so the index
/// holds exactly one row per repository.
pub fn save_repo(ctx: &ExecCtx, name: &str, repo: &Repo) -> Result<(), Error> {
    if let Some(old) = REPOS.get(ctx, &name.to_owned())? {
        ACTIVITY.remove(ctx, &(newest_first(old.last_activity), name.to_owned()));
    }
    ACTIVITY.insert(ctx, &(newest_first(repo.last_activity), name.to_owned()));
    REPOS.put(ctx, &name.to_owned(), repo);
    Ok(())
}

/// How many ops forge has accepted so far (0 before any).
pub(crate) fn writes(ctx: &QueryCtx) -> Result<u64, Error> {
    Ok(WRITES.get(ctx)?.unwrap_or(0))
}

/// Counts an accepted op.
pub(crate) fn wrote(ctx: &ExecCtx) -> Result<(), Error> {
    WRITES.put(ctx, &(writes(ctx)? + 1));
    Ok(())
}

/// An activity key: a later height sorts first.
fn newest_first(height: u64) -> u64 {
    u64::MAX - height
}

pub fn repo_hash(repo: &Repo) -> Hash {
    hash_of(repo.hash)
}

pub fn ref_key(name: &str, reference: &[u8]) -> (String, Vec<u8>) {
    (name.to_owned(), reference.to_vec())
}

/// Every ref of a repository: what a push is checked against and git is told.
pub fn load_refs(ctx: &QueryCtx, name: &str, hash: Hash) -> Result<BTreeMap<Vec<u8>, Oid>, Error> {
    REFS.scan(ctx, REFS.prefix_of(&name.to_owned()))?
        .into_iter()
        .map(|((_, reference), bytes)| {
            let target = Oid::from_bytes(hash, &bytes).map_err(|error| {
                Error::new(code::PROTOCOL, format!("ref {reference:?} holds {error}"))
            })?;
            Ok((reference, target))
        })
        .collect()
}

pub fn load_ref(
    ctx: &QueryCtx,
    name: &str,
    reference: &[u8],
    hash: Hash,
) -> Result<Option<Oid>, Error> {
    REFS.get(ctx, &ref_key(name, reference))?
        .map(|bytes| Oid::from_bytes(hash, &bytes).map_err(|e| storage(e.to_string())))
        .transpose()
}

pub fn set_ref(ctx: &ExecCtx, name: &str, reference: &[u8], target: &Oid) -> Result<(), Error> {
    let key = ref_key(name, reference);
    let old = REFS.get(ctx, &key)?;
    REFS.put(ctx, &key, &target.as_bytes().to_vec());
    moved(ctx, name, reference, old, Some(target.to_hex()))
}

pub fn delete_ref(ctx: &ExecCtx, name: &str, reference: &[u8]) -> Result<(), Error> {
    let key = ref_key(name, reference);
    let old = REFS.get(ctx, &key)?;
    REFS.remove(ctx, &key);
    moved(ctx, name, reference, old, None)
}

// Every ref move tells the watchers, whichever op made it.
fn moved(
    ctx: &ExecCtx,
    name: &str,
    reference: &[u8],
    old: Option<Vec<u8>>,
    new: Option<String>,
) -> Result<(), Error> {
    let hex =
        |bytes: Vec<u8>| -> String { bytes.iter().map(|byte| format!("{byte:02x}")).collect() };
    let event = Watched::Moved {
        repo: name.to_owned(),
        reference: reference.to_vec(),
        old: old.map(hex),
        new,
        by: ctx.env().sender.clone(),
    };
    crate::watchers::tell(ctx, &event)
}

// Whether `branch` is protected in the repository `name`: its head always is.
pub fn protected(ctx: &QueryCtx, name: &str, repo: &Repo, branch: &[u8]) -> bool {
    repo.settings.head == branch || PROTECTED.has(ctx, &ref_key(name, branch))
}

// The open changes of the repository `name` whose source is `branch`.
pub fn sourced_by(ctx: &QueryCtx, name: &str, branch: &[u8]) -> Result<Vec<u64>, Error> {
    let source = Revision::Ref(branch.to_vec());
    let changes = CHANGES.scan(ctx, CHANGES.prefix_of(&name.to_owned()))?;
    Ok(changes
        .into_iter()
        .filter(|(_, change)| change.state == ChangeState::Open && change.from == source)
        .map(|((_, n), _)| n)
        .collect())
}

/// Resolving an op's endpoint reads consensus refs only, never objects.
pub fn resolve(ctx: &QueryCtx, name: &str, revision: &Revision, hash: Hash) -> Result<Oid, Error> {
    match revision {
        Revision::Oid(hex) => parse_oid(hash, hex),
        Revision::Ref(reference) => {
            if !gitcore::server::valid_ref_name(reference) {
                return Err(invalid("revision must name a full ref"));
            }
            let missing = || {
                let shown = String::from_utf8_lossy(reference);
                not_found(format!("repository {name} has no ref {shown}"))
            };
            load_ref(ctx, name, reference, hash)?.ok_or_else(missing)
        }
    }
}

pub fn parse_oid(hash: Hash, hex: &str) -> Result<Oid, Error> {
    let oid = Oid::from_hex(hash, hex)
        .map_err(|_| invalid("oid has the wrong length or hex for this repo"))?;
    if oid.is_zero() {
        return Err(invalid("an object id cannot be zero"));
    }
    Ok(oid)
}

/// The number a new change of this repository takes. Issues would share it.
pub fn next_number(ctx: &QueryCtx, name: &str) -> Result<u64, Error> {
    next(NUMBERS.get(ctx, &name.to_owned())?.unwrap_or(0))
}

/// The id the next system line forge posts into chat takes, unclaimed.
pub fn peek_message(ctx: &QueryCtx) -> Result<String, Error> {
    Ok(message_id(next_message_number(ctx)?))
}

/// Claims the next id of a system line forge posts into chat.
pub fn next_message(ctx: &ExecCtx) -> Result<String, Error> {
    let n = next_message_number(ctx)?;
    MESSAGES.put(ctx, &n);
    Ok(message_id(n))
}

fn next_message_number(ctx: &QueryCtx) -> Result<u64, Error> {
    MESSAGES
        .get(ctx)?
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| Error::new(code::EXHAUSTED, "system message counter exhausted"))
}

fn message_id(n: u64) -> String {
    chat::namespace::id(crate::PROGRAM, &format!("{n:016x}"))
}

/// Change `n`'s hidden chat channel: `forge:<repo>:<n>`.
pub(crate) fn channel_id(repo: &str, n: u64) -> String {
    chat::namespace::id(crate::PROGRAM, &format!("{repo}:{n}"))
}

/// A counter one step on, refused rather than wrapped.
pub fn next(n: u64) -> Result<u64, Error> {
    n.checked_add(1)
        .ok_or_else(|| Error::new(code::EXHAUSTED, "counter exhausted"))
}

pub fn load_change(ctx: &QueryCtx, repo: &str, n: u64) -> Result<Change, Error> {
    CHANGES
        .get(ctx, &(repo.to_owned(), n))?
        .ok_or_else(|| not_found(format!("no change {repo}#{n}")))
}

/// Stores the change and keeps [`INVOLVED`] in step with it: its author and
/// every requested reviewer are involved; a reviewer taken off the request
/// stays involved only if they reviewed it. A new change claims its number.
pub fn save_change(ctx: &ExecCtx, repo: &str, change: &Change) -> Result<(), Error> {
    let row = (repo.to_owned(), change.n);
    match CHANGES.get(ctx, &row)? {
        None => NUMBERS.put(ctx, &repo.to_owned(), &change.n),
        Some(old) => {
            let dropped: Vec<Principal> = old
                .reviewers
                .into_iter()
                .filter(|principal| {
                    !change.reviewers.contains(principal)
                        && *principal != change.author
                        && !has_reviewed(ctx, repo, change.n, principal)
                })
                .collect();
            for principal in dropped {
                INVOLVED.remove(ctx, &(principal, repo.to_owned(), change.n));
            }
        }
    }
    for principal in std::iter::once(&change.author).chain(&change.reviewers) {
        involve(ctx, principal, repo, change.n);
    }
    CHANGES.put(ctx, &row, change);
    Ok(())
}

pub fn involve(ctx: &ExecCtx, principal: &Principal, repo: &str, n: u64) {
    INVOLVED.insert(ctx, &(principal.clone(), repo.to_owned(), n));
}

fn has_reviewed(ctx: &QueryCtx, repo: &str, n: u64, principal: &Principal) -> bool {
    !ctx.scan(
        AUTHORED
            .prefix_of(&(repo.to_owned(), n, principal.clone()))
            .limit(1),
    )
    .is_empty()
}

pub fn save_review(ctx: &ExecCtx, repo: &str, n: u64, review: &Review) {
    REVIEWS.put(ctx, &(repo.to_owned(), n, review.id), review);
    AUTHORED.insert(ctx, &(repo.to_owned(), n, review.author.clone(), review.id));
    involve(ctx, &review.author, repo, n);
}

pub fn load_review(ctx: &QueryCtx, repo: &str, n: u64, id: u64) -> Result<Review, Error> {
    REVIEWS
        .get(ctx, &(repo.to_owned(), n, id))?
        .ok_or_else(|| storage("authored review missing"))
}

impl Kind for Review {
    const KIND: &'static str = "forge.review";
}

// Every table here but REVIEWS is general data: only this program's code governs it.
pub(crate) const TABLES: &[::program::Table] = &[
    BOUNDS.table(),
    WRITES.table(),
    REPOS.table(),
    ACTIVITY.table(),
    REFS.table(),
    NUMBERS.table(),
    CHANGES.table(),
    REVIEWS.table(),
    AUTHORED.table(),
    INVOLVED.table(),
    MESSAGES.table(),
    PROTECTED.table(),
    WATCHERS.table(),
    ARCHIVING.table(),
    LOGGING.table(),
    PREVIEWS.table(),
    STATUSES.table(),
    REQUIRED.table(),
];
