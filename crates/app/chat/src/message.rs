//! Pure conversation message types and Markdown parsing. No host or consensus runtime.
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use serde::{Deserialize, Serialize};

use crate::Principal;

/// inline formatting applied to a [`Span`]. mentions are structured so
/// hook parsing stays deterministic.
#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
    Debug,
    Clone,
    PartialEq,
    Eq,
)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Mark {
    Bold,
    Italic,
    Link(String),
    /// a mention names a principal: `<@7>` an account.
    /// chat keeps it as typed; a view names it at render time.
    Mention(Principal),
    /// an inline code span: rendered mono, never parsed further.
    Code,
}

/// a run of text with uniform marks.
#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
    Debug,
    Clone,
    PartialEq,
    Eq,
)]
#[serde(deny_unknown_fields)]
pub struct Span {
    pub text: String,
    pub marks: Vec<Mark>,
}

impl Span {
    /// a plain, unmarked span.
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            marks: Vec::new(),
        }
    }
}

/// one block of a message body.
#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
    Debug,
    Clone,
    PartialEq,
    Eq,
)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Block {
    Paragraph(Vec<Span>),
    Code { lang: Option<String>, text: String },
    Quote(Vec<Span>),
    Divider,
    Attachment(Attachment),
}

impl Block {
    /// a single-span plain paragraph.
    pub fn paragraph(text: impl Into<String>) -> Self {
        Self::Paragraph(vec![Span::plain(text)])
    }
}

#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
    Debug,
    Clone,
    PartialEq,
    Eq,
)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
    pub path: String,
    pub snapshot: [u8; 32],
    pub name: String,
    pub size: u64,
    pub content_type: String,
}

/// A one-level list item's marker: `- ` / `* ` bullets and `1. ` numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListMarker {
    Bullet,
    Ordered(u64),
}

/// A paragraph that opens with a list marker, read at display time: the
/// marker and the item's spans without it. Chat keeps the line as typed —
/// a list item is a [`Block::Paragraph`] on the wire — so a view reads it
/// here. One level: no nesting, no checkboxes; a marker inside a mark
/// (`**- x**`) is text.
pub fn list_item(spans: &[Span]) -> Option<(ListMarker, Vec<Span>)> {
    let first = spans.first().filter(|span| span.marks.is_empty())?;
    let (marker, rest) = if let Some(rest) = first
        .text
        .strip_prefix("- ")
        .or_else(|| first.text.strip_prefix("* "))
    {
        (ListMarker::Bullet, rest)
    } else {
        let digits = first.text.bytes().take_while(u8::is_ascii_digit).count();
        let rest = first.text[digits..].strip_prefix(". ")?;
        (
            ListMarker::Ordered(first.text[..digits].parse().ok()?),
            rest,
        )
    };
    let rest = rest.trim_start();
    let mut item: Vec<Span> = (!rest.is_empty())
        .then(|| Span::plain(rest))
        .into_iter()
        .collect();
    item.extend(spans[1..].iter().cloned());
    (!item.is_empty()).then_some((marker, item))
}

/// Parse composer text into wire `Block`s: fenced ```code``` (optional language),
/// `>` quotes, `---`/`***` dividers, and paragraphs with inline `**bold**` /
/// `__bold__`, `*italic*` / `_italic_`, `` `code` ``, `<@7>` mentions,
/// `[label](url)` references and bare `http(s)`/`duck` links. Everything the
/// `chat` wire enums can round-trip — nothing client-only.
///
/// A SINGLE NEWLINE IS A HARD BREAK, not CommonMark's soft break. The composer
/// hint says `⇧↵ newline` and `⇧↵` really does put a `\n` in the buffer, so
/// folding consecutive lines into one paragraph with a space posted a typed
/// list as "- apples - bananas - pears" — and the fold happens on the way to
/// the CHAIN, so no renderer recovers it. Each line is therefore its own block.
/// A rendered break has to be a block boundary rather than a `\n` inside one:
/// a marked-up line renders as a single rich-text paragraph (`run_spans`),
/// one paragraph widget per typed line.
pub fn parse_message(input: &str) -> Vec<Block> {
    let lines: Vec<&str> = input.lines().collect();
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim();
        let opens_fence = trimmed.starts_with("```");
        let is_divider = trimmed == "---" || trimmed == "***";
        let is_quote = trimmed.starts_with('>');
        let is_blank = trimmed.is_empty();
        if opens_fence {
            index = push_code_block(&lines, index, trimmed, &mut blocks);
        } else if is_divider {
            blocks.push(Block::Divider);
            index += 1;
        } else if is_quote {
            index = push_quote_block(&lines, index, &mut blocks);
        } else if is_blank {
            index += 1;
        } else {
            index = push_paragraph_block(&lines, index, &mut blocks);
        }
    }
    if blocks.is_empty() {
        blocks.push(Block::paragraph(input.trim().to_string()));
    }
    blocks
}

