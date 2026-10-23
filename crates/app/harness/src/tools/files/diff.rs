//! A per-file text diff at two explicit files snapshots, with metadata and ranged patch reads.

use ::program::{BlobId, Error, ExecCtx, invalid, not_found};
use files::{EntryInfo, Node};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{entry, named_snapshot, read_all, stat};
use crate::tools::{Run, Seat, Tool, WHOLE_UP_TO, blob, lines, object, unexpected};

#[derive(Deserialize)]
pub(crate) struct Args {
    path: String,
    from: String,
    to: String,
    lines: Option<(u64, u64)>,
    whole: Option<bool>,
}

pub(crate) struct Diff;

fn forge_bounds(ctx: &ExecCtx) -> Result<forge::Bounds, Error> {
    match ctx.query(forge::PROGRAM, &forge::Query::Bounds)? {
        forge::Reply::Bounds { bounds, .. } => Ok(bounds),
        other => Err(unexpected(forge::PROGRAM, "Bounds", &other)),
    }
}

fn size_of(entry: Option<&EntryInfo>) -> u64 {
    match entry.map(|entry| &entry.node) {
        Some(Node::File { size, .. }) => *size,
        Some(Node::Symlink { target }) => target.len() as u64,
        Some(Node::Dir { .. }) | None => 0,
    }
}

fn contents(
    ctx: &ExecCtx,
    path: &str,
    at: &BlobId,
    entry: Option<&EntryInfo>,
) -> Result<Vec<u8>, Error> {
    let Some(entry) = entry else {
        return Ok(Vec::new());
    };
    if matches!(entry.node, Node::Dir { .. }) {
        return Err(invalid(format!("{path} is a directory, not a file")));
    }
    read_all(ctx, path, Some(at))
}

fn patch(text: &str, pick: Option<(u64, u64)>, whole: bool) -> Result<Value, Error> {
    let mut value = json!({
        "size": text.len(),
        "total_lines": text.split_inclusive('\n').count(),
    });
    if let Some((from, to)) = pick {
        let picked = lines::range(text, from, to)?;
        value["lines"] = json!([picked.first, picked.last]);
        value["eof"] = json!(picked.last >= picked.total);
        value["text"] = json!(picked.text);
        return Ok(value);
    }
    let long = !whole && text.len() as u64 > WHOLE_UP_TO;
    if long {
        value["left_out"] = json!(format!(
            "the diff is over {WHOLE_UP_TO} bytes: read it by lines, or ask whole: true for all of it"
        ));
        return Ok(value);
    }
    value["text"] = json!(text);
    value["eof"] = json!(true);
    Ok(value)
}

impl Tool for Diff {
    const NAME: &'static str = "files_diff";
    const DESCRIPTION: &'static str = "Compare one file at two explicit filesystem snapshots. Returns zero-context unified text hunks and each side's metadata; an absent side is an addition or removal. Directories and a path missing from both snapshots are refused, and missing or pruned data is never replaced with the head. Binary contents are reported without a text patch, and so is a side larger than forge's founded blob_bytes (oversize: too large, neither side read) or an edit past forge's founded merge_cost (oversize: too different). A long patch answers its size and line count: read it by diff-line ranges or with whole: true.";
    type Args = Args;

    fn schema() -> Value {
        object(
            json!({
                "path": { "type": "string", "description": "one absolute file path, the same on both sides" },
                "from": { "type": "string", "description": "the earlier side's explicit snapshot id (64 hex digits)" },
                "to": { "type": "string", "description": "the later side's explicit snapshot id (64 hex digits)" },
                "lines": { "type": "array", "items": { "type": "integer", "minimum": 1 }, "minItems": 2, "maxItems": 2, "description": "[from, to] lines of the unified patch, counted from 1, both included; these are diff lines, not file lines" },
                "whole": { "type": "boolean", "description": "read a long patch whole; give without lines" },
            }),
            &["path", "from", "to"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: Args) -> Result<Run, Error> {
        let whole = args.whole.unwrap_or(false);
        let conflicting_slices = whole && args.lines.is_some();
        if conflicting_slices {
            return Err(invalid(
                "whole reads the patch whole: give it without lines",
            ));
        }
        let from = named_snapshot(&args.from)?;
        let to = named_snapshot(&args.to)?;
        let before = stat(ctx, &args.path, Some(&from))?;
        let after = stat(ctx, &args.path, Some(&to))?;
        let absent = before.is_none() && after.is_none();
        if absent {
            return Err(not_found(format!(
                "{} is absent from both snapshots",
                args.path
            )));
        }
        let bounds = forge_bounds(ctx)?;
        let mut value = json!({
            "path": args.path,
            "from": blob(&from),
            "to": blob(&to),
            "before": before.as_ref().map(entry),
            "after": after.as_ref().map(entry),
            "changed": before != after,
        });
        let largest_side = size_of(before.as_ref()).max(size_of(after.as_ref()));
        let too_large = largest_side > bounds.blob_bytes;
        if too_large {
            value["oversize"] = json!("too large");
            return Ok(Run::Done(value));
        }
        let old = contents(ctx, &args.path, &from, before.as_ref())?;
        let new = contents(ctx, &args.path, &to, after.as_ref())?;
        let binary = old.contains(&0)
            || new.contains(&0)
            || std::str::from_utf8(&old).is_err()
            || std::str::from_utf8(&new).is_err();
        value["binary"] = json!(binary);
        if binary {
            return Ok(Run::Done(value));
        }
        let max_cost = usize::try_from(bounds.merge_cost).unwrap_or(usize::MAX);
        let bytes = match gitcore::diff::unified(&old, &new, 0, max_cost) {
            Err(gitcore::Error::CapReached) => {
                value["oversize"] = json!("too different");
                return Ok(Run::Done(value));
            }
            unified => {
                unified.map_err(|error| invalid(format!("cannot diff {}: {error}", args.path)))?
            }
        };
        let text = String::from_utf8(bytes)
            .map_err(|error| invalid(format!("the text patch is not UTF-8: {error}")))?;
        value["patch"] = patch(&text, args.lines, whole)?;
        Ok(Run::Done(value))
    }
}
