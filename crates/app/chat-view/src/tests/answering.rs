//! Who is answering in a room: a draft chat holds is drawn at the foot of the list it answers in,
//! with what its author is doing and its text so far, and goes once chat drops it. A draft under a
//! thread is the thread's: the channel only marks its root's reply row.
use super::*;
use std::cell::RefCell;
use std::rc::Rc;

fn draft(author: u64, thread: Option<u64>, doing: &str, text: &str) -> chat::Draft {
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
fn a_draft_is_drawn_at_the_rooms_foot_until_chat_drops_it() {
    let held = Rc::new(RefCell::new(vec![
        draft(8, None, "running forge_read", "Looking at the build"),
        draft(10, Some(2), "", "Under the second message"),
    ]));
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let answering = held.clone();
    let mut room = rooms_alone(|query| Ok(chat_reply(query)));
    cx.host()
        .handle::<Ask<::chat::Chat>>(move |query| match query {
            Query::Drafts { .. } => Ok(Reply::Drafts(answering.borrow().clone())),
            query => room(query),
        });
    let _view = cx.open::<Chat>();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    assert!(cx.has_text("running forge_read…"), "{:?}", cx.texts());
    let row = cx.interactivity("chat-answering-8");
    assert_eq!(
        row.aria.label.as_deref(),
        Some("reviewer running forge_read…"),
        "a screen reader hears what the row shows"
    );
    assert!(
        !cx.has_text("is answering…"),
        "what it is doing takes the place of is answering: {:?}",
        cx.texts()
    );
    assert!(cx.has_text("Looking at the build"));
    assert!(
        !cx.has_text("Under the second message"),
        "a draft under a thread is the thread's"
    );
    held.borrow_mut().clear();
    let dropped = chat::tables::DRAFTS.key(&("general".to_owned(), Principal::Account(8)));
    let changes = cx.host().stream::<Changes<::chat::Chat>>();
    changes.send(block(4, vec![dropped]));
    cx.run_until_parked();
    assert!(!cx.has_text("running forge_read…"), "{:?}", cx.texts());
    assert!(!cx.has_text("Looking at the build"));
}

#[test]
fn a_drafts_text_is_drawn_as_its_post_will_be_never_as_its_raw_marks() {
    let held = vec![draft(8, None, "", "Looking at the **build** in `ci`")];
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let mut room = rooms_alone(|query| Ok(chat_reply(query)));
    cx.host()
        .handle::<Ask<::chat::Chat>>(move |query| match query {
            Query::Drafts { .. } => Ok(Reply::Drafts(held.clone())),
            Query::Roots { channel_id, .. } if channel_id == "general" => {
                let posted = vec![parsed_row(1, 7, "hello"), parsed_row(2, 8, "**hi** there")];
                Ok(Reply::Roots(page(posted)))
            }
            query => room(query),
        });
    let _view = cx.open::<Chat>();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    let texts = cx.texts();
    assert!(
        cx.has_text("hi there"),
        "the post as chat keeps it: {texts:?}"
    );
    assert!(cx.has_text("is answering…"), "{texts:?}");
    let row = cx.interactivity("chat-answering-8");
    assert_eq!(
        row.aria.label.as_deref(),
        Some("reviewer is answering…"),
        "a draft that says nothing it is doing is answering"
    );
    let posted = texts.iter().position(|text| text == "hi there");
    let draft = texts.iter().position(|text| text == "is answering…");
    let named = draft
        .and_then(|at| at.checked_sub(1))
        .map(|at| texts[at].as_str());
    assert_eq!(
        named,
        Some("reviewer"),
        "the draft has its author line: {texts:?}"
    );
    assert!(posted < draft, "{texts:?}");
    assert!(texts.iter().any(|text| text.contains("build")), "{texts:?}");
    assert!(
        !texts
            .iter()
            .any(|text| text.contains("**") || text.contains('`')),
        "{texts:?}"
    );
}

// reviewer answers at the room's foot and ada under #general's second message, whose thread has no
// reply yet
fn answering_in_both(cx: &TestAppContext) {
    let held = vec![
        draft(8, None, "", "Looking at the build"),
        draft(10, Some(2), "", "Under the second message"),
    ];
    let mut room = rooms_alone(|query| Ok(chat_reply(query)));
    cx.host()
        .handle::<Ask<::chat::Chat>>(move |query| match query {
            Query::Drafts { .. } => Ok(Reply::Drafts(held.clone())),
            Query::Thread { .. } => Ok(Reply::Thread {
                root: Some(row(2, 8, "**hi** there")),
                replies: page(Vec::new()),
            }),
            query => room(query),
        });
}

fn drawn(cx: &TestAppContext, text: &str) -> usize {
    cx.texts().iter().filter(|said| *said == text).count()
}

#[test]
fn a_threaded_draft_is_drawn_in_its_thread_and_marked_on_its_roots_reply_row() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    answering_in_both(&cx);
    let view = cx.open::<Chat>();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    assert!(
        cx.has_text("Looking at the build"),
        "a draft with no thread is the channel's"
    );
    assert!(!cx.has_text("Under the second message"), "{:?}", cx.texts());
    let mark = cx.interactivity("chat-message-m2-answering");
    assert_eq!(mark.role, Some(view::Role::Status));
    assert_eq!(drawn(&cx, "ada is answering…"), 1, "{:?}", cx.texts());
    assert!(cx.find("chat-message-m1-answering").is_none());
    thread(&mut cx, &view, 2);
    assert_eq!(
        drawn(&cx, "Under the second message"),
        1,
        "in the thread alone"
    );
    assert_eq!(
        drawn(&cx, "Looking at the build"),
        1,
        "the channel's stays at its foot"
    );
}

#[test]
fn on_a_phone_a_threaded_draft_is_drawn_in_its_thread_and_marked_on_its_root() {
    let (mut cx, view) = super::phone::phone();
    answering_in_both(&cx);
    cx.simulate_click("chat-rooms-channel-general");
    cx.run_until_parked();
    assert!(cx.has_text("Looking at the build"), "{:?}", cx.texts());
    assert!(!cx.has_text("Under the second message"), "{:?}", cx.texts());
    assert!(cx.find("chat-message-m2-answering").is_some());
    assert_eq!(drawn(&cx, "ada is answering…"), 1, "{:?}", cx.texts());
    thread(&mut cx, &view, 2);
    assert!(cx.has_text("Under the second message"), "{:?}", cx.texts());
    assert!(
        !cx.has_text("Looking at the build"),
        "the thread holds its own draft alone"
    );
}

#[test]
fn the_mark_names_everyone_answering_under_a_root() {
    let names = |names: &[&str]| {
        names
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(crate::words::answering_under(&[]), None);
    assert_eq!(
        crate::words::answering_under(&names(&["ada"])).as_deref(),
        Some("ada is answering…")
    );
    assert_eq!(
        crate::words::answering_under(&names(&["ada", "rex", "tech"])).as_deref(),
        Some("ada, rex and tech are answering…")
    );
}
