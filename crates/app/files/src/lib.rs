//! The `files` program: duckfs, the network's versioned filesystem. Its wire types live
//! here; `program.rs` dispatches them, `ops.rs`/`commit.rs` write and `queries.rs` reads. Who
//! writes a path is its nearest write list in the access program, else anyone; `gate.rs`
//! answers what each op needs there and who owns each path.
mod commit;
mod cursor;
mod gate;
mod objects;
mod ops;
pub mod paths;
mod program;
mod queries;
mod state;
#[cfg(test)]
mod tests;
mod tree;

use std::collections::BTreeMap;

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

pub use ::program::{AccountNumber, BlobId, Ownership, Party, Principal};
pub use gate::pin_key;
pub use objects::{Access, CHUNK, FILE, FileObj, SNAPSHOT, SnapshotObj, TREE, Tree};
pub use program::Files;
pub use store::{PageRequest, PageResponse};

pub const PROGRAM: &str = "files";

pub const CHUNK_SIZE: u64 = 1 << 20;

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Op {
    /// Retired: file bytes never ride in a transaction ([`carried`]), so
    /// it is always refused. It stays so the ops of older blocks still read
    /// (`op_variants_only_append`).
    Stage {
        bytes: Vec<u8>,
    },
    Commit {
        base: Option<BlobId>,
        message: String,
        changes: Vec<Change>,
    },
    Pin {
        name: String,
        snapshot: BlobId,
    },
    Unpin {
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Change {
    Put {
        path: String,
        exec: bool,
        meta: BTreeMap<String, String>,
        content: Content,
    },
    Mkdir {
        path: String,
    },
    Remove {
        path: String,
    },
    Move {
        from: String,
        to: String,
    },
    Symlink {
        path: String,
        target: String,
    },
    SetOwnership {
        path: String,
        write: Option<Ownership>,
    },
}

impl Change {
    pub fn paths(&self) -> Vec<&str> {
        match self {
            Change::Put { path, .. }
            | Change::Mkdir { path }
            | Change::Remove { path }
            | Change::Symlink { path, .. }
            | Change::SetOwnership { path, .. } => vec![path],
            Change::Move { from, to } => vec![from, to],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Content {
    /// Retired as [`Op::Stage`] is: always refused, kept so older commits
    /// still read.
    Inline(Vec<u8>),
    /// `size` bytes in `chunks`, in order: blobs of kind [`CHUNK`] the
    /// network holds, each [`CHUNK_SIZE`] but the last. Their bytes go up
    /// through the node's blob upload, apart from any transaction.
    Chunks { size: u64, chunks: Vec<BlobId> },
}

/// The refusal of file bytes in a transaction, [`Op::Stage`] or
/// [`Content::Inline`]: a transaction names chunks, never carries them.
pub(crate) fn carried() -> ::program::Error {
    ::program::invalid(
        "file bytes never ride in a transaction: put them as chunk blobs through the \
         node's blob upload and name them in Content::Chunks",
    )
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Node {
    File { file: BlobId, size: u64, exec: bool },
    Dir { tree: BlobId, entries: u64 },
    Symlink { target: String },
}

impl Node {
    pub fn kind(&self) -> &'static str {
        match self {
            Node::File { .. } => "file",
            Node::Dir { .. } => "dir",
            Node::Symlink { .. } => "symlink",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct EntryInfo {
    pub path: String,
    pub node: Node,
    pub meta: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct SnapshotRow {
    pub id: BlobId,
    pub seq: u64,
    pub parent: Option<BlobId>,
    pub root: BlobId,
    pub author: Principal,
    pub height: u64,
    pub time: u64,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct PinRow {
    pub name: String,
    pub snapshot: BlobId,
    pub owner: Principal,
    pub height: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Contents {
    pub snapshot: BlobId,
    pub size: u64,
    pub offset: u64,
    pub bytes: Vec<u8>,
    pub eof: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Hit {
    pub path: String,
    pub line: u64,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum DiffKind {
    Added,
    Removed,
    Modified,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct DiffEntry {
    pub path: String,
    pub kind: DiffKind,
}

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Ask,
)]
#[ask(Files)]
pub enum Query {
    #[ask(Reply::Head(Option<SnapshotRow>))]
    #[reads(state::HEAD)]
    Head,
    #[ask(Reply::Snapshot(Option<SnapshotRow>))]
    #[reads(state::HISTORY, state::SNAPSHOTS)]
    Snapshot { id: BlobId },
    #[ask(Reply::History(PageResponse<SnapshotRow>))]
    #[reads(state::HISTORY)]
    History { page: PageRequest },
    #[ask(Reply::Stat { snapshot: Option<BlobId>, entry: Option<EntryInfo> })]
    #[reads(state::HEAD, state::HISTORY, state::SNAPSHOTS)]
    Stat { path: String, at: Option<BlobId> },
    #[ask(Reply::Listing { snapshot: Option<BlobId>, entries: PageResponse<EntryInfo> })]
    #[reads(state::HEAD, state::HISTORY, state::SNAPSHOTS)]
    List {
        path: String,
        at: Option<BlobId>,
        page: PageRequest,
    },
    #[ask(Reply::Contents(crate::Contents))]
    #[reads(state::HEAD, state::HISTORY, state::SNAPSHOTS)]
    Read {
        path: String,
        at: Option<BlobId>,
        offset: u64,
        len: u64,
    },
    #[ask(Reply::Listing { snapshot: Option<BlobId>, entries: PageResponse<EntryInfo> })]
    #[reads(state::HEAD, state::HISTORY, state::SNAPSHOTS)]
    Find {
        prefix: String,
        at: Option<BlobId>,
        page: PageRequest,
    },
    #[ask(Reply::Hits { snapshot: Option<BlobId>, hits: PageResponse<Hit> })]
    #[reads(state::HEAD, state::HISTORY, state::SNAPSHOTS)]
    Grep {
        pattern: String,
        prefix: String,
        at: Option<BlobId>,
        page: PageRequest,
    },
    #[ask(Reply::Diff(Vec<DiffEntry>))]
    #[reads(state::HISTORY, state::SNAPSHOTS)]
    Diff {
        from: BlobId,
        to: BlobId,
        prefix: String,
    },
    #[ask(Reply::Pins(PageResponse<PinRow>))]
    #[reads(state::PINS)]
    Pins { page: PageRequest },
    #[ask(Reply::Pin(Option<PinRow>))]
    #[reads(state::PINS)]
    Pin { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Head(Option<SnapshotRow>),
    Snapshot(Option<SnapshotRow>),
    History(PageResponse<SnapshotRow>),
    Stat {
        snapshot: Option<BlobId>,
        entry: Option<EntryInfo>,
    },
    Listing {
        snapshot: Option<BlobId>,
        entries: PageResponse<EntryInfo>,
    },
    Contents(Contents),
    Hits {
        snapshot: Option<BlobId>,
        hits: PageResponse<Hit>,
    },
    Diff(Vec<DiffEntry>),
    Pins(PageResponse<PinRow>),
    Pin(Option<PinRow>),
}

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let hash = |id: &BlobId| Value::Hash(id.digest().to_vec());
    let (title, fields) = match op {
        Op::Stage { bytes } => (
            "Stage a chunk".to_owned(),
            vec![field("bytes", Value::count(bytes.len() as u64))],
        ),
        Op::Commit {
            base,
            message,
            changes,
        } => {
            let paths = changes
                .iter()
                .flat_map(Change::paths)
                .map(Value::text)
                .collect();
            let mut fields = vec![
                field("message", Value::text(message)),
                field("paths", Value::List(paths)),
            ];
            fields.extend(changes.iter().filter_map(write_list));
            if let Some(base) = base {
                fields.push(field("base", hash(base)));
            }
            (format!("Commit {} changes", changes.len()), fields)
        }
        Op::Pin { name, snapshot } => (
            format!("Pin {name}"),
            vec![field("snapshot", hash(snapshot))],
        ),
        Op::Unpin { name } => (format!("Unpin {name}"), Vec::new()),
    };
    describe::Description { title, fields }
}

fn write_list(change: &Change) -> Option<describe::Field> {
    use describe::{Value, field};
    let Change::SetOwnership { path, write } = change else {
        return None;
    };
    let creator = || Value::text("its creator");
    let who = match write {
        None => Value::text("no list"),
        Some(Ownership::Owner) => creator(),
        Some(Ownership::Listed(listed)) => Value::List(
            std::iter::once(creator())
                .chain(listed.iter().map(party))
                .collect(),
        ),
        Some(Ownership::Anyone) => Value::text("anyone"),
    };
    Some(field(
        "write list",
        Value::List(vec![Value::text(path), who]),
    ))
}

fn party(party: &Party) -> describe::Value {
    match party {
        Party::Account(number) => describe::Value::Account(*number),
        Party::Program(program) => describe::Value::Program(program.clone()),
    }
}

describe::export!(Op, describe);

#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        ["Stage", "Commit", "Pin", "Unpin"]
    );
}
