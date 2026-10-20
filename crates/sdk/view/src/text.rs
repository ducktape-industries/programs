//! The view's copy of a text field. The host's editing engine owns the
//! text: this copy follows every [`TextChange`] the host sends and asks for
//! edits against the host's `revision`, so no key waits on the view and
//! nothing typed is lost.
use crate::wire::{self, TextChange, TextRange, TextToken};
use gpui::ElementId;
use serde::{Deserialize, Serialize};
use std::cell::{Cell, Ref, RefCell};
use std::ops::Range;
use std::rc::Rc;

thread_local! {
    static GENERATIONS: Cell<u64> = const { Cell::new(0) };
}

fn fresh_generation() -> u64 {
    GENERATIONS.with(|generations| {
        let next = generations.get() + 1;
        generations.set(next);
        next
    })
}

pub(crate) fn lowered_generation(generation: u64) {
    GENERATIONS.with(|generations| generations.set(generations.get().max(generation)));
}

/// A field's text as the view knows it, bound to the host's field by
/// [`Input::new`](crate::Input::new) or
/// [`Textarea::new`](crate::Textarea::new): what is typed lands here, and
/// the view that drew the field renders. [`text`](Self::text) reads it;
/// [`replace`](Self::replace) and [`replace_all`](Self::replace_all) ask
/// the host for an edit the writer can undo; [`reset`](Self::reset) is a
/// new document, adopted whole, which is the one way the writer's undo is
/// cleared.
///
/// A field is a handle, as gpui's `Entity<Editor>` is: a `Clone` of a field
/// is the same field, so the element drawn from it and the state it lives
/// in share one text. (`vec![Form::default(); n]` is therefore one field
/// `n` times; build each with `Default::default()`.) Equality, `Debug` and
/// the snapshot are by value.
#[derive(Clone)]
pub struct TextField(Rc<RefCell<State>>);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename = "TextField")]
pub(crate) struct State {
    pub(crate) text: String,
    pub(crate) cursor: TextRange,
    pub(crate) tokens: Vec<TextToken>,
    pub(crate) preedit: Option<TextRange>,
    pub(crate) generation: u64,
    pub(crate) revision: u64,
}

impl Default for TextField {
    fn default() -> Self {
        Self::new("")
    }
}

impl PartialEq for TextField {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0) || *self.0.borrow() == *other.0.borrow()
    }
}
impl Eq for TextField {}

impl std::fmt::Debug for TextField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.0.borrow();
        f.debug_struct("TextField")
            .field("text", &state.text)
            .field("cursor", &state.cursor)
            .field("tokens", &state.tokens)
            .field("preedit", &state.preedit)
            .field("generation", &state.generation)
            .field("revision", &state.revision)
            .finish()
    }
}

impl Serialize for TextField {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.borrow().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TextField {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        State::deserialize(deserializer).map(|state| Self(Rc::new(RefCell::new(state))))
    }
}

impl State {
    fn new(text: String) -> Self {
        Self {
            cursor: TextRange::caret(text.len()),
            text,
            tokens: Default::default(),
            preedit: None,
            generation: fresh_generation(),
            revision: 0,
        }
    }
}

impl TextField {
    pub fn new(text: impl Into<String>) -> Self {
        Self(Rc::new(RefCell::new(State::new(text.into()))))
    }

    /// The text, as of the host's last word on it.
    pub fn text(&self) -> String {
        self.0.borrow().text.clone()
    }

