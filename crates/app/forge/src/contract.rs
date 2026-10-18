//! Borsh is the program contract; the view links these types as they are.
use ::program::{BlobId, HashKind};
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

use crate::state::{ACTIVITY, AUTHORED, CHANGES, INVOLVED, PROTECTED, REFS, REPOS, REVIEWS};

pub use crate::read_contract::*;
pub use crate::review_contract::*;
pub use ::program::Principal;
pub use store::{PageRequest, PageResponse};

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Bounds {
    pub max_objects: u64,
    pub max_delta_depth: u64,
    pub max_object_size: u64,
    pub push_walk: u64,
    pub fetch_walk: u64,
    /// Largest edit distance searched for one file's diff; a file past it
    /// is answered `Oversize`, without hunks. A one-sided edit (a file
    /// added, deleted or grown in one place) costs nothing.
    pub merge_cost: u64,
    pub page_size: u32,
    /// Maximum commits in a history/compare walk; a log's `exclude` side
    /// walks under its own budget of the same size.
    pub log_walk: u64,
    pub tree_walk: u64,
    /// Aggregate object bytes read by a query (including trees and commits).
    pub diff_bytes: u64,
    /// Largest blob displayed inline. Larger blobs return a header.
    pub blob_bytes: u64,
    /// Maximum encoded change or review record.
    pub record_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Settings {
    pub head: Vec<u8>,
    pub allow_force: bool,
    pub allow_delete: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            head: b"refs/heads/main".to_vec(),
            allow_force: false,
            allow_delete: false,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Repo {
    pub hash: HashKind,
    /// The account that created the repository: a person's, a program's own
    /// or an agent's.
    pub owner: Principal,
    pub settings: Settings,
    pub refs_count: u64,
    pub last_activity: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Op {
    Create {
        repo: String,
        hash: HashKind,
    },
    Configure {
        repo: String,
        settings: Settings,
    },
    Grant {
        repo: String,
        principal: Principal,
    },
    Revoke {
        repo: String,
        principal: Principal,
    },
    Push {
        repo: String,
        request: Vec<u8>,
        pack: Option<BlobId>,
    },
    /// The client publishes immutable objects first. Consensus only checks both heads.
    Merge {
        repo: String,
        into: Vec<u8>,
        from: Revision,
        expected_into: String,
        expected_from: String,
        result: String,
        change: Option<u64>,
    },
    ChangeOpen {
        repo: String,
        from: Revision,
        into: Vec<u8>,
        title: String,
        body: String,
        reviewers: Vec<Principal>,
    },
    ChangeEdit {
        repo: String,
        n: u64,
        title: Option<String>,
        body: Option<String>,
        reviewers: Option<Vec<Principal>>,
    },
    ChangeClose {
        repo: String,
        n: u64,
    },
    ReviewSubmit {
        repo: String,
        n: u64,
        review: ReviewDraft,
    },
    Stage {
        repo: String,
        pack: BlobId,
    },
    // forge builds the merge itself: one squash commit on `into`'s head, under `Merge`'s CAS.
    SquashMerge {
        repo: String,
        into: Vec<u8>,
        from: Revision,
        expected_into: String,
        expected_from: String,
        message: Option<String>,
        change: Option<u64>,
    },
    // forge rebuilds change `n` on `onto` as one squash commit, and the change's source moves
    // to it, under a CAS on `expected_head`.
    Rebase {
        repo: String,
        n: u64,
        onto: Revision,
        expected_head: String,
    },
    // Change `n` takes `from` as its source; its number, reviews and channel stay.
    ChangeRepoint {
        repo: String,
        n: u64,
        from: Revision,
    },
    // A branch still at `expected`, that no protection covers and no open change names, goes.
    DeleteBranch {
        repo: String,
        branch: Vec<u8>,
        expected: String,
    },
    Protect {
        repo: String,
        branch: Vec<u8>,
        protected: bool,
    },
    // A program hears of every ref forge moves and every change it opens, each as
    // `deliver ++ borsh(Watched)`; an empty `deliver` stops it.
    Watch {
        deliver: Vec<u8>,
    },
    // Archives the channels of `repo`'s merged and closed changes numbered above `after`,
    // `limit` numbers of them, that chat still shows unarchived; open to anyone.
    ArchiveEnded {
        repo: String,
        after: Option<u64>,
        limit: u64,
    },
}

impl Op {
    pub fn repo(&self) -> Option<&str> {
        match self {
            Op::Create { repo, .. }
            | Op::Configure { repo, .. }
            | Op::Grant { repo, .. }
            | Op::Revoke { repo, .. }
            | Op::Push { repo, .. }
            | Op::Merge { repo, .. }
            | Op::ChangeOpen { repo, .. }
            | Op::ChangeEdit { repo, .. }
            | Op::ChangeClose { repo, .. }
            | Op::ReviewSubmit { repo, .. }
            | Op::Stage { repo, .. }
            | Op::SquashMerge { repo, .. }
            | Op::Rebase { repo, .. }
            | Op::ChangeRepoint { repo, .. }
            | Op::DeleteBranch { repo, .. }
            | Op::Protect { repo, .. }
            | Op::ArchiveEnded { repo, .. } => Some(repo),
            Op::Watch { .. } => None,
        }
    }
}

pub fn repo_key(repo: &str) -> String {
    repo.to_owned()
}

pub fn settings_key(repo: &str) -> String {
    format!("{repo}/settings")
}

pub fn change_key(repo: &str, n: u64) -> String {
    format!("{repo}/changes/{n}")
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    PartialOrd,
    Ord,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
)]
pub enum Service {
    ReceivePack,
    UploadPack,
}

/// A read. Each variant names the tables it is answered from (`#[reads]`,
/// [`program::Reads`]), so a view following `program.changes` re-reads it
/// only for a block that wrote to one of them; a judgment is answered
/// partly from chat's and names those too. Git objects are not keys: a
/// view reaches one through a ref, and the push or merge that moves the ref
/// brings it (a stage stores objects no ref reaches yet), so a read of
/// objects follows `REFS`. The git client's own questions declare
/// nothing: no view follows them.
#[derive(
    Clone,
    Debug,
    PartialEq,
    PartialOrd,
    Ord,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    ::program::Ask,
)]
#[ask(crate::Forge)]
pub enum Query {
    #[reads(ACTIVITY, REPOS)]
    Repos {
        page: PageRequest,
    },
    #[reads(REPOS)]
    Repo {
        repo: String,
    },
    #[reads(REFS)]
    Refs {
        repo: String,
        page: PageRequest,
    },
    Advertise {
        repo: String,
        service: Service,
    },
    Upload {
        repo: String,
        request: Vec<u8>,
    },
    /// The history `from` reaches, less what `exclude` reaches (`git log
    /// exclude..from`): a change's own commits exclude its target.
    #[reads(REFS)]
    Log {
        repo: String,
        from: Revision,
        exclude: Option<Revision>,
        page: PageRequest,
    },
    #[reads(REFS)]
    Tree {
        repo: String,
        at: String,
        path: Vec<u8>,
        page: PageRequest,
    },
    #[reads(REFS)]
    Blob {
        repo: String,
        oid: String,
        range: Option<ByteRange>,
    },
    /// None base means the empty tree, for a root commit's diff.
    #[reads(REFS)]
    Diff {
        repo: String,
        base: Option<String>,
        head: String,
        path: Option<Vec<u8>>,
        page: PageRequest,
    },
    #[reads(REFS)]
    Compare {
        repo: String,
        from: Revision,
        into: Revision,
    },
    #[reads(REPOS)]
    Activity {
        repo: String,
    },
    #[reads(CHANGES, INVOLVED)]
    Changes {
        repo: String,
        filter: ChangeFilter,
        page: PageRequest,
    },
    /// The change, its two heads and a page of its reviews.
    #[reads(CHANGES, REVIEWS, REFS)]
    Change {
        repo: String,
        n: u64,
        page: PageRequest,
    },
    /// What one person owes across every repository. The threads it waits
    /// on are chat's (`discussion`): a reply moves the author's `ANSWERED`
    /// entry and the root row's `last_reply_seq`, in chat's `MESSAGES`, and
    /// a review's root is found through `MESSAGE_IDS`. The reply rows and
    /// reactions are not read.
    #[reads(CHANGES, REVIEWS, AUTHORED, REFS)]
    #[reads(chat::Chat: chat::tables::ANSWERED, chat::tables::MESSAGE_IDS, chat::tables::MESSAGES)]
    Judgment {
        principal: Principal,
        page: PageRequest,
    },
    Closure {
        repo: String,
        walk: Walk,
    },
    // A repository's protected branches, in byte-name order; its head is protected besides.
    #[reads(PROTECTED, REPOS)]
    Protected {
        repo: String,
        page: PageRequest,
    },
    // the refs whose name starts with `prefix` ("" for every ref), byte-name order, names as text
    #[reads(REFS)]
    RefNames {
        repo: String,
        prefix: String,
        page: PageRequest,
    },
    // what `Diff` answers less the hunks: each file's paths as text, kinds, sizes and line counts
    #[reads(REFS)]
    DiffStat {
        repo: String,
        base: Option<String>,
        head: String,
        page: PageRequest,
    },
}

