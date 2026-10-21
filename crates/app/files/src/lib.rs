//! The `files` program: duckfs, the network's versioned filesystem. `program.rs` dispatches its
//! wire types to `ops.rs`/`commit.rs`, which write, and `queries.rs`, which reads. Who writes a
//! path is the policy of its nearest guard at or above it (`guards.rs`), else anyone.
mod adopt;
mod commit;
mod cursor;
mod examples;
mod gate;
pub mod grep;
mod guards;
mod hold;
mod objects;
mod ops;
pub mod paths;
mod program;
mod queries;
mod state;
#[cfg(test)]
mod tests;
mod tree;
mod watchers;

use std::collections::BTreeMap;

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

pub use ::program::{AccountNumber, BlobId, Ownership, Party, Principal, code};
pub use gate::pin_key;
pub use objects::{Access, CHUNK, FILE, FileObj, SNAPSHOT, SnapshotObj, TREE, Tree};
pub use program::Files;
pub use state::{PathGuard, guard_key};
pub use store::{PageRequest, PageResponse};

pub mod tables {
    pub use crate::state::{GUARDED, PINS};
}

pub const PROGRAM: &str = "files";

pub const CHUNK_SIZE: u64 = 1 << 20;

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Described,
)]
#[tool_about(
    "The network's versioned filesystem: every commit is a snapshot. A path's writers are the policy of its nearest guard at or above it in the ownership program, else anyone. The namespace roots files' params name are written by the chain alone: write beneath them. Only the head is kept whole: a read at an older snapshot answers not_found (pruned on this node) once a newer commit let its objects go."
)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum Op {
    #[tool_op(description = "Make one snapshot of changes: Put (a file's chunks, each a 1 MiB blob of kind chunk put through the node's blob upload first, the last shorter), Mkdir, Remove, Move, Symlink. With base the snapshot you read, it is refused as stale if a path it touches changed since; with base null it only creates, refused if anything is at a path it touches. Each path is written where its write list admits you. SetOwnership is retired: a write list is set in the access program. Returns the new snapshot's id.", returns = BlobId, example = examples::commit)]
    Commit {
        base: Option<BlobId>,
        message: String,
        changes: Vec<Change>,
    } = 1,
    #[tool_op(description = "Name a snapshot, so it stays findable by that name. The name is not empty, has no control characters, and is not taken. Anyone may pin; the pin is yours.", example = examples::pin)]
    Pin { name: String, snapshot: BlobId },
    #[tool_op(description = "Drop a pin. Its owner, or whom the owner lists.", example = examples::unpin)]
    Unpin { name: String },
    // A program hears of every commit, each as `deliver ++ borsh(SnapshotRow)`; an empty
    // `deliver` stops it.
    #[tool_op(description = "From a program only: be told of every commit, each as a message of deliver followed by the snapshot's row in borsh. An empty deliver stops it.", example = examples::watch)]
    Watch { deliver: Vec<u8> },
    #[tool_op(description = "Guard a path: from then on the ownership program's policy for it decides who writes it and everything beneath it that keeps no guard of its own. Its owner alone guards it: the account that made it, for a namespace root's folders the account or program files' params name. The guard starts on the owner's default policy for files.path; change it with ownership Relink.", example = examples::guard)]
    Guard { path: String },
    #[tool_op(description = "Drop a path's guard, so the guard above it decides again, else anyone. Whom the guard's policy admits.", example = examples::unguard)]
    Unguard { path: String },
    #[tool_op(description = "One-time backfill of owners for files' guarded paths (files.path: each write list the access program keeps for files, and each file beneath a chat message) and pins (files.pin), a page at a time; returns the key to resume after, or null when done.", returns = Option<Vec<u8>>, example = examples::adopt)]
    AdoptOwners {
        kind: String,
        after: Option<Vec<u8>>,
        limit: u64,
    },
    #[tool_op(description = "From files itself only: drop the guards a move carried away from their old paths, once ownership moved their records.", example = examples::vacate)]
    Vacate { paths: Vec<String> },
    #[tool_op(description = "Hold one folder of the head on its lines: the first folder after `after` in path order (the root and the snapshot when null), its tree and each file object in it, put again with the same bytes, then book the next call a block later. A one-time step for objects put before files held them on lines; open to anyone, and a second chain changes nothing. Returns the folder it held, or null once none is left.", returns = Option<String>, example = examples::hold_head)]
    HoldHead { after: Option<String> },
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
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum Content {
    /// `size` bytes in `chunks`, in order: blobs of kind [`CHUNK`] the
    /// network holds, each [`CHUNK_SIZE`] but the last. Their bytes go up
    /// through the node's blob upload, apart from any transaction.
    Chunks { size: u64, chunks: Vec<BlobId> } = 1,
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
    Clone,
    Debug,
    PartialEq,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    ::program::Ask,
    ::program::Described,
)]
#[ask(Files)]
pub enum Query {
    #[ask(Reply::Head(Option<SnapshotRow>))]
    #[reads(state::HEAD)]
    #[tool_query(description = "The newest snapshot, or null before the first commit.", example = examples::head)]
    Head,
    #[ask(Reply::Snapshot(Option<SnapshotRow>))]
    #[reads(state::HISTORY, state::SNAPSHOTS)]
    #[tool_query(description = "One snapshot by id, or null.", example = examples::snapshot)]
    Snapshot { id: BlobId },
    #[ask(Reply::History(PageResponse<SnapshotRow>))]
    #[reads(state::HISTORY)]
    #[tool_query(description = "Every snapshot, newest first, a page at a time.", example = examples::history)]
    History { page: PageRequest },
    #[ask(Reply::Stat { snapshot: Option<BlobId>, entry: Option<EntryInfo> })]
    #[reads(state::HEAD, state::HISTORY, state::SNAPSHOTS)]
    #[tool_query(description = "What is at a path (a file with its size and exec bit, a directory, a symlink) and its meta, or null; at null reads the head.", example = examples::stat)]
    Stat { path: String, at: Option<BlobId> },
    #[ask(Reply::Listing { snapshot: Option<BlobId>, entries: PageResponse<EntryInfo> })]
    #[reads(state::HEAD, state::HISTORY, state::SNAPSHOTS)]
    #[tool_query(description = "A directory's entries, a page at a time.", example = examples::list)]
    List {
        path: String,
        at: Option<BlobId>,
        page: PageRequest,
    },
    #[ask(Reply::Contents(crate::Contents))]
    #[reads(state::HEAD, state::HISTORY, state::SNAPSHOTS)]
    #[tool_query(description = "len bytes of a file from offset, with the file's size and whether the read reached its end (eof).", example = examples::read)]
    Read {
        path: String,
        at: Option<BlobId>,
        offset: u64,
        len: u64,
    },
    #[ask(Reply::Listing { snapshot: Option<BlobId>, entries: PageResponse<EntryInfo> })]
    #[reads(state::HEAD, state::HISTORY, state::SNAPSHOTS)]
    #[tool_query(description = "Every file and directory whose path starts with prefix, in path order.", example = examples::find)]
    Find {
        prefix: String,
        at: Option<BlobId>,
        page: PageRequest,
    },
    #[ask(Reply::Hits { snapshot: Option<BlobId>, hits: PageResponse<Hit> })]
    #[reads(state::HEAD, state::HISTORY, state::SNAPSHOTS)]
    #[tool_query(description = "Each line holding pattern as literal text, with its path and line number, under prefix. A pattern holding newlines matches across lines (CRLF counts as one newline): its hit is the line it starts on, with every line it spans. A page ends once it holds its limit of hits or has read files' chunk size (1 MiB) of file bytes, and next goes on from there, inside a file if need be: a rare pattern over a large tree answers pages with few hits or none, each with next.", example = examples::grep)]
    Grep {
        pattern: String,
        prefix: String,
        at: Option<BlobId>,
        page: PageRequest,
    },
    #[ask(Reply::Diff(Vec<DiffEntry>))]
    #[reads(state::HISTORY, state::SNAPSHOTS, state::DIFFS, state::RECORDED_FROM)]
    #[tool_query(description = "The paths under prefix that were added, removed or modified between two snapshots, folded from what each commit recorded: a removed or moved folder is one entry.", example = examples::diff)]
    Diff {
        from: BlobId,
        to: BlobId,
        prefix: String,
    },
    #[ask(Reply::Pins(PageResponse<PinRow>))]
    #[reads(state::PINS)]
    #[tool_query(description = "Every pin, a page at a time.", example = examples::pins)]
    Pins { page: PageRequest },
    #[ask(Reply::Pin(Option<PinRow>))]
    #[reads(state::PINS)]
    #[tool_query(description = "One pin by name, or null.", example = examples::one_pin)]
    Pin { name: String },
    #[ask(Reply::Guarded(Option<String>))]
    #[reads(state::GUARDED)]
    #[tool_query(description = "The nearest guarded path at or above path, whose policy in the ownership program decides who writes it, or null where anyone does.", example = examples::guarded)]
    Guarded { path: String },
    #[ask(Reply::Owner(Option<Party>))]
    #[reads(state::HEAD, state::HISTORY, state::SNAPSHOTS)]
    #[tool_query(description = "Who owns path by files' rule, and so alone may guard it: the account that made it, for a namespace root's folders the account or program files' params name; null where nothing owns it.", example = examples::owner)]
    Owner { path: String },
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
    Guarded(Option<String>),
    Owner(Option<Party>),
}

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let hash = |id: &BlobId| Value::Hash(id.digest().to_vec());
    let (title, fields) = match op {
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
        Op::Watch { deliver } => match deliver.is_empty() {
            true => ("Stop hearing of every commit".to_owned(), Vec::new()),
            false => ("Hear of every commit".to_owned(), Vec::new()),
        },
        Op::Guard { path } => (format!("Guard {path}"), Vec::new()),
        Op::Unguard { path } => (format!("Unguard {path}"), Vec::new()),
        Op::AdoptOwners { kind, .. } => (format!("Record owners · {kind}"), Vec::new()),
        Op::HoldHead { .. } => (
            "Hold a folder of the head on its lines".to_owned(),
            Vec::new(),
        ),
        Op::Vacate { paths } => (
            "Drop moved guards".to_owned(),
            vec![field(
                "paths",
                Value::List(paths.iter().map(Value::text).collect()),
            )],
        ),
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
        [
            "-",
            "Commit",
            "Pin",
            "Unpin",
            "Watch",
            "Guard",
            "Unguard",
            "AdoptOwners",
            "Vacate",
            "HoldHead"
        ]
    );
}
