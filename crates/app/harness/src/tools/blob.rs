//! `blob_put`, a blob the agent puts in its call's own run, as harness puts its own: text it
//! gives or a file of `files` read whole, of any kind, for later ops to name, such as the body of
//! an egress request.
use std::fmt;

use ::program::{BlobId, Error, ExecCtx, HashKind};
use serde::Deserialize;
use serde_json::{Value, json};

use super::files::{snapshot, snapshot_schema};
use super::{Run, Seat, Tool, object};
use crate::agents::read;

const HELD: &str = "on the network from this call on: the run that put it is recorded on the chain, so any later op may name it, and egress takes it as a request's body part. To make it code the network may run (kind program), send the blobs program {\"Put\": {\"blobs\": [entry]}} with this answer's entry as it is: its len is the framed length the blobs program records, and a Put whose len is wrong is never held. Until a Put of it is held, the blobs program's Held query answers null for it.";

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Hash {
    #[default]
    Sha256,
    Sha1,
}

impl From<Hash> for HashKind {
    fn from(hash: Hash) -> HashKind {
        match hash {
            Hash::Sha256 => HashKind::Sha256,
            Hash::Sha1 => HashKind::Sha1,
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct Given {
    kind: String,
    content: Option<String>,
    source: Option<String>,
    snapshot: Option<String>,
    #[serde(default)]
    hash: Hash,
}

pub(crate) enum Bytes {
    Content(String),
    Source { path: String, at: Option<String> },
}

pub(crate) enum Unsourced {
    Both,
    Neither,
    SnapshotWithContent,
}

impl fmt::Display for Unsourced {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Unsourced::Both => "give content or source, not both",
            Unsourced::Neither => "give content or source",
            Unsourced::SnapshotWithContent => {
                "a snapshot goes with source: it names the snapshot the file is read at"
            }
        })
    }
}

#[derive(Deserialize)]
#[serde(try_from = "Given")]
pub(crate) struct PutArgs {
    kind: String,
    bytes: Bytes,
    hash: Hash,
}

impl TryFrom<Given> for PutArgs {
    type Error = Unsourced;

    fn try_from(given: Given) -> Result<PutArgs, Unsourced> {
        let bytes = match (given.content, given.source, given.snapshot) {
            (Some(_), Some(_), _) => return Err(Unsourced::Both),
            (None, None, _) => return Err(Unsourced::Neither),
            (Some(_), None, Some(_)) => return Err(Unsourced::SnapshotWithContent),
            (Some(content), None, None) => Bytes::Content(content),
            (None, Some(path), at) => Bytes::Source { path, at },
        };
        Ok(PutArgs {
            kind: given.kind,
            bytes,
            hash: given.hash,
        })
    }
}

fn named(id: &BlobId) -> (String, Value) {
    let digest = abi::hex(id.digest());
    let (scheme, variant) = match id.kind() {
        HashKind::Sha256 => ("sha256", "Sha256"),
        HashKind::Sha1 => ("sha1", "Sha1"),
    };
    (format!("{scheme}:{digest}"), json!({ variant: digest }))
}

pub(crate) struct Put;

impl Tool for Put {
    const NAME: &'static str = "blob_put";
    const DESCRIPTION: &'static str = "Put a blob of any kind on the network, in this call's own run: content as its UTF-8 bytes, or source, a file of the network's filesystem read whole at snapshot or the newest. It answers the blob's id as `blob` and as `in_network_call`, the form a network_call op names it in: the body of an egress request (kind part), a pods input, a files chunk. It also answers `entry`, the blob as a blobs-program Put names it (id, kind and len, the framed length), to send as it is in blobs {\"Put\": {\"blobs\": [entry]}}. The blob is on the network from this call on, so any later op may name it; until a Put of that entry is held, the blobs program's Held query answers null for it. A blob has no size bound of its own: the run's memory and fuel, which the network sets, limit what one call carries, and past them the call fails. Split a large body into several part blobs: egress sends a request's body parts joined in order.";
    const WRITES: bool = true;
    type Args = PutArgs;

    fn schema() -> Value {
        object(
            json!({
                "kind": { "type": "string", "description": "the blob's kind, one word: part for an egress request's body, or program, chunk, any other" },
                "content": { "type": "string", "description": "the blob's bytes as text, put as UTF-8; give this or source" },
                "source": { "type": "string", "description": "an absolute file path of the network's filesystem, whose whole file is the blob's bytes; give this or content" },
                "snapshot": snapshot_schema(),
                "hash": { "type": "string", "enum": ["sha256", "sha1"], "description": "the hash the blob's id is taken with; sha256 if omitted" },
            }),
            &["kind"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: PutArgs) -> Result<Run, Error> {
        let body = match args.bytes {
            Bytes::Content(content) => content.into_bytes(),
            Bytes::Source { path, at } => read(ctx, &path, snapshot(&at)?)?.1,
        };
        let size = body.len();
        let id = ctx.blob_put(args.hash.into(), args.kind.as_str(), body)?;
        let (blob, in_network_call) = named(&id);
        let framed_len = ::blob::header(&args.kind, size as u64).len() + size;
        let entry = json!({ "id": in_network_call, "kind": args.kind, "len": framed_len });
        Ok(Run::Done(json!({
            "blob": blob,
            "size": size,
            "kind": args.kind,
            "in_network_call": in_network_call,
            "entry": entry,
            "held": HELD,
        })))
    }
}
