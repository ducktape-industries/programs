//! Tools over `files`, the network's filesystem: reads at the head or at a snapshot, and
//! commits the agent makes, which files allows where it allows the agent's account or harness.
use std::collections::BTreeMap;

use ::program::{BlobId, Error, ExecCtx, HashKind, invalid};
use files::{Change, Content, EntryInfo, Node, Ownership, Party, Query, Reply};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    Run, Seat, Tool, blob, cursor_schema, next, object, page, principal, unexpected, write,
};

pub(super) fn snapshot(text: &Option<String>) -> Result<Option<BlobId>, Error> {
    let Some(text) = text else {
        return Ok(None);
    };
    let digest: [u8; 32] = abi::unhex(text)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| invalid("a snapshot is 64 hex digits"))?;
    Ok(Some(BlobId::Sha256(digest)))
}

pub(super) fn snapshot_schema() -> Value {
    json!({ "type": "string", "description": "a snapshot id (64 hex digits) to read at; omitted, the newest" })
}

fn ask(ctx: &ExecCtx, query: Query) -> Result<Reply, Error> {
    ctx.query(files::PROGRAM, &query)
}

fn entry(entry: &EntryInfo) -> Value {
    let mut value = json!({ "path": entry.path, "kind": entry.node.kind() });
    match &entry.node {
        Node::File { size, exec, .. } => {
            value["size"] = json!(size);
            value["executable"] = json!(exec);
        }
        Node::Dir { entries, .. } => value["entries"] = json!(entries),
        Node::Symlink { target } => value["target"] = json!(target),
    }
    if !entry.meta.is_empty() {
        value["meta"] = json!(entry.meta);
    }
    value
}

fn listing(snapshot: Option<BlobId>, entries: &files::PageResponse<EntryInfo>) -> Value {
    json!({
        "snapshot": snapshot.as_ref().map(blob),
        "entries": entries.items.iter().map(entry).collect::<Vec<_>>(),
        "next": next(&entries.next),
    })
}

#[derive(Deserialize)]
pub(crate) struct ListArgs {
    path: String,
    snapshot: Option<String>,
    cursor: Option<String>,
}

pub(crate) struct List;

impl Tool for List {
    const NAME: &'static str = "files_list";
    const DESCRIPTION: &'static str = "List a directory of the network's shared filesystem: each entry's path, kind and size. Paths are absolute: / holds /home and /shared.";
    type Args = ListArgs;

