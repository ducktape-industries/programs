//! The composer's draft over the host's text field: the @-mention menu,
//! formatting marks, and the sends it made. The host's engine owns the
//! text, its undo and its clipboard; the draft follows it and asks for
//! edits.

use super::{Attaching, Outcome, Target, Upload};
use chat::message::Attachment;
use serde::{Deserialize, Serialize};
use std::ops::Range;
use view::{
    Bounds, ElementId, Modifiers, Pixels, ScrollStrategy, TextField, UniformListScrollHandle,
    Window, wire,
};
use wire::keyboard::{Key, Named};

/// The id of the editor [`view`](super::view) draws for the draft `key`.
pub(crate) fn editor_id(key: &str) -> String {
    format!("{key}/editor")
}

/// Gives the keyboard to the editor [`view`](super::view) drew for the
/// draft `key`.
pub fn focus(window: &mut Window, key: &str) {
    window.focus(ElementId::Name(editor_id(key).into()));
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MentionChoice {
    pub token: String,
    pub label: String,
    /// a word beside the label, said before the pick: why an agent would
    /// not answer the writer
    pub hint: Option<String>,
    // false for a name `@` does not offer (a program's own account, an
    // agent that does not act): it still names a mention already written
    pub offered: bool,
    // what is typed before it: `@` for an account, `#` for a room
    pub sigil: char,
    // offered only once something is typed after its sigil: a work room, or
    // one the reader is not in
    #[serde(default)]
    pub typed: bool,
}

impl MentionChoice {
    // How the field and the menu show it: `@ada`, `#general`.
    pub(crate) fn shown(&self) -> String {
        format!("{}{}", self.sigil, self.label)
    }

    // Whether a body's `token` names what this choice does: its own token,
    // or a link to the same room under an older label.
    fn stands_for(&self, token: &str) -> bool {
        self.token == token
            || room_link(&self.token).is_some_and(|link| room_link(token) == Some(link))
    }
}

// The link of a room's token, `[#name](link)`.
fn room_link(token: &str) -> Option<&str> {
    let (_, link) = token.strip_prefix("[#")?.split_once("](")?;
    link.strip_suffix(')')
}

// The length of the token `text` opens with: an account's `<@n>`, or a
// room's `[#name](link)`.
fn token_len(text: &str) -> Option<usize> {
    if text.starts_with("<@") {
        return text.find('>').map(|end| end + 1);
    }
    let close = text.find("](")?;
    let end = text[close..].find(')')? + close;
    Some(end + 1)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Send {
    pub body: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl From<&Bounds<Pixels>> for Place {
    fn from(bounds: &Bounds<Pixels>) -> Self {
        Self {
            x: bounds.origin.x.into(),
            y: bounds.origin.y.into(),
            width: bounds.size.width.into(),
            height: bounds.size.height.into(),
        }
    }
}

#[derive(Clone, Default)]
pub struct MenuScroll(pub(crate) UniformListScrollHandle);

impl MenuScroll {
    fn reveal(&self, index: usize) {
        self.0.scroll_to_item(index, ScrollStrategy::Nearest);
    }
}

impl std::fmt::Debug for MenuScroll {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MenuScroll")
    }
}

/// A mention is an atomic span of the field whose token is what it means.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Draft {
    pub field: TextField,
    pub failed_send: Option<Send>,
    pub submitted: Option<Send>,
    pub in_flight: Vec<Send>,
    pub note: String,
    pub menu_index: usize,
    pub menu_dismissed: bool,
    #[serde(default)]
    pub placed: Option<Place>,
    #[serde(skip)]
    pub menu_scroll: MenuScroll,
    /// Bytes of the field a send asked the host to clear, where they now
    /// lie: spoken for, not the draft's to send again, until the host's word
    /// shows them gone. A replacement view starts with none: the old
    /// view's ask went with its queue.
    #[serde(skip)]
    pub cleared: Vec<wire::TextRange>,
    // the message its files are filed under, minted with the first
    #[serde(default)]
    pub message_id: Option<String>,
    // every name a commit was asked for under that message: its folder may
    // hold each, whatever became of the file in the draft
    #[serde(default)]
    pub filed: Vec<String>,
    #[serde(default)]
    pub attachments: Vec<Attaching>,
    // how many files were attached: the next one's id
    #[serde(default)]
    pub attached: u64,
    // the files an edit's message carries, which the edit keeps
    #[serde(default)]
    pub kept: Vec<Attachment>,
    // a send that waits for files still going up
    #[serde(default)]
    pub queued: Option<(Send, Target)>,
    #[serde(skip)]
    pub minting: bool,
}

/// What a formatting mark wraps its selection in.
fn markers(tag: &str) -> Option<(&'static str, &'static str)> {
    match tag {
        "bold" => Some(("**", "**")),
        "italic" => Some(("*", "*")),
        "code" => Some(("```\n", "\n```")),
        "quote" => Some(("> ", "")),
        _ => None,
    }
}

pub(crate) fn matching_choices<'a>(
    choices: &'a [MentionChoice],
    sigil: char,
    partial: &str,
) -> Vec<&'a MentionChoice> {
    let needle = partial.to_lowercase();
    choices
        .iter()
        .filter(|choice| {
            choice.sigil == sigil
                && choice.offered
                && !(choice.typed && needle.is_empty())
                && words_start(&choice.label.to_lowercase(), &needle)
        })
        .collect()
}

