//! The reader's own typing, told to the room as chat's empty draft: a keystroke that takes a
//! composer from empty to words begins it, and an event ends it (a send, an emptied composer,
//! leaving, the next start); whoever else holds a place is named with the block they began at.
use super::*;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;
use view::methods::{self, StoreGet, StoreSet};

const TYPED: &str = "reads/0102/typing";

#[derive(Debug, PartialEq)]
enum Told {
    Typing(String, Option<u64>),
    Undraft(String),
    Posted(String),
    Broke(String, Option<u64>),
}

fn typing(channel: &str, thread: Option<u64>) -> Told {
    Told::Typing(channel.into(), thread)
}

fn undraft(channel: &str) -> Told {
    Told::Undraft(channel.into())
}

fn told(cx: &TestAppContext) -> Vec<Told> {
    let ops = cx.host().requests::<Submit<::chat::Chat>>();
    ops.into_iter()
        .filter_map(|op| match op {
            Op::Drafting {
                channel_id,
                thread,
                text,
                anew,
                doing,
            } => {
                assert!(
                    text.is_empty() && anew && doing.as_deref() == Some("typing"),
                    "a person's draft says typing alone: {text:?} {anew} {doing:?}"
                );
                Some(Told::Typing(channel_id, thread))
            }
            Op::Undraft { channel_id } => Some(Told::Undraft(channel_id)),
            Op::PostMessage { channel_id, .. } => Some(Told::Posted(channel_id)),
            Op::Break { channel_id, thread } => Some(Told::Broke(channel_id, thread)),
            _ => None,
        })
        .collect()
}

fn typed_in(cx: &TestAppContext) -> Option<BTreeSet<String>> {
    let sets = cx.host().requests::<StoreSet>();
    let (_, value) = sets.into_iter().rfind(|(key, _)| key == TYPED)?;
    Some(value.map_or_else(BTreeSet::new, |value| methods::decode(&value).unwrap()))
}

