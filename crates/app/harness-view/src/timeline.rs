//! A transcript as a person reads it: its frames grouped into entries. A person's message is
//! one entry; the model's turns that asked for tool calls, one after another, are one entry
//! with each call's result beside its call; a turn that ended otherwise (an answer) is one
//! entry; a summary and what harness noted are one each. So a conversation of 200 frames reads
//! as about one entry per turn. Both trees draw these entries.
use std::collections::HashMap;

use harness::{Body, Frame, Said, Stop};

use crate::words::upper_first;

pub(crate) enum Entry<'a> {
    Wrote(&'a Frame),
    Worked(Work<'a>),
    Answered(&'a Frame),
    Summary(&'a Frame),
    Noted(&'a Frame),
    Stray(&'a Frame),
}

pub(crate) struct Work<'a> {
    pub turns: Vec<&'a Frame>,
    /// each call's result or refusal, by its call id
    pub results: HashMap<&'a str, &'a Frame>,
}

pub(crate) struct Call<'a> {
    pub id: &'a str,
    pub tool: &'a str,
}

pub(crate) fn entries(frames: &[Frame]) -> Vec<Entry<'_>> {
    let mut entries = Vec::new();
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
                Said::ToolUse { id, name, .. } => Some(Call { id, tool: name }),
                Said::Text(_)
                | Said::Thinking { .. }
                | Said::RedactedThinking(_)
                | Said::ServerToolUse { .. }
                | Said::ServerToolResult(_)
                | Said::Cited { .. } => None,
            })
    }

    fn uses(&self) -> impl Iterator<Item = (&'a str, &'a str)> + '_ {
        self.turns
            .iter()
            .flat_map(|turn| said(turn))
            .filter_map(|said| match said {
                Said::ToolUse { name, input, .. } | Said::ServerToolUse { name, input, .. } => {
                    Some((name.as_str(), input.as_str()))
                }
                Said::Text(_)
                | Said::Thinking { .. }
                | Said::RedactedThinking(_)
                | Said::ServerToolResult(_)
                | Said::Cited { .. } => None,
            })
    }

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
        for (tool, input) in self.uses() {
            let verb = tool_action_verb(tool, input);
            let same = |(said, named, _): &&mut (Option<String>, &str, u64)| match &verb {
                Some(_) => *said == verb,
                None => said.is_none() && *named == tool,
            };
            match done.iter_mut().find(same) {
                Some((_, _, count)) => *count += 1,
                None => done.push((verb, tool, 1)),
            }
        }
        let words: Vec<String> = done
            .into_iter()
            .map(|(verb, tool, count)| match (verb, count) {
                (Some(verb), 1) if verb == FILES_READ => "read a file".to_owned(),
                (Some(verb), count) if verb == FILES_READ => format!("read {count} files"),
                (Some(verb), 1) if verb == WEB_FETCHED => "fetched a page".to_owned(),
                (Some(verb), count) if verb == WEB_FETCHED => format!("fetched {count} pages"),
                (Some(verb), _) => verb,
                (None, 1) => tool.to_owned(),
                (None, count) => format!("{tool} ×{count}"),
            })
            .collect();
        upper_first(&words.join(", "))
    }
}

fn said(turn: &Frame) -> &[Said] {
    match &turn.body {
        Body::Turn { said, .. } => said,
        _ => &[],
    }
}

const FILES_READ: &str = "read files";

const WEB_FETCHED: &str = "fetched pages";

fn tool_action_verb(tool: &str, input: &str) -> Option<String> {
    Some(match tool {
        "chat_post" => "posted in chat".into(),
        "chat_react" => "reacted".into(),
        "chat_channels" | "chat_messages" | "chat_thread" | "chat_search" => "read chat".into(),
        "files_read" => FILES_READ.into(),
        "files_write" => match written_file_name(input) {
            Some(name) => format!("wrote {name}"),
            None => "wrote a file".into(),
        },
        "files_list" | "files_find" | "files_grep" => "searched files".into(),
        "files_history" => "looked at file history".into(),
        "thread_topic" => "set its topic".into(),
        "web_search" => "searched the web".into(),
        "web_fetch" => WEB_FETCHED.into(),
        tool if tool.starts_with("forge_") => "looked at forge".into(),
        _ => return None,
    })
}

