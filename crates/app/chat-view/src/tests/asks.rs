//! An ask put to the reader: a button per option and Other under it, a press posting the
//! option in the ask's thread, and what an addressee answered in their place.
use super::*;
use std::cell::RefCell;
use std::rc::Rc;

type Replies = Rc<RefCell<Vec<MsgRow>>>;

fn ask_row(to: Vec<u64>) -> MsgRow {
    let ask = chat::asking::Ask {
        to,
        question: "Which cache size should the node use?".into(),
        options: vec![
            chat::asking::Choice {
                label: "64 MB".into(),
                detail: "fits the small pods".into(),
            },
            chat::asking::Choice {
                label: "128 MB".into(),
                detail: String::new(),
            },
        ],
        default: 1,
    };
    let blocks = chat::asking::blocks(&ask);
    MsgRow {
        text: chat::plain_text(&blocks),
        blocks,
        ..row(3, 8, "")
    }
}

fn reply(seq: u64, author: u64, text: &str) -> MsgRow {
    MsgRow {
        thread: Some(3),
        ..row(seq, author, text)
    }
}

fn land(cx: &mut TestAppContext, height: u64) {
    cx.host()
        .stream::<Changes<::chat::Chat>>()
        .send(block(height, posted("general")));
    cx.run_until_parked();
}

fn serve(cx: &mut TestAppContext, ask: MsgRow, replies: Replies) {
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Roots { .. } => Reply::Roots(page(vec![
                    row(1, 7, "hello"),
                    row(2, 8, "**hi** there"),
                    ask.clone(),
                ])),
                Query::Repliers { roots, .. } => Reply::Repliers(
                    roots
                        .into_iter()
                        .map(|root| (root, replies.borrow().clone()))
                        .collect(),
                ),
                query => chat_reply(query),
            })
        }));
    land(cx, 4);
}

fn asked(ask: MsgRow, replies: Replies) -> (TestAppContext, Entity<Chat>) {
    let (mut cx, view) = opened();
    serve(&mut cx, ask, replies);
    (cx, view)
}

fn shows(cx: &TestAppContext, words: &str) -> bool {
    cx.texts().iter().any(|text| text.contains(words))
}

fn picked(cx: &TestAppContext) -> Vec<String> {
    let posted = cx.host().requests::<Submit<::chat::Chat>>();
    posted
        .iter()
        .filter_map(|op| match op {
            Op::PostMessage {
                thread: Some(3),
                blocks,
                ..
            } => Some(chat::plain_text(blocks)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_reader_asked_gets_a_button_per_option_and_other() {
    let (cx, _) = asked(ask_row(vec![7]), Rc::default());
    assert!(shows(&cx, "Which cache size should the node use?"));
    assert!(
        cx.find("chat-message-m3-ask-1").is_some(),
        "{:?}",
        cx.texts()
    );
    assert!(cx.find("chat-message-m3-ask-2").is_some());
    assert!(cx.find("chat-message-m3-ask-other").is_some());
    assert!(cx.has_text("128 MB (default)"), "{:?}", cx.texts());
    let asked = cx.host().requests::<Ask<::chat::Chat>>();
    let read = asked
        .iter()
        .any(|query| matches!(query, Query::Repliers { roots, .. } if *roots == [3]));
    assert!(read, "who answered is read for the ask alone: {asked:?}");
}

#[test]
fn anyone_not_asked_reads_the_ask_as_text() {
    let (cx, _) = asked(ask_row(vec![10]), Rc::default());
    assert!(shows(&cx, "Which cache size should the node use?"));
    assert!(shows(&cx, "64 MB"), "{:?}", cx.texts());
    assert!(cx.find("chat-message-m3-ask").is_none());
    assert!(cx.find("chat-message-m3-ask-1").is_none());
    let asked = cx.host().requests::<Ask<::chat::Chat>>();
    let read = asked
        .iter()
        .any(|query| matches!(query, Query::Repliers { .. }));
    assert!(!read, "an ask put to others is never read: {asked:?}");
}

#[test]
fn a_press_posts_the_option_in_the_asks_thread_and_the_buttons_give_way() {
    let replies = Replies::default();
    let (mut cx, _) = asked(ask_row(vec![7]), replies.clone());
    cx.simulate_click("chat-message-m3-ask-1");
    cx.run_until_parked();
    assert_eq!(picked(&cx), ["64 MB"]);
    assert!(cx.has_text("Answered: 64 MB"), "{:?}", cx.texts());
    assert!(
        cx.find("chat-message-m3-ask-1").is_none(),
        "one press, one answer"
    );
    replies.borrow_mut().push(MsgRow {
        message_id: "message-1".into(),
        ..reply(4, 7, "64 MB")
    });
    land(&mut cx, 5);
    assert!(cx.has_text("Answered: 64 MB"), "{:?}", cx.texts());
    assert_eq!(picked(&cx), ["64 MB"]);
}

#[test]
fn other_opens_the_asks_thread_with_its_reply_field_taking_the_keys() {
    let (mut cx, view) = asked(ask_row(vec![7]), Rc::default());
    cx.simulate_click("chat-message-m3-ask-other");
    cx.run_until_parked();
    view.read(|chat| {
        let thread = chat.room.as_ref().and_then(|room| room.thread.as_ref());
        assert_eq!(thread.map(|thread| thread.root), Some(3));
    });
    assert_eq!(
        cx.focused().and_then(|node| node.key()),
        Some("draft-general/3/editor"),
        "the reply field takes the keys"
    );
    assert!(picked(&cx).is_empty(), "nothing is posted");
}

#[test]
fn an_addressees_reply_takes_the_buttons_and_says_what_it_chose() {
    let replies = Rc::new(RefCell::new(vec![reply(4, 9, "64 MB")]));
    let (mut cx, _) = asked(ask_row(vec![7, 10]), replies.clone());
    assert!(
        cx.find("chat-message-m3-ask-1").is_some(),
        "a reply from someone not asked answers nothing: {:?}",
        cx.texts()
    );
    replies.borrow_mut().push(reply(5, 10, "2"));
    land(&mut cx, 5);
    assert!(cx.has_text("Answered: 128 MB"), "{:?}", cx.texts());
    assert!(cx.find("chat-message-m3-ask-1").is_none());
    replies.borrow_mut()[1] = reply(5, 10, "let us measure first");
    land(&mut cx, 6);
    assert!(cx.has_text("Answered in the thread"), "{:?}", cx.texts());
}

#[test]
fn a_phone_draws_the_buttons_and_a_tap_posts_the_option() {
    let (mut cx, view) = super::phone::phone();
    cx.simulate_click("chat-rooms-channel-general");
    cx.run_until_parked();
    serve(&mut cx, ask_row(vec![7]), Rc::default());
    assert!(
        cx.find("chat-message-m3-ask-2").is_some(),
        "{:?}",
        cx.texts()
    );
    assert!(cx.find("chat-message-m3-ask-other").is_some());
    cx.simulate_click("chat-message-m3-ask-2");
    cx.run_until_parked();
    assert_eq!(picked(&cx), ["128 MB"]);
    assert!(cx.has_text("Answered: 128 MB"), "{:?}", cx.texts());
    view.read(|chat| assert!(chat.menu.is_none(), "the press opens no actions sheet"));
}