impl Query {
    /// The listing a page's cursor is bound to: this query with its page
    /// taken out.
    pub fn scope(&self) -> Vec<u8> {
        let mut scope = self.clone();
        if let Some(page) = scope.page_mut() {
            *page = PageRequest::default();
        }
        PageRequest::scope_of(&scope)
    }

    /// The page a listing asks for; an unpaged query has none.
    pub fn page(&self) -> Option<&PageRequest> {
        match self {
            Query::Repos { page }
            | Query::Refs { page, .. }
            | Query::Log { page, .. }
            | Query::Tree { page, .. }
            | Query::Diff { page, .. }
            | Query::Changes { page, .. }
            | Query::Change { page, .. }
            | Query::Judgment { page, .. }
            | Query::Protected { page, .. }
            | Query::RefNames { page, .. }
            | Query::DiffStat { page, .. } => Some(page),
            Query::Advertise { .. }
            | Query::Upload { .. }
            | Query::Repo { .. }
            | Query::Blob { .. }
            | Query::Compare { .. }
            | Query::Activity { .. }
            | Query::Closure { .. } => None,
        }
    }

    /// [`Query::page`], to continue a listing with its `next` cursor.
    pub fn page_mut(&mut self) -> Option<&mut PageRequest> {
        match self {
            Query::Repos { page }
            | Query::Refs { page, .. }
            | Query::Log { page, .. }
            | Query::Tree { page, .. }
            | Query::Diff { page, .. }
            | Query::Changes { page, .. }
            | Query::Change { page, .. }
            | Query::Judgment { page, .. }
            | Query::Protected { page, .. }
            | Query::RefNames { page, .. }
            | Query::DiffStat { page, .. } => Some(page),
            Query::Advertise { .. }
            | Query::Upload { .. }
            | Query::Repo { .. }
            | Query::Blob { .. }
            | Query::Compare { .. }
            | Query::Activity { .. }
            | Query::Closure { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct RepoInfo {
    pub name: String,
    pub repo: Repo,
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct RefInfo {
    pub name: Vec<u8>,
    pub target: String,
}
// A ref as `RefNames` answers it: its name as text (a byte that is not UTF-8 reads as U+FFFD;
// `Refs` keeps the exact bytes).
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct RefName {
    pub name: String,
    pub target: String,
}
// One file of a `DiffStat`: its path as text (the new path, or the old one for a delete), and
// what `FileDiff` says of it besides its hunks.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct FileStat {
    pub path: String,
    pub status: FileStatus,
    pub old_kind: Option<EntryKind>,
    pub new_kind: Option<EntryKind>,
    pub old_size: u64,
    pub new_size: u64,
    pub content: Content,
    pub additions: u64,
    pub deletions: u64,
}

/// One read's answer. A reply is decoded once and read in place, so its
/// widest variant (a whole change record) is not boxed.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Repos {
        height: u64,
        page: PageResponse<RepoInfo>,
    },
    Repo {
        height: u64,
        repo: RepoInfo,
        bounds: Bounds,
    },
    Refs {
        height: u64,
        page: PageResponse<RefInfo>,
    },
    Log {
        height: u64,
        tip: String,
        page: PageResponse<CommitInfo>,
    },
    Tree {
        height: u64,
        tree: String,
        page: PageResponse<TreeInfo>,
    },
    Blob {
        height: u64,
        blob: BlobView,
    },
    Diff {
        height: u64,
        base: Option<String>,
        head: String,
        total_files: u64,
        page: PageResponse<FileDiff>,
    },
    Compare {
        height: u64,
        comparison: Comparison,
    },
    Activity {
        height: u64,
        last_height: u64,
    },
    Changes {
        height: u64,
        page: PageResponse<ChangeSummary>,
    },
    Change {
        height: u64,
        change: Change,
        source_head: Option<String>,
        target_head: Option<String>,
        reviews: PageResponse<Review>,
    },
    Judgment {
        height: u64,
        page: PageResponse<Judgment>,
    },
    Closure {
        height: u64,
        listed: Vec<Listed>,
        next: Walk,
    },
    Protected {
        height: u64,
        page: PageResponse<Vec<u8>>,
    },
    RefNames {
        height: u64,
        page: PageResponse<RefName>,
    },
    DiffStat {
        height: u64,
        base: Option<String>,
        head: String,
        total_files: u64,
        page: PageResponse<FileStat>,
    },
}

#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    PartialOrd,
    Ord,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
)]
pub struct Walk {
    pub haves: Vec<String>,
    pub commits: Vec<QueuedCommit>,
    pub objects: Vec<String>,
    pub trees: Vec<PendingTree>,
}

