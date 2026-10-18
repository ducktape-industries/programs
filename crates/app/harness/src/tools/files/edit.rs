//! `files_edit`: change part of a text file without re-sending the rest. The file is read whole
//! at one snapshot, the edits apply in order, and the new text commits based on that snapshot, so
//! a file changed since is refused stale, never overwritten. The file keeps its mode and meta.
use ::program::{Error, ExecCtx, invalid, not_found};
use files::{Change, EntryInfo, Node};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{chunked, commit, committed, message_schema, read_all, resolved, snapshot, stat};
use crate::tools::edits::{EditArg, applied, edit_schema, steps};
use crate::tools::{Run, Seat, Tool, object};

#[derive(Deserialize)]
pub(crate) struct EditArgs {
    path: String,
    edits: Vec<EditArg>,
    base: Option<String>,
    message: Option<String>,
}

pub(crate) struct Edit;

impl Tool for Edit {
    const NAME: &'static str = "files_edit";
    const DESCRIPTION: &'static str = "Change part of a text file of the network's filesystem without re-sending the rest, in one commit. Each edit replaces a span, inserts text before or after one, or appends text at the end; a span must occur exactly once in the file, or the call is refused with where it occurs. Edits apply in order, each to the text the one before it left, and commit together or not at all. The file keeps its mode and meta. The same write lists as files_write decide. The answer names the new snapshot.";
    const WRITES: bool = true;
    type Args = EditArgs;

    fn schema() -> Value {
        object(
            json!({
                "path": { "type": "string", "description": "an absolute path of a text file that exists; files_write makes a new one" },
                "edits": { "type": "array", "items": edit_schema(), "description": "the edits, applied in order" },
                "base": { "type": "string", "description": "the snapshot to read the file at and base the commit on, from a read; the edit is refused if the file changed since. Omitted, the newest" },
                "message": message_schema(),
            }),
            &["path", "edits"],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: EditArgs) -> Result<Run, Error> {
        if args.edits.is_empty() {
            return Err(invalid("files_edit takes at least one edit"));
        }
        let steps = steps(args.edits)?;
        let base = resolved(ctx, snapshot(&args.base)?)?;
        let (exec, meta) = match stat(ctx, &args.path, base.as_ref())? {
            Some(EntryInfo {
                node: Node::File { exec, .. },
                meta,
                ..
            }) => (exec, meta),
            Some(_) => return Err(invalid(format!("{} is not a file", args.path))),
            None => {
                return Err(not_found(format!(
                    "there is no file at {} to edit",
                    args.path
                )));
            }
        };
        let text = String::from_utf8(read_all(ctx, &args.path, base.as_ref())?)
            .map_err(|_| invalid(format!("{} is not text", args.path)))?;
        let text = applied(text, steps, &args.path)?;
        let put = Change::Put {
            path: args.path,
            exec,
            meta,
            content: chunked(ctx, text.as_bytes())?,
        };
        Ok(commit(seat, base, args.message, vec![put]))
    }

    fn render(output: &[u8]) -> Result<Value, Error> {
        committed(output)
    }
}
