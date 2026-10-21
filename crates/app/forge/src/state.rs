//! Every table forge keeps, declared once: the records, the indexes over
//! them and the counters. Git objects are not here: an object's blob id is
//! its oid (`objects`). People are [`Principal`]s: accounts.

use std::collections::BTreeMap;

use ::program::{Error, ProgramId, code};
use ::program::{ExecCtx, QueryCtx, invalid, not_found};
use borsh::{BorshDeserialize, BorshSerialize};
use gitcore::{Hash, Oid};
use store::{Item, Kind, Map, Owned, Set};

use crate::contract::{
    Bounds, Change, ChangeState, CommitStatus, PreviewInfo, Principal, QueueEntry, Repo, Required,
    Review, Revision, Watched, valid_repo_name,
};
use crate::objects::hash_of;

/// The bounds forge was founded with.
pub const BOUNDS: Item<Bounds> = Item::new("bounds");
/// How many ops forge has accepted. Every listing cursor is pinned to it:
/// only a forge op rewrites a listing, and two ops in one block share a
/// height but not a count.
pub const WRITES: Item<u64> = Item::new("writes");
/// One record per repository, by name.
pub const REPOS: Map<String, Repo> = Map::new("p/");
/// Index: every repository by its last activity, newest first.
pub const ACTIVITY: Set<(u64, String)> = Set::new("a/");
/// Each repository's refs and the oid bytes each points at.
pub const REFS: Map<(String, Vec<u8>), Vec<u8>> = Map::new("r/");
/// The last change number each repository gave out.
pub const NUMBERS: Map<String, u64> = Map::new("n/");
/// Changes by repository and number; a scan across repositories lists them
/// by name (Judgment pages this table whole).
pub const CHANGES: Map<(String, u64), Change> = Map::new("c/");
/// Reviews by repository, change number and review id.
// Each review is its author's, of the kind `forge.review`: no op rewrites or deletes one.
pub const REVIEWS: Map<(String, u64, u64), Owned<Review>> = Map::new("v/");
// Each change's authorship, of the kind `forge.change`: its author's from `ChangeOpen` on. The
// change's record (`CHANGES`) is shared, since reviews, closes and merges by others write it; the
// ops that reshape what the author set (`ChangeEdit`, `ChangeRepoint`, `Rebase`) write this row
// too, so the change's policy decides them. A change opened before this row existed has none
// until a migration writes it.
pub const AUTHORSHIP: Map<(String, u64), Owned<Authorship>> = Map::new("o/");
// Each repository's custody, of the kind `forge.repo`: its creator's from `Create` on. The
// repository's record (`REPOS`) is shared, since every op on it marks it active; each op the
// repository's policy decides (`ops::governed`: pushes, merges, branch rules, closing another's
// change) writes this row too. A repository created before this row existed has none until a
// migration writes it.
pub const CUSTODY: Map<String, Owned<Custody>> = Map::new("k/");
/// Index: the reviews one principal submitted on one change, oldest first.
pub const AUTHORED: Set<(String, u64, Principal, u64)> = Set::new("u/");
/// Index: the changes a principal authored, is asked to review, or reviewed.
pub const INVOLVED: Set<(Principal, String, u64)> = Set::new("i/");
pub const OPEN_CHANGES_BY_SOURCE: Set<(String, Vec<u8>, u64)> = Set::new("j/");
pub const REPOS_WITH_OPEN_CHANGES_INDEXED: Set<String> = Set::new("t/");
/// The last system message id forge posted into chat.
pub const MESSAGES: Item<u64> = Item::new("system-message-seq");
// Each repository's protected branches besides its head, which always is: no rebase moves
// one and no `DeleteBranch` deletes one.
pub const PROTECTED: Set<(String, Vec<u8>)> = Set::new("b/");
// The programs that watch forge, each with the prefix it hears events under.
pub const WATCHERS: Map<ProgramId, Vec<u8>> = Map::new("w/");
// The archives forge asked chat for, by the message's seq, each naming its change. chat
// answers in the frame that asked, which takes the row away again.
pub const ARCHIVING: Map<u64, (String, u64)> = Map::new("x/");
// The lines forge posted to say chat refused an archive, by the message's seq, until chat
// answers them (in the same frame).
pub const LOGGING: Set<u64> = Set::new("l/");
// Each change's latest Preview.
pub const PREVIEWS: Map<(String, u64), PreviewInfo> = Map::new("m/");
// Every status a reporter gave a commit: by repository, commit, reporter and context.
pub const STATUSES: Map<(String, String, Principal, String), CommitStatus> = Map::new("s/");
// The branches that move only by a merge whose Preview is green.
pub const REQUIRED: Map<(String, Vec<u8>), Required> = Map::new("q/");
// Each branch's merge queue, in the order its changes joined: by repository, branch and place.
pub const QUEUE: Map<(String, Vec<u8>, u64), QueueEntry> = Map::new("e/");
// Index: the branch and place each queued change holds.
pub const QUEUED: Map<(String, u64), (Vec<u8>, u64)> = Map::new("g/");
// The last place a queue gave out.
pub const QUEUE_SEQ: Item<u64> = Item::new("queue-seq");
// The archived repositories, by name. `ACTIVITY` holds none of them, so `Repos` lists none.
pub const ARCHIVED: Set<String> = Set::new("h/");
pub const REPOS_DEFAULTING_TO_OWNER: Set<String> = Set::new("d/");

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
        return Err(invalid(format!(
            "{name:?} is not a repository name: choose a repository name using letters, digits, dots, underscores or hyphens"
        )));
    }
    REPOS
        .get(ctx, &name.to_owned())?
        .ok_or_else(|| not_found(format!("no repository named {name}")))
}

