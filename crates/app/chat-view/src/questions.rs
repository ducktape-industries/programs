//! The questions the open room stands on the node (`program.watch`): each answer lands where its
//! read would, the room reads one itself when the node can't answer it, and a block's change
//! reads again only what no answered question covers.
use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;

use chat::view::Written;
use chat::{MsgRow, PageRequest, PageResponse, ask};
use view::prelude::*;

use crate::room::{marked, room_of};
use crate::{Chat, Room, queries, words};

pub(crate) struct Question {
    standing: Standing,
    relied: Rc<Cell<bool>>,
}

impl Question {
    pub(crate) fn relied_on(&self) -> bool {
        self.standing.stands() && self.relied.get()
    }

    fn asks<A: program::Ask>(&self, ask: A) -> bool {
        self.standing.asks(ask)
    }
}

pub(crate) fn relied_on(question: &Option<Question>) -> bool {
    question.as_ref().is_some_and(Question::relied_on)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum RowsAsked {
    Tail { limit: usize, from: u64 },
    Span(u64, u64),
}

fn every() -> PageRequest {
    PageRequest {
        after: None,
        limit: None,
    }
}

fn stand<A>(
    cx: &mut Context<Chat>,
    ask: A,
    relied: bool,
    what: &'static str,
    land: impl Fn(&mut Chat, A::Reply, &mut Context<Chat>) + 'static,
    read: impl Fn(&mut Chat, &mut Context<Chat>) + 'static,
) -> Question
where
    A: program::Ask + 'static,
    A::Reply: 'static,
{
    let relied = Rc::new(Cell::new(relied));
    let relying = relied.clone();
    let standing = cx.watch(ask, move |chat, pushed, cx| match pushed {
        Pushed::Answer(reply) => {
            relying.set(true);
            chat.questions_answered = true;
            land(chat, reply, cx);
        }
        Pushed::Refused(refusal) => cx.log_refused(what, &refusal),
        Pushed::Moved | Pushed::Ended(_) if relying.get() => read(chat, cx),
        Pushed::Moved | Pushed::Ended(_) => {}
    });
    Question { standing, relied }
}

impl Chat {
    pub(crate) fn settle_questions(&mut self, cx: &mut Context<Self>) {
        self.stand_rows(cx);
        self.stand_thread(cx);
        self.stand_reactors(cx);
        self.stand_answers(cx);
        self.stand_lists(cx);
    }

    pub(crate) fn stand_drafts(&mut self, cx: &mut Context<Self>) {
        let Some(room) = &self.room else { return };
        let id = room.id.clone();
        let drafts = || ask::Drafts {
            channel_id: id.clone(),
        };
        let held = room.drafts_question.as_ref();
        let standing = held.is_some_and(|held| held.relied_on() && held.asks(drafts()));
        if standing {
            return;
        }
        let question = stand(
            cx,
            drafts(),
            true,
            words::ANSWERING_READ,
            |chat, drafts, cx| chat.answering_landed(Ok(drafts), true, cx),
            Self::read_answering,
        );
        room_of(self).drafts_question = Some(question);
    }

    pub(crate) fn stand_members(&mut self, relied: bool, cx: &mut Context<Self>) {
        let Some(room) = &self.room else { return };
        let id = room.id.clone();
        let members = ask::Members {
            channel_id: id.clone(),
            page: every(),
        };
        let question = stand(
            cx,
            members,
            relied,
            "the members",
            move |chat, members, cx| chat.members_answered(&id, members, cx),
            Self::read_members,
        );
        room_of(self).members_question = Some(question);
    }

    fn members_answered(
        &mut self,
        id: &str,
        members: PageResponse<chat::MemberRow>,
        cx: &mut Context<Self>,
    ) {
        if members.next.is_some() {
            return self.read_members(cx);
        }
        let Some(room) = self.room.as_mut().filter(|room| room.id == id) else {
            return;
        };
        if room.members.ready() != Some(&members.items) {
            room.members = Loadable::Ready(members.items);
            cx.notify();
        }
    }

    fn stand_rows(&mut self, cx: &mut Context<Self>) {
        let viewer = self.viewer();
        let screenful = self.screenful();
        let relied = self.questions_answered;
        let Some(room) = &self.room else { return };
        let Some(rows) = room.messages.ready() else {
            return;
        };
        let id = room.id.clone();
        let held = room.rows_question.as_ref();
        let asked_before = held.map(|(_, asked)| *asked);
        let asked = match (room.landed, asked_before) {
            (false, Some(tail @ RowsAsked::Tail { .. })) => tail,
            (false, _) => RowsAsked::Tail {
                limit: rows.len().max(screenful),
                from: rows.first().map_or(0, |row| row.seq),
            },
            (true, _) => match (rows.first(), rows.last()) {
                (Some(first), Some(last)) => RowsAsked::Span(first.seq, last.seq),
                _ => return,
            },
        };
        let asked_again = asked_before == Some(asked);
        let landing = id.clone();
        let question = match asked {
            RowsAsked::Tail { limit, .. } => {
                let roots = || ask::Roots {
                    channel_id: id.clone(),
                    viewer: viewer.clone(),
                    page: PageRequest {
                        after: None,
                        limit: Some(limit as u64),
                    },
                };
                let standing = asked_again && held.is_some_and(|(held, _)| held.asks(roots()));
                if standing {
                    return;
                }
                stand(
                    cx,
                    roots(),
                    relied,
                    "the room",
                    move |chat, page, cx| chat.tail_answered(&landing, page, cx),
                    Self::reread_rows,
                )
            }
            RowsAsked::Span(first, last) => {
                let around = || ask::MessagesAround {
                    channel_id: id.clone(),
                    seq: first.midpoint(last),
                    viewer: viewer.clone(),
                    page: PageRequest {
                        after: None,
                        limit: Some(last - first + 1),
                    },
                };
                let standing = asked_again && held.is_some_and(|(held, _)| held.asks(around()));
                if standing {
                    return;
                }
                stand(
                    cx,
                    around(),
                    relied,
                    "the room",
                    move |chat, rows, cx| chat.span_answered(&landing, (first, last), rows, cx),
                    Self::reread_rows,
                )
            }
        };
        room_of(self).rows_question = Some((question, asked));
    }

    fn tail_answered(&mut self, id: &str, page: PageResponse<MsgRow>, cx: &mut Context<Self>) {
        self.catch_ups_asked += 1;
        let asked = self.catch_ups_asked;
        let Some(room) = (self.room.as_mut()).filter(|room| room.id == id && !room.landed) else {
            return;
        };
        let Some((_, RowsAsked::Tail { limit, from })) = &mut room.rows_question else {
            return;
        };
        let whole = page.items.len() >= *limit;
        let more = page.next.is_some();
        let short = more && !whole;
        if short {
            return self.reread_rows(cx);
        }
        let fresh = queries::sorted(page.items);
        if let Some(oldest) = fresh.first() {
            *from = oldest.seq;
        }
        room.tail_landed(fresh, more, asked, cx);
    }

    fn span_answered(
        &mut self,
        id: &str,
        (first, last): (u64, u64),
        rows: Vec<MsgRow>,
        cx: &mut Context<Self>,
    ) {
        let Some(room) = (self.room.as_mut()).filter(|room| room.id == id && room.landed) else {
            return;
        };
        let held =
            (room.messages.ready()).and_then(|rows| Some((rows.first()?.seq, rows.last()?.seq)));
        let moved_since = held != Some((first, last));
        if moved_since {
            return;
        }
        let in_span = rows
            .into_iter()
            .filter(|row| (first..=last).contains(&row.seq));
        let rows = queries::sorted(in_span.collect());
        let moved = room.messages.ready() != Some(&rows);
        if moved {
            room.messages = Loadable::Ready(rows);
        }
        room.landed_rows(moved, cx);
    }

    fn stand_thread(&mut self, cx: &mut Context<Self>) {
        let viewer = self.viewer();
        let relied = self.questions_answered;
        let Some(room) = &self.room else { return };
        let Some(thread) = &room.thread else { return };
        let read = thread.replies.ready().is_some();
        let paging = thread.paging.is_some();
        let settled = read && !paging;
        if !settled {
            return;
        }
        let (id, root) = (room.id.clone(), thread.root);
        let replies_of = || ask::Thread {
            channel_id: id.clone(),
            root_seq: root,
            viewer: viewer.clone(),
            page: every(),
        };
        let held = thread.question.as_ref();
        let standing = held.is_some_and(|held| held.asks(replies_of()));
        if standing {
            return;
        }
        let landing = id.clone();
        let question = stand(
            cx,
            replies_of(),
            relied,
            "the thread",
            move |chat, (_, replies), cx| chat.thread_answered(&landing, root, replies, cx),
            |chat, cx| chat.reread_thread(false, cx),
        );
        if let Some(thread) = room_of(self).thread.as_mut() {
            thread.question = Some(question);
        }
    }

    fn thread_answered(
        &mut self,
        id: &str,
        root: u64,
        replies: PageResponse<MsgRow>,
        cx: &mut Context<Self>,
    ) {
        if replies.next.is_some() {
            return self.reread_thread(true, cx);
        }
        let Some(room) = self.room.as_mut().filter(|room| room.id == id) else {
            return;
        };
        let Some(thread) = room.thread.as_mut().filter(|thread| thread.root == root) else {
            return;
        };
        let fresh = queries::sorted(replies.items);
        let marks = marked(&fresh);
        let moved = thread.replies.ready() != Some(&fresh) || thread.next.is_some();
        if moved {
            thread.replies = Loadable::Ready(fresh);
            thread.has_more = false;
            thread.next = None;
            thread.earlier = false;
        }
        room.landed_some(moved, marks, cx);
    }

    fn stand_reactors(&mut self, cx: &mut Context<Self>) {
        let relied = self.questions_answered;
        let Some(room) = &mut self.room else { return };
        let seqs = room.reacted_seqs();
        if seqs.is_empty() {
            room.reactors_question = None;
            return;
        }
        let id = room.id.clone();
        let chosen = || ask::Reactors {
            channel_id: id.clone(),
            seqs: seqs.clone(),
        };
        let held = room.reactors_question.as_ref();
        let standing = held.is_some_and(|held| held.asks(chosen()));
        if standing {
            return;
        }
        let landing = id.clone();
        let question = stand(
            cx,
            chosen(),
            relied,
            "who reacted",
            move |chat, chosen, cx| chat.reactors_answered(&landing, chosen, cx),
            |chat, cx| room_of(chat).read_all_reactors(cx),
        );
        room_of(self).reactors_question = Some(question);
    }

    fn reactors_answered(
        &mut self,
        id: &str,
        chosen: Vec<chat::ReactedBy>,
        cx: &mut Context<Self>,
    ) {
        let Some(room) = self.room.as_mut().filter(|room| room.id == id) else {
            return;
        };
        room.reading_reactors = None;
        let reactors = queries::reactors_of(chosen);
        if reactors != room.reactors {
            room.reactors = reactors;
            cx.notify();
        }
    }

    fn stand_answers(&mut self, cx: &mut Context<Self>) {
        let relied = self.questions_answered;
        let Some(room) = &mut self.room else { return };
        let asked = crate::asks::asked(room.messages.ready().into_iter().flatten());
        let roots = crate::asks::put_to(asked, room.reader);
        if roots.is_empty() {
            room.answers_question = None;
            return;
        }
        let id = room.id.clone();
        let repliers = || ask::Repliers {
            channel_id: id.clone(),
            roots: roots.clone(),
        };
        let held = room.answers_question.as_ref();
        let standing = held.is_some_and(|held| held.asks(repliers()));
        if standing {
            return;
        }
        let landing = id.clone();
        let question = stand(
            cx,
            repliers(),
            relied,
            "who answered",
            move |chat, threads, cx| chat.answers_answered(&landing, threads, cx),
            |chat, cx| room_of(chat).read_answers(cx),
        );
        room_of(self).answers_question = Some(question);
    }

    fn answers_answered(
        &mut self,
        id: &str,
        threads: Vec<(u64, Vec<MsgRow>)>,
        cx: &mut Context<Self>,
    ) {
        let Some(room) = self.room.as_mut().filter(|room| room.id == id) else {
            return;
        };
        let mut answers = room.answers.clone();
        answers.extend(threads);
        if answers != room.answers {
            room.answers = answers;
            cx.notify();
        }
    }

    fn stand_lists(&mut self, cx: &mut Context<Self>) {
        let Some(me) = self.my_account() else { return };
        if !self.listening_asked {
            return;
        }
        let relied = self.questions_answered;
        let listening = || ask::Listening {
            account: me,
            page: every(),
        };
        let held = self.listening_question.as_ref();
        if !held.is_some_and(|held| held.asks(listening())) {
            let question = stand(
                cx,
                listening(),
                relied,
                "the rooms listened to",
                move |chat, rooms, cx| chat.listening_answered(me, rooms, cx),
                Self::read_listening,
            );
            self.listening_question = Some(question);
        }
        let posted = || ask::PostedIn {
            account: me,
            page: every(),
        };
        let held = self.posted_question.as_ref();
        if !held.is_some_and(|held| held.asks(posted())) {
            let question = stand(
                cx,
                posted(),
                relied,
                "the rooms written in",
                move |chat, rooms, cx| chat.posted_answered(me, rooms, cx),
                Self::read_posted,
            );
            self.posted_question = Some(question);
        }
    }

    fn listening_answered(&mut self, me: u64, rooms: PageResponse<String>, cx: &mut Context<Self>) {
        let theirs = self.my_account() == Some(me);
        if !theirs {
            return;
        }
        if rooms.next.is_some() {
            return self.read_listening(cx);
        }
        let rooms: BTreeSet<String> = rooms.items.into_iter().collect();
        if rooms != self.listening {
            self.listening = rooms;
            cx.notify();
        }
    }

    fn posted_answered(&mut self, me: u64, rooms: PageResponse<String>, cx: &mut Context<Self>) {
        let theirs = self.my_account() == Some(me);
        if !theirs {
            return;
        }
        if rooms.next.is_some() {
            return self.read_posted(cx);
        }
        let rooms: BTreeSet<String> = rooms.items.into_iter().collect();
        if rooms != self.posted {
            self.posted = rooms;
            cx.notify();
        }
    }

    pub(crate) fn reread_lists(&mut self, lists: [(bool, bool); 2], cx: &mut Context<Self>) {
        let any_moved = lists.iter().any(|(moved, _)| *moved);
        let any_answered = lists.iter().any(|(_, answered)| *answered);
        let all_again = any_moved && !any_answered;
        if all_again {
            return self.load_listening(cx);
        }
        if !any_answered {
            return;
        }
        let [listening, posted] = lists.map(|(moved, answered)| moved && !answered);
        if listening {
            self.read_listening(cx);
        }
        if posted {
            self.read_posted(cx);
        }
    }
}

impl Room {
    pub(crate) fn rows_relied_on(&self) -> bool {
        let Some((question, asked)) = &self.rows_question else {
            return false;
        };
        let tail = matches!(asked, RowsAsked::Tail { .. });
        let fits = tail != self.landed;
        fits && question.relied_on()
    }

    pub(crate) fn uncovered(&self, mut written: Written, rows: bool, thread: bool) -> Written {
        let from = match &self.rows_question {
            Some((_, RowsAsked::Tail { from, .. })) => *from,
            _ => 0,
        };
        let replies = self.thread.as_ref().and_then(|open| open.replies.ready());
        let threaded: BTreeSet<u64> = replies.into_iter().flatten().map(|row| row.seq).collect();
        written.seqs.retain(|seq| match threaded.contains(seq) {
            true => !thread,
            false => !rows || *seq < from,
        });
        written.roots &= !rows;
        written
    }
}
