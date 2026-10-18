//! A transcript as a person reads it: its frames grouped into entries. A person's message is
//! one entry; the model's turns that asked for tool calls, one after another, are one entry
//! with each call's result beside its call; a turn that ended otherwise (an answer) is one
//! entry; a summary and what harness noted are one each. So a conversation of 200 frames reads
//! as about one entry per turn. Both trees draw these entries.
use std::collections::HashMap;

use harness::{Body, Frame, Said, Stop};

pub(crate) enum Entry<'a> {
    /// a person's message, or the chain's
    Wrote(&'a Frame),
    /// a burst of turns that asked for tool calls, and their results
    Worked(Work<'a>),
    /// a turn that ended otherwise: an answer, max tokens, a refusal
    Answered(&'a Frame),
    /// a summary the model sees in place of the frames before it
    Summary(&'a Frame),
    /// what harness noted: a model call that failed, a call taken back
    Noted(&'a Frame),
    /// a result or refusal whose call no turn here made
    Stray(&'a Frame),
}

/// The model's turns that asked for tool calls, one after another, and what came back.
pub(crate) struct Work<'a> {
    pub turns: Vec<&'a Frame>,
    /// each call's result or refusal, by its call id
    pub results: HashMap<&'a str, &'a Frame>,
}

/// One tool call a turn made.
pub(crate) struct Call<'a> {
    pub id: &'a str,
    pub tool: &'a str,
    pub input: &'a str,
}

pub(crate) fn entries(frames: &[Frame]) -> Vec<Entry<'_>> {
    let mut entries = Vec::new();
    // the burst a turn asking for tool calls joins, while nothing else came between
    let mut working: Option<usize> = None;
    let mut made_in: HashMap<&str, usize> = HashMap::new();
    for frame in frames {
        match &frame.body {
            Body::Turn {
                stop: Stop::ToolUse,
                said,
                ..
            } => {
                let at = *working.get_or_insert_with(|| {
                    entries.push(Entry::Worked(Work {
                        turns: Vec::new(),
                        results: HashMap::new(),
                    }));
                    entries.len() - 1
                });
                if let Entry::Worked(work) = &mut entries[at] {
                    work.turns.push(frame);
                }
                for said in said {
                    if let Said::ToolUse { id, .. } = said {
                        made_in.insert(id.as_str(), at);
                    }
                }
            }
            Body::Result { call, .. } | Body::Refused { call, .. } => {
                match made_in.get(call.as_str()).map(|at| &mut entries[*at]) {
                    Some(Entry::Worked(work)) => {
                        work.results.insert(call.as_str(), frame);
                    }
                    _ => entries.push(Entry::Stray(frame)),
                }
            }
            Body::Post { .. } => {
                working = None;
                entries.push(Entry::Wrote(frame));
            }
            Body::Turn { .. } => {
                working = None;
                entries.push(Entry::Answered(frame));
            }
            Body::Compaction { .. } => {
                working = None;
                entries.push(Entry::Summary(frame));
            }
            Body::Notice { .. } => {
                working = None;
                entries.push(Entry::Noted(frame));
            }
        }
    }
    entries
}

impl<'a> Work<'a> {
    pub fn calls(&self) -> impl Iterator<Item = Call<'a>> + '_ {
        self.turns
            .iter()
            .flat_map(|turn| said(turn))
            .filter_map(|said| match said {
                Said::ToolUse { id, name, input } => Some(Call {
                    id,
                    tool: name,
                    input,
                }),
                Said::Text(_) | Said::Thinking { .. } | Said::RedactedThinking(_) => None,
            })
    }

    /// Every frame of the burst, its turns and its results, in order.
    fn frames(&self) -> impl Iterator<Item = &'a Frame> + '_ {
        self.turns
            .iter()
            .copied()
            .chain(self.results.values().copied())
    }

    pub fn first(&self) -> &'a Frame {
        self.turns[0]
    }

    /// The burst's last frame: its last result, or its last turn while none is back.
    pub fn last(&self) -> &'a Frame {
        self.frames()
            .max_by_key(|frame| frame.seq)
            .unwrap_or(self.turns[0])
    }

    /// How long the burst took, from its first turn to its last frame.
    pub fn took(&self) -> u64 {
        self.last().time.saturating_sub(self.first().time)
    }

    /// What the burst did, in one line: "Posted in chat, read 3 files, searched files". A
    /// verb is said once, in the order it was first done; a tool the verbs lack reads as its
    /// name and how often it ran, "network_call ×2".
    pub fn summary(&self) -> String {
        let mut done: Vec<(Option<String>, &str, u64)> = Vec::new();
        for call in self.calls() {
            let verb = verb(call.tool, call.input);
            let same = |(said, tool, _): &&mut (Option<String>, &str, u64)| match &verb {
                Some(_) => *said == verb,
                None => said.is_none() && *tool == call.tool,
            };
            match done.iter_mut().find(same) {
                Some((_, _, count)) => *count += 1,
                None => done.push((verb, call.tool, 1)),
            }
        }
        let words: Vec<String> = done
            .into_iter()
            .map(|(verb, tool, count)| match (verb, count) {
                (Some(verb), 1) if verb == FILES_READ => "read a file".to_owned(),
                (Some(verb), count) if verb == FILES_READ => format!("read {count} files"),
                (Some(verb), _) => verb,
                (None, 1) => tool.to_owned(),
                (None, count) => format!("{tool} ×{count}"),
            })
            .collect();
        capitalized(&words.join(", "))
    }
}

