//! Find in the open room (Cmd/Ctrl+F): the loaded rows holding every word, then the room's older
//! matches a page at a time from chat's `text_search`, each landed on as a link lands.
use std::collections::BTreeSet;
use std::ops::Range;

use view::prelude::*;
use view::{ElementId, HighlightStyle, Keystroke, TextField, UnderlineStyle, Window};

use crate::composer::Target;
use crate::message::ChatMessage;
use crate::queries::text_hits;
use crate::{Chat, Pane};

pub(crate) const FIELD: &str = "chat-find";

/// The find bar over the open room.
#[derive(Default)]
pub(crate) struct Find {
    pub(crate) draft: TextField,
    pub(crate) terms: Vec<String>,
    pub(crate) current: Option<u64>,
    pub(crate) hits: BTreeSet<u64>,
    pub(crate) next_page: Option<Vec<u8>>,
    pub(crate) no_older_page: bool,
    pub(crate) asking: Option<view::Task<()>>,
    pub(crate) refusal: Option<String>,
}

pub(crate) fn asks_to_find(keystroke: &Keystroke) -> bool {
    let held = &keystroke.modifiers;
    keystroke.key == "f" && held.platform != held.control && !held.shift && !held.alt
}

pub(crate) fn holds(blocks: &[chat::Block], terms: &[String]) -> bool {
    let folded = chat::search_terms(&chat::plain_text(blocks)).join(" ");
    !terms.is_empty() && terms.iter().all(|term| folded.contains(term.as_str()))
}