// Whether a word of `name` starts with `needle`: the name's start, or the
// place after a space, `-`, `_`, `.` or a folder's `/`.
fn words_start(name: &str, needle: &str) -> bool {
    let after_break = name
        .char_indices()
        .filter(|(_, c)| matches!(c, ' ' | '-' | '_' | '.' | '/'))
        .map(|(at, c)| at + c.len_utf8());
    std::iter::once(0)
        .chain(after_break)
        .any(|at| name[at..].starts_with(needle))
}

impl Draft {
    #[cfg(test)]
    pub fn from_body(body: &str, roster: &[MentionChoice]) -> Self {
        let mut draft = Self::default();
        draft.seed(body, roster);
        draft
    }

    /// A new document in the field: `body` with its `<@n>` tokens shown
    /// as the names the roster gives them.
    pub fn seed(&mut self, body: &str, roster: &[MentionChoice]) {
        let body = body.replace("\r\n", "\n").replace('\r', "\n");
        let mut text = String::new();
        let mut tokens = Vec::new();
        let mut remaining = body.as_str();
        let opens = |rest: &str| ["<@", "[#"].iter().filter_map(|open| rest.find(open)).min();
        while let Some(start) = opens(remaining) {
            text.push_str(&remaining[..start]);
            remaining = &remaining[start..];
            let Some(len) = token_len(remaining) else {
                text.push_str(&remaining[..2]);
                remaining = &remaining[2..];
                continue;
            };
            let token = &remaining[..len];
            match roster.iter().find(|choice| choice.stands_for(token)) {
                Some(choice) => {
                    let start = text.len();
                    text.push_str(&choice.shown());
                    tokens.push(wire::TextToken {
                        range: wire::TextRange::from(start..text.len()),
                        id: choice.token.clone(),
                    });
                }
                None => text.push_str(token),
            }
            remaining = &remaining[len..];
        }
        text.push_str(remaining);
        self.field.reset_with_tokens(text, tokens);
        self.cleared.clear();
        self.menu_index = 0;
        self.menu_dismissed = false;
    }

    /// The text with every mention as its token, less what a send asked the
    /// host to clear.
    pub fn body(&self) -> String {
        let text = self.field.text();
        let tokens = self.field.tokens();
        let mut body = String::new();
        let mut at = 0;
        let end = wire::TextRange::caret(text.len());
        for piece in self.cleared.iter().chain([&end]) {
            let mut from = at;
            for token in &tokens {
                if token.range.start as usize >= at && token.range.end <= piece.start {
                    body.push_str(&text[from..token.range.start as usize]);
                    body.push_str(&token.id);
                    from = token.range.end as usize;
                }
            }
            body.push_str(&text[from..piece.start as usize]);
            at = piece.end as usize;
        }
        body
    }

    /// The host's word on the field. Typing closes a dismissed menu's
    /// dismissal and starts the next menu at its first row, and the edit the
    /// host made carries what a send asked to clear to where it now lies:
    /// gone once the clear landed, or the writer deleted it. The host's edit,
    /// not a diff of the two texts: "oko" cleared of "ok" reads "o", and a
    /// diff cannot tell the typed-ahead "o" from the sent one. A word on a
    /// document `seed` has since left is nothing here, and the adopt of the
    /// seeded one carries no edit: what a send spoke for in its bytes stays
    /// where it is.
    pub fn changed(&mut self, change: &wire::TextChange) {
        if !self.field.apply(change) {
            return;
        }
        if let Some(edit) = change.edit {
            self.menu_index = 0;
            self.menu_dismissed = false;
            self.menu_scroll.reveal(0);
            self.cleared = self
                .cleared
                .iter()
                .flat_map(|piece| edit.cut(*piece))
                .collect();
        }
    }

    pub fn can_send(&self) -> bool {
        let failed = self.attachments.iter().any(|file| file.failed().is_some());
        let files = !self.kept.is_empty() || self.attachments.iter().any(|file| !file.sent);
        !failed && (files || !self.body().trim().is_empty())
    }

    // the pasted `text` over the selection: one edit, which the host undoes
    // as one
    pub fn paste(&self, target: &str, text: &str) -> Option<wire::WidgetCommand> {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if text.is_empty() {
            return None;
        }
        let range = self.field.selection();
        let caret = range.start + text.len();
        let target = ElementId::Name(target.to_owned().into());
        Some(self.field.replace(target, range, text, None, caret))
    }