    fn schema() -> Value {
        object(
            json!({
                "path": { "type": "string", "description": "an absolute directory path, like /shared" },
                "snapshot": snapshot_schema(),
                "cursor": cursor_schema(),
            }),
            &["path"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: ListArgs) -> Result<Run, Error> {
        let query = Query::List {
            path: args.path,
            at: snapshot(&args.snapshot)?,
            page: page(&args.cursor)?,
        };
        match ask(ctx, query)? {
            Reply::Listing { snapshot, entries } => Ok(Run::Done(listing(snapshot, &entries))),
            other => Err(unexpected(files::PROGRAM, "List", &other)),
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct ReadArgs {
    path: String,
    snapshot: Option<String>,
    offset: Option<u64>,
    length: Option<u64>,
}

pub(crate) struct Read;

impl Tool for Read {
    const NAME: &'static str = "files_read";
    const DESCRIPTION: &'static str = "Read a file of the network's filesystem as text, from an offset, as much as files answers in one read; `eof` says whether the file goes on. The answer names the snapshot it read, which an edit can pass as its base.";
    type Args = ReadArgs;

    fn schema() -> Value {
        object(
            json!({
                "path": { "type": "string", "description": "an absolute file path" },
                "snapshot": snapshot_schema(),
                "offset": { "type": "integer", "minimum": 0, "description": "the first byte to read; 0 if omitted" },
                "length": { "type": "integer", "minimum": 1, "description": "how many bytes; the rest of the file if omitted" },
            }),
            &["path"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: ReadArgs) -> Result<Run, Error> {
        let query = Query::Read {
            path: args.path,
            at: snapshot(&args.snapshot)?,
            offset: args.offset.unwrap_or(0),
            len: args.length.unwrap_or(u64::MAX),
        };
        let Reply::Contents(contents) = ask(ctx, query)? else {
            return Err(invalid("files answered a read with something else"));
        };
        let mut value = json!({
            "snapshot": blob(&contents.snapshot),
            "size": contents.size,
            "offset": contents.offset,
            "eof": contents.eof,
        });
        match String::from_utf8(contents.bytes) {
            Ok(text) => value["text"] = json!(text),
            Err(bytes) => value["binary_bytes"] = json!(bytes.into_bytes().len()),
        }
        Ok(Run::Done(value))
    }
}

#[derive(Deserialize)]
pub(crate) struct FindArgs {
    prefix: String,
    snapshot: Option<String>,
    cursor: Option<String>,
}

pub(crate) struct Find;

impl Tool for Find {
    const NAME: &'static str = "files_find";
    const DESCRIPTION: &'static str = "Find every file and directory of the network's filesystem whose path starts with a prefix, in path order.";
    type Args = FindArgs;

    fn schema() -> Value {
        object(
            json!({
                "prefix": { "type": "string", "description": "a path prefix, like /shared/notes/" },
                "snapshot": snapshot_schema(),
                "cursor": cursor_schema(),
            }),
            &["prefix"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: FindArgs) -> Result<Run, Error> {
        let query = Query::Find {
            prefix: args.prefix,
            at: snapshot(&args.snapshot)?,
            page: page(&args.cursor)?,
        };
        match ask(ctx, query)? {
            Reply::Listing { snapshot, entries } => Ok(Run::Done(listing(snapshot, &entries))),
            other => Err(unexpected(files::PROGRAM, "Find", &other)),
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct GrepArgs {
    pattern: String,
    prefix: Option<String>,
    snapshot: Option<String>,
    cursor: Option<String>,
}

pub(crate) struct Grep;

impl Tool for Grep {
    const NAME: &'static str = "files_grep";
    const DESCRIPTION: &'static str = "Search the text of the network's files for a literal string: each matching line with its path and line number.";
    type Args = GrepArgs;

    fn schema() -> Value {
        object(
            json!({
                "pattern": { "type": "string", "description": "the literal text to find" },
                "prefix": { "type": "string", "description": "search only paths starting with this; / if omitted" },
                "snapshot": snapshot_schema(),
                "cursor": cursor_schema(),
            }),
            &["pattern"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: GrepArgs) -> Result<Run, Error> {
        let query = Query::Grep {
            pattern: args.pattern,
            prefix: args.prefix.unwrap_or_else(|| "/".into()),
            at: snapshot(&args.snapshot)?,
            page: page(&args.cursor)?,
        };
        let Reply::Hits { snapshot, hits } = ask(ctx, query)? else {
            return Err(invalid("files answered a grep with something else"));
        };
        let found: Vec<Value> = hits
            .items
            .iter()
            .map(|hit| json!({ "path": hit.path, "line": hit.line, "text": hit.text }))
            .collect();
        Ok(Run::Done(json!({
            "snapshot": snapshot.as_ref().map(blob),
            "hits": found,
            "next": next(&hits.next),
        })))
    }
}

#[derive(Deserialize)]
pub(crate) struct HistoryArgs {
    cursor: Option<String>,
}

pub(crate) struct History;

impl Tool for History {
    const NAME: &'static str = "files_history";
    const DESCRIPTION: &'static str =
        "The filesystem's snapshots, newest first: who committed each, when, and its message.";
    type Args = HistoryArgs;

    fn schema() -> Value {
        object(json!({ "cursor": cursor_schema() }), &[])
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: HistoryArgs) -> Result<Run, Error> {
        let Reply::History(rows) = ask(
            ctx,
            Query::History {
                page: page(&args.cursor)?,
            },
        )?
        else {
            return Err(invalid("files answered history with something else"));
        };
        let snapshots: Vec<Value> = rows
            .items
            .iter()
            .map(|row| {
                json!({
                    "snapshot": blob(&row.id),
                    "seq": row.seq,
                    "author": principal(&row.author),
                    "height": row.height,
                    "message": row.message,
                })
            })
            .collect();
        Ok(Run::Done(
            json!({ "snapshots": snapshots, "next": next(&rows.next) }),
        ))
    }
}

fn commit(
    ctx: &ExecCtx,
    seat: &Seat,
    base: &Option<String>,
    message: Option<String>,
    changes: Vec<Change>,
) -> Result<Run, Error> {
    let base = match snapshot(base)? {
        Some(base) => Some(base),
        None => match ask(ctx, Query::Head)? {
            Reply::Head(head) => head.map(|row| row.id),
            other => return Err(unexpected(files::PROGRAM, "Head", &other)),
        },
    };
    let message =
        message.unwrap_or_else(|| format!("harness thread {} call {}", seat.thread, seat.call));
    let op = files::Op::Commit {
        base,
        message,
        changes,
    };
    Ok(write(files::PROGRAM, abi::encode(&op)))
}

fn committed(output: &[u8]) -> Result<Value, Error> {
    Ok(match abi::decode::<BlobId>(output) {
        Ok(snapshot) => json!({ "snapshot": blob(&snapshot) }),
        Err(_) => json!({ "ok": true }),
    })
}

fn base_schema() -> Value {
    json!({ "type": "string", "description": "the snapshot your edit is based on, from a read; the edit is refused if a path it touches changed since. Omitted, it applies over the newest" })
}

fn message_schema() -> Value {
    json!({ "type": "string", "description": "the commit message" })
}

#[derive(Deserialize)]
pub(crate) struct WriteArgs {
    path: String,
    content: String,
    executable: Option<bool>,
    base: Option<String>,
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "who", rename_all = "snake_case")]
pub(crate) enum WriteList {
    Creator,
    Listed {
        #[serde(default)]
        accounts: Vec<u64>,
        #[serde(default)]
        programs: Vec<String>,
    },
    Anyone,
}

impl From<WriteList> for Ownership {
    fn from(list: WriteList) -> Ownership {
        match list {
            WriteList::Creator => Ownership::Owner,
            WriteList::Listed { accounts, programs } => Ownership::Listed(
                accounts
                    .into_iter()
                    .map(Party::Account)
                    .chain(programs.into_iter().map(Party::Program))
                    .collect(),
            ),
            WriteList::Anyone => Ownership::Anyone,
        }
    }
}

fn write_list_schema() -> Value {
    json!({
        "type": "object",
        "description": "who may write the path and everything beneath it that sets no list of its own",
        "properties": {
            "who": {
                "type": "string",
                "enum": ["creator", "listed", "anyone"],
                "description": "creator: its owner alone; listed: its owner and the accounts and programs listed; anyone: everyone"
            },
            "accounts": { "type": "array", "items": { "type": "integer", "minimum": 1 }, "description": "with listed: account numbers that may write it" },
            "programs": { "type": "array", "items": { "type": "string" }, "description": "with listed: programs, by id, whose writes may change it, whatever account they act as" },
        },
        "required": ["who"],
    })
}

pub(crate) struct Write;

#[derive(Deserialize)]
pub(crate) struct SetListArgs {
    path: String,
    write: Option<WriteList>,
}

pub(crate) struct SetList;

impl Tool for SetList {
    const NAME: &'static str = "files_set_list";
    const DESCRIPTION: &'static str = "Set who may write a path of the network's filesystem and everything beneath it that sets no list of its own, or clear its list with no `write`. Only the path's owner sets it: the account that made it, or for /home/<name> the account named so. The list lives in the access program, which checks every write against the nearest list at or above a path.";
    const WRITES: bool = true;
    type Args = SetListArgs;

    fn schema() -> Value {
        object(
            json!({
                "path": { "type": "string", "description": "an absolute path" },
                "write": write_list_schema(),
            }),
            &["path"],
        )
    }

    fn run(ctx: &ExecCtx, _seat: &Seat, args: SetListArgs) -> Result<Run, Error> {
        let set = access::Op::Set {
            program: files::PROGRAM.into(),
            key: args.path,
            list: args.write.map(Ownership::from),
        };
        Ok(Run::Write(vec![super::Write {
            target: ctx.env().role(abi::role::ACCESS)?.clone(),
            payload: abi::encode(&set),
        }]))
    }
}

/// `bytes` as `files` keeps a file's: `chunk` blobs this program puts,
/// which the commit names; its bytes never ride in the commit.
fn chunked(ctx: &ExecCtx, bytes: &[u8]) -> Result<Content, Error> {
    let chunks = bytes
        .chunks(files::CHUNK_SIZE as usize)
        .map(|chunk| ctx.blob_put(HashKind::Sha256, files::CHUNK, chunk))
        .collect::<Result<_, _>>()?;
    Ok(Content::Chunks {
        size: bytes.len() as u64,
        chunks,
    })
}

impl Tool for Write {
    const NAME: &'static str = "files_write";
    const DESCRIPTION: &'static str = "Write a text file to the network's filesystem, creating its directories, in one commit. Anyone writes a path that no write list covers; where the path or a folder above it carries a write list, the nearest list decides: its owner alone, its owner and the accounts and programs it lists, or anyone. A program on a list allows every write harness makes, for any agent. A refusal names the list and whom it allows. /home, /shared and each /home/<name> folder itself are the network's; write inside them. Set a list with files_set_list.";
    const WRITES: bool = true;
    type Args = WriteArgs;

    fn schema() -> Value {
        object(
            json!({
                "path": { "type": "string", "description": "an absolute file path" },
                "content": { "type": "string", "description": "the whole file" },
                "executable": { "type": "boolean" },
                "write": write_list_schema(),
                "base": base_schema(),
                "message": message_schema(),
            }),
            &["path", "content"],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: WriteArgs) -> Result<Run, Error> {
        let put = Change::Put {
            path: args.path.clone(),
            exec: args.executable.unwrap_or(false),
            meta: BTreeMap::new(),
            content: chunked(ctx, args.content.as_bytes())?,
        };
        commit(ctx, seat, &args.base, args.message, vec![put])
    }

    fn render(output: &[u8]) -> Result<Value, Error> {
        committed(output)
    }
}

#[derive(Deserialize)]
pub(crate) struct RemoveArgs {
    path: String,
    base: Option<String>,
    message: Option<String>,
}

pub(crate) struct Remove;

impl Tool for Remove {
    const NAME: &'static str = "files_remove";
    const DESCRIPTION: &'static str = "Remove a file or a whole directory from the network's filesystem, in one commit. Anyone removes a path that no write list covers; a write list on the path or a folder above it decides otherwise, the nearest list first.";
    const WRITES: bool = true;
    type Args = RemoveArgs;

    fn schema() -> Value {
        object(
            json!({
                "path": { "type": "string" },
                "base": base_schema(),
                "message": message_schema(),
            }),
            &["path"],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: RemoveArgs) -> Result<Run, Error> {
        let change = Change::Remove { path: args.path };
        commit(ctx, seat, &args.base, args.message, vec![change])
    }

    fn render(output: &[u8]) -> Result<Value, Error> {
        committed(output)
    }
}

#[derive(Deserialize)]
pub(crate) struct MoveArgs {
    from: String,
    to: String,
    base: Option<String>,
    message: Option<String>,
}

pub(crate) struct Move;

impl Tool for Move {
    const NAME: &'static str = "files_move";
    const DESCRIPTION: &'static str = "Move or rename a file or directory of the network's filesystem, in one commit. The destination must not exist. Anyone writes a path that no write list covers; a write list on either path or a folder above it decides otherwise, the nearest list first. A moved path keeps its creator and its list.";
    const WRITES: bool = true;
    type Args = MoveArgs;

    fn schema() -> Value {
        object(
            json!({
                "from": { "type": "string" },
                "to": { "type": "string" },
                "base": base_schema(),
                "message": message_schema(),
            }),
            &["from", "to"],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: MoveArgs) -> Result<Run, Error> {
        let change = Change::Move {
            from: args.from,
            to: args.to,
        };
        commit(ctx, seat, &args.base, args.message, vec![change])
    }

    fn render(output: &[u8]) -> Result<Value, Error> {
        committed(output)
    }
}
