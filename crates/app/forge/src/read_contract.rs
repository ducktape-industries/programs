//! Read projections: paths and source text are bytes, exactly as Git
//! stores them.
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

#[derive(
    Clone, Debug, PartialEq, PartialOrd, Ord, Eq, BorshSerialize, BorshDeserialize, BorshSchema,
)]
pub enum Revision {
    Ref(Vec<u8>),
    Oid(String),
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct GitSignature {
    pub name: Vec<u8>,
    pub email: Vec<u8>,
    pub time: i64,
    pub offset_minutes: i16,
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct CommitInfo {
    pub oid: String,
    pub tree: String,
    pub parents: Vec<String>,
    pub author: GitSignature,
    pub committer: GitSignature,
    pub message: Vec<u8>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum EntryKind {
    File,
    Executable,
    Symlink,
    Directory,
    Gitlink,
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct TreeInfo {
    pub name: Vec<u8>,
    pub oid: String,
    pub kind: EntryKind,
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
pub struct ByteRange {
    pub offset: u64,
    pub len: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Content {
    Text,
    Binary,
    Oversize,
    Gitlink,
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct BlobView {
    pub oid: String,
    pub size: u64,
    pub content: Content,
    pub range: ByteRange,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum FileStatus {
    Added,
    Deleted,
    Modified,
    ModeChanged,
    TypeChanged,
}
/// One-based start; an empty range names the preceding line (0 at the beginning).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct LineRange {
    pub start: u64,
    pub count: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum LineKind {
    Context,
    Added,
    Deleted,
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct DiffLine {
    pub kind: LineKind,
    pub old_line: Option<u64>,
    pub new_line: Option<u64>,
    /// Includes the original newline, if any. No patch markers or escaping.
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct DiffHunk {
    pub old: LineRange,
    pub new: LineRange,
    pub lines: Vec<DiffLine>,
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct FileDiff {
    pub old_path: Option<Vec<u8>>,
    pub new_path: Option<Vec<u8>>,
    pub status: FileStatus,
    pub old_oid: Option<String>,
    pub new_oid: Option<String>,
    pub old_kind: Option<EntryKind>,
    pub new_kind: Option<EntryKind>,
    pub old_size: u64,
    pub new_size: u64,
    pub content: Content,
    pub additions: u64,
    pub deletions: u64,
    pub hunks: Vec<DiffHunk>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Mergeability {
    UpToDate,
    FastForward,
    Diverged,
    Unrelated,
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Comparison {
    pub from: String,
    pub into: String,
    pub base: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    pub mergeability: Mergeability,
}

// One branch of a `Compares`: what it was asked as, and its comparison or why there is none.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Compared {
    pub from: Revision,
    pub tally: Tally,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Tally {
    Counted(Comparison),
    Refused(String),
}
