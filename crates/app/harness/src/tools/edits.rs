//! Span edits of a text, as `files_edit` and `forge_commit` take them: each replaces a span,
//! inserts text before or after one, or appends at the end, a span found exactly once, in order.
use ::program::{Error, invalid, not_found};
use serde::Deserialize;
use serde_json::{Value, json};

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

pub(crate) enum Step {
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

// The edits as steps, each checked for its shape before any applies.
pub(crate) fn steps(edits: Vec<EditArg>) -> Result<Vec<Step>, Error> {
    edits
        .into_iter()
        .enumerate()
        .map(|(index, edit)| edit.step(index + 1))
        .collect()
}

// The schema of one edit.
pub(crate) fn edit_schema() -> Value {
    json!({
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
    })
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
pub(crate) fn applied(mut text: String, steps: Vec<Step>, path: &str) -> Result<String, Error> {
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
