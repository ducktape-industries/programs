//! One text model. The host's editing engine owns a field's text; the view
//! holds a copy that follows every [`TextChange`] and asks for edits with
//! [`WidgetCommand::Replace`](crate::WidgetCommand::Replace) against the
//! `revision` it has seen. The host rebases a stale ask over what was typed
//! since, so no key waits on the view and nothing typed is lost.
use serde::{Deserialize, Serialize};
use std::ops::Range;

/// Bytes `start..end` of a field's text; a caret when empty.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextRange {
    pub start: u32,
    pub end: u32,
}

impl TextRange {
    pub fn caret(at: usize) -> Self {
        Self {
            start: at as u32,
            end: at as u32,
        }
    }

    pub fn range(self) -> Range<usize> {
        self.start as usize..self.end as usize
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

impl From<Range<usize>> for TextRange {
    fn from(range: Range<usize>) -> Self {
        Self {
            start: range.start as u32,
            end: range.end as u32,
        }
    }
}

/// One edit the engine made: `range` of the text before it became `len`
/// bytes. The host's edit log is these, each at the revision it made, and
/// each [`TextChange`] carries the one that made its revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    pub range: TextRange,
    pub len: u32,
}

impl Edit {
    fn shift(self, offset: u32) -> u32 {
        offset - self.range.end + self.range.start + self.len
    }

    /// Where an offset read before this edit stands after it, as a place
    /// before anything inserted exactly there: one at or before the edit's
    /// start stays; one at or after its end moves with the text; one inside
    /// the replaced span lands after the replacement.
    pub fn map(self, offset: u32) -> u32 {
        if offset <= self.range.start {
            offset
        } else if offset >= self.range.end {
            self.shift(offset)
        } else {
            self.range.start + self.len
        }
    }

    /// The same, as a place after anything inserted exactly there: only an
    /// offset before the edit's start stays.
    pub fn map_after(self, offset: u32) -> u32 {
        if offset < self.range.start {
            offset
        } else if offset >= self.range.end {
            self.shift(offset)
        } else {
            self.range.start + self.len
        }
    }

    /// What stands of `piece`, a span of the text before this edit, after
    /// it: the part before the replaced span as it was, the part after it
    /// moved with the text. The part the edit replaced is gone, and what it
    /// inserted was never the piece's.
    pub fn cut(self, piece: TextRange) -> impl Iterator<Item = TextRange> {
        let before = (piece.start < self.range.start).then(|| TextRange {
            start: piece.start,
            end: piece.end.min(self.range.start),
        });
        let after = (piece.end > self.range.end).then(|| TextRange {
            start: self.shift(piece.start.max(self.range.end)),
            end: self.shift(piece.end),
        });
        before.into_iter().chain(after)
    }
}

/// A `Replace` read at an older text, carried over the edits made since.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rebased {
    /// The span of the text as it stands that the replacement covers: from
    /// the first byte the view read that still stands to the last.
    pub range: TextRange,
    /// The bytes inside `range` typed since the view read, in order: not
    /// the view's to replace, they stay, after its text.
    pub kept: Vec<TextRange>,
    /// Where the cursor the view gave, in the text after its own edit,
    /// stands once the replacement has landed.
    pub cursor: TextRange,
}

impl Rebased {
    /// What goes in place of `range`: the view's `text`, then what was
    /// typed into the span since, as it reads in `current`.
    pub fn replacement(&self, current: &str, text: &str) -> String {
        let mut replacement = text.to_owned();
        for kept in &self.kept {
            replacement.push_str(current.get(kept.range()).unwrap_or_default());
        }
        replacement
    }
}