fn push_code_block(lines: &[&str], start: usize, opener: &str, blocks: &mut Vec<Block>) -> usize {
    let lang = opener.trim_start_matches('`').trim().to_string();
    let mut index = start + 1;
    let mut code = Vec::new();
    while index < lines.len() && lines[index].trim() != "```" {
        code.push(lines[index]);
        index += 1;
    }
    let closed = index < lines.len();
    blocks.push(Block::Code {
        lang: (!lang.is_empty()).then_some(lang),
        text: code.join("\n"),
    });
    if closed { index + 1 } else { index }
}

fn push_quote_block(lines: &[&str], start: usize, blocks: &mut Vec<Block>) -> usize {
    let mut index = start;
    while index < lines.len() && lines[index].trim().starts_with('>') {
        let stripped = lines[index].trim().trim_start_matches('>').trim_start();
        blocks.push(Block::Quote(inline_spans(stripped)));
        index += 1;
    }
    index
}

fn push_paragraph_block(lines: &[&str], start: usize, blocks: &mut Vec<Block>) -> usize {
    let mut index = start;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        let breaks = trimmed.is_empty()
            || trimmed.starts_with('>')
            || trimmed.starts_with("```")
            || trimmed == "---"
            || trimmed == "***";
        if breaks {
            break;
        }
        blocks.push(Block::Paragraph(inline_spans(trimmed)));
        index += 1;
    }
    index
}

/// Scan a single line of text for inline marks, preserving mention identity
/// inside emphasis. Bare `http(s)://` and `duck://`
/// runs become `Link`s, as does a `[label](url)` reference — one span whose
/// text is the label and whose mark carries the target. A backtick run opens
/// a code span that only a run of the same length closes; nothing inside it
/// is a mark, and an unclosed run stays the plain text it was typed as.
pub fn inline_spans(text: &str) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans: Vec<Span> = Vec::new();
    let mut plain = String::new();
    let mut index = 0;
    while index < chars.len() {
        let url = bare_link_char_count(&chars, index);
        let reference = labelled_link_at(&chars, index);
        let bold = fenced(&chars, index, "**").or_else(|| fenced(&chars, index, "__"));
        let italic = fenced(&chars, index, "*").or_else(|| fenced(&chars, index, "_"));
        if let Some((inner, len)) = code_span_at(&chars, index) {
            flush_plain(&mut plain, &mut spans);
            spans.push(Span {
                text: inner,
                marks: vec![Mark::Code],
            });
            index += len;
        } else if let Some((target, len)) = mention_at(&chars, index) {
            flush_plain(&mut plain, &mut spans);
            let handle: String = chars[index..index + len].iter().collect();
            spans.push(Span {
                text: handle,
                marks: vec![Mark::Mention(target)],
            });
            index += len;
        } else if let Some((label, target, len)) = reference {
            flush_plain(&mut plain, &mut spans);
            spans.push(Span {
                text: label,
                marks: vec![Mark::Link(target)],
            });
            index += len;
        } else if let Some(len) = url {
            flush_plain(&mut plain, &mut spans);
            let target: String = chars[index..index + len].iter().collect();
            spans.push(Span {
                text: target.clone(),
                marks: vec![Mark::Link(target)],
            });
            index += len;
        } else if let Some((inner, len)) = bold {
            flush_plain(&mut plain, &mut spans);
            spans.extend(inline_spans(&inner).into_iter().map(|mut span| {
                span.marks.push(Mark::Bold);
                span
            }));
            index += len;
        } else if let Some((inner, len)) = italic {
            flush_plain(&mut plain, &mut spans);
            spans.extend(inline_spans(&inner).into_iter().map(|mut span| {
                span.marks.push(Mark::Italic);
                span
            }));
            index += len;
        } else {
            plain.push(chars[index]);
            index += 1;
        }
    }
    flush_plain(&mut plain, &mut spans);
    if spans.is_empty() {
        spans.push(Span::plain(String::new()));
    }
    spans
}

fn flush_plain(plain: &mut String, spans: &mut Vec<Span>) {
    if !plain.is_empty() {
        spans.push(Span::plain(std::mem::take(plain)));
    }
}

