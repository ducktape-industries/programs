//! `files_edit`: change part of a text file without re-sending the rest. The file is read whole
//! at one snapshot, the edits apply in order, and the new text commits based on that snapshot, so
//! a file changed since is refused stale, never overwritten. The file keeps its mode and meta.
use ::program::{Error, ExecCtx, invalid, not_found};
use files::{Change, EntryInfo, Node};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{chunked, commit, committed, message_schema, read_all, resolved, snapshot, stat};
use crate::tools::{Run, Seat, Tool, object};

// One edit as the agent writes it: `replace` with `with`, `after` or `before` with `insert`,
// or `append` alone.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EditArg {
    replace: Option<String>,
    with: Option<String>,
    after: Option<String>,
    before: Option<String>,
    insert: Option<String>,
    append: Option<String>,
}

enum Step {
    Replace {
        span: String,
        with: String,
    },
    Insert {
        span: String,
        text: String,
        after: bool,
    },
    Append(String),
}

impl EditArg {
    // The edit numbered `n`, counted from 1, as one step.
    fn step(self, n: usize) -> Result<Step, Error> {
        match self {
            EditArg {
                replace: Some(span),
                with: Some(with),
                after: None,
                before: None,
                insert: None,
                append: None,
            } => Ok(Step::Replace { span, with }),
            EditArg {
                replace: None,
                with: None,
                after: Some(span),
                before: None,
                insert: Some(text),
                append: None,
            } => Ok(Step::Insert {
                span,
                text,
                after: true,
            }),
            EditArg {
                replace: None,
                with: None,
                after: None,
                before: Some(span),
                insert: Some(text),
                append: None,
            } => Ok(Step::Insert {
                span,
                text,
                after: false,
            }),
            EditArg {
                replace: None,
                with: None,
                after: None,
                before: None,
                insert: None,
                append: Some(text),
            } => Ok(Step::Append(text)),
            _ => Err(invalid(format!(
                "edit {n} is one of: replace with with, after with insert, before with insert, or append alone"
            ))),
        }
    }
}

// The line, counted from 1, that byte `at` of `text` is on.
fn line(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count() + 1
}

// Where `span` starts in `text`, which must hold it exactly once; occurrences that overlap
// count apart, so `aa` is in `aaa` twice.
fn unique(text: &str, span: &str, path: &str, n: usize) -> Result<usize, Error> {
    if span.is_empty() {
        return Err(invalid(format!("edit {n} names an empty span")));
    }
    let found: Vec<usize> = (0..text.len())
        .filter(|&at| text.is_char_boundary(at) && text[at..].starts_with(span))
        .collect();
    match found.as_slice() {
        [at] => Ok(*at),
        [] => Err(not_found(format!("edit {n}: the span is not in {path}"))),
        many => {
            let lines: Vec<String> = many.iter().map(|&at| line(text, at).to_string()).collect();
            Err(invalid(format!(
                "edit {n}: the span is in {path} {} times, at lines {}; widen it until the file holds it once",
                many.len(),
                lines.join(", ")
            )))
        }
    }
}

// `text` once every step applies, in order, to what the one before it left.
fn applied(mut text: String, steps: Vec<Step>, path: &str) -> Result<String, Error> {
    for (index, step) in steps.into_iter().enumerate() {
        let n = index + 1;
        match step {
            Step::Replace { span, with } => {
                let at = unique(&text, &span, path, n)?;
                text.replace_range(at..at + span.len(), &with);
            }
            Step::Insert {
                span,
                text: inserted,
                after,
            } => {
                let at = unique(&text, &span, path, n)?;
                let at = match after {
                    true => at + span.len(),
                    false => at,
                };
                text.insert_str(at, &inserted);
            }
            Step::Append(more) => text.push_str(&more),
        }
    }
    Ok(text)
}

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
        let edit = json!({
            "type": "object",
            "description": "one edit: replace with with, after or before with insert, or append alone",
            "properties": {
                "replace": { "type": "string", "description": "a span of the file to replace; it must occur exactly once" },
                "with": { "type": "string", "description": "with replace: the text that takes the span's place, empty to delete it" },
                "after": { "type": "string", "description": "a span to insert after; it must occur exactly once" },
                "before": { "type": "string", "description": "a span to insert before; it must occur exactly once" },
                "insert": { "type": "string", "description": "with after or before: the text to insert" },
                "append": { "type": "string", "description": "text to add at the end of the file" },
            },
        });
        object(
            json!({
                "path": { "type": "string", "description": "an absolute path of a text file that exists; files_write makes a new one" },
                "edits": { "type": "array", "items": edit, "description": "the edits, applied in order" },
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
        let steps = args
            .edits
            .into_iter()
            .enumerate()
            .map(|(index, edit)| edit.step(index + 1))
            .collect::<Result<Vec<_>, _>>()?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn edited(text: &str, steps: Vec<Step>) -> Result<String, Error> {
        applied(text.into(), steps, "/f")
    }

    fn replace(span: &str, with: &str) -> Step {
        Step::Replace {
            span: span.into(),
            with: with.into(),
        }
    }

    #[test]
    fn overlapping_occurrences_count_apart_and_name_their_lines() {
        let error = edited("aaa\n", vec![replace("aa", "b")]).unwrap_err();
        assert!(
            error.message.contains("2 times, at lines 1, 1"),
            "{error:?}"
        );
        let error = edited("x\ny\nx\n", vec![replace("x", "z")]).unwrap_err();
        assert!(error.message.contains("at lines 1, 3"), "{error:?}");
    }

    #[test]
    fn a_span_is_found_on_character_boundaries_only() {
        assert_eq!(edited("é-é", vec![replace("-", "+")]).unwrap(), "é+é");
    }

    #[test]
    fn each_step_sees_what_the_one_before_left() {
        let steps = vec![
            replace("one", "two"),
            Step::Insert {
                span: "two".into(),
                text: " three".into(),
                after: true,
            },
            Step::Append("!".into()),
        ];
        assert_eq!(edited("one", steps).unwrap(), "two three!");
    }

    #[test]
    fn an_edit_that_names_two_kinds_or_none_is_refused() {
        let mixed: EditArg =
            serde_json::from_value(json!({ "replace": "a", "with": "b", "append": "c" })).unwrap();
        assert!(mixed.step(1).is_err());
        let lone: EditArg = serde_json::from_value(json!({ "insert": "a" })).unwrap();
        assert!(lone.step(2).is_err());
        assert!(serde_json::from_value::<EditArg>(json!({ "prepend": "a" })).is_err());
    }
}
