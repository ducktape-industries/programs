//! Tools over the thread itself: every frame of its transcript stays whole in harness, so the
//! model searches it and reads a frame in parts, an input left out of its context window included;
//! and the thread's topic, the one line the agent says it is about, which people read beside it.
use ::program::{Error, ExecCtx, invalid, not_found};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Run, Seat, Tool, object};
use crate::Frame;
use crate::state::{FRAMES, TOPICS};

fn frame(ctx: &ExecCtx, thread: u64, seq: u64) -> Result<Frame, Error> {
    FRAMES
        .get(ctx, &(thread, seq))?
        .ok_or_else(|| not_found(format!("this thread has no frame {seq}")))
}

fn frames(ctx: &ExecCtx, thread: u64, only: Option<u64>) -> Result<Vec<Frame>, Error> {
    if let Some(seq) = only {
        return Ok(vec![frame(ctx, thread, seq)?]);
    }
    let whole = FRAMES.scan(ctx, FRAMES.prefix_of(&thread))?;
    Ok(whole.into_iter().map(|(_, frame)| frame).collect())
}

fn frame_schema(what: &str) -> Value {
    json!({ "type": "integer", "minimum": 1, "description": what })
}

#[derive(Deserialize)]
pub(crate) struct SearchArgs {
    pattern: String,
    frame: Option<u64>,
    around: Option<u64>,
}

pub(crate) struct Search;

struct Line<'a> {
    number: usize,
    start: usize,
    text: &'a str,
}

fn lines(text: &str) -> impl Iterator<Item = Line<'_>> {
    text.split('\n')
        .enumerate()
        .scan(0, |start, (index, line)| {
            let found = Line {
                number: index + 1,
                start: *start,
                text: line,
            };
            *start += line.len() + 1;
            Some(found)
        })
}

fn excerpt(line: &str, at: usize, len: usize, around: u64) -> String {
    let around = usize::try_from(around).unwrap_or(usize::MAX);
    let start = line.floor_char_boundary(at.saturating_sub(around));
    let end = line.ceil_char_boundary(at.saturating_add(len).saturating_add(around));
    line[start..end].to_owned()
}

fn hits(frame: &Frame, pattern: &str, around: Option<u64>) -> Vec<Value> {
    let text = frame.body.text();
    lines(&text)
        .filter_map(|line| {
            let found: Vec<usize> = line.text.match_indices(pattern).map(|(at, _)| at).collect();
            if found.is_empty() {
                return None;
            }
            let offsets: Vec<usize> = found.iter().map(|at| line.start + at).collect();
            let mut hit = json!({ "frame": frame.seq, "line": line.number, "offsets": offsets });
            match around {
                None => hit["text"] = json!(line.text),
                Some(around) => {
                    let excerpts: Vec<String> = found
                        .iter()
                        .map(|at| excerpt(line.text, *at, pattern.len(), around))
                        .collect();
                    hit["excerpts"] = json!(excerpts);
                }
            }
            Some(hit)
        })
        .collect()
}

impl Tool for Search {
    const NAME: &'static str = "thread_search";
    const DESCRIPTION: &'static str = "Find a literal string in this thread's transcript, everything said in it kept whole: what a summary covers, and an input left out of your context because it alone does not fit it. Each matching line comes with its frame, its line number and the byte offsets of the matches, to read around with thread_read.";
    type Args = SearchArgs;

    fn schema() -> Value {
        object(
            json!({
                "pattern": { "type": "string", "description": "the literal text to find" },
                "frame": frame_schema("search only this frame; the whole thread if omitted"),
                "around": { "type": "integer", "minimum": 0, "description": "how many bytes on each side of each match to show; the whole line if omitted" },
            }),
            &["pattern"],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: SearchArgs) -> Result<Run, Error> {
        if args.pattern.is_empty() {
            return Err(invalid("a pattern is some text to find"));
        }
        let found: Vec<Value> = frames(ctx, seat.thread, args.frame)?
            .iter()
            .flat_map(|frame| hits(frame, &args.pattern, args.around))
            .collect();
        Ok(Run::Done(json!({ "hits": found })))
    }
}

#[derive(Deserialize)]
pub(crate) struct ReadArgs {
    frame: u64,
    offset: Option<u64>,
    length: Option<u64>,
}

pub(crate) struct Read;

fn index(at: u64) -> usize {
    usize::try_from(at).unwrap_or(usize::MAX)
}

impl Tool for Read {
    const NAME: &'static str = "thread_read";
    const DESCRIPTION: &'static str = "Read one frame of this thread's transcript as text, whole or a byte range of it: an input left out of your context names its frame and size, and thread_search names the frame and offsets of each match.";
    type Args = ReadArgs;

    fn schema() -> Value {
        object(
            json!({
                "frame": frame_schema("the frame to read"),
                "offset": { "type": "integer", "minimum": 0, "description": "the first byte to read; 0 if omitted" },
                "length": { "type": "integer", "minimum": 1, "description": "how many bytes; the rest of the frame if omitted" },
            }),
            &["frame"],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: ReadArgs) -> Result<Run, Error> {
        let text = frame(ctx, seat.thread, args.frame)?.body.text();
        let wanted = index(args.offset.unwrap_or(0));
        let start = text.floor_char_boundary(wanted);
        let end = match args.length {
            Some(length) => text.ceil_char_boundary(wanted.saturating_add(index(length))),
            None => text.len(),
        };
        Ok(Run::Done(json!({
            "frame": args.frame,
            "offset": start,
            "size": text.len(),
            "text": &text[start..end],
            "rest": text.len() - end,
        })))
    }
}

/// How many characters a topic holds at most: one line beside a conversation's name.
pub(crate) const TOPIC_MAX: usize = 120;

#[derive(Deserialize)]
pub(crate) struct TopicArgs {
    topic: String,
}

pub(crate) struct Topic;

impl Tool for Topic {
    const NAME: &'static str = "thread_topic";
    const DESCRIPTION: &'static str = "Say in one line what this thread is about and where it stands, for the people who read it beside the thread's name: \"Finding why payments is slow; the timeout change looks like it\". It replaces the topic set before. It is not what the model sees in place of earlier frames.";
    type Args = TopicArgs;

    fn schema() -> Value {
        object(
            json!({
                "topic": {
                    "type": "string",
                    "maxLength": TOPIC_MAX,
                    "description": format!("one line, at most {TOPIC_MAX} characters"),
                },
            }),
            &["topic"],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: TopicArgs) -> Result<Run, Error> {
        let topic = args.topic.trim();
        if topic.is_empty() {
            return Err(invalid("a topic is a line of words"));
        }
        if topic.contains(['\n', '\r']) {
            return Err(invalid("a topic is one line"));
        }
        let length = topic.chars().count();
        if length > TOPIC_MAX {
            return Err(invalid(format!(
                "a topic is at most {TOPIC_MAX} characters; this one is {length}"
            )));
        }
        TOPICS.put(ctx, &seat.thread, &topic.to_owned());
        Ok(Run::Done(json!({ "topic": topic })))
    }
}