fn bare_link_char_count(chars: &[char], at: usize) -> Option<usize> {
    let starts_link = LINK_SCHEMES.iter().any(|scheme| {
        let mut rest = chars[at..].iter();
        scheme.chars().all(|c| rest.next() == Some(&c))
    });
    if !starts_link {
        return None;
    }
    // A run stops at whitespace, and the sentence's punctuation after it (a
    // closing `.`, `,` or a `)` that opens nowhere in it) is the prose's
    let run: String = chars[at..]
        .iter()
        .take_while(|c| !c.is_whitespace())
        .collect();
    let len = run[..ducklink::bare_end(&run)].chars().count();
    (len > 0).then_some(len)
}

fn labelled_link_at(chars: &[char], at: usize) -> Option<(String, String, usize)> {
    if chars[at] != '[' {
        return None;
    }
    let label_end = chars[at + 1..]
        .iter()
        .position(|c| *c == ']' || *c == '[')?
        + at
        + 1;
    let labelled = chars[label_end] == ']' && chars.get(label_end + 1) == Some(&'(');
    if !labelled {
        return None;
    }
    let url_start = label_end + 2;
    let url_end = chars[url_start..]
        .iter()
        .position(|c| *c == ')' || c.is_whitespace())?
        + url_start;
    let closed = chars[url_end] == ')';
    if !closed {
        return None;
    }
    let label: String = chars[at + 1..label_end].iter().collect();
    let target: String = chars[url_start..url_end].iter().collect();
    let linkable = !label.is_empty() && LINK_SCHEMES.iter().any(|s| target.starts_with(s));
    linkable.then(|| (label, target, url_end + 1 - at))
}

/// A canonical `<@account>` token at `at` as a mention mark, and its length.
pub fn mention_at(chars: &[char], at: usize) -> Option<(Principal, usize)> {
    let opens = chars.get(at) == Some(&'<') && chars.get(at + 1) == Some(&'@');
    if !opens {
        return None;
    }
    let end = chars[at + 2..].iter().position(|c| *c == '>')? + at + 2;
    let id: String = chars[at + 2..end].iter().collect();
    let decimal = !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit());
    if !decimal {
        return None;
    }
    Some((Principal::Account(id.parse().ok()?), end + 1 - at))
}

fn code_span_at(chars: &[char], at: usize) -> Option<(String, usize)> {
    let open = chars[at..].iter().take_while(|&&c| c == '`').count();
    if open == 0 {
        return None;
    }
    let mut cursor = at + open;
    while cursor < chars.len() {
        let run = chars[cursor..].iter().take_while(|&&c| c == '`').count();
        if run == open {
            let inner: String = chars[at + open..cursor].iter().collect();
            let padded = inner.len() > 1
                && inner.starts_with(' ')
                && inner.ends_with(' ')
                && !inner.trim().is_empty();
            let inner = if padded {
                inner[1..inner.len() - 1].to_owned()
            } else {
                inner
            };
            return Some((inner, cursor + run - at));
        }
        cursor += run.max(1);
    }
    None
}

fn fenced(chars: &[char], at: usize, marker: &str) -> Option<(String, usize)> {
    let marks: Vec<char> = marker.chars().collect();
    if !chars[at..].starts_with(marks.as_slice()) {
        return None;
    }
    let word_bound = marks[0] == '_';
    let body_start = at + marks.len();
    if chars
        .get(body_start)
        .is_none_or(|next| next.is_whitespace())
    {
        return None;
    }
    let run_start = (0..at).rev().take_while(|&i| chars[i] == marks[0]).count();
    if word_bound && at > run_start && chars[at - run_start - 1].is_alphanumeric() {
        return None;
    }
    let mut cursor = body_start;
    while cursor + marks.len() <= chars.len() {
        if chars[cursor..].starts_with(marks.as_slice()) {
            if cursor == body_start {
                return None;
            }
            let end = cursor + marks.len();
            let space_before = chars[cursor - 1].is_whitespace();
            let word_after = word_bound
                && chars[end..]
                    .iter()
                    .find(|&&next| next != marks[0])
                    .is_some_and(|next| next.is_alphanumeric());
            if !space_before && !word_after {
                let inner: String = chars[body_start..cursor].iter().collect();
                return Some((inner, end - at));
            }
        }
        cursor += 1;
    }
    None
}

const LINK_SCHEMES: [&str; 3] = ["http://", "https://", "duck://"];
