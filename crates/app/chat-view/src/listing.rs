//! A pane's list as both designs keep it (the desktop's grid and the
//! phone's rows draw over it): which rows it holds and under which keys,
//! the list state those keys splice, where a landed room opens, and the
//! intro, closing line and unread marker around the rows.
use std::collections::HashMap;
use std::ops::Range;

use view::prelude::*;

use crate::message::{ChatMessage, unread_seq};
use crate::{Chat, Pane, Shape};

pub(crate) const OVERDRAW: f32 = 600.;

impl Shape {
    fn of(chat: &Chat, pane: Pane) -> Self {
        let landed = chat.room.as_ref().is_some_and(|room| room.landed);
        match (pane, landed) {
            (Pane::Thread, _) => Self::Thread,
            (Pane::Timeline, true) => Self::Landed,
            (Pane::Timeline, false) => Self::Tail,
        }
    }
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Tail => "chat-message-rows",
            Self::Landed => "chat-landed-rows",
            Self::Thread => "chat-thread-rows",
        }
    }
    fn alignment(self) -> ListAlignment {
        match self {
            Self::Tail => ListAlignment::Bottom,
            Self::Landed | Self::Thread => ListAlignment::Top,
        }
    }
    // A room at its head follows its end, and a thread once every reply is
    // in (`whole`): gpui follows while the reader is at the end, by where
    // the list stands at each layout, and stops when they scroll up. A
    // landing, or a thread with pages left, has no end on screen to follow.
    fn follow(self, whole: bool) -> FollowMode {
        match (self, whole) {
            (Self::Tail, _) | (Self::Thread, true) => FollowMode::Tail,
            (Self::Landed, _) | (Self::Thread, false) => FollowMode::Normal,
        }
    }
}

/// A pane's list as both designs draw over it: the list's state under the
/// rows' keys, the room's intro leading them once its history is all
/// here, whether "No replies yet" closes a thread that is only its root,
/// and the first unread message.
pub(crate) struct Listing {
    pub shape: Shape,
    pub intro: Option<String>,
    pub bare: bool,
    pub state: ListState,
    pub unread: Option<u64>,
}

/// The list of `pane`'s `messages`, its state spliced to their keys and
/// its scroll heard by the model.
pub(crate) fn listing(
    chat: &Chat,
    pane: Pane,
    messages: &[ChatMessage],
    cx: &mut Context<Chat>,
) -> Listing {
    let room = chat.room.as_ref();
    let shape = Shape::of(chat, pane);
    let lead = pane == Pane::Timeline && room.is_some_and(|room| !room.has_older);
    let intro = lead.then(|| chat.intro()).flatten();
    let bare = pane == Pane::Thread
        && messages.len() == 1
        && room
            .and_then(|room| room.thread.as_ref())
            .is_some_and(|thread| thread.replies.ready().is_some_and(Vec::is_empty));
    let keys = lead
        .then_some("intro".to_owned())
        .into_iter()
        .chain(messages.iter().map(|message| message.id.clone()))
        .chain(bare.then_some("no-replies".to_owned()))
        .collect::<Vec<_>>();
    let landing = || match shape {
        Shape::Landed => Some(landing(chat, messages) + usize::from(lead)),
        Shape::Thread => thread_landing(chat, messages),
        Shape::Tail => None,
    };
    let whole = room
        .and_then(|room| room.thread.as_ref())
        .is_some_and(|thread| !thread.has_more && thread.replies.ready().is_some());
    let state = list_state(chat, pane, (shape, whole), &keys, landing);
    state.set_scroll_handler(cx.listener(move |chat, event, _window, cx| {
        chat.list_scrolled(pane, event, cx);
    }));
    let unread = unread_seq(
        messages,
        (pane == Pane::Timeline).then_some(chat.reads.boundary),
    );
    Listing {
        shape,
        intro,
        bare,
        state,
        unread,
    }
}

/// Where a landed room opens: the row of the message it landed on (a
/// reply's, its root's).
pub(crate) fn landing(chat: &Chat, messages: &[ChatMessage]) -> usize {
    let Some(room) = chat.room.as_ref().filter(|room| room.landed) else {
        return 0;
    };
    let target = room
        .messages
        .ready()
        .and_then(|rows| rows.iter().find(|row| row.seq == room.target))
        .and_then(|row| row.thread)
        .unwrap_or(room.target);
    messages
        .iter()
        .position(|message| message.seq >= target)
        .unwrap_or(messages.len().saturating_sub(1))
}