    pub(crate) fn state(&self) -> Ref<'_, State> {
        self.0.borrow()
    }

    /// The atomic spans of the text and what they mean, where they now lie.
    pub fn tokens(&self) -> Vec<TextToken> {
        self.0.borrow().tokens.clone()
    }

    /// The document this field holds, by number: it moves on `reset`.
    pub fn generation(&self) -> u64 {
        self.0.borrow().generation
    }

    /// A new document in the field: `text`, caret at its end, no tokens.
    pub fn reset(&mut self, text: impl Into<String>) {
        self.reset_with_tokens(text, Vec::new());
    }

    /// A new document in the field: `text`, caret at its end, `tokens` its
    /// atomic spans: in text order, none empty, none over another, each on
    /// char boundaries and with an id, or the host refuses the frame.
    pub fn reset_with_tokens(&mut self, text: impl Into<String>, tokens: Vec<TextToken>) {
        let mut state = self.0.borrow_mut();
        *state = State {
            tokens,
            revision: state.revision,
            ..State::new(text.into())
        };
    }

    /// The host's word on the text, taken as is — when it is on this
    /// document. A change to a generation this field has left (one the host
    /// echoed before it adopted the reset) is not: the reset's text stands
    /// until the host adopts it. Answers whether the change was taken, and
    /// taking the same change twice is taking it once: the bound field's
    /// route applies each change, and a view that keeps its own gate on the
    /// answer may ask again.
    pub fn apply(&self, change: &TextChange) -> bool {
        let mut state = self.0.borrow_mut();
        if change.generation != state.generation {
            return false;
        }
        state.text.clone_from(&change.text);
        state.cursor = change.cursor;
        state.preedit = change.preedit;
        state.tokens.clone_from(&change.tokens);
        state.revision = change.revision;
        true
    }

    pub fn is_blank(&self) -> bool {
        self.0.borrow().text.trim().is_empty()
    }

    pub fn selection(&self) -> Range<usize> {
        self.0.borrow().cursor.range()
    }

    /// Asks the host to put `text` in place of `range` of the text as
    /// known here, the caret at `cursor` after it — an offset into the new
    /// text, or one before `range`. With `token`, the inserted text is one
    /// atomic span meaning `token`. The ask names this document's
    /// generation: after a `reset`, `revision` is the host's count of the
    /// old one, and only the generation says which text `range` is bytes of.
    /// This builds the ask; [`Window::dispatch`](crate::Window::dispatch)
    /// sends it.
    #[must_use]
    pub fn replace(
        &self,
        target: impl Into<ElementId>,
        range: Range<usize>,
        text: impl Into<String>,
        token: Option<String>,
        cursor: usize,
    ) -> wire::WidgetCommand {
        let state = self.0.borrow();
        wire::WidgetCommand::Replace {
            target: vec![crate::element::wire_id(target.into())],
            generation: state.generation,
            revision: state.revision,
            range: TextRange::from(range),
            text: text.into(),
            token,
            cursor: TextRange::caret(cursor),
        }
    }

    /// Asks the host to put `text` in place of the whole text as known
    /// here, the caret after it. As [`replace`](Self::replace), it builds
    /// the ask.
    #[must_use]
    pub fn replace_all(
        &self,
        target: impl Into<ElementId>,
        text: impl Into<String>,
    ) -> wire::WidgetCommand {
        let text = text.into();
        let caret = text.len();
        let whole = 0..self.0.borrow().text.len();
        self.replace(target, whole, text, None, caret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_fresh_field_is_a_generation_the_host_has_not_seen() {
        let first = TextField::default();
        let second = TextField::new("x");
        assert!(second.generation() > first.generation());
        lowered_generation(1_000);
        let mut later = TextField::default();
        assert!(later.generation() > 1_000);
        let before = later.generation();
        later.reset("again");
        assert!(later.generation() > before);
        assert_eq!(later.state().cursor, TextRange::caret(5));
    }

    #[test]
    fn a_reset_seats_the_spans_it_is_given() {
        let mut field = TextField::new("old");
        let before = field.generation();
        let span = TextToken {
            range: TextRange::from(3..7),
            id: "ada".into(),
        };
        field.reset_with_tokens("Hi @Ada", vec![span.clone()]);
        assert!(field.generation() > before);
        assert_eq!(
            (field.text().as_str(), field.tokens(), field.selection()),
            ("Hi @Ada", vec![span], 7..7)
        );
        field.reset("plain");
        assert!(field.tokens().is_empty());
    }

    #[test]
    fn a_replace_speaks_at_the_revision_the_field_knows() {
        let mut field = TextField::new("say word now");
        let span = TextToken {
            range: TextRange::from(4..8),
            id: "word".into(),
        };
        let change = |generation, revision, text: &str| TextChange {
            generation,
            revision,
            edit: None,
            text: text.into(),
            cursor: TextRange::from(4..8),
            preedit: None,
            tokens: vec![span.clone()],
        };
        let current = field.generation();
        assert!(field.apply(&change(current, 7, "say word now")));
        assert_eq!(
            (field.selection(), field.tokens()),
            (4..8, vec![span.clone()])
        );
        let wire::WidgetCommand::Replace {
            generation,
            revision,
            range,
            text,
            cursor,
            ..
        } = field.replace_all("f", "")
        else {
            unreachable!()
        };
        assert_eq!(
            (generation, revision, range, text, cursor),
            (
                current,
                7,
                TextRange::from(0..12),
                String::new(),
                TextRange::caret(0)
            )
        );
        field.reset("fresh");
        assert!(!field.apply(&change(current, 8, "say word now!")));
        assert_eq!(
            (field.text().as_str(), field.state().revision),
            ("fresh", 7)
        );
        let current = field.generation();
        assert!(field.apply(&change(current, 9, "fresh")));
        assert_eq!(field.state().revision, 9);
    }

    #[test]
    fn a_clone_of_a_field_is_the_same_field() {
        let mut field = TextField::new("one");
        let clone = field.clone();
        let generation = field.generation();
        assert!(clone.apply(&TextChange {
            generation,
            revision: 1,
            edit: None,
            text: "one two".into(),
            cursor: TextRange::caret(7),
            preedit: None,
            tokens: Default::default(),
        }));
        assert_eq!(field.text(), "one two");
        field.reset("three");
        assert_eq!(clone.text(), "three");
        assert_eq!(field, clone);

        let restored: TextField =
            serde_json::from_str(&serde_json::to_string(&field).unwrap()).unwrap();
        assert_eq!(restored, field, "equal by value");
        field.reset("four");
        assert_eq!(restored.text(), "three", "and a field of its own");
        assert_ne!(restored, field);
    }
}