pub(crate) fn found_ranges(text: &str, terms: &[String]) -> Vec<Range<usize>> {
    let mut folded = String::new();
    let mut origin: Vec<Range<usize>> = Vec::new();
    for (at, character) in text.char_indices() {
        folded.extend(character.to_lowercase());
        origin.resize(folded.len(), at..at + character.len_utf8());
    }
    let mut found: Vec<Range<usize>> = terms
        .iter()
        .filter(|term| !term.is_empty())
        .flat_map(|term| {
            let origin = &origin;
            folded
                .match_indices(term.as_str())
                .map(move |(at, term)| origin[at].start..origin[at + term.len() - 1].end)
        })
        .collect();
    found.sort_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in found {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

/// The words a message's lines mark, and how.
#[derive(Default)]
pub(crate) struct Found {
    terms: Vec<String>,
    mark: HighlightStyle,
}

impl Found {
    pub(crate) fn of(chat: &Chat, message: &ChatMessage, theme: &Theme) -> Self {
        let terms = chat.find_terms();
        let holds_every = !message.deleted && holds(&message.blocks, terms);
        let current = message.seq > 0 && chat.find_current() == Some(message.seq);
        let underline = UnderlineStyle {
            thickness: px(1.),
            color: Some(theme.warning),
            wavy: false,
        };
        Self {
            terms: match holds_every {
                true => terms.to_vec(),
                false => Vec::new(),
            },
            mark: HighlightStyle {
                background_color: Some(theme.warning_soft),
                underline: current.then_some(underline),
                ..HighlightStyle::default()
            },
        }
    }

    pub(crate) fn marks(
        &self,
        text: &str,
        runs: Vec<(Range<usize>, HighlightStyle)>,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let found = found_ranges(text, &self.terms);
        if found.is_empty() {
            return runs;
        }
        if runs.is_empty() {
            return found.into_iter().map(|range| (range, self.mark)).collect();
        }
        let mut marked = Vec::new();
        for (range, style) in runs {
            let mut at = range.start;
            let inside = found
                .iter()
                .filter(|hit| hit.start < range.end && hit.end > range.start);
            for hit in inside {
                let (start, end) = (hit.start.max(at), hit.end.min(range.end));
                if at < start {
                    marked.push((at..start, style));
                }
                let mark = HighlightStyle {
                    background_color: self.mark.background_color,
                    underline: self.mark.underline.or(style.underline),
                    ..style
                };
                marked.push((start..end, mark));
                at = end;
            }
            if at < range.end {
                marked.push((at..range.end, style));
            }
        }
        marked
    }
}

impl Chat {
    pub(crate) fn find_terms(&self) -> &[String] {
        self.find.as_ref().map_or(&[], |find| &find.terms)
    }

    pub(crate) fn find_current(&self) -> Option<u64> {
        self.find.as_ref()?.current
    }

    pub(crate) fn find_open(&mut self, window: &mut Window) {
        self.find.get_or_insert_with(Find::default);
        window.focus(ElementId::Name(FIELD.into()));
    }

    pub(crate) fn find_close(&mut self, window: &mut Window) {
        self.find = None;
        let Some(room) = &self.room else {
            return;
        };
        let target = Target::Post {
            channel: room.id.clone(),
            thread: None,
        };
        crate::composer::focus(window, &target.key());
    }

    pub(crate) fn find_changed(&mut self, cx: &mut Context<Self>) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        let terms = chat::search_terms(&find.draft.text());
        if terms == find.terms {
            return;
        }
        *find = Find {
            draft: std::mem::take(&mut find.draft),
            terms,
            ..Find::default()
        };
        if let Some(&newest) = self.find_matches().last() {
            self.find_land(newest, cx);
        }
    }

    fn find_matches(&self) -> BTreeSet<u64> {
        let Some(find) = &self.find else {
            return BTreeSet::new();
        };
        if find.terms.is_empty() {
            return BTreeSet::new();
        }
        let loaded = self.shown(Pane::Timeline).chain(self.shown(Pane::Thread));
        loaded
            .filter(|row| row.seq > 0 && !row.deleted && holds(&row.blocks, &find.terms))
            .map(|row| row.seq)
            .chain(find.hits.iter().copied())
            .collect()
    }

    pub(crate) fn find_step(&mut self, older: bool, cx: &mut Context<Self>) {
        let matches = self.find_matches();
        let Some(find) = self.find.as_ref() else {
            return;
        };
        let next = match (older, find.current) {
            (true, Some(at)) => matches.range(..at).next_back(),
            (true, None) => matches.last(),
            (false, Some(at)) => matches.range(at + 1..).next(),
            (false, None) => None,
        };
        let further = older && !find.no_older_page && !find.terms.is_empty();
        match next.copied() {
            Some(seq) => self.find_land(seq, cx),
            None if further => self.find_further(cx),
            None => {}
        }
    }

    fn find_further(&mut self, cx: &mut Context<Self>) {
        let (viewer, screenful) = (self.viewer(), self.screenful());
        let Some(room) = self.room.as_ref().map(|room| room.id.clone()) else {
            return;
        };
        let Some(find) = self.find.as_mut() else {
            return;
        };
        if find.asking.is_some() {
            return;
        }
        let text = find.terms.join(" ");
        let asked = text_hits(
            cx.host(),
            text,
            room,
            viewer,
            find.next_page.clone(),
            screenful,
        );
        find.refusal = None;
        find.asking = Some(cx.land(asked, |chat, result, cx| {
            cx.notify();
            let Some(find) = chat.find.as_mut() else {
                return;
            };
            find.asking = None;
            match result {
                Ok((rows, next)) => {
                    find.no_older_page = next.is_none();
                    find.next_page = next;
                    let hits = rows.iter().filter(|row| !row.deleted);
                    find.hits.extend(hits.map(|row| row.seq));
                    chat.find_step(true, cx);
                }
                Err(refusal) => find.refusal = Some(design::sentence(&refusal)),
            }
        }));
    }

    fn find_land(&mut self, seq: u64, cx: &mut Context<Self>) {
        if let Some(find) = self.find.as_mut() {
            find.current = Some(seq);
        }
        for pane in [Pane::Timeline, Pane::Thread] {
            let id = self
                .shown(pane)
                .find(|row| row.seq == seq)
                .map(|row| row.message_id.clone());
            let Some(id) = id else {
                continue;
            };
            let key = self.key_of(&id);
            let cursor = self.cursor_mut(pane);
            cursor.id = Some(id);
            cursor.cell = 0;
            self.drawn(pane).borrow_mut().reveal = Some(key);
            return;
        }
        if let Some(room) = self.room.as_ref().map(|room| room.id.clone()) {
            self.open_at(room, seq, cx);
        }
    }

    pub(crate) fn find_said(&self) -> Option<String> {
        let find = self.find.as_ref()?;
        if find.terms.is_empty() {
            return None;
        }
        if let Some(refusal) = &find.refusal {
            return Some(format!("Couldn’t search further: {refusal}"));
        }
        if find.asking.is_some() {
            return Some("Searching…".to_owned());
        }
        let matches = self.find_matches();
        let more = match find.no_older_page {
            true => "",
            false => "+",
        };
        Some(match find.current {
            Some(at) => format!("{} of {}{more}", matches.range(at..).count(), matches.len()),
            None if find.no_older_page => "No matches".to_owned(),
            None => "None loaded · Enter searches further back".to_owned(),
        })
    }
}