    // The `@name` or `#room` being typed at the caret, if any: its span and
    // the name so far. None with a selection, inside a mention, or after
    // Escape.
    pub(crate) fn query(&self) -> Option<(Range<usize>, String)> {
        let cursor = self.field.selection();
        if self.menu_dismissed || cursor.start != cursor.end {
            return None;
        }
        let text = self.field.text();
        let at = cursor.start;
        let before = text.get(..at)?;
        let after = text.get(at..)?;
        let handle_char = |c: char| c.is_alphanumeric() || matches!(c, '-' | '_' | '.');
        if after.chars().next().is_some_and(handle_char) {
            return None;
        }
        let line = before.rsplit('\n').next()?;
        let start = line.rfind(['@', '#'])?;
        let partial = &line[start + 1..];
        let valid = partial.chars().all(|c| handle_char(c) || c == ' ')
            && line[..start]
                .chars()
                .next_back()
                .is_none_or(char::is_whitespace);
        if !valid {
            return None;
        }
        let range = before.len() - line.len() + start..at;
        let overlaps = self.field.tokens().iter().any(|token| {
            range.start < token.range.end as usize && range.end > token.range.start as usize
        });
        if overlaps {
            return None;
        }
        Some((range, partial.into()))
    }

    // What the query asks for: the sigil typed, and the name so far.
    pub(crate) fn typed(&self) -> Option<(char, String)> {
        let (range, partial) = self.query()?;
        let sigil = self.field.text()[range.start..].chars().next()?;
        Some((sigil, partial))
    }

    // Whether the menu is open: a name is being typed that a choice matches.
    fn menu_open(&self, choices: &[MentionChoice]) -> bool {
        self.menu(choices)
            .is_some_and(|(matches, _)| !matches.is_empty())
    }