fn rooms(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn started_with(left: BTreeSet<String>) -> TestAppContext {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host()
        .handle::<StoreGet>(move |key| Ok((key == TYPED).then(|| methods::encode(&left))));
    let _view = cx.open::<Chat>();
    seat(&mut cx);
    cx.run_until_parked();
    cx
}

#[test]
fn typing_tells_the_room_once_and_emptying_the_composer_takes_it_down() {
    let (mut cx, _view) = opened();
    cx.simulate_input("draft-general/editor", "h");
    cx.run_until_parked();
    assert_eq!(told(&cx), vec![typing("general", None)]);
    assert_eq!(typed_in(&cx), Some(rooms(&["general"])));
    cx.simulate_input("draft-general/editor", "hello");
    cx.run_until_parked();
    assert_eq!(
        told(&cx),
        vec![typing("general", None)],
        "once, not per key"
    );
    cx.simulate_input("draft-general/editor", "");
    cx.run_until_parked();
    assert_eq!(told(&cx), vec![typing("general", None), undraft("general")]);
    assert_eq!(typed_in(&cx), Some(rooms(&[])));
}

#[test]
fn a_send_ends_the_typing_with_its_post_and_the_next_words_begin_it_again() {
    let (mut cx, _view) = opened();
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    cx.simulate_focus("draft-general/editor");
    cx.simulate_input("draft-general/editor", "mine");
    cx.simulate_field_key("draft-general/editor", "enter");
    cx.run_until_parked();
    assert_eq!(
        told(&cx),
        vec![typing("general", None), Told::Posted("general".into())],
        "the post takes the draft down itself"
    );
    cx.simulate_input("draft-general/editor", "next");
    cx.run_until_parked();
    assert_eq!(
        told(&cx),
        vec![
            typing("general", None),
            Told::Posted("general".into()),
            typing("general", None)
        ]
    );
}

#[test]
fn leaving_the_room_takes_the_typing_down_and_coming_back_to_its_words_sends_nothing() {
    let (mut cx, view) = opened();
    cx.simulate_input("draft-general/editor", "half a thought");
    cx.run_until_parked();
    cx.simulate_click("chat-sidebar-dm-8");
    cx.run_until_parked();
    view.read(|chat| assert_eq!(chat.room.as_ref().unwrap().id, "dm-7-8"));
    assert_eq!(told(&cx), vec![typing("general", None), undraft("general")]);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    view.read(|chat| assert_eq!(chat.drafts["draft-general"].body(), "half a thought"));
    assert_eq!(
        told(&cx),
        vec![typing("general", None), undraft("general")],
        "words merely there are no keystroke"
    );
}

#[test]
fn a_view_restored_with_words_in_its_composer_sends_nothing() {
    let (mut cx, _view) = opened();
    cx.simulate_input("draft-general/editor", "saved words");
    cx.run_until_parked();
    let (restored, view) = restored(&cx);
    view.read(|chat| assert_eq!(chat.drafts["draft-general"].body(), "saved words"));
    assert_eq!(told(&restored), vec![]);
}

#[test]
fn the_composer_typed_into_last_is_where_the_reader_types() {
    let (mut cx, view) = opened();
    thread(&mut cx, &view, 2);
    cx.simulate_input("draft-general/2/editor", "a reply");
    cx.run_until_parked();
    assert_eq!(told(&cx), vec![typing("general", Some(2))]);
    cx.simulate_input("draft-general/editor", "and a post");
    cx.run_until_parked();
    assert_eq!(
        told(&cx),
        vec![typing("general", Some(2)), typing("general", None)]
    );
    cx.simulate_input("draft-general/editor", "");
    cx.run_until_parked();
    let ended = vec![
        typing("general", Some(2)),
        typing("general", None),
        undraft("general"),
    ];
    assert_eq!(told(&cx), ended, "the thread's words are no keystroke");
    cx.update(&view, |chat, _, cx| {
        chat.close_thread();
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(told(&cx), ended);
}

#[test]
fn at_start_every_room_left_typed_in_is_taken_down() {
    let cx = started_with(rooms(&["general", "dm-7-8"]));
    let mut said = told(&cx);
    said.sort_by_key(|told| format!("{told:?}"));
    assert_eq!(said, vec![undraft("dm-7-8"), undraft("general")]);
    assert_eq!(typed_in(&cx), Some(rooms(&[])));
}

#[test]
fn at_start_a_room_typed_in_this_session_stays_typed_in() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host().never::<StoreGet>();
    let _view = cx.open::<Chat>();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    cx.simulate_input("draft-general/editor", "still here");
    cx.run_until_parked();
    assert_eq!(told(&cx), vec![typing("general", None)]);
    cx.host().handle::<StoreGet>(|key| {
        Ok((key == TYPED).then(|| methods::encode(&rooms(&["general", "dm-7-8"]))))
    });
    cx.host().answer_held::<StoreGet>();
    cx.run_until_parked();
    assert_eq!(told(&cx), vec![typing("general", None), undraft("dm-7-8")]);
    assert_eq!(typed_in(&cx), Some(rooms(&["general"])));
}

#[test]
fn a_refused_undraft_keeps_its_room_and_the_next_start_takes_it_down() {
    let (mut cx, _view) = opened();
    cx.simulate_input("draft-general/editor", "h");
    cx.run_until_parked();
    cx.host()
        .refuse::<Submit<::chat::Chat>>("unavailable", "offline");
    cx.simulate_input("draft-general/editor", "");
    cx.run_until_parked();
    assert_eq!(told(&cx), vec![typing("general", None), undraft("general")]);
    let kept = typed_in(&cx).expect("the typing was kept");
    assert_eq!(kept, rooms(&["general"]), "kept until chat takes it down");
    let next = started_with(kept);
    assert_eq!(told(&next), vec![undraft("general")]);
    assert_eq!(typed_in(&next), Some(rooms(&[])));
}

fn a_draft(author: u64, thread: Option<u64>, doing: &str, text: &str) -> chat::Draft {
    chat::Draft {
        author: Principal::Account(author),
        thread,
        height: 3,
        time: 0,
        doing: doing.into(),
        text: text.into(),
    }
}

#[test]
fn the_readers_own_draft_is_never_drawn_back_to_them() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let held = vec![
        a_draft(7, None, "typing", ""),
        a_draft(8, None, "", "Looking at the build"),
    ];
    let mut room = rooms_alone(|query| Ok(chat_reply(query)));
    cx.host()
        .handle::<Ask<::chat::Chat>>(move |query| match query {
            Query::Drafts { .. } => Ok(Reply::Drafts(held.clone())),
            query => room(query),
        });
    let _view = cx.open::<Chat>();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    assert!(cx.find("chat-answering-8").is_some(), "{:?}", cx.texts());
    assert!(cx.find("chat-answering-7").is_none(), "{:?}", cx.texts());
}

fn holder(principal: u64, kind: chat::Holding, height: u64) -> chat::Holder {
    chat::Holder {
        principal: Principal::Account(principal),
        kind,
        height,
    }
}

type Held = Rc<RefCell<Vec<(Option<u64>, chat::Holder)>>>;

fn busy_room(cx: &TestAppContext, owner: u64, busy: Held) {
    let mut room = rooms_alone(move |query| {
        Ok(match query {
            Query::Channels { .. } => {
                let mut general = channel("general", "General", 3);
                general.channel.owner = Principal::Account(owner);
                Reply::Channels(page(vec![general, channel("dm-7-8", "dm", 1)]))
            }
            query => chat_reply(query),
        })
    });
    cx.host()
        .handle::<Ask<::chat::Chat>>(move |query| match query {
            Query::Busy {
                channel_id,
                thread,
                for_account,
            } => {
                assert_eq!((channel_id.as_str(), for_account), ("general", 7));
                let held = busy.borrow();
                let held = held.iter().find(|(at, _)| *at == thread);
                Ok(Reply::Busy(held.map(|(_, holder)| holder.clone())))
            }
            Query::Drafts { .. } => {
                let held = busy.borrow();
                let drafted = held.iter().filter(|(_, h)| h.kind == chat::Holding::Draft);
                let drafts = drafted.map(|(thread, holder)| chat::Draft {
                    author: holder.principal.clone(),
                    thread: *thread,
                    height: holder.height,
                    time: 0,
                    doing: "typing".into(),
                    text: String::new(),
                });
                Ok(Reply::Drafts(drafts.collect()))
            }
            query => room(query),
        });
}

fn drawn(cx: &TestAppContext, text: &str) -> usize {
    cx.texts().iter().filter(|said| *said == text).count()
}

#[test]
fn a_held_foot_says_who_holds_it_since_when_and_its_writer_breaks_it() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let held = Rc::new(RefCell::new(vec![(
        None,
        holder(8, chat::Holding::Lease, 42),
    )]));
    busy_room(&cx, 7, held.clone());
    let _view = cx.open::<Chat>();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    let asked = cx.host().requests::<Ask<::chat::Chat>>();
    let foot = Query::Busy {
        channel_id: "general".into(),
        thread: None,
        for_account: 7,
    };
    assert!(asked.contains(&foot), "{asked:?}");
    assert!(
        cx.has_text("reviewer is answering here since block 42"),
        "{:?}",
        cx.texts()
    );
    let line = cx.interactivity("chat-busy-timeline");
    assert_eq!(line.role, Some(view::Role::Status));
    cx.simulate_click("chat-busy-timeline-break");
    cx.run_until_parked();
    assert_eq!(told(&cx), vec![Told::Broke("general".into(), None)]);
    held.borrow_mut().clear();
    let ended = chat::tables::LEASES.key(&("general".to_owned(), None::<u64>));
    cx.host()
        .stream::<Changes<::chat::Chat>>()
        .send(block(4, vec![ended]));
    cx.run_until_parked();
    assert!(cx.find("chat-busy-timeline").is_none(), "{:?}", cx.texts());
    assert!(!cx.has_text("reviewer is answering here since block 42"));
}

