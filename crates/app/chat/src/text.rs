//! What search and tags read out of a message, at read time: its flat
//! text and its `#tags`.
use std::ops::Range;

use unicode_normalization::UnicodeNormalization;

use crate::{Block, Mark, Notice};

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
            Block::Notice(notice) => notice_text(notice),
            Block::Divider => continue,
        };
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&piece);
    }
    out
}

pub fn notice_text(notice: &Notice) -> String {
    let mut text = format!("{} {}", notice.subject, notice.status.word());
    if !notice.detail.is_empty() {
        text.push_str(" — ");
        text.push_str(&notice.detail);
    }
    if let Some(link) = &notice.link {
        text.push_str(&format!(" ({link})"));
    }
    text
}

pub(crate) fn fold(text: &str) -> String {
    text.nfc().collect::<String>().to_lowercase()
}

pub fn search_terms(text: &str) -> Vec<String> {
    fold(text).split_whitespace().map(str::to_owned).collect()
}

pub fn sentences_holding(text: &str, terms: &[String]) -> Vec<String> {
    sentences(text)
        .into_iter()
        .filter(|sentence| {
            let folded = fold(sentence);
            terms.iter().any(|term| folded.contains(term.as_str()))
        })
        .map(str::to_owned)
        .collect()
}

pub fn around_matches(text: &str, terms: &[String], bytes: usize) -> Vec<String> {
    let text: String = text.nfc().collect();
    let mut folded_by_char = String::new();
    let mut origin: Vec<Range<usize>> = Vec::new();
    for (at, c) in text.char_indices() {
        let piece: String = c.to_lowercase().collect();
        origin.resize(origin.len() + piece.len(), at..at + c.len_utf8());
        folded_by_char.push_str(&piece);
    }
    let folded_whole = text.to_lowercase();
    let lengths_agree = folded_whole.len() == folded_by_char.len();
    let folded = if lengths_agree {
        folded_whole
    } else {
        folded_by_char
    };
    let mut windows: Vec<Range<usize>> = terms
        .iter()
        .flat_map(|term| folded.match_indices(term.as_str()))
        .map(|(from, found)| origin[from].start..origin[from + found.len() - 1].end)
        .map(|found| widened(&text, found, bytes))
        .collect();
    windows.sort_unstable_by_key(|window| window.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for window in windows {
        match merged.last_mut() {
            Some(last) if window.start <= last.end => last.end = last.end.max(window.end),
            _ => merged.push(window),
        }
    }
    merged
        .into_iter()
        .map(|window| text[window].to_owned())
        .collect()
}

fn widened(text: &str, found: Range<usize>, bytes: usize) -> Range<usize> {
    let mut start = found.start.saturating_sub(bytes);
    while !text.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = found.end.saturating_add(bytes).min(text.len());
    while !text.is_char_boundary(end) {
        end += 1;
    }
    start..end
}

fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        let closes_a_sentence = matches!(c, '.' | '?' | '!');
        let space_follows = chars.peek().is_some_and(|(_, next)| next.is_whitespace());
        let ends_here = closes_a_sentence && space_follows;
        if ends_here {
            let end = at + c.len_utf8();
            push_trimmed(&mut out, &text[start..end]);
            start = end;
        }
    }
    push_trimmed(&mut out, &text[start..]);
    out
}

fn push_trimmed<'a>(out: &mut Vec<&'a str>, piece: &'a str) {
    let piece = piece.trim();
    if !piece.is_empty() {
        out.push(piece);
    }
}

pub fn tags(blocks: &[Block]) -> Vec<String> {
    let spans = blocks.iter().flat_map(|block| match block {
        Block::Paragraph(spans) | Block::Quote(spans) => spans.as_slice(),
        Block::Code { .. } | Block::Divider | Block::Attachment(_) | Block::Notice(_) => &[],
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

    #[test]
    fn a_search_shows_the_sentences_holding_its_terms_or_the_bytes_around_each_match() {
        let text = "The ferry left. Nothing here! Is NOON late? noon again";
        let terms = search_terms("ferry noon");
        assert_eq!(
            sentences_holding(text, &terms),
            ["The ferry left.", "Is NOON late?", "noon again"]
        );
        assert_eq!(
            around_matches(text, &terms, 2),
            ["e ferry l", "s NOON l", "? noon a"]
        );
        assert_eq!(
            around_matches("ab ferry noon cd", &terms, 3),
            ["ab ferry noon cd"]
        );
        assert_eq!(
            around_matches("é ferry", &search_terms("ferry"), 2),
            ["é ferry"]
        );
        assert_eq!(
            sentences_holding("v1.2 is out. Ship it", &search_terms("ship")),
            ["Ship it"]
        );
        assert_eq!(around_matches("ΟΔΟΣ", &search_terms("οδος"), 3), ["ΟΔΟΣ"]);
        assert_eq!(sentences_holding("ΟΔΟΣ", &search_terms("οδος")), ["ΟΔΟΣ"]);
    }
}
