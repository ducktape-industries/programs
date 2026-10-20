//! Who is answering in a room: a draft chat holds is drawn at the foot of the list it answers in,
//! with what its author is doing and its text so far, and goes once chat drops it.
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
    assert!(cx.has_text("is answering…"), "{:?}", cx.texts());
    assert!(cx.has_text("running forge_read…"));
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
    assert!(!cx.has_text("is answering…"), "{:?}", cx.texts());
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