#[test]
fn a_drafting_holder_is_drawn_once_its_row_saying_since_when_without_break_for_a_reader() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let held = Rc::new(RefCell::new(vec![(
        None,
        holder(8, chat::Holding::Draft, 9),
    )]));
    busy_room(&cx, 9, held);
    let view = cx.open::<Chat>();
    seat(&mut cx);
    cx.update(&view, |chat, _, cx| chat.choose("general".into(), cx));
    cx.run_until_parked();
    assert_eq!(drawn(&cx, "is answering…"), 1, "{:?}", cx.texts());
    assert!(cx.has_text("since block 9"), "{:?}", cx.texts());
    assert!(!cx.has_text("reviewer is answering here since block 9"));
    assert!(cx.find("chat-busy-timeline").is_some());
    assert!(cx.find("chat-busy-timeline-break").is_none());
}

#[test]
fn a_held_thread_says_so_on_its_drafts_row_and_breaks_under_its_root() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let held = Rc::new(RefCell::new(vec![(
        Some(2),
        holder(10, chat::Holding::Draft, 5),
    )]));
    busy_room(&cx, 7, held);
    let view = cx.open::<Chat>();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    assert!(cx.find("chat-busy-timeline").is_none(), "the foot is free");
    thread(&mut cx, &view, 2);
    let asked = cx.host().requests::<Ask<::chat::Chat>>();
    let under = Query::Busy {
        channel_id: "general".into(),
        thread: Some(2),
        for_account: 7,
    };
    assert!(asked.contains(&under), "{asked:?}");
    assert!(cx.has_text("since block 5"), "{:?}", cx.texts());
    assert!(!cx.has_text("ada is answering here since block 5"));
    cx.simulate_click("chat-busy-thread-break");
    cx.run_until_parked();
    assert_eq!(told(&cx), vec![Told::Broke("general".into(), Some(2))]);
}

