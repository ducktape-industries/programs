//! A pane's list as both designs keep it (the desktop's grid and the
//! phone's rows draw over it): which rows it holds and under which keys,
//! the list state those keys splice, where a landed room opens, and the
//! intro, closing line and unread marker around the rows.
use std::collections::HashMap;
use std::ops::Range;

use view::prelude::*;

use crate::message::{ChatMessage, unread_seq};
use crate::{Chat, Drawn, Pane, Shape};

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
    fn follow(self, whole: bool) -> FollowMode {
        match (self, whole) {
            (Self::Tail, _) | (Self::Thread, true) => FollowMode::Tail,
            (Self::Landed, _) | (Self::Thread, false) => FollowMode::Normal,
        }
    }
}

pub(crate) struct Listing {
    pub shape: Shape,
    pub intro: Option<String>,
    pub bare: bool,
    pub state: ListState,
    pub unread: Option<u64>,
    // who is answering in the pane, after its rows (`Chat::answering`)
    pub answering: Vec<chat::Draft>,
    pub busy: Option<chat::Holder>,
    pub held_draft: Option<chat::Holder>,
}

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
    let answering = chat.answering(pane);
    let (held_draft, busy) = match chat.busy(pane) {
        Some(holder) if drawn_in(&answering, &holder) => (Some(holder), None),
        busy => (None, busy),
    };
    let keys = lead
        .then_some("intro".to_owned())
        .into_iter()
        .chain(messages.iter().map(|message| message.id.clone()))
        .chain(bare.then_some("no-replies".to_owned()))
        .chain(answering.iter().map(answering_key))
        .chain(busy.as_ref().map(|_| "busy".to_owned()))
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
        answering,
        busy,
        held_draft,
    }
}

fn drawn_in(answering: &[chat::Draft], holder: &chat::Holder) -> bool {
    let a_draft = holder.kind == chat::Holding::Draft;
    a_draft
        && answering
            .iter()
            .any(|draft| draft.author == holder.principal)
}

// a draft's row key, by its author: one draft per author in a room
pub(crate) fn answering_key(draft: &chat::Draft) -> String {
    match &draft.author {
        chat::Principal::Account(number) => format!("answering-{number}"),
        chat::Principal::Root => "answering-system".to_owned(),
    }
}

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

fn thread_landing(chat: &Chat, messages: &[ChatMessage]) -> Option<usize> {
    let target = chat.room.as_ref()?.thread.as_ref()?.target;
    let at = messages.iter().position(|message| message.seq >= target);
    at.filter(|_| target > 0)
}

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
        restore(&mut drawn, &state, keys);
        return state;
    }
    let state = ListState::new(keys.len(), shape.alignment(), px(OVERDRAW));
    state.set_follow_mode(shape.follow(whole));
    if let Some(item_ix) = landing() {
        state.scroll_to(at(item_ix));
    }
    if std::mem::take(&mut drawn.follow) {
        state.scroll_to_end();
    }
    restore(&mut drawn, &state, keys);
    drawn.whole = whole;
    drawn.list = Some((shape, state.clone()));
    drawn.keys = keys.to_vec();
    state
}

fn restore(drawn: &mut Drawn, state: &ListState, keys: &[String]) {
    let held = drawn.restore.as_ref();
    let Some(item_ix) = held.and_then(|(id, _)| keys.iter().position(|key| key == id)) else {
        return;
    };
    let Some((_, offset)) = drawn.restore.take() else {
        return;
    };
    state.scroll_to(ListOffset {
        item_ix,
        offset_in_item: px(offset),
    });
}

pub(crate) fn first_row(chat: &Chat, pane: Pane) -> String {
    let room = chat.room.as_ref();
    let oldest = match pane {
        Pane::Timeline => room.and_then(|room| room.messages.ready()?.first()),
        Pane::Thread => room.and_then(|room| room.thread.as_ref()?.replies.ready()?.first()),
    };
    format!("chat-first-row-{}", oldest.map_or(0, |row| row.seq))
}

pub(crate) fn edged(
    chat: &Chat,
    pane: Pane,
    at: usize,
    id: &str,
    row: AnyElement,
    cx: &mut Context<Chat>,
) -> AnyElement {
    if !chat.before_first(pane, at) {
        return row;
    }
    let seen = cx.listener(move |chat, _: &Bounds<Pixels>, _window, cx| {
        chat.reached_first(pane, cx);
    });
    let sensor = sensor(first_row(chat, pane), div().size_full())
        .absolute()
        .inset_0()
        .on_bounds(seen);
    div()
        .id(format!("chat-message-{id}-first"))
        .relative()
        .w_full()
        .flex()
        .flex_col()
        .child(sensor)
        .child(row)
        .into_any_element()
}

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