/// A `Replace` read at an older text, carried over the `edits` made since,
/// oldest first: the ask replaces only the bytes that existed when the
/// view read, so a letter typed into the span since stays. The cursor is
/// the one the view gave in the text after the replace's own edit of
/// `len` bytes.
pub fn rebase(
    range: TextRange,
    len: usize,
    cursor: TextRange,
    edits: impl IntoIterator<Item = Edit> + Clone,
) -> Rebased {
    let len = len as u32;
    let map = |offset: u32| {
        edits
            .clone()
            .into_iter()
            .fold(offset, |at, edit| edit.map(at))
    };
    let map_after = |offset: u32| {
        edits
            .clone()
            .into_iter()
            .fold(offset, |at, edit| edit.map_after(at))
    };
    let mut pieces = if range.is_empty() {
        Vec::new()
    } else {
        vec![range]
    };
    for edit in edits.clone() {
        pieces = pieces
            .into_iter()
            .flat_map(|piece| edit.cut(piece))
            .collect();
    }
    let (covered, kept) = match (pieces.first(), pieces.last()) {
        (Some(first), Some(last)) => (
            TextRange {
                start: first.start,
                end: last.end,
            },
            pieces
                .windows(2)
                .map(|pair| TextRange {
                    start: pair[0].end,
                    end: pair[1].start,
                })
                .collect(),
        ),
        _ => {
            let at = match range.is_empty() {
                true => map(range.start),
                false => map_after(range.start),
            };
            (TextRange::caret(at as usize), Vec::new())
        }
    };
    let removed: u32 = pieces.iter().map(|piece| piece.end - piece.start).sum();
    let own_end = range.start + len;
    let place = |at: u32| {
        if at >= own_end {
            (map_after(range.end + (at - own_end)) + len).saturating_sub(removed)
        } else if at >= range.start {
            covered.start + (at - range.start)
        } else {
            map(at)
        }
    };
    Rebased {
        range: covered,
        kept,
        cursor: TextRange {
            start: place(cursor.start),
            end: place(cursor.end),
        },
    }
}

/// An atomic span of a field's text (a mention): the engine moves over,
/// selects and deletes it whole. `id` is what the span means to the view.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextToken {
    pub range: TextRange,
    pub id: String,
}

// A token whose id starts with `CHIP` is a chip, a fixed prefix of its field (a folder before a
// name): a host that draws chips pads it and lays a fill under it. Any other token (a mention) is
// drawn as its text.
pub const CHIP: &str = "chip:";

/// A key the view hears (`on_key`) instead of the engine, matched after
/// IME processing against the key's state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyClaim {
    pub key: crate::keyboard::Key,
    pub modifiers: gpui::Modifiers,
    /// Add the host platform's command modifier (platform on macOS, control elsewhere).
    pub command: bool,
}

impl KeyClaim {
    pub fn matches(&self, key: &crate::keyboard::KeyState, macos: bool) -> bool {
        let mut modifiers = self.modifiers;
        if self.command {
            if macos {
                modifiers.platform = true;
            } else {
                modifiers.control = true;
            }
        }
        self.key == key.key && modifiers == key.modifiers
    }

    /// The engine's own keys, which no view claims: Backspace, Delete, Tab
    /// and the undo and redo chords, under the command key or either
    /// modifier it stands for. Editing and history are the engine's.
    pub fn engine_owned(&self) -> bool {
        use crate::keyboard::{Key, Named};
        match &self.key {
            Key::Named(Named::Backspace | Named::Delete | Named::Tab) => true,
            Key::Character(character) => {
                (self.command || self.modifiers.control || self.modifiers.platform)
                    && matches!(character.to_ascii_lowercase().as_str(), "z" | "y")
            }
            _ => false,
        }
    }
}

/// The host's word on a field's text: the whole text as the engine holds it
/// at `revision`, the view's `generation` that text is a document of, the
/// edit that made that revision from the text before it, its cursor, the
/// span an IME is still composing in, and the atomic spans where they now
/// lie. The edit is told, not left for the view to diff out: two texts
/// cannot say which of two equal bytes went. It is `None` when only the
/// cursor or the preedit moved, and when the change is the adopt of
/// `generation`: a new document in the field, not an edit of the old one,
/// so nothing the view holds in the new document's bytes moves with it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextChange {
    pub generation: u64,
    pub revision: u64,
    pub edit: Option<Edit>,
    pub text: String,
    pub cursor: TextRange,
    pub preedit: Option<TextRange>,
    pub tokens: Vec<TextToken>,
}