#[test]
fn on_a_phone_a_held_foot_says_so_too() {
    let (mut cx, _view) = super::phone::phone();
    let held = Rc::new(RefCell::new(vec![(
        None,
        holder(8, chat::Holding::Lease, 42),
    )]));
    busy_room(&cx, 7, held);
    cx.simulate_click("chat-rooms-channel-general");
    cx.run_until_parked();
    assert!(
        cx.has_text("reviewer is answering here since block 42"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.find("chat-busy-timeline-break").is_some());
}

#[test]
fn a_keystroke_heard_once_the_field_holds_it_begins_typing() {
    let (mut cx, view) = opened();
    cx.simulate_input("draft-general/editor", "h");
    view.read(|chat| assert_eq!(chat.drafts["draft-general"].field.text(), "h"));
    cx.run_until_parked();
    assert_eq!(told(&cx), vec![typing("general", None)]);
}

#[test]
fn an_edit_on_the_composer_a_send_cleared_begins_typing_again_and_reads_no_stale_range() {
    let (mut cx, view) = opened();
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    cx.simulate_focus("draft-general/editor");
    cx.simulate_input("draft-general/editor", "sent");
    cx.simulate_field_key("draft-general/editor", "enter");
    cx.simulate_input("draft-general/editor", "");
    cx.run_until_parked();
    view.read(|chat| assert_eq!(chat.drafts["draft-general"].body(), ""));
    cx.simulate_input("draft-general/editor", "x");
    cx.run_until_parked();
    let posted = Told::Posted("general".into());
    let next = vec![typing("general", None), posted, typing("general", None)];
    assert_eq!(told(&cx), next);
}
