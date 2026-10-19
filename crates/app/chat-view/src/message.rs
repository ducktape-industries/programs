//! One message as the frame draws it: a row the program served, folded
//! with the name directory into author lines, bodies and styled runs, and
//! grouped into runs the way Slack groups them.
use chat::{Block, Mark, MsgRow, Principal, Reaction, Span};
use view::HighlightStyle;
use view::prelude::*;

#[cfg(not(feature = "notifier"))]
use crate::Chat;
use crate::names::mention_token;
use crate::words;
use chat::view::Names;

/// One message as the frame draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatMessage {
    pub id: String,
    /// 0 for a pending row
    pub seq: u64,
    /// who wrote it; a run of messages is one principal's
    pub from: Principal,
    pub author: String,
    /// the message as one run of plain text: the copy range's line
    pub body: String,
    /// the editable markdown of the same body, mentions as stable tokens
    pub edit_body: String,
    /// the message as chat keeps it; the frame styles it as it draws
    pub blocks: Vec<Block>,
    pub pending: bool,
    pub rev: u32,
    pub edited: bool,
    pub deleted: bool,
    pub reply_count: u64,
    pub thread: Option<u64>,
    /// Opens a run: the first message, one whose author differs from the
    /// one above, the first unread, or one after a long quiet (see
    /// [`mark_message_groups`]).
    pub show_author: bool,
    pub initial: String,
    /// the author is an agent: its face wears the agent tint
    pub agent: bool,
    /// what the author is beside a person, by its name: "Agent · managed
    /// by Dev", "Program · forge" ([`Names::badge`])
    pub badge: Option<String>,
    pub height: u64,
    /// block time in milliseconds; 0 for a pending row
    pub time: u64,
    pub reactions: Vec<Reaction>,
    // a program's own post ([`chat::program_post`]): shown as that program's event, not as a
    // code block
    pub system: Option<SystemLine>,
}

// A program's own post as the frame draws it: the program, what the line says, and the
// program's room it is about (`forge:web:3`), which its link opens where the program shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemLine {
    pub program: String,
    pub said: String,
    pub room: String,
}

// A program's post in a room of its own says its code, about that room. Posted in another room
// (forge's lines in a job's room), a code ending `<name> <n>` is about the program's room
// `<program>:<name>:<n>` and reads `<name> #<n> <event>`: `opened programs 213` is
// `programs #213 opened`, about `forge:programs:213`.
pub fn system_line(program: &str, code: &str, channel: &str) -> SystemLine {
    let own = chat::namespace::program(channel) == Some(program);
    let about = || {
        let (rest, n) = code.rsplit_once(' ')?;
        let n: u64 = n.parse().ok()?;
        let (event, name) = rest.rsplit_once(' ')?;
        Some((event, name, n))
    };
    match about().filter(|_| !own) {
        Some((event, name, n)) => SystemLine {
            program: program.into(),
            said: format!("{name} #{n} {event}"),
            room: chat::namespace::id(program, &format!("{name}:{n}")),
        },
        None => SystemLine {
            program: program.into(),
            said: code.into(),
            room: channel.into(),
        },
    }
}

