//! A room's rules: the composer says the room has some and opens its details, which show the text
//! or a named file's content (read again once a files block moves it), and set and clear them.
use super::*;
use abi::BlobId;
use std::cell::RefCell;
use std::rc::Rc;

const FILE: &str = "/shared/rules.md";

fn general_with(rules: Option<&str>) -> Vec<ChannelInfo> {
    let mut general = channel("general", "General", 3);
    if let Some(rules) = rules {
        general.meta.insert("rules".into(), rules.into());
    }
    vec![general, channel("dm-7-8", "dm", 1)]
}

fn answer(cx: &TestAppContext, rooms: Vec<ChannelInfo>) {
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Channels { .. } => Reply::Channels(page(rooms.clone())),
                query => chat_reply(query),
            })
        }));
}

fn serve_file(cx: &TestAppContext, held: Rc<RefCell<(u8, String)>>) {
    cx.host().handle::<Ask<files::Files>>(move |query| {
        let snapshot = BlobId::Sha256([9; 32]);
        let (object, text) = held.borrow().clone();
        Ok(match query {
            files::Query::Stat { path, .. } => files::Reply::Stat {
                snapshot: Some(snapshot),
                entry: (path == FILE).then(|| files::EntryInfo {
                    path: path.clone(),
                    node: files::Node::File {
                        file: BlobId::Sha256([object; 32]),
                        size: text.len() as u64,
                        exec: false,
                    },
                    meta: Default::default(),
                }),
            },
            files::Query::Read { offset, .. } => files::Reply::Contents(files::Contents {
                snapshot,
                size: text.len() as u64,
                offset,
                bytes: text.as_bytes()[offset as usize..].to_vec(),
                eof: true,
            }),
            query => panic!("unexpected files query: {query:?}"),
        })
    });
}

fn desk(cx: &mut TestAppContext) -> Entity<Chat> {
    let view = cx.open::<Chat>();
    seat(cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    view
}

fn last_submitted(cx: &TestAppContext) -> Option<Op> {
    cx.host().requests::<Submit<::chat::Chat>>().pop()
}

#[test]
fn the_composer_says_a_room_has_rules_and_a_press_opens_them_in_its_details() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    answer(&cx, general_with(Some("Write in French.")));
    let _view = desk(&mut cx);
    assert!(cx.has_text("This room has rules"), "{:?}", cx.texts());
    assert!(
        !cx.has_text("Write in French."),
        "no excerpt over the composer"
    );
    for _ in 0..2 {
        cx.simulate_click("chat-room-rules");
        cx.run_until_parked();
        assert!(cx.has_text("Write in French."), "{:?}", cx.texts());
    }
}

#[test]
fn details_set_the_rules_and_clear_them() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    answer(&cx, general_with(None));
    let _view = desk(&mut cx);
    assert!(cx.find("chat-room-rules").is_none(), "no rules, no line");
    cx.simulate_click("chat-room-details");
    assert!(cx.find("chat-details-clear-rules").is_none());
    assert!(cx.find("chat-rules").is_none());

    answer(&cx, general_with(Some("Be brief.")));
    cx.simulate_input("chat-details-rules", " Be brief. ");
    cx.simulate_click("chat-details-set-rules");
    cx.run_until_parked();
    assert!(matches!(
        last_submitted(&cx),
        Some(Op::SetChannelMeta { channel_id, key, value })
            if channel_id == "general" && key == "rules" && value == "Be brief."
    ));
    assert!(cx.has_text("Be brief."), "{:?}", cx.texts());
    assert!(cx.find("chat-details-clear-rules").is_some());
    assert!(cx.find("chat-room-rules").is_some());

    answer(&cx, general_with(None));
    cx.simulate_click("chat-details-clear-rules");
    cx.run_until_parked();
    assert!(matches!(
        last_submitted(&cx),
        Some(Op::RemoveChannelMeta { channel_id, key })
            if channel_id == "general" && key == "rules"
    ));
    assert!(cx.find("chat-details-clear-rules").is_none());
    assert!(cx.find("chat-room-rules").is_none());
}

#[test]
fn a_path_shows_its_files_content_and_reads_it_again_once_it_moves() {
    let held = Rc::new(RefCell::new((1, "Be brief.".to_owned())));
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    answer(&cx, general_with(Some(FILE)));
    serve_file(&cx, held.clone());
    let _view = desk(&mut cx);
    assert!(!cx.has_text("Be brief."), "the file is read in the details");
    cx.simulate_click("chat-room-details");
    cx.run_until_parked();
    assert!(cx.has_text("Be brief."), "{:?}", cx.texts());
    assert!(cx.has_text("From /shared/rules.md"));
    *held.borrow_mut() = (2, "Be briefer.".to_owned());
    let files = cx.host().stream::<Changes<files::Files>>();
    files.send(block(10, Vec::new()));
    cx.run_until_parked();
    assert!(cx.has_text("Be briefer."), "{:?}", cx.texts());
}