// Where a thread opened on a reply opens: that reply's row, once it is read.
fn thread_landing(chat: &Chat, messages: &[ChatMessage]) -> Option<usize> {
    let target = chat.room.as_ref()?.thread.as_ref()?.target;
    let at = messages.iter().position(|message| message.seq >= target);
    at.filter(|_| target > 0)
}

// `whole`: every reply of the thread is in.
fn list_state(
    chat: &Chat,
    pane: Pane,
    (shape, whole): (Shape, bool),
    keys: &[String],
    landing: impl FnOnce() -> Option<usize>,
) -> ListState {
    let mut drawn = chat.drawn(pane).borrow_mut();
    let at = |item_ix| ListOffset {
        item_ix,
        offset_in_item: px(0.),
    };
    if let Some((held, state)) = drawn.list.clone()
        && held == shape
    {
        for (old, count) in splices(&drawn.keys, keys).into_iter().rev() {
            state.splice(old, count);
        }
        if drawn.keys != keys {
            drawn.keys = keys.to_vec();
        }
        // the thread's last page in: it follows from here on, from where
        // the reader is (gpui puts a list that starts following at its end)
        if shape == Shape::Thread && drawn.whole != whole {
            drawn.whole = whole;
            state.set_follow_mode(shape.follow(whole));
            if whole {
                state.scroll_to(state.logical_scroll_top());
            }
        }
        if std::mem::take(&mut drawn.follow) {
            state.scroll_to_end();
        }
        return state;
    }
    let state = ListState::new(keys.len(), shape.alignment(), px(OVERDRAW));
    state.set_follow_mode(shape.follow(whole));
    match landing() {
        Some(item_ix) => state.scroll_to(at(item_ix)),
        // a whole thread opens on its root, and follows once the reader
        // reaches its end
        None if shape == Shape::Thread && whole => state.scroll_to(at(0)),
        None => {}
    }
    if std::mem::take(&mut drawn.follow) {
        state.scroll_to_end();
    }
    drawn.whole = whole;
    drawn.list = Some((shape, state.clone()));
    drawn.keys = keys.to_vec();
    state
}

/// The splices that turn the list of `old` keys into `new`'s, last first
/// applied: each the range of old rows it replaces and how many new ones.
pub(crate) fn splices(old: &[String], new: &[String]) -> Vec<(Range<usize>, usize)> {
    let mut edits = Vec::new();
    diff(old, new, 0, &mut edits);
    edits
}

fn common<'a>(
    old: impl Iterator<Item = &'a String>,
    new: impl Iterator<Item = &'a String>,
) -> usize {
    old.zip(new).take_while(|(a, b)| a == b).count()
}

fn diff(old: &[String], new: &[String], at: usize, edits: &mut Vec<(Range<usize>, usize)>) {
    let prefix = common(old.iter(), new.iter());
    let (old, new) = (&old[prefix..], &new[prefix..]);
    let suffix = common(old.iter().rev(), new.iter().rev());
    let (old, new) = (&old[..old.len() - suffix], &new[..new.len() - suffix]);
    let at = at + prefix;
    let unchanged = old.is_empty() && new.is_empty();
    if unchanged {
        return;
    }
    let Some((i, j, len)) = longest_run(old, new) else {
        edits.push((at..at + old.len(), new.len()));
        return;
    };
    diff(&old[..i], &new[..j], at, edits);
    diff(&old[i + len..], &new[j + len..], at + i + len, edits);
}

fn longest_run(old: &[String], new: &[String]) -> Option<(usize, usize, usize)> {
    let at: HashMap<&str, usize> = old
        .iter()
        .enumerate()
        .map(|(index, key)| (key.as_str(), index))
        .collect();
    let mut longest: Option<(usize, usize, usize)> = None;
    let mut j = 0;
    while j < new.len() {
        let Some(&i) = at.get(new[j].as_str()) else {
            j += 1;
            continue;
        };
        let len = common(old[i..].iter(), new[j..].iter());
        if longest.is_none_or(|(_, _, most)| len > most) {
            longest = Some((i, j, len));
        }
        j += len;
    }
    longest
}