#[derive(
    Clone, Debug, PartialEq, PartialOrd, Ord, Eq, BorshSerialize, BorshDeserialize, BorshSchema,
)]
pub struct QueuedCommit {
    pub oid: String,
    pub time: i64,
    pub uninteresting: bool,
}

#[derive(
    Clone, Debug, PartialEq, PartialOrd, Ord, Eq, BorshSerialize, BorshDeserialize, BorshSchema,
)]
pub struct PendingTree {
    pub tree: String,
    pub against: Vec<String>,
    pub from: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Listed {
    pub oid: String,
    pub kind: ObjectKind,
}

impl Walk {
    pub fn of(walk: gitcore::walk::Walk) -> Walk {
        Walk {
            haves: hexes(&walk.haves),
            commits: walk
                .commits
                .into_iter()
                .map(|queued| QueuedCommit {
                    oid: queued.id.to_hex(),
                    time: queued.time,
                    uninteresting: queued.uninteresting,
                })
                .collect(),
            objects: hexes(&walk.objects),
            trees: walk
                .trees
                .into_iter()
                .map(|pending| PendingTree {
                    tree: pending.tree.to_hex(),
                    against: hexes(&pending.against),
                    from: pending.from as u64,
                })
                .collect(),
        }
    }

    pub fn parse(&self, hash: HashKind) -> Option<gitcore::walk::Walk> {
        let hash = crate::objects::hash_of(hash);
        let mut trees = Vec::with_capacity(self.trees.len());
        for pending in &self.trees {
            trees.push(gitcore::walk::Pending {
                tree: gitcore::Oid::from_hex(hash, &pending.tree).ok()?,
                against: oids(hash, &pending.against)?,
                from: usize::try_from(pending.from).ok()?,
            });
        }
        let mut commits = Vec::with_capacity(self.commits.len());
        for queued in &self.commits {
            commits.push(gitcore::walk::Queued {
                id: gitcore::Oid::from_hex(hash, &queued.oid).ok()?,
                time: queued.time,
                uninteresting: queued.uninteresting,
            });
        }
        Some(gitcore::walk::Walk {
            haves: oids(hash, &self.haves)?,
            commits,
            objects: oids(hash, &self.objects)?,
            trees,
        })
    }
}

impl Listed {
    pub fn of(listed: gitcore::walk::Listed) -> Listed {
        let kind = match listed.kind {
            gitcore::Kind::Commit => ObjectKind::Commit,
            gitcore::Kind::Tree => ObjectKind::Tree,
            gitcore::Kind::Blob => ObjectKind::Blob,
            gitcore::Kind::Tag => ObjectKind::Tag,
        };
        Listed {
            oid: listed.id.to_hex(),
            kind,
        }
    }

