//! What search and tags read out of a message, at read time: its flat
//! text and its `#tags`.
use std::ops::Range;

use unicode_normalization::UnicodeNormalization;

use crate::{Block, Mark};

/// The message as one line of text, blocks joined by a space. A block's
/// spans are runs of one text split where a mark starts or ends, so they
/// join as they are.
pub fn plain_text(blocks: &[Block]) -> String {
    let mut out = String::new();
    for block in blocks {
        let piece = match block {
            Block::Paragraph(spans) | Block::Quote(spans) => {
                spans.iter().map(|s| s.text.as_str()).collect::<String>()
            }
            Block::Code { text, .. } => text.clone(),
            Block::Attachment(file) => file.name.clone(),
            Block::Divider => continue,
        };
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&piece);
    }
    out
}

pub(crate) fn fold(text: &str) -> String {
    text.nfc().collect::<String>().to_lowercase()
}

pub fn tags(blocks: &[Block]) -> Vec<String> {
    let spans = blocks.iter().flat_map(|block| match block {
        Block::Paragraph(spans) | Block::Quote(spans) => spans.as_slice(),
        Block::Code { .. } | Block::Divider | Block::Attachment(_) => &[],
    });
    let mut out: Vec<String> = Vec::new();
    for span in spans.filter(|span| {
        !span
            .marks
            .iter()
            .any(|m| matches!(m, Mark::Link(_) | Mark::Code))
    }) {
        for label in span_tags(&span.text) {
            if !out.contains(&label) {
                out.push(label);
            }
        }
    }
    out
}

pub(crate) fn tag_label(text: &str) -> String {
    fold(text.trim_start_matches('#'))
}

fn span_tags(text: &str) -> Vec<String> {
    tag_ranges(text)
        .into_iter()
        .map(|(_, body)| tag_label(body))
        .collect()
}

// Where each `#tag` of one run of text lies, its `#` included, and its word:
// `[alnum_-]` after a `#` that opens at a word boundary (never `##`, `/#`,
// `&#`).
pub fn tag_ranges(text: &str) -> Vec<(Range<usize>, &str)> {
    let mut out = Vec::new();
    let mut prev: Option<char> = None;
    let mut from = 0;
    while let Some(found) = text[from..].find('#') {
        let at = from + found;
        let before = text[from..at].chars().next_back().or(prev);
        let opens = before.is_none_or(|p| !p.is_alphanumeric() && !matches!(p, '#' | '/' | '&'));
        let len: usize = text[at + 1..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .map(char::len_utf8)
            .sum();
        let body = &text[at + 1..at + 1 + len];
        if opens && !body.is_empty() {
            out.push((at..at + 1 + len, body));
        }
        prev = Some(body.chars().next_back().unwrap_or('#'));
        from = at + 1 + len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_message;

    #[test]
    fn tags_open_at_a_word_boundary_outside_code_and_links() {
        let blocks = parse_message("#Ship it, #ship ##no a#no /#no &#no #café\n```\n#code\n```");
        assert_eq!(tags(&blocks), ["ship", "café"]);
        let linked = parse_message("[#x](https://x) #y");
        assert_eq!(tags(&linked), ["y"]);
        assert_eq!(tag_ranges("é #Ship, a#no"), [(3..8, "Ship")]);
    }
}