    /// The menu's choices and which row is picked.
    fn menu<'a>(&self, choices: &'a [MentionChoice]) -> Option<(Vec<&'a MentionChoice>, usize)> {
        let (sigil, partial) = self.typed()?;
        let matches = matching_choices(choices, sigil, &partial);
        let selected = self.menu_index.min(matches.len().saturating_sub(1));
        Some((matches, selected))
    }

    /// The keys the composer hears instead of the engine: Enter to send or
    /// to pick a mention, the arrows and Escape while the menu is open, and
    /// the formatting chords.
    pub(super) fn claims(&self, choices: &[MentionChoice]) -> Vec<wire::KeyClaim> {
        let bare = |key| wire::KeyClaim {
            key: Key::Named(key),
            modifiers: Modifiers::default(),
            command: false,
        };
        let chord = |key: &str, shift| wire::KeyClaim {
            key: Key::Character(key.into()),
            modifiers: Modifiers {
                shift,
                ..Modifiers::default()
            },
            command: true,
        };
        let mut claims = vec![bare(Named::Enter)];
        if self.menu_open(choices) {
            claims.extend([Named::ArrowUp, Named::ArrowDown, Named::Escape].map(bare));
        }
        claims.extend(
            [
                ("b", false),
                ("i", false),
                ("c", true),
                ("9", true),
                ("v", false),
            ]
            .map(|(k, s)| chord(k, s)),
        );
        claims
    }

    /// What a claimed key does; `None` leaves the draft as it is.
    pub(super) fn key_tag(
        &self,
        key: &wire::keyboard::KeyState,
        repeat: bool,
        choices: &[MentionChoice],
    ) -> Option<String> {
        if key.modifiers.control || key.modifiers.platform {
            return match (&key.key, key.modifiers.shift) {
                (Key::Character(key), false) if key == "b" => Some("bold".into()),
                (Key::Character(key), false) if key == "i" => Some("italic".into()),
                (Key::Character(key), false) if key == "v" => Some("paste".into()),
                (Key::Character(key), true) if key == "c" => Some("code".into()),
                (Key::Character(key), true) if key == "9" => Some("quote".into()),
                _ => None,
            };
        }
        match &key.key {
            // a name no choice matches is text: Enter sends it
            Key::Named(Named::Enter) => match self.menu(choices) {
                Some((matches, selected)) if matches.get(selected).is_some() => {
                    Some("mention".into())
                }
                _ if repeat => None,
                _ => Some("send".into()),
            },
            // the arrows and Escape are claimed only while the menu is open;
            // one from a frame before it shut is the engine's to have had
            Key::Named(Named::ArrowDown) if self.menu_open(choices) => Some("menu-next".into()),
            Key::Named(Named::ArrowUp) if self.menu_open(choices) => Some("menu-previous".into()),
            Key::Named(Named::Escape) if self.menu_open(choices) => Some("menu-dismiss".into()),
            _ => None,
        }
    }

    /// The edits `tag` asks of the host's field `target`, if any, and
    /// whether the view has a send to make.
    pub(super) fn act(
        &mut self,
        tag: &str,
        target: &str,
        choices: &[MentionChoice],
    ) -> (Vec<wire::WidgetCommand>, Outcome) {
        // the view's to do: the host's picker, its clipboard, a file let go
        if tag == "attach" || tag == "paste" || tag.starts_with("remove:") {
            return (Vec::new(), Outcome::Action(tag.to_owned()));
        }
        let last = self
            .menu(choices)
            .map_or(0, |(matches, _)| matches.len().saturating_sub(1));
        let target = ElementId::Name(target.to_owned().into());
        let field = &self.field;
        let len = field.text().len();
        let edits = match tag {
            "menu-next" => {
                self.menu_index = self.menu_index.saturating_add(1).min(last);
                self.menu_scroll.reveal(self.menu_index);
                Vec::new()
            }
            "menu-previous" => {
                self.menu_index = self.menu_index.saturating_sub(1);
                self.menu_scroll.reveal(self.menu_index);
                Vec::new()
            }
            "menu-dismiss" => {
                self.menu_dismissed = true;
                Vec::new()
            }
            "send" => {
                if !self.can_send() {
                    return (Vec::new(), Outcome::Updated);
                }
                self.submitted = Some(Send {
                    body: self.body().trim().to_owned(),
                });
                // files alone: no text to clear
                if len == 0 {
                    return (Vec::new(), Outcome::Action("send".into()));
                }
                // the whole text is spoken for until the host's word shows
                // the clear landed: a second Enter before then sends only
                // what was typed since
                self.cleared = vec![wire::TextRange::from(0..len)];
                let clear = field.replace_all(target, "");
                return (vec![clear], Outcome::Action("send".into()));
            }
            "restore" => {
                if len > 0 {
                    return (Vec::new(), Outcome::Updated);
                }
                if let Some(send) = self.failed_send.take() {
                    self.seed(&send.body, choices);
                }
                Vec::new()
            }
            // the picked row, or the one the press named
            "mention" => match self.menu(choices) {
                Some((matches, selected)) => matches
                    .get(selected)
                    .map_or_else(Vec::new, |choice| self.mention(choice, &target)),
                None => Vec::new(),
            },
            _ => match (markers(tag), tag.strip_prefix("mention:")) {
                (Some((open, close)), _) => {
                    let range = field.selection();
                    if range.is_empty() {
                        let caret = range.start + open.len();
                        vec![field.replace(target, range, format!("{open}{close}"), None, caret)]
                    } else {
                        // two insertions, so a mention inside the selection
                        // stays one span: the host rebases the second over
                        // the first
                        let mut edits = vec![field.replace(
                            target.clone(),
                            range.start..range.start,
                            open,
                            None,
                            range.start + open.len(),
                        )];
                        if !close.is_empty() {
                            edits.push(field.replace(
                                target,
                                range.end..range.end,
                                close,
                                None,
                                range.end + close.len(),
                            ));
                        }
                        edits
                    }
                }
                (None, Some(token)) => choices
                    .iter()
                    .find(|choice| choice.token == token)
                    .map_or_else(Vec::new, |choice| self.mention(choice, &target)),
                (None, None) => Vec::new(),
            },
        };
        (edits, Outcome::Updated)
    }

    /// The `@name` being typed becomes the mention `choice`, a space after
    /// it for the next word.
    fn mention(&self, choice: &MentionChoice, target: &ElementId) -> Vec<wire::WidgetCommand> {
        let Some((range, _)) = self.query() else {
            return Vec::new();
        };
        let label = choice.shown();
        let end = range.start + label.len();
        vec![
            self.field.replace(
                target.clone(),
                range.clone(),
                label,
                Some(choice.token.clone()),
                end,
            ),
            self.field.replace(
                target.clone(),
                range.end..range.end,
                " ",
                None,
                range.end + 1,
            ),
        ]
    }

    pub fn failed(&mut self, send: Send) {
        match &mut self.failed_send {
            Some(previous) => {
                if !previous.body.is_empty() && !send.body.is_empty() {
                    previous.body.push('\n');
                }
                previous.body.push_str(&send.body);
            }
            None => self.failed_send = Some(send),
        }
    }

    pub fn complete_send(&mut self, send: &Send) {
        let Some(index) = self.in_flight.iter().position(|pending| pending == send) else {
            return;
        };
        self.in_flight.remove(index);
    }

    pub fn retire_device_requests(&mut self) {
        if let Some((send, _)) = self.queued.take() {
            self.in_flight.push(send);
        }
        for file in &mut self.attachments {
            if file.landed().is_none() && file.failed().is_none() {
                let cut = "The upload was cut off; remove the file and attach it again";
                file.upload = Upload::Failed(cut.into());
            }
        }
        for send in std::mem::take(&mut self.in_flight) {
            self.failed(send);
            self.note = "A send was interrupted; check the conversation before restoring it".into();
        }
    }
}

/// What the composer's tests say for the host: its word reaches a field
/// through `apply`, as it reaches a view's.
#[cfg(test)]
pub(super) mod host {
    use std::ops::Range;
    use view::{TextField, wire};