    pub fn parse(&self, hash: HashKind) -> Option<gitcore::walk::Listed> {
        let kind = match self.kind {
            ObjectKind::Commit => gitcore::Kind::Commit,
            ObjectKind::Tree => gitcore::Kind::Tree,
            ObjectKind::Blob => gitcore::Kind::Blob,
            ObjectKind::Tag => gitcore::Kind::Tag,
        };
        let id = gitcore::Oid::from_hex(crate::objects::hash_of(hash), &self.oid).ok()?;
        Some(gitcore::walk::Listed { id, kind })
    }
}

fn hexes(ids: &[gitcore::Oid]) -> Vec<String> {
    ids.iter().map(gitcore::Oid::to_hex).collect()
}

fn oids(hash: gitcore::Hash, hexes: &[String]) -> Option<Vec<gitcore::Oid>> {
    hexes
        .iter()
        .map(|hex| gitcore::Oid::from_hex(hash, hex).ok())
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum ObjectKind {
    Commit,
    Tree,
    Blob,
    Tag,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum OpReply {
    Change {
        height: u64,
        n: u64,
    },
    Review {
        height: u64,
        n: u64,
        id: u64,
    },
    Merged {
        height: u64,
        oid: String,
        change: Option<u64>,
    },
    Rebased {
        height: u64,
        n: u64,
        oid: String,
    },
    // What `ArchiveEnded` archived, and the number to pass as `after` when more follow.
    ArchivedEnded {
        height: u64,
        archived: Vec<u64>,
        next: Option<u64>,
    },
}

// What a watcher hears (`Op::Watch`): a ref a push, a merge or a squash moved, `old` or `new`
// None where it was made or deleted, and a change opened; each names who did it.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Watched {
    Moved {
        repo: String,
        reference: Vec<u8>,
        old: Option<String>,
        new: Option<String>,
        by: Option<Principal>,
    },
    Opened {
        repo: String,
        n: u64,
        from: Revision,
        into: Vec<u8>,
        head: String,
        by: Principal,
    },
}

pub const PACK_KIND: &str = "pack";

pub fn valid_repo_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && !name.starts_with('.')
        && !name.ends_with(".git")
}