fn written_file_name(input: &str) -> Option<String> {
    let input: serde_json::Value = serde_json::from_str(input).ok()?;
    let path = input.get("path")?.as_str()?;
    path.rsplit('/').next().map(str::to_owned)
}

pub(crate) fn server_head(name: &str, input: &str, forming: bool) -> String {
    let asked = |key: &str| {
        serde_json::from_str::<serde_json::Value>(input)
            .ok()
            .and_then(|input| input.get(key)?.as_str().map(str::to_owned))
            .unwrap_or_else(|| input.to_owned())
    };
    match (name, forming) {
        ("web_search", false) => format!("Searched the web: {}", asked("query")),
        ("web_search", true) => format!("Searching the web: {}", asked("query")),
        ("web_fetch", false) => format!("Fetched {}", asked("url")),
        ("web_fetch", true) => format!("Fetching {}", asked("url")),
        _ => format!("{name} {input}"),
    }
}

pub(crate) struct Outcome {
    pub words: String,
    pub failed: bool,
    pub sources: Vec<(String, String)>,
}

pub(crate) fn server_outcome(block: &str) -> Outcome {
    let block: serde_json::Value = serde_json::from_str(block).unwrap_or_default();
    let content = &block["content"];
    let field =
        |value: &serde_json::Value, key: &str| value[key].as_str().unwrap_or_default().to_owned();
    if let Some(code) = content["error_code"].as_str() {
        return Outcome {
            words: format!("failed: {code}"),
            failed: true,
            sources: Vec::new(),
        };
    }
    if let Some(results) = content.as_array() {
        let sources: Vec<(String, String)> = results
            .iter()
            .map(|result| (field(result, "title"), field(result, "url")))
            .collect();
        let words = match sources.len() {
            1 => "1 result".to_owned(),
            count => format!("{count} results"),
        };
        return Outcome {
            words,
            failed: false,
            sources,
        };
    }
    let title = field(&content["content"], "title");
    let url = field(content, "url");
    let words = match title.is_empty() {
        true => url.clone(),
        false => format!("“{title}”"),
    };
    Outcome {
        words,
        failed: false,
        sources: vec![(title, url)],
    }
}

pub(crate) fn result_of(block: &str) -> String {
    serde_json::from_str::<serde_json::Value>(block)
        .ok()
        .and_then(|block| block["tool_use_id"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

pub(crate) fn server_result<'a>(frames: &'a [Frame], id: &str) -> Option<&'a str> {
    frames.iter().flat_map(said).find_map(|said| match said {
        Said::ServerToolResult(block) if result_of(block) == id => Some(block.as_str()),
        _ => None,
    })
}

pub(crate) fn server_use_in(frames: &[Frame], id: &str) -> bool {
    frames
        .iter()
        .flat_map(said)
        .any(|said| matches!(said, Said::ServerToolUse { id: used, .. } if used == id))
}

pub(crate) fn cited_sources(citations: &[String]) -> Vec<String> {
    let mut sources: Vec<String> = Vec::new();
    for citation in citations {
        let citation: serde_json::Value = serde_json::from_str(citation).unwrap_or_default();
        let named = ["title", "document_title", "url"]
            .iter()
            .find_map(|key| citation[*key].as_str().filter(|name| !name.is_empty()));
        if let Some(named) = named.filter(|named| !sources.iter().any(|source| source == named)) {
            sources.push(named.to_owned());
        }
    }
    sources
}

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
                    ("h", "thread_topic", r#"{"topic":"payments"}"#),
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
        assert_eq!(work.calls().count(), 8);
        assert_eq!(
            work.summary(),
            "Posted in chat, read 2 files, wrote memory.md, searched files, network_call ×2, set its topic"
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