pub fn validate_field(
    value: &str,
    cursor: TextRange,
    tokens: &[TextToken],
    claims: &[KeyClaim],
) -> Result<(), &'static str> {
    if u32::try_from(value.len()).is_err() {
        return Err("field text is longer than a u32 offset reaches");
    }
    let on_text = |range: TextRange| {
        range.start <= range.end
            && value.is_char_boundary(range.start as usize)
            && value.is_char_boundary(range.end as usize)
    };
    if !on_text(cursor) {
        return Err("field cursor is off its text");
    }
    let mut last = 0;
    for token in tokens {
        let range = token.range;
        if range.start < last || range.is_empty() || !on_text(range) {
            return Err("field token is off its text");
        }
        if token.id.is_empty() {
            return Err("field token id is empty");
        }
        last = range.end;
    }
    if claims.iter().any(KeyClaim::engine_owned) {
        return Err("field claims a key the engine owns");
    }
    Ok(())
}

/// The one span of `before` that `after` rewrote, and what it reads now:
/// the host's edit log is built from it. Equal blocks are compared first,
/// so a letter typed into a long text costs its neighbourhood, not the text.
pub fn changed_span(before: &str, after: &str) -> Option<(TextRange, String)> {
    if before == after {
        return None;
    }
    let (b, a) = (before.as_bytes(), after.as_bytes());
    let limit = b.len().min(a.len());
    let mut start = 0;
    while start + 64 <= limit && b[start..start + 64] == a[start..start + 64] {
        start += 64;
    }
    while start < limit && b[start] == a[start] {
        start += 1;
    }
    while !(before.is_char_boundary(start) && after.is_char_boundary(start)) {
        start -= 1;
    }
    let limit = limit - start;
    let mut suffix = 0;
    while suffix + 64 <= limit
        && b[b.len() - suffix - 64..b.len() - suffix] == a[a.len() - suffix - 64..a.len() - suffix]
    {
        suffix += 64;
    }
    while suffix < limit && b[b.len() - suffix - 1] == a[a.len() - suffix - 1] {
        suffix += 1;
    }
    while !(before.is_char_boundary(b.len() - suffix) && after.is_char_boundary(a.len() - suffix)) {
        suffix -= 1;
    }
    Some((
        TextRange::from(start..b.len() - suffix),
        after[start..a.len() - suffix].to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(before: &str, after: &str) -> Option<(Range<usize>, String)> {
        changed_span(before, after).map(|(range, text)| (range.range(), text))
    }

    #[test]
    fn the_changed_span_is_the_smallest_on_character_boundaries() {
        assert_eq!(span("say word now", "say x now"), Some((4..8, "x".into())));
        assert_eq!(span("abc", "abc"), None);
        assert_eq!(span("", "abc"), Some((0..0, "abc".into())));
        assert_eq!(span("abc", ""), Some((0..3, String::new())));
        assert_eq!(span("aé", "aè"), Some((1..3, "è".into())));
        assert_eq!(span("é", "e"), Some((0..2, "e".into())));
        let long = "x".repeat(10_000);
        assert_eq!(
            span(&long, &format!("{long}y")),
            Some((10_000..10_000, "y".into()))
        );
        assert_eq!(span("aaa", "aaaa"), Some((3..3, "a".into())));
    }

    #[test]
    fn a_stale_replace_is_carried_over_the_edits_made_since() {
        let edit = |range: Range<usize>, len| Edit {
            range: range.into(),
            len,
        };
        let at = |range: Range<usize>, len, cursor, edits: &[Edit]| {
            let rebased = rebase(range.into(), len, TextRange::caret(cursor), edits.to_vec());
            (
                rebased.range.range(),
                rebased
                    .kept
                    .iter()
                    .map(|kept| (kept.start as usize, kept.end as usize))
                    .collect::<Vec<_>>(),
                rebased.cursor.range(),
            )
        };
        assert_eq!(at(0..5, 0, 0, &[edit(5..5, 1)]), (0..5, vec![], 1..1));
        assert_eq!(at(0..5, 0, 0, &[edit(0..0, 1)]), (1..6, vec![], 1..1));
        assert_eq!(at(0..5, 0, 0, &[edit(3..3, 1)]), (0..6, vec![(3, 4)], 1..1));
        assert_eq!(
            rebase(
                TextRange::from(0..5),
                0,
                TextRange::caret(0),
                [edit(3..3, 1)]
            )
            .replacement("helxlo", ""),
            "x"
        );
        assert_eq!(at(0..5, 0, 0, &[edit(2..4, 1)]), (0..4, vec![(2, 3)], 1..1));
        assert_eq!(at(7..7, 1, 8, &[edit(4..7, 6)]), (10..10, vec![], 11..11));
        assert_eq!(at(4..7, 6, 10, &[edit(7..7, 1)]), (4..7, vec![], 11..11));
        assert_eq!(
            at(7..7, 1, 8, &[edit(7..7, 1), edit(4..7, 6)]),
            (10..10, vec![], 12..12)
        );
        assert_eq!(at(5..5, 2, 7, &[edit(2..2, 2)]), (7..7, vec![], 9..9));
        assert_eq!(at(2..4, 1, 3, &[edit(1..6, 2)]), (3..3, vec![], 4..4));
        assert_eq!(at(2..4, 1, 3, &[edit(9..9, 3)]), (2..4, vec![], 3..3));
    }

    #[test]
    fn a_field_is_valid_on_its_own_text_and_claims_no_engine_key() {
        use crate::keyboard::{Key, Named};
        let claim = |key| KeyClaim {
            key,
            modifiers: Default::default(),
            command: false,
        };
        let token = |start, end| TextToken {
            range: TextRange::from(start..end),
            id: "<@1>".into(),
        };
        assert_eq!(
            validate_field("hi @al", TextRange::caret(6), &[token(3, 6)], &[]),
            Ok(())
        );
        assert_eq!(
            validate_field("é", TextRange::caret(1), &[], &[]),
            Err("field cursor is off its text")
        );
        assert_eq!(
            validate_field(
                "abcd",
                TextRange::caret(0),
                &[token(2, 4), token(0, 2)],
                &[]
            ),
            Err("field token is off its text")
        );
        assert_eq!(
            validate_field(
                "abcd",
                TextRange::caret(0),
                &[token(0, 3), token(2, 4)],
                &[]
            ),
            Err("field token is off its text")
        );
        for owned in [Named::Backspace, Named::Delete, Named::Tab] {
            assert_eq!(
                validate_field("", TextRange::default(), &[], &[claim(Key::Named(owned))]),
                Err("field claims a key the engine owns"),
                "{owned:?}"
            );
        }
        let undo = KeyClaim {
            command: true,
            ..claim(Key::Character("z".into()))
        };
        assert!(undo.engine_owned());
        for (control, platform, shift) in [
            (true, false, false),
            (false, true, false),
            (true, false, true),
        ] {
            let modifiers = gpui::Modifiers {
                control,
                platform,
                shift,
                ..Default::default()
            };
            let chord = KeyClaim {
                modifiers,
                ..claim(Key::Character("z".into()))
            };
            assert!(chord.engine_owned(), "{modifiers:?}");
        }
        assert!(!claim(Key::Character("z".into())).engine_owned());
        assert!(!claim(Key::Named(Named::Enter)).engine_owned());
    }
}