// Stores the record and moves it in the activity index, so the index holds exactly one row per
// repository that is not archived.
pub fn save_repo(ctx: &ExecCtx, name: &str, repo: &Repo) -> Result<(), Error> {
    if let Some(old) = REPOS.get(ctx, &name.to_owned())? {
        ACTIVITY.remove(ctx, &(newest_first(old.last_activity), name.to_owned()));
    }
    if !archived(ctx, name) {
        ACTIVITY.insert(ctx, &(newest_first(repo.last_activity), name.to_owned()));
    }
    REPOS.put(ctx, &name.to_owned(), repo);
    Ok(())
}

pub fn archived(ctx: &QueryCtx, name: &str) -> bool {
    ARCHIVED.has(ctx, &name.to_owned())
}

pub(crate) fn writes(ctx: &QueryCtx) -> Result<u64, Error> {
    Ok(WRITES.get(ctx)?.unwrap_or(0))
}

pub(crate) fn wrote(ctx: &ExecCtx) -> Result<(), Error> {
    WRITES.put(ctx, &(writes(ctx)? + 1));
    Ok(())
}

fn newest_first(height: u64) -> u64 {
    u64::MAX - height
}

pub fn repo_hash(repo: &Repo) -> Hash {
    hash_of(repo.hash)
}

pub fn ref_key(name: &str, reference: &[u8]) -> (String, Vec<u8>) {
    (name.to_owned(), reference.to_vec())
}

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
    let head_moved = old.as_deref() != Some(target.as_bytes());
    REFS.put(ctx, &key, &target.as_bytes().to_vec());
    moved(ctx, name, reference, old, Some(target.to_hex()))?;
    if head_moved {
        crate::discussion::heads_moved(ctx, name, reference, &target.to_hex())?;
    }
    Ok(())
}

pub fn delete_ref(ctx: &ExecCtx, name: &str, reference: &[u8]) -> Result<(), Error> {
    let key = ref_key(name, reference);
    let old = REFS.get(ctx, &key)?;
    REFS.remove(ctx, &key);
    moved(ctx, name, reference, old, None)
}

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
                return Err(invalid(
                    "revision must name a full ref: supply the full ref name under refs/",
                ));
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
    let old = CHANGES.get(ctx, &row)?;
    if let Some(branch) = old.as_ref().and_then(open_source_branch) {
        OPEN_CHANGES_BY_SOURCE.remove(ctx, &(repo.to_owned(), branch.to_vec(), change.n));
    }
    if let Some(branch) = open_source_branch(change) {
        OPEN_CHANGES_BY_SOURCE.insert(ctx, &(repo.to_owned(), branch.to_vec(), change.n));
    }
    match old {
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

pub fn index_open_changes_by_source_once(ctx: &ExecCtx, repo: &str) -> Result<(), Error> {
    let name = repo.to_owned();
    if REPOS_WITH_OPEN_CHANGES_INDEXED.has(ctx, &name) {
        return Ok(());
    }
    for ((_, n), change) in CHANGES.scan(ctx, CHANGES.prefix_of(&name))? {
        if let Some(branch) = open_source_branch(&change) {
            OPEN_CHANGES_BY_SOURCE.insert(ctx, &(name.clone(), branch.to_vec(), n));
        }
    }
    REPOS_WITH_OPEN_CHANGES_INDEXED.insert(ctx, &name);
    Ok(())
}

fn open_source_branch(change: &Change) -> Option<&[u8]> {
    let open = change.state == ChangeState::Open;
    match &change.from {
        Revision::Ref(branch) if open => Some(branch),
        _ => None,
    }
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

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Authorship {
    pub author: Principal,
    // how many times the author's side of the change was reshaped
    pub rev: u64,
}

impl Kind for Authorship {
    const KIND: &'static str = "forge.change";
}

// The authorship of change `n` of `repo`, written again: a no-op for a change that has none.
pub fn reshaped(ctx: &ExecCtx, repo: &str, n: u64) -> Result<(), Error> {
    let at = (repo.to_owned(), n);
    if let Some(mut authorship) = AUTHORSHIP.get(ctx, &at)? {
        authorship.rev += 1;
        AUTHORSHIP.put(ctx, &at, &authorship);
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Custody {
    pub owner: Principal,
    // how many ops the repository's policy decided
    pub rev: u64,
}

impl Kind for Custody {
    const KIND: &'static str = "forge.repo";
}

// The custody of `repo`, written again: a no-op for a repository that has none.
pub fn kept(ctx: &ExecCtx, repo: &str) -> Result<(), Error> {
    let at = repo.to_owned();
    if let Some(mut custody) = CUSTODY.get(ctx, &at)? {
        custody.rev += 1;
        CUSTODY.put(ctx, &at, &custody);
    }
    Ok(())
}