impl ChatMessage {
    /// Whether the reader's own reactions on it include `emoji`.
    pub(crate) fn reacted(&self, emoji: &str) -> bool {
        self.reactions
            .iter()
            .any(|reaction| reaction.emoji == emoji && reaction.reacted_by_me)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatSpan {
    pub text: String,
    pub style: SpanStyle,
}

/// The one style arm a run renders through: a link outranks every other
/// mark, a mention outranks code, code outranks emphasis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpanStyle {
    Plain,
    Bold,
    Italic,
    BoldItalic,
    Link(String),
    /// the account the mention names, in decimal ("" for a program)
    Mention(String),
    Code,
}

pub fn chat_message(row: MsgRow, names: &Names, rooms: &crate::links::Rooms) -> ChatMessage {
    let system = chat::program_post(&row, names.program(&row.author))
        .map(|(program, code)| system_line(program, code, &row.channel_id));
    let edited = row.rev > 0;
    let (body, edit_body, blocks) = if row.deleted {
        (
            words::MESSAGE_DELETED.to_string(),
            String::new(),
            vec![Block::paragraph(words::MESSAGE_DELETED)],
        )
    } else {
        (
            message_body(&row.blocks, names),
            draft_body(&row.blocks),
            rooms.blocks(row.blocks),
        )
    };
    ChatMessage {
        id: row.message_id,
        seq: row.seq,
        author: names.author(&row.author),
        body,
        edit_body,
        blocks,
        pending: row.seq == 0,
        rev: row.rev,
        edited,
        deleted: row.deleted,
        reply_count: row.reply_count,
        thread: row.thread,
        show_author: true,
        initial: design::initial(&names.member(&row.author)),
        agent: agent(names, &row.author),
        badge: names.badge(&row.author),
        from: row.author,
        height: row.height,
        time: row.time,
        reactions: row.reactions,
        system,
    }
}

/// Whether the roster says `principal` is an agent: its face wears the
/// agent tint.
pub fn agent(names: &Names, principal: &chat::Principal) -> bool {
    matches!(
        names.kind(principal),
        Some(chat::Kind::Program {
            category: chat::Category::Agent,
            ..
        })
    )
}

/// A quiet longer than this opens a new run, as Slack's does.
pub const GROUP_GAP_MS: u64 = 5 * 60 * 1000;

/// The first message past the read `boundary` — the row the "New messages"
/// divider sits above. None when there is no boundary.
pub fn unread_seq(messages: &[ChatMessage], boundary: Option<u64>) -> Option<u64> {
    let boundary = boundary.filter(|b| *b > 0)?;
    messages
        .iter()
        .find(|message| !message.pending && message.seq > boundary)
        .map(|message| message.seq)
}

const DAY_MS: u64 = 86_400_000;

pub fn zone() -> u64 {
    design::local(DAY_MS)
}

/// The reader's day a time in milliseconds falls on, as a number two
/// times compare by.
pub fn day_of(millis: u64) -> u64 {
    design::local(millis) / DAY_MS
}

/// The day message `index` opens, when it is the first of its day: a
/// served row whose day differs from the served row above it (or that has
/// none above). Pending rows carry no time and open nothing.
pub fn new_day(messages: &[ChatMessage], index: usize) -> Option<String> {
    let time = messages.get(index)?.time;
    let above = messages[..index].iter().rev().find(|above| above.time > 0);
    let opens = time > 0
        && above
            .is_none_or(|above| design::local(above.time) / DAY_MS != design::local(time) / DAY_MS);
    opens.then(|| design::day(time))
}

/// Slack-style grouping: a message shows its author header only when it
/// opens a run. Deleted messages, the unread divider, and a quiet longer
/// than [`GROUP_GAP_MS`] always break a run.
pub fn mark_message_groups(messages: &mut [ChatMessage], boundary: Option<u64>) {
    let unread = unread_seq(messages, boundary);
    for index in 0..messages.len() {
        let this = &messages[index];
        let opens = index.checked_sub(1).is_none_or(|i| {
            let above = &messages[i];
            this.deleted
                || above.deleted
                || above.from != this.from
                || unread == Some(this.seq)
                || this.time.saturating_sub(above.time) > GROUP_GAP_MS
                // a pending row (no time yet) runs on under its author
                || (above.time > 0
                    && this.time > 0
                    && design::local(above.time) / DAY_MS != design::local(this.time) / DAY_MS)
        });
        messages[index].show_author = opens;
    }
}

/// The message as one run of plain text — the copy range's lines and the
/// search hit's preview. A mention reads as the NAME it addresses.
pub fn message_body(blocks: &[Block], names: &Names) -> String {
    blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph(spans) => span_text(spans, names),
            Block::Quote(spans) => format!("“{}”", span_text(spans, names)),
            Block::Code { lang, text } => match lang {
                Some(lang) => format!("{lang}\n{text}"),
                None => text.clone(),
            },
            Block::Divider => "────────".to_owned(),
            Block::Attachment(file) => file.name.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Editable markdown with stable mention identities: a mention keeps its
/// `<@7>` token rather than the name it renders as today.
fn draft_body(blocks: &[Block]) -> String {
    blocks
        .iter()
        .filter(|block| !matches!(block, Block::Attachment(_)))
        .map(|block| match block {
            Block::Paragraph(spans) => draft_spans(spans),
            Block::Quote(spans) => format!("> {}", draft_spans(spans)),
            Block::Code { lang, text } => {
                format!("```{}\n{text}\n```", lang.clone().unwrap_or_default())
            }
            Block::Divider => "---".to_owned(),
            Block::Attachment(_) => String::new(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn draft_spans(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|span| {
            let mention = span.marks.iter().find_map(|mark| match mark {
                Mark::Mention(principal) => Some(mention_token(principal)),
                _ => None,
            });
            let mut text = mention.unwrap_or_else(|| span.text.clone());
            for mark in &span.marks {
                text = match mark {
                    Mark::Bold => format!("**{text}**"),
                    Mark::Italic => format!("_{text}_"),
                    Mark::Link(url) => format!("[{text}]({url})"),
                    Mark::Mention(_) => text,
                    Mark::Code => format!("`{text}`"),
                };
            }
            text
        })
        .collect()
}

/// A paragraph's or quote's spans as the runs the frame styles; empty when
/// no span carries a mark, so the block draws as one plain text.
pub fn styled_spans(spans: &[Span], names: &Names) -> Vec<ChatSpan> {
    if spans.iter().all(|span| span.marks.is_empty()) {
        return Vec::new();
    }
    spans
        .iter()
        .filter_map(|span| {
            let text = span_display(span, names);
            if text.is_empty() {
                return None;
            }
            let link = span.marks.iter().find_map(|mark| match mark {
                Mark::Link(url) => Some(url.clone()),
                _ => None,
            });
            let mention = span.marks.iter().find_map(|mark| match mark {
                Mark::Mention(Principal::Account(account)) => Some(account.to_string()),
                Mark::Mention(_) => Some(String::new()),
                _ => None,
            });
            let bold = span.marks.contains(&Mark::Bold);
            let italic = span.marks.contains(&Mark::Italic);
            let code = span.marks.contains(&Mark::Code);
            let style = match (link, mention, bold, italic) {
                (Some(url), _, _, _) => SpanStyle::Link(url),
                (None, Some(account), _, _) => SpanStyle::Mention(account),
                (None, None, _, _) if code => SpanStyle::Code,
                (None, None, true, true) => SpanStyle::BoldItalic,
                (None, None, true, false) => SpanStyle::Bold,
                (None, None, false, true) => SpanStyle::Italic,
                (None, None, false, false) => SpanStyle::Plain,
            };
            Some(ChatSpan { text, style })
        })
        .collect()
}

/// Spans to text; a mention plate shows the account's current name.
pub fn span_text(spans: &[Span], names: &Names) -> String {
    spans.iter().map(|span| span_display(span, names)).collect()
}

fn span_display(span: &Span, names: &Names) -> String {
    span.marks
        .iter()
        .find_map(|mark| match mark {
            Mark::Mention(principal) => Some(names.mention(principal)),
            _ => None,
        })
        .unwrap_or_else(|| span.text.clone())
}

// A line of `text` with no runs.
pub(crate) fn plain_line(id: ElementId, text: &str) -> InteractiveText {
    InteractiveText::new(id, StyledText::new(text.to_owned())).w_full()
}

/// One styled run of a rich line, as a design draws it: the words it adds,
/// how they look, where a press on them goes, whether they are in the
/// data face.
pub(crate) struct Run {
    pub text: String,
    pub style: HighlightStyle,
    pub target: Option<String>,
    pub mono: bool,
}

impl Run {
    /// `span`'s own words in `style`, pressing through to `target` unless
    /// it is empty.
    pub(crate) fn of(span: &ChatSpan, style: HighlightStyle, target: Option<&String>) -> Run {
        Run {
            text: span.text.clone(),
            style,
            target: target.filter(|target| !target.is_empty()).cloned(),
            mono: false,
        }
    }
}

#[cfg(not(feature = "notifier"))]
/// A line of `spans`, each run drawn by `run` (one design's), a press on a
/// link or mention opening it.
pub(crate) fn line(
    id: ElementId,
    spans: &[Span],
    names: &Names,
    cx: &mut Context<Chat>,
    run: impl Fn(&ChatSpan) -> Run,
) -> InteractiveText {
    let styled = styled_spans(spans, names);
    if styled.is_empty() {
        return plain_line(id, &span_text(spans, names));
    }
    let mut text = String::new();
    let mut highlights = Vec::new();
    let mut clickable = Vec::new();
    let mut targets = Vec::new();
    let mut mono = Vec::new();
    for span in &styled {
        let start = text.len();
        let drawn = run(span);
        text.push_str(&drawn.text);
        let range = start..text.len();
        if let Some(target) = drawn.target {
            clickable.push(range.clone());
            targets.push(target);
        }
        if drawn.mono {
            mono.push((range.clone(), design::fonts::FAMILY_MONO.into()));
        }
        highlights.push((range, drawn.style));
    }
    let styled = StyledText::new(text)
        .with_highlights(highlights)
        .with_font_family_overrides(mono);
    let open = cx.processor(move |chat, index: usize, _window, cx| {
        if let Some(target) = targets.get(index) {
            cx.notify();
            chat.open_pressed(target.clone(), cx);
        }
    });
    InteractiveText::new(id, styled)
        .w_full()
        .on_click(clickable, open)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Account 7 saying "hi" as row `seq`, posted at `time`.
    fn at(seq: u64, time: u64) -> ChatMessage {
        let row = MsgRow {
            seq,
            time,
            blocks: vec![Block::paragraph("hi")],
            ..MsgRow::by(Principal::Account(7))
        };
        chat_message(row, &Names::empty(), &crate::links::Rooms::default())
    }

    #[test]
    fn a_programs_line_in_another_room_names_the_room_it_is_about() {
        let line = system_line("forge", "review 2 programs 213", "j-101");
        assert_eq!(line.said, "programs #213 review 2");
        assert_eq!(line.room, "forge:programs:213");
        for (code, channel) in [
            ("opened", "forge:programs:213"),
            ("opened programs 213", "forge:programs:213"),
            ("opened", "j-101"),
            ("opened programs x", "j-101"),
        ] {
            let line = system_line("forge", code, channel);
            assert_eq!((line.said.as_str(), line.room.as_str()), (code, channel));
        }
    }

    #[test]
    fn a_run_breaks_at_the_unread_divider_and_after_a_long_quiet() {
        let minute = 60 * 1000;
        let mut messages = vec![
            at(1, 0),
            at(2, minute),
            at(3, 2 * minute),
            at(4, 3 * minute + GROUP_GAP_MS),
        ];
        mark_message_groups(&mut messages, Some(2));
        let heads: Vec<bool> = messages.iter().map(|m| m.show_author).collect();
        // 2 runs on; 3 is the first unread; 4 comes after a quiet past the gap
        assert_eq!(heads, [true, false, true, true]);

        mark_message_groups(&mut messages, None);
        let heads: Vec<bool> = messages.iter().map(|m| m.show_author).collect();
        assert_eq!(heads, [true, false, false, true]);
    }

    #[test]
    fn a_pending_row_runs_on_under_its_authors_served_row() {
        // 24 Sep 2026, 15:42 UTC, then a pending post (seq 0, no time yet)
        let mut messages = vec![at(1, 1_790_264_527_000), at(0, 0)];
        mark_message_groups(&mut messages, None);
        let heads: Vec<bool> = messages.iter().map(|m| m.show_author).collect();
        assert_eq!(heads, [true, false]);
    }

    #[test]
    fn a_draft_writes_marks_back_as_the_markdown_that_parses_to_them() {
        let source = "say **hi** and `code` to <@7> at [x](https://x.example)";
        let blocks = chat::message::parse_message(source);
        assert_ne!(blocks[0], Block::paragraph(source));
        assert_eq!(draft_body(&blocks), source);
        assert_eq!(chat::message::parse_message(&draft_body(&blocks)), blocks);
    }
}