fn said(turn: &Frame) -> &[Said] {
    match &turn.body {
        Body::Turn { said, .. } => said,
        _ => &[],
    }
}

/// files_read's verb, counted apart from the rest.
const FILES_READ: &str = "read files";

/// What one call did, as a verb a person reads; none for a tool this table lacks.
fn verb(tool: &str, input: &str) -> Option<String> {
    Some(match tool {
        "chat_post" => "posted in chat".into(),
        "chat_react" => "reacted".into(),
        "chat_channels" | "chat_messages" | "chat_thread" | "chat_search" => "read chat".into(),
        "files_read" => FILES_READ.into(),
        "files_write" => match written(input) {
            Some(name) => format!("wrote {name}"),
            None => "wrote a file".into(),
        },
        "files_list" | "files_find" | "files_grep" => "searched files".into(),
        "files_history" => "looked at file history".into(),
        tool if tool.starts_with("forge_") => "looked at forge".into(),
        _ => return None,
    })
}

/// The name of the file a files_write input writes, its path's last part.
fn written(input: &str) -> Option<String> {
    let input: serde_json::Value = serde_json::from_str(input).ok()?;
    let path = input.get("path")?.as_str()?;
    path.rsplit('/').next().map(str::to_owned)
}

fn capitalized(words: &str) -> String {
    let mut chars = words.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// A span of milliseconds as a person reads it: `40 s`, `2 min`, `3 h`.
pub(crate) fn took_words(millis: u64) -> String {
    let seconds = millis / 1000;
    match seconds {
        0..60 => format!("{seconds} s"),
        60..3_600 => format!("{} min", seconds / 60),
        _ => format!("{} h", seconds / 3_600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness::{BlobId, Principal, Usage};

    fn frame(seq: u64, body: Body) -> Frame {
        Frame {
            seq,
            height: 100 + seq,
            time: seq * 30_000,
            body,
        }
    }

    fn asks(seq: u64, calls: &[(&str, &str, &str)]) -> Frame {
        let said = calls
            .iter()
            .map(|(id, name, input)| Said::ToolUse {
                id: (*id).into(),
                name: (*name).into(),
                input: (*input).into(),
            })
            .collect();
        frame(
            seq,
            Body::Turn {
                model: "m".into(),
                config: BlobId::Sha256([0; 32]),
                said,
                stop: Stop::ToolUse,
                usage: Usage::default(),
            },
        )
    }

    fn result(seq: u64, call: &str) -> Frame {
        frame(
            seq,
            Body::Result {
                call: call.into(),
                tool: "t".into(),
                text: "ok".into(),
                failed: false,
            },
        )
    }

    /// A burst of turns asking for calls reads as one entry, its results inside it, and the
    /// answer after it as its own.
    #[test]
    fn a_burst_of_calls_is_one_entry_with_its_results() {
        let frames = vec![
            frame(
                1,
                Body::Post {
                    author: Principal::Account(7),
                    text: "hi".into(),
                },
            ),
            asks(
                2,
                &[
                    ("a", "chat_post", "{}"),
                    ("b", "files_read", "{}"),
                    ("c", "files_read", "{}"),
                ],
            ),
            result(3, "a"),
            result(4, "b"),
            result(5, "c"),
            asks(
                6,
                &[
                    ("d", "files_write", r#"{"path":"/shared/memory.md"}"#),
                    ("e", "files_grep", "{}"),
                    ("f", "network_call", "{}"),
                    ("g", "network_call", "{}"),
                ],
            ),
            result(7, "d"),
            result(8, "e"),
            frame(
                9,
                Body::Turn {
                    model: "m".into(),
                    config: BlobId::Sha256([0; 32]),
                    said: vec![Said::Text("done".into())],
                    stop: Stop::EndTurn,
                    usage: Usage::default(),
                },
            ),
        ];
        let entries = entries(&frames);
        assert_eq!(entries.len(), 3);
        let Entry::Worked(work) = &entries[1] else {
            panic!("the burst is one entry");
        };
        assert_eq!(work.turns.len(), 2);
        assert_eq!(work.results.len(), 5);
        assert_eq!(work.calls().count(), 7);
        assert_eq!(
            work.summary(),
            "Posted in chat, read 2 files, wrote memory.md, searched files, network_call ×2"
        );
        assert_eq!(took_words(work.took()), "3 min");
        assert!(matches!(entries[2], Entry::Answered(_)));
    }

    #[test]
    fn a_result_no_turn_made_stands_alone() {
        let frames = vec![result(1, "x")];
        assert!(matches!(entries(&frames)[..], [Entry::Stray(_)]));
    }
}
