//! Borsh is the program contract; the view links these types as they are.
use ::program::{BlobId, HashKind};
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

use crate::state::{
    ACTIVITY, ARCHIVED, AUTHORED, CHANGES, INVOLVED, PREVIEWS, PROTECTED, QUEUE, REFS, REPOS,
    REQUIRED, REVIEWS, STATUSES,
};

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

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Described,
)]
#[tool_about(
    "Git repositories on the network: refs, commits, trees and blobs to read, pushes, changes (pull requests) with reviews, squash merges, rebases, required checks and merge queues. Refs, paths and git names are bytes: write them as hex or {\"utf8\": text}; oids are full lowercase hex. Clone and push with git over duck:// as well."
)]
pub enum Op {
    #[tool_op(description = "Create a repository you own, with sha1 or sha256 objects. Its name is unique and every op names it.", example = crate::examples::create)]
    Create { repo: String, hash: HashKind },
    #[tool_op(description = "Set a repository's settings: its head branch and whether pushes may force or delete refs. Whoever its policy admits (its owner by default).", example = crate::examples::configure)]
    Configure { repo: String, settings: Settings },
    #[tool_op(description = "Retired: a repository's writers are its list in the access program, which its owner sets.", example = crate::examples::grant)]
    Grant { repo: String, principal: Principal },
    #[tool_op(description = "Retired with Grant.", example = crate::examples::revoke)]
    Revoke { repo: String, principal: Principal },
    #[tool_op(description = "A git receive-pack: request is its pkt-lines (`<old> <new> <ref>\\0report-status`, then 0000) and pack a blob of the pack. Objects land once the pack unpacks; each ref moves where the repository's policy admits you, its settings allow it and it fast-forwards unless force is allowed. Returns git's report.", example = crate::examples::push)]
    Push {
        repo: String,
        request: Vec<u8>,
        pack: Option<BlobId>,
    },
    /// The client publishes immutable objects first. Consensus only checks both heads.
    #[tool_op(description = "Move into to a result commit you built and published, with both heads checked against expected_into and expected_from, and mark change merged when one is named. Whoever the repository's policy admits; review verdicts are not enforced. Returns Merged.", returns = OpReply, example = crate::examples::merge)]
    Merge {
        repo: String,
        into: Vec<u8>,
        from: Revision,
        expected_into: String,
        expected_from: String,
        result: String,
        change: Option<u64>,
    },
    #[tool_op(description = "Open a change (a pull request) from a branch or commit into a branch, with reviewers. Anyone may; it opens a chat channel forge:<repo>:<n> for its conversation. Returns the change's number.", returns = OpReply, example = crate::examples::open)]
    ChangeOpen {
        repo: String,
        from: Revision,
        into: Vec<u8>,
        title: String,
        body: String,
        reviewers: Vec<Principal>,
    },
    #[tool_op(description = "Change a change's title, body or reviewers; a field left null stays. Its author, or whom the change's policy admits.", returns = OpReply, example = crate::examples::edit)]
    ChangeEdit {
        repo: String,
        n: u64,
        title: Option<String>,
        body: Option<String>,
        reviewers: Option<Vec<Principal>>,
    },
    #[tool_op(description = "Close a change without merging; closing is final. Its author, or whom the repository's policy admits.", returns = OpReply, example = crate::examples::close)]
    ChangeClose { repo: String, n: u64 },
    #[tool_op(description = "Review a change at commit_oid: Approve, RequestChanges or Comment, a body, and line comments (path, side Old or New, line). Anyone may; every field is given. Returns the review's id.", returns = OpReply, example = crate::examples::review)]
    ReviewSubmit {
        repo: String,
        n: u64,
        review: ReviewDraft,
    },
    #[tool_op(description = "Store a pack's objects without moving a ref, for a Merge result. Whoever the repository's policy admits.", example = crate::examples::stage)]
    Stage { repo: String, pack: BlobId },
    // forge builds the merge itself: one squash commit on `into`'s head, under `Merge`'s CAS.
    #[tool_op(description = "forge builds one squash commit of from onto into's head and moves into to it, both heads checked, and marks change merged; message null takes the change's title. Whoever the repository's policy admits. A text conflict is refused naming each path. Returns Merged.", returns = OpReply, example = crate::examples::squash)]
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
    #[tool_op(description = "Rebuild change n as one commit on onto and move the change's own branch there, while its head is still expected_head. A conflict is refused naming the paths. Whoever both the change's and the repository's policies admit. Returns Rebased.", returns = OpReply, example = crate::examples::rebase)]
    Rebase {
        repo: String,
        n: u64,
        onto: Revision,
        expected_head: String,
    },
    // Change `n` takes `from` as its source; its number, reviews and channel stay.
    #[tool_op(description = "Point change n at another source; its number, reviews and channel stay. Its author, or whom the change's policy admits.", returns = OpReply, example = crate::examples::repoint)]
    ChangeRepoint {
        repo: String,
        n: u64,
        from: Revision,
    },
    // A branch still at `expected`, that no protection covers and no open change names, goes.
    #[tool_op(description = "Delete a branch still at expected, that no protection covers and no open change names. Whoever the repository's policy admits.", example = crate::examples::delete_branch)]
    DeleteBranch {
        repo: String,
        branch: Vec<u8>,
        expected: String,
    },
    #[tool_op(description = "Protect a branch from deletion and force pushes, or stop. A repository's head is always protected. Whoever its policy admits.", example = crate::examples::protect)]
    Protect {
        repo: String,
        branch: Vec<u8>,
        protected: bool,
    },
    // A program hears of every ref forge moves and every change it opens, each as
    // `deliver ++ borsh(Watched)`; an empty `deliver` stops it.
    #[tool_op(description = "From a program only: be told of every ref forge moves and every change it opens, each as deliver followed by the borsh of what happened. An empty deliver stops it.", example = crate::examples::watch)]
    Watch { deliver: Vec<u8> },
    // Archives the channels of `repo`'s merged and closed changes numbered above `after`,
    // `limit` numbers of them, that chat still shows unarchived; open to anyone.
    #[tool_op(description = "Archive the chat channels of a repository's merged and closed changes numbered after after, limit numbers of them. Anyone may. Returns where to resume.", returns = OpReply, example = crate::examples::archive_ended)]
    ArchiveEnded {
        repo: String,
        after: Option<u64>,
        limit: u64,
    },
    // forge builds change `n`'s squash on its target's head, as `SquashMerge` would land it, and
    // points `refs/changes/<n>/merge` at it: the commit a gated merge's checks run on.
    #[tool_op(description = "Build, without moving the target, the squash change n would land on its target's current head, and point refs/changes/<n>/merge at it: the commit required checks run on. Anyone may. Returns the preview.", returns = OpReply, example = crate::examples::preview)]
    Preview { repo: String, n: u64 },
    // The sender's status of a commit in one context (e.g. `gates.toml#gates`).
    #[tool_op(description = "Report your status of a commit in one context (such as gates.toml#gates): Pending, Passed, Failed or Errored, with an optional url. Recorded as yours; anyone may report.", example = crate::examples::status)]
    Status {
        repo: String,
        commit: String,
        context: String,
        state: CheckState,
        url: Option<String>,
    },
    // `branch` moves only by a merge of a change whose Preview the reporter marks Passed in
    // every context; None drops the requirement.
    #[tool_op(description = "Make branch move only by merging a change whose Preview the reporter marks Passed in every context; null drops the requirement. Whoever the repository's policy admits.", example = crate::examples::require)]
    RequireChecks {
        repo: String,
        branch: Vec<u8>,
        required: Option<Required>,
    },
    // Change `n`, still at `expected_head`, joins its target's merge queue: it lands, as its
    // Preview's commit, in the op that marks that Preview green in every required context.
    #[tool_op(description = "Put change n, still at expected_head, in its target's merge queue: it lands as its Preview's commit when that Preview turns green in every required context. Whoever the repository's policy admits.", example = crate::examples::enqueue)]
    Enqueue {
        repo: String,
        n: u64,
        expected_head: String,
    },
    #[tool_op(description = "Take change n out of its target's merge queue. Whoever the repository's policy admits.", example = crate::examples::dequeue)]
    Dequeue { repo: String, n: u64 },
    // one-time (J-011 ⑥): records owners for forge's owned rows from before the guard
    #[tool_op(description = "One-time backfill of owners for forge's rows from before ownership was recorded, a page at a time; returns the key to resume after, or null when done.", returns = Option<Vec<u8>>, example = crate::examples::adopt)]
    AdoptOwners {
        kind: String,
        after: Option<Vec<u8>>,
        limit: u64,
    },
    // An archived repository leaves `Repos`, refuses every op but this one and still answers
    // every read and clone; `archived: false` restores it.
    #[tool_op(description = "Archive a repository (it leaves Repos and refuses every op but this one, while every read and clone still answers) or restore it with archived false. Whoever its policy admits.", example = crate::examples::archive)]
    Archive { repo: String, archived: bool },
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
            | Op::ArchiveEnded { repo, .. }
            | Op::Preview { repo, .. }
            | Op::Status { repo, .. }
            | Op::RequireChecks { repo, .. }
            | Op::Enqueue { repo, .. }
            | Op::Dequeue { repo, .. }
            | Op::Archive { repo, .. } => Some(repo),
            Op::Watch { .. } | Op::AdoptOwners { .. } => None,
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
    ::program::Described,
)]
#[ask(crate::Forge)]
pub enum Query {
    #[reads(ACTIVITY, REPOS)]
    #[ask(Reply::Repos { height: u64, page: PageResponse<RepoInfo> })]
    #[tool_query(description = "Every repository but the archived ones, by latest activity: name, hash, owner, settings, ref count. Limits run 1 to the founded page size.", example = crate::examples::repos)]
    Repos { page: PageRequest },
    #[reads(REPOS)]
    #[ask(Reply::Repo { height: u64, repo: RepoInfo, bounds: Bounds })]
    #[tool_query(description = "One repository's record and the founded bounds (page size, walk and byte limits).", example = crate::examples::one_repo)]
    Repo { repo: String },
    #[reads(REFS)]
    #[ask(Reply::Refs { height: u64, page: PageResponse<RefInfo> })]
    #[tool_query(description = "A repository's refs in byte-name order, each name as bytes with its target oid.", example = crate::examples::refs)]
    Refs { repo: String, page: PageRequest },
    #[tool_query(description = "Git's own wire: the ref advertisement of a service, as git's raw bytes, not a Reply. For a git client; read refs with Refs or RefNames.", example = crate::examples::advertise)]
    Advertise { repo: String, service: Service },
    #[tool_query(description = "Git's own wire: an upload-pack (protocol v2) request answered with git's raw bytes, not a Reply. For a git client; read history with Log and objects with Tree and Blob.", example = crate::examples::upload)]
    Upload { repo: String, request: Vec<u8> },
    /// The history `from` reaches, less what `exclude` reaches (`git log
    /// exclude..from`): a change's own commits exclude its target.
    #[reads(REFS)]
    #[ask(Reply::Log { height: u64, tip: String, page: PageResponse<CommitInfo> })]
    #[tool_query(description = "The commits from reaches, less what exclude reaches (git log exclude..from), newest first: oid, tree, parents, message, author and committer.", example = crate::examples::log)]
    Log {
        repo: String,
        from: Revision,
        exclude: Option<Revision>,
        page: PageRequest,
    },
    #[reads(REFS)]
    #[ask(Reply::Tree { height: u64, tree: String, page: PageResponse<TreeInfo> })]
    #[tool_query(description = "The entries of a directory at a commit or tree oid; path [] is the root.", example = crate::examples::tree)]
    Tree {
        repo: String,
        at: String,
        path: Vec<u8>,
        page: PageRequest,
    },
    #[reads(REFS)]
    #[ask(Reply::Blob { height: u64, blob: BlobView })]
    #[tool_query(description = "A file's bytes by blob oid, whole or a byte range, with its size and whether it is text.", example = crate::examples::blob)]
    Blob {
        repo: String,
        oid: String,
        range: Option<ByteRange>,
    },
    /// None base means the empty tree, for a root commit's diff.
    #[reads(REFS)]
    #[ask(Reply::Diff { height: u64, base: Option<String>, head: String, total_files: u64, page: PageResponse<FileDiff> })]
    #[tool_query(description = "What changed from base (null: the empty tree) to head, per file with hunks, or one file when path is given.", example = crate::examples::diff)]
    Diff {
        repo: String,
        base: Option<String>,
        head: String,
        path: Option<Vec<u8>>,
        page: PageRequest,
    },
    #[reads(REFS)]
    #[ask(Reply::Compare { height: u64, comparison: Comparison })]
    #[tool_query(description = "Two revisions' resolved heads, merge base, ahead and behind, and whether from merges into into cleanly.", example = crate::examples::compare)]
    Compare {
        repo: String,
        from: Revision,
        into: Revision,
    },
    #[reads(REPOS)]
    #[ask(Reply::Activity { height: u64, last_height: u64 })]
    #[tool_query(description = "The height of a repository's last successful op.", example = crate::examples::activity)]
    Activity { repo: String },
    #[reads(CHANGES, INVOLVED)]
    #[ask(Reply::Changes { height: u64, page: PageResponse<ChangeSummary> })]
    #[tool_query(description = "A repository's changes in number order, filtered by author, involvement or state: author, endpoints, counts and verdicts. A filtered page can be empty with a next cursor: go on until next is null.", example = crate::examples::changes)]
    Changes {
        repo: String,
        filter: ChangeFilter,
        page: PageRequest,
    },
    /// The change, its two heads and a page of its reviews.
    #[reads(CHANGES, REVIEWS, REFS)]
    #[ask(Reply::Change { height: u64, change: Change, source_head: Option<String>, target_head: Option<String>, reviews: PageResponse<Review> })]
    #[tool_query(description = "One change: its record and body, its current source and target heads, and a page of its reviews.", example = crate::examples::change)]
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
    #[ask(Reply::Judgment { height: u64, page: PageResponse<Judgment> })]
    #[tool_query(description = "What one account owes across every repository: open changes asking its review at their current head, or holding a thread it wrote that someone answered.", example = crate::examples::judgment)]
    Judgment {
        principal: Principal,
        page: PageRequest,
    },
    #[ask(Reply::Closure { height: u64, listed: Vec<Listed>, next: Walk })]
    #[tool_query(description = "For a fetch: the objects a walk's wants reach less what its haves reach, a page at a time; send next back for the rest.", example = crate::examples::closure)]
    Closure { repo: String, walk: Walk },
    // A repository's protected branches, in byte-name order; its head is protected besides.
    #[reads(PROTECTED, REPOS)]
    #[ask(Reply::Protected { height: u64, page: PageResponse<Vec<u8>> })]
    #[tool_query(description = "A repository's protected branches in byte order; its head is protected besides.", example = crate::examples::protected)]
    Protected { repo: String, page: PageRequest },
    // the refs whose name starts with `prefix` ("" for every ref), byte-name order, names as text
    #[reads(REFS)]
    #[ask(Reply::RefNames { height: u64, page: PageResponse<RefName> })]
    #[tool_query(description = "The refs whose name starts with prefix (\"\" for every ref), names as text, with their target oids.", example = crate::examples::ref_names)]
    RefNames {
        repo: String,
        prefix: String,
        page: PageRequest,
    },
    // what `Diff` answers less the hunks: each file's paths as text, kinds, sizes and line counts
    #[reads(REFS)]
    #[ask(Reply::DiffStat { height: u64, base: Option<String>, head: String, total_files: u64, page: PageResponse<FileStat> })]
    #[tool_query(description = "What Diff answers less the hunks: each file's path as text, status, kinds, sizes and line counts.", example = crate::examples::diff_stat)]
    DiffStat {
        repo: String,
        base: Option<String>,
        head: String,
        page: PageRequest,
    },
    // change `n`'s latest Preview
    #[reads(PREVIEWS)]
    #[ask(Reply::Preview { height: u64, preview: Option<PreviewInfo> })]
    #[tool_query(description = "Change n's latest Preview: the heads it read and the squash it made, or null.", example = crate::examples::preview_of)]
    Preview { repo: String, n: u64 },
    // every status of a commit, by reporter, then context
    #[reads(STATUSES)]
    #[ask(Reply::Statuses { height: u64, page: PageResponse<CommitStatus> })]
    #[tool_query(description = "Every status of a commit, by reporter then context, with state, url and height.", example = crate::examples::statuses)]
    Statuses {
        repo: String,
        commit: String,
        page: PageRequest,
    },
    // a repository's branches with required checks, in byte-name order
    #[reads(REQUIRED)]
    #[ask(Reply::Required { height: u64, page: PageResponse<RequiredBranch> })]
    #[tool_query(description = "A repository's branches with required checks, each with its reporter and contexts.", example = crate::examples::required)]
    Required { repo: String, page: PageRequest },
    // a branch's merge queue, first in line first
    #[reads(QUEUE)]
    #[ask(Reply::Queue { height: u64, page: PageResponse<QueueEntry> })]
    #[tool_query(description = "A branch's merge queue, first in line first, each change with its head when it joined and who queued it.", example = crate::examples::queue)]
    Queue {
        repo: String,
        branch: Vec<u8>,
        page: PageRequest,
    },
    // whether a repository is archived
    #[reads(ARCHIVED)]
    #[ask(Reply::Archived { height: u64, archived: bool })]
    #[tool_query(description = "Whether a repository is archived.", example = crate::examples::archived)]
    Archived { repo: String },
    // the archived repositories, in name order: what `Repos` leaves out
    #[reads(ARCHIVED, REPOS)]
    #[ask(Reply::ArchivedRepos { height: u64, page: PageResponse<RepoInfo> })]
    #[tool_query(description = "The archived repositories, which Repos leaves out, in name order.", example = crate::examples::archived_repos)]
    ArchivedRepos { page: PageRequest },
    // `Closure` less the trees: the commits the walk reaches, newest first, and the tags, trees
    // and blobs a want names itself; the client expands each commit's tree against its parents'
    // from tree bodies it reads itself (`gitcore::walk::Trees`). Answers `Reply::Closure`.
    #[ask(Reply::Closure { height: u64, listed: Vec<Listed>, next: Walk })]
    #[tool_query(description = "Closure less the trees: the commits a walk reaches, newest first, and the tags, trees and blobs a want names itself; send next back for the rest.", example = crate::examples::history)]
    History { repo: String, walk: Walk },
    // `History` cut `deepen` commits below each want, for a shallow fetch (`git fetch --depth`),
    // and the boundary that leaves (`gitcore::walk::shallow_history`).
    #[ask(Reply::Shallow { height: u64, listed: Vec<Listed>, next: DeepWalk, shallow: Vec<String>, unshallow: Vec<String> })]
    #[tool_query(description = "For a shallow fetch (git fetch --depth): History cut deepen commits below each want, and once its depths are walked the boundary, shallow (new) and unshallow (the client's, now passed); send next back for the rest.", example = crate::examples::shallow)]
    Shallow { repo: String, walk: DeepWalk },
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
            | Query::DiffStat { page, .. }
            | Query::Statuses { page, .. }
            | Query::Required { page, .. }
            | Query::Queue { page, .. }
            | Query::ArchivedRepos { page } => Some(page),
            Query::Advertise { .. }
            | Query::Upload { .. }
            | Query::Repo { .. }
            | Query::Blob { .. }
            | Query::Compare { .. }
            | Query::Activity { .. }
            | Query::Closure { .. }
            | Query::Preview { .. }
            | Query::Archived { .. }
            | Query::History { .. }
            | Query::Shallow { .. } => None,
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
            | Query::DiffStat { page, .. }
            | Query::Statuses { page, .. }
            | Query::Required { page, .. }
            | Query::Queue { page, .. }
            | Query::ArchivedRepos { page } => Some(page),
            Query::Advertise { .. }
            | Query::Upload { .. }
            | Query::Repo { .. }
            | Query::Blob { .. }
            | Query::Compare { .. }
            | Query::Activity { .. }
            | Query::Closure { .. }
            | Query::Preview { .. }
            | Query::Archived { .. }
            | Query::History { .. }
            | Query::Shallow { .. } => None,
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
    Preview {
        height: u64,
        preview: Option<PreviewInfo>,
    },
    Statuses {
        height: u64,
        page: PageResponse<CommitStatus>,
    },
    Required {
        height: u64,
        page: PageResponse<RequiredBranch>,
    },
    Queue {
        height: u64,
        page: PageResponse<QueueEntry>,
    },
    Archived {
        height: u64,
        archived: bool,
    },
    ArchivedRepos {
        height: u64,
        page: PageResponse<RepoInfo>,
    },
    // A `Shallow` page: `listed` and `next` as `Closure`'s; once the depths are walked, `shallow`
    // names the new boundary and `unshallow` the client's boundaries the walk went past.
    Shallow {
        height: u64,
        listed: Vec<Listed>,
        next: DeepWalk,
        shallow: Vec<String>,
        unshallow: Vec<String>,
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

// A shallow fetch's walk (`gitcore::walk::Deep`): `walk` as `History` takes it, the depth it is cut
// at (None: only at the client's own boundaries, `client_shallows`), and the depths walked so far,
// which the client sends back as `next` gave them.
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
pub struct DeepWalk {
    pub walk: Walk,
    pub deepen: Option<u32>,
    pub client_shallows: Vec<String>,
    pub depths: Vec<(String, u32)>,
    pub frontier: Vec<(String, u32)>,
}

impl DeepWalk {
    pub fn of(deep: gitcore::walk::Deep) -> DeepWalk {
        DeepWalk {
            walk: Walk::of(deep.walk),
            deepen: deep.deepen,
            client_shallows: hexes(&deep.client_shallows),
            depths: hexes_at(&deep.depths),
            frontier: hexes_at(&deep.frontier),
        }
    }

    pub fn parse(&self, hash: HashKind) -> Option<gitcore::walk::Deep> {
        let oid_hash = crate::objects::hash_of(hash);
        Some(gitcore::walk::Deep {
            walk: self.walk.parse(hash)?,
            deepen: self.deepen,
            client_shallows: oids(oid_hash, &self.client_shallows)?,
            depths: oids_at(oid_hash, &self.depths)?,
            frontier: oids_at(oid_hash, &self.frontier)?,
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

fn hexes_at(ids: &[(gitcore::Oid, u32)]) -> Vec<(String, u32)> {
    ids.iter()
        .map(|(id, depth)| (id.to_hex(), *depth))
        .collect()
}

fn oids_at(hash: gitcore::Hash, hexes: &[(String, u32)]) -> Option<Vec<(gitcore::Oid, u32)>> {
    hexes
        .iter()
        .map(|(hex, depth)| Some((gitcore::Oid::from_hex(hash, hex).ok()?, *depth)))
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
    Previewed {
        height: u64,
        n: u64,
        oid: String,
    },
}

// What a watcher hears (`Op::Watch`): a ref a push, a merge or a squash moved, `old` or `new`
// None where it was made or deleted, and a change opened, closed or merged; each names who did it.
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
    // change `n`'s Preview made a new commit, `merge`
    Previewed {
        repo: String,
        n: u64,
        merge: String,
        by: Principal,
    },
    Closed {
        repo: String,
        n: u64,
        by: Principal,
    },
    Merged {
        repo: String,
        n: u64,
        oid: String,
        by: Principal,
    },
}

// A commit's status in one context, as its reporter sees it; Errored means the checker or what
// it runs on broke.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum CheckState {
    Pending,
    Passed,
    Failed,
    Errored,
}

impl CheckState {
    pub fn word(self) -> &'static str {
        match self {
            CheckState::Pending => "pending",
            CheckState::Passed => "passed",
            CheckState::Failed => "failed",
            CheckState::Errored => "errored",
        }
    }
}

// What a branch with required checks needs: `reporter`'s Passed in each context.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Required {
    pub reporter: Principal,
    pub contexts: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct RequiredBranch {
    pub branch: Vec<u8>,
    pub required: Required,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct CommitStatus {
    pub reporter: Principal,
    pub context: String,
    pub state: CheckState,
    pub url: Option<String>,
    pub height: u64,
}

// A Preview: the two heads it read and the squash it made of them.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct PreviewInfo {
    pub n: u64,
    pub into_head: String,
    pub from_head: String,
    pub merge: String,
    pub height: u64,
}

// A change in a merge queue: its head when it joined and who queued it.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct QueueEntry {
    pub n: u64,
    pub head: String,
    pub by: Principal,
    pub height: u64,
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