    /// The host's count of `field`'s edits, as an ask of it names it.
    pub fn revision(field: &TextField) -> u64 {
        let wire::WidgetCommand::Replace { revision, .. } = field.replace_all("field", "") else {
            unreachable!()
        };
        revision
    }

    /// The host's word that `cursor` is selected at its count `revision`:
    /// the text and its spans stand.
    fn says(field: &TextField, cursor: Range<usize>, revision: u64) {
        let taken = field.apply(&wire::TextChange {
            generation: field.generation(),
            revision,
            edit: None,
            text: field.text(),
            cursor: cursor.into(),
            preedit: None,
            tokens: field.tokens(),
        });
        assert!(taken, "a word on the field's own document");
    }

    /// The host's word that `cursor` is selected.
    pub fn selects(field: &TextField, cursor: Range<usize>) {
        says(field, cursor, revision(field));
    }

    /// The host's word that its count is `revision`.
    pub fn at_revision(field: &TextField, revision: u64) {
        says(field, field.selection(), revision);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster() -> Vec<MentionChoice> {
        vec![MentionChoice {
            token: "<@7>".into(),
            label: "Ada".into(),
            hint: None,
            offered: true,
            sigil: '@',
            typed: false,
        }]
    }

    fn replaces(edits: &[wire::WidgetCommand]) -> Vec<(Range<usize>, &str, Option<&str>, usize)> {
        edits
            .iter()
            .map(|edit| match edit {
                wire::WidgetCommand::Replace {
                    range,
                    text,
                    token,
                    cursor,
                    ..
                } => (
                    range.range(),
                    text.as_str(),
                    token.as_deref(),
                    cursor.start as usize,
                ),
                other => panic!("not a replace: {other:?}"),
            })
            .collect()
    }

    /// The host's word on document `generation` at its count `revision`:
    /// `edit` (the range it replaced and the length put there) made `text`,
    /// the caret after it.
    fn change(
        generation: u64,
        revision: u64,
        edit: Option<(Range<usize>, u32)>,
        text: &str,
        tokens: Vec<wire::TextToken>,
    ) -> wire::TextChange {
        wire::TextChange {
            generation,
            revision,
            edit: edit.map(|(range, len)| wire::Edit {
                range: range.into(),
                len,
            }),
            text: text.into(),
            cursor: wire::TextRange::caret(text.len()),
            preedit: None,
            tokens,
        }
    }

    /// Bold is `**…**` and italic `*…*`: the chat message parser reads
    /// emphasis by the flanking rule, so an `_` inside a word is a letter.
    /// A selection is wrapped by two insertions, so the mention in it
    /// stays one span.
    #[test]
    fn formatting_wraps_the_selection_around_a_mention() {
        let mut draft = Draft::from_body("Hi <@7>", &roster());
        assert_eq!(draft.field.text(), "Hi @Ada");
        host::selects(&draft.field, 3..7);
        let (bold, _) = draft.act("bold", "c/editor", &roster());
        assert_eq!(
            replaces(&bold),
            [(3..3, "**", None, 5), (7..7, "**", None, 9)]
        );
        let (quote, _) = draft.act("quote", "c/editor", &roster());
        assert_eq!(replaces(&quote), [(3..3, "> ", None, 5)]);
        host::selects(&draft.field, 7..7);
        let (italic, _) = draft.act("italic", "c/editor", &roster());
        assert_eq!(replaces(&italic), [(7..7, "**", None, 8)]);
    }

    #[test]
    fn a_picked_mention_is_one_span_with_a_space_after_it() {
        let mut draft = Draft::from_body("hi @A", &roster());
        assert_eq!(draft.query(), Some((3..5, "A".into())));
        let (edits, _) = draft.act("mention:<@7>", "c/editor", &roster());
        assert_eq!(
            replaces(&edits),
            [(3..5, "@Ada", Some("<@7>"), 7), (5..5, " ", None, 6)]
        );
        // the host's answer: the span sits where the engine put it
        let ada = wire::TextToken {
            range: wire::TextRange::from(3..7),
            id: "<@7>".into(),
        };
        let generation = draft.field.generation();
        draft.changed(&change(
            generation,
            1,
            Some((3..5, 5)),
            "hi @Ada ",
            vec![ada],
        ));
        assert_eq!(draft.body(), "hi <@7> ");
        assert_eq!(draft.query(), None, "a mention is never a query");
    }

    // A name `@` does not offer still names the mention a message already
    // holds, as one span, but typing it finds nothing.
    #[test]
    fn a_name_not_offered_keeps_a_written_mention_and_is_never_matched() {
        let forge = [MentionChoice {
            token: "<@9>".into(),
            label: "forge".into(),
            hint: None,
            offered: false,
            sigil: '@',
            typed: false,
        }];
        let draft = Draft::from_body("ask <@9> @fo", &forge);
        assert_eq!(draft.field.text(), "ask @forge @fo");
        assert_eq!(draft.body(), "ask <@9> @fo");
        host::selects(&draft.field, 14..14);
        assert_eq!(draft.query(), Some((11..14, "fo".into())));
        assert_eq!(draft.menu(&forge), Some((Vec::new(), 0)));
    }

    // `@` matches the start of any word of a name, in any case, and a
    // bare `@` every name offered, in the roster's order.
    #[test]
    fn a_query_matches_the_start_of_any_word_of_a_name() {
        fn found<'a>(choices: &'a [MentionChoice], partial: &str) -> Vec<&'a str> {
            matching_choices(choices, '@', partial)
                .into_iter()
                .map(|choice| choice.label.as_str())
                .collect()
        }
        let choices = ["Ada Lovelace", "Alan", "bo.k", "qa-mention-b"].map(|label| MentionChoice {
            token: label.into(),
            label: label.into(),
            hint: None,
            offered: true,
            sigil: '@',
            typed: false,
        });
        let every = ["Ada Lovelace", "Alan", "bo.k", "qa-mention-b"];
        assert_eq!(found(&choices, ""), every);
        assert_eq!(found(&choices, "a"), ["Ada Lovelace", "Alan"]);
        assert_eq!(found(&choices, "lo"), ["Ada Lovelace"]);
        assert_eq!(found(&choices, "LOVE"), ["Ada Lovelace"]);
        assert_eq!(found(&choices, "ada l"), ["Ada Lovelace"]);
        assert_eq!(found(&choices, "men"), ["qa-mention-b"]);
        assert_eq!(found(&choices, "k"), ["bo.k"]);
        assert!(found(&choices, "ve").is_empty(), "inside a word");
    }

    // `#` asks for a room: only the rooms match it, and a pick puts a link
    // to the room in the field as one span, which the body carries as the
    // markdown chat reads as a link. A body naming the room under an older
    // label shows its name now.
    #[test]
    fn a_picked_room_is_a_link_to_it() {
        let link = "duck://testnet-0a1b2c3d/chat/general";
        let room = MentionChoice {
            token: format!("[#General]({link})"),
            label: "General".into(),
            hint: None,
            offered: true,
            sigil: '#',
            typed: false,
        };
        let choices = [roster().remove(0), room.clone()];
        let mut draft = Draft::from_body("hi #ge", &choices);
        assert_eq!(draft.typed(), Some(('#', "ge".into())));
        assert_eq!(draft.menu(&choices), Some((vec![&room], 0)));
        let tag = format!("mention:{}", room.token);
        let (edits, _) = draft.act(&tag, "c/editor", &choices);
        assert_eq!(
            replaces(&edits),
            [
                (3..6, "#General", Some(room.token.as_str()), 11),
                (6..6, " ", None, 7)
            ]
        );
        let seeded = Draft::from_body(&format!("see [#old]({link})"), &choices);
        assert_eq!(seeded.field.text(), "see #General");
        assert_eq!(seeded.body(), format!("see [#General]({link})"));
        let blocks = chat::parse_message(&seeded.body());
        let chat::message::Block::Paragraph(spans) = &blocks[0] else {
            panic!("one paragraph")
        };
        assert_eq!(spans[1].marks, [chat::message::Mark::Link(link.into())]);
    }

    // A choice offered only once typed waits for a letter after its sigil,
    // and a folder's `/` starts a word.
    #[test]
    fn a_choice_offered_once_typed_waits_for_a_letter() {
        let room = MentionChoice {
            token: "[#jobs/j-127](duck://testnet-0a1b2c3d/chat/j-127)".into(),
            label: "jobs/j-127 · a filter".into(),
            hint: None,
            offered: true,
            sigil: '#',
            typed: true,
        };
        let found = |partial| matching_choices(std::slice::from_ref(&room), '#', partial).len();
        assert_eq!(found(""), 0);
        assert_eq!(found("j-1"), 1);
        assert_eq!(found("jobs"), 1);
        assert_eq!(found("filter"), 1);
    }

    #[test]
    fn a_send_clears_the_field_at_the_revision_it_knows_and_keeps_the_body() {
        let mut draft = Draft::from_body("first <@7>", &roster());
        host::at_revision(&draft.field, 4);
        let (edits, outcome) = draft.act("send", "c/editor", &roster());
        assert!(matches!(outcome, Outcome::Action(tag) if tag == "send"));
        let [
            wire::WidgetCommand::Replace {
                revision,
                range,
                text,
                ..
            },
        ] = edits.as_slice()
        else {
            panic!("one clear")
        };
        assert_eq!((revision, range.range(), text.as_str()), (&4, 0..10, ""));
        assert_eq!(draft.submitted.take().unwrap().body, "first <@7>");
        let blank = Draft::from_body("  ", &[]);
        assert!(!blank.can_send());
    }

    /// A send speaks for the text it asked the host to clear: until the
    /// host's word shows it gone, a second Enter sends only what was typed
    /// since, and nothing when nothing was. The host answers the clear one
    /// tick and two frames later; the keys do not wait for it.
    #[test]
    fn a_second_send_before_the_clear_lands_sends_only_what_was_typed_since() {
        let mut draft = Draft::from_body("hi <@7>", &roster());
        assert_eq!(draft.field.text(), "hi @Ada");
        let change = |draft: &mut Draft,
                      revision,
                      edit: (Range<usize>, u32),
                      text: &str,
                      tokens: Vec<wire::TextToken>| {
            draft.changed(&change(
                draft.field.generation(),
                revision,
                Some(edit),
                text,
                tokens,
            ))
        };
        let send = |draft: &mut Draft| {
            let (edits, outcome) = draft.act("send", "c/editor", &roster());
            let sent = matches!(outcome, Outcome::Action(tag) if tag == "send");
            let clears = replaces(&edits);
            let [clear] = clears.as_slice() else {
                assert!(edits.is_empty(), "one clear or none: {edits:?}");
                return (None, None);
            };
            (
                Some(clear.0.clone()),
                draft
                    .submitted
                    .take()
                    .filter(|_| sent)
                    .map(|send| send.body),
            )
        };
        assert_eq!(send(&mut draft), (Some(0..7), Some("hi <@7>".into())));
        // Enter again, the field still showing the text: nothing to send
        assert!(!draft.can_send());
        assert_eq!(send(&mut draft), (None, None));
        // " yo" typed before the clear landed: that, and only that, is the
        // next send; its clear covers the whole text as the draft sees it
        let ada = |range: Range<usize>| wire::TextToken {
            range: range.into(),
            id: "<@7>".into(),
        };
        change(&mut draft, 1, (7..7, 1), "hi @Ada y", vec![ada(3..7)]);
        change(&mut draft, 2, (8..8, 1), "hi @Ada yo", vec![ada(3..7)]);
        assert_eq!(draft.cleared, [wire::TextRange::from(0..7)]);
        assert_eq!(draft.body(), " yo");
        assert_eq!(send(&mut draft), (Some(0..10), Some("yo".into())));
        // the first clear lands, then the second (rebased over the first by
        // the host): nothing is spoken for
        change(&mut draft, 3, (0..7, 0), " yo", Vec::new());
        assert_eq!(draft.cleared, [wire::TextRange::from(0..3)]);
        assert!(!draft.can_send());
        change(&mut draft, 4, (0..3, 0), "", Vec::new());
        assert!(draft.cleared.is_empty());
        // typed into the span being cleared: still the writer's, the clear
        // lands round it as one edit that puts the kept byte back
        change(&mut draft, 5, (0..0, 3), "abc", Vec::new());
        assert_eq!(send(&mut draft), (Some(0..3), Some("abc".into())));
        change(&mut draft, 6, (2..2, 1), "abXc", Vec::new());
        assert_eq!(
            draft.cleared,
            [wire::TextRange::from(0..2), wire::TextRange::from(3..4)]
        );
        assert_eq!(draft.body(), "X");
        change(&mut draft, 7, (0..4, 1), "X", Vec::new());
        assert!(draft.cleared.is_empty());
        assert_eq!(draft.body(), "X");
    }

    /// The host tells the edit it made; the draft never diffs it out. "ok"
    /// sent, "o" typed ahead, the clear lands and the field reads "o": a
    /// diff of "oko" and "o" takes the first "o" for the one that stayed
    /// and speaks for the typed one, so Send rested, Enter did nothing, and
    /// once "k" followed Enter sent "k" for "ok".
    #[test]
    fn type_ahead_that_repeats_the_sent_texts_start_stays_the_writers() {
        let mut draft = Draft::from_body("ok", &[]);
        let change = |draft: &mut Draft, revision, edit: (Range<usize>, u32), text: &str| {
            draft.changed(&change(
                draft.field.generation(),
                revision,
                Some(edit),
                text,
                Vec::new(),
            ))
        };
        let send = |draft: &mut Draft| {
            let (_, outcome) = draft.act("send", "c/editor", &[]);
            assert!(matches!(outcome, Outcome::Action(tag) if tag == "send"));
            draft.submitted.take().unwrap().body
        };
        assert_eq!(send(&mut draft), "ok");
        // "o" typed at the end before the clear landed
        change(&mut draft, 1, (2..2, 1), "oko");
        assert_eq!(draft.body(), "o");
        // the clear lands round it: the host took 0..2, the typed "o" stands
        change(&mut draft, 2, (0..2, 0), "o");
        assert_eq!(
            (
                draft.cleared.as_slice(),
                draft.body().as_str(),
                draft.can_send()
            ),
            (&[][..], "o", true)
        );
        change(&mut draft, 3, (2..2, 1), "ok");
        assert_eq!(send(&mut draft), "ok");
    }

    /// A Restore and an Enter the view handles in one tick, before the host
    /// has adopted the seeded text: the send speaks for the seeded bytes
    /// and asks a clear of them, both in the new document's coordinates, so
    /// the ask names the new generation. The host's word on the old document
    /// (a keystroke it echoed before it saw the reset) is nothing here; its
    /// adopt of the new one carries no edit, so what the send spoke for
    /// stays where it is until the clear lands.
    #[test]
    fn restore_then_send_in_one_tick_speak_of_the_seeded_document() {
        let mut draft = Draft::from_body("", &[]);
        let left = draft.field.generation();
        host::at_revision(&draft.field, 4);
        draft.failed_send = Some(Send {
            body: "hello".into(),
        });
        draft.act("restore", "c/editor", &[]);
        assert_eq!(draft.field.text(), "hello");
        let seeded = draft.field.generation();
        assert!(seeded > left);
        let (asks, outcome) = draft.act("send", "c/editor", &[]);
        assert!(matches!(outcome, Outcome::Action(tag) if tag == "send"));
        assert_eq!(draft.submitted.take().unwrap().body, "hello");
        let [
            wire::WidgetCommand::Replace {
                generation,
                revision,
                range,
                ..
            },
        ] = asks.as_slice()
        else {
            panic!("one clear: {asks:?}")
        };
        assert_eq!((*generation, *revision, range.range()), (seeded, 4, 0..5));
        // "a" typed into the empty field as Restore was clicked: a word on
        // the document the reset left
        draft.changed(&change(left, 5, Some((0..0, 1)), "a", Vec::new()));
        assert_eq!(
            (draft.field.text().as_str(), host::revision(&draft.field)),
            ("hello", 4)
        );
        // the host adopts the seeded text: not an edit of anything
        draft.changed(&change(seeded, 6, None, "hello", Vec::new()));
        assert_eq!(draft.cleared, [wire::TextRange::from(0..5)]);
        assert_eq!((draft.body().as_str(), draft.can_send()), ("", false));
        // the clear lands
        draft.changed(&change(seeded, 7, Some((0..5, 0)), "", Vec::new()));
        assert!(draft.cleared.is_empty());
        assert_eq!(
            (draft.body().as_str(), host::revision(&draft.field)),
            ("", 7)
        );
    }

    #[test]
    fn restore_seeds_an_empty_field_only() {
        let mut draft = Draft::from_body("new typing", &[]);
        for body in ["first", "second"] {
            draft.failed(Send { body: body.into() });
        }
        assert_eq!(draft.failed_send.as_ref().unwrap().body, "first\nsecond");
        draft.act("restore", "c/editor", &[]);
        assert_eq!(draft.field.text(), "new typing");
        assert!(draft.failed_send.is_some());
        let generation = draft.field.generation();
        draft.changed(&change(generation, 9, Some((0..10, 0)), "", Vec::new()));
        draft.act("restore", "c/editor", &[]);
        assert_eq!(draft.field.text(), "first\nsecond");
        assert!(
            draft.field.generation() > generation,
            "a restore is a new document"
        );
        assert_eq!(host::revision(&draft.field), 9);
        assert!(draft.failed_send.is_none());
    }

    /// A replacement keeps the text, its mention as the account it names,
    /// and an interrupted send as one to restore.
    #[test]
    fn replacement_retains_the_bodies_of_interrupted_send_tasks() {
        let mut draft = Draft::from_body("new typing to <@7>", &roster());
        draft.in_flight.push(Send {
            body: "in flight".into(),
        });
        let mut restored: Draft =
            serde_json::from_slice(&serde_json::to_vec(&draft).unwrap()).unwrap();
        restored.retire_device_requests();
        assert_eq!(restored.field.text(), "new typing to @Ada");
        assert_eq!(restored.body(), "new typing to <@7>");
        assert_eq!(restored.failed_send.as_ref().unwrap().body, "in flight");
        assert!(restored.in_flight.is_empty());
    }

    #[test]
    fn the_menu_opens_on_a_name_being_typed_and_nowhere_else() {
        let caret = |text: &str, at: usize| {
            let draft = Draft::from_body(text, &roster());
            host::selects(&draft.field, at..at);
            draft
        };
        assert_eq!(caret("@A", 2).query(), Some((0..2, "A".into())));
        assert_eq!(
            caret("mail@A", 6).query(),
            None,
            "an address is not a mention"
        );
        assert_eq!(caret("@A b", 2).query(), Some((0..2, "A".into())));
        let selected = Draft::from_body("@A", &roster());
        host::selects(&selected.field, 0..2);
        assert_eq!(selected.query(), None, "a selection is not a caret");
        assert_eq!(
            caret("@Ab", 2).query(),
            None,
            "the caret is inside the name"
        );
        let mut dismissed = caret("@A", 2);
        dismissed.act("menu-dismiss", "c/editor", &roster());
        assert_eq!(dismissed.query(), None);
        let generation = dismissed.field.generation();
        dismissed.changed(&change(generation, 1, Some((2..2, 1)), "@Al", Vec::new()));
        assert_eq!(
            dismissed.query(),
            Some((0..3, "Al".into())),
            "typing on reopens it"
        );
    }
}
