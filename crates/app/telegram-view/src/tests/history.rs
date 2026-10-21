use super::{
    chats::{listed, message, peer},
    server::{self, Boxed},
};
use grammers_tl_types as tl;

pub(super) fn history() -> Vec<u8> {
    tl::types::messages::Messages {
        messages: vec![message(
            10,
            peer(8),
            peer(8),
            server::UNIX as i32,
            "Hello from Bob",
        )],
        topics: vec![],
        chats: vec![],
        users: vec![],
    }
    .boxed()
}

pub(super) fn opened() -> (view::testing::TestAppContext, super::server::Server) {
    let (mut cx, mut server) = listed();
    cx.simulate_click("chat-u8");
    server.pump(&mut cx);
    let (id, request) = server.take::<tl::functions::messages::GetHistory>();
    assert_eq!(
        request.peer,
        tl::enums::InputPeer::User(tl::types::InputPeerUser {
            user_id: 8,
            access_hash: 88
        })
    );
    server.reply(&mut cx, id, history());
    (cx, server)
}

#[test]
fn history_draws_senders_times_and_media_and_fetches_older_messages_on_scroll() {
    let (mut cx, mut server) = listed();
    cx.simulate_click("chat-u8");
    server.pump(&mut cx);
    let (id, request) = server.take::<tl::functions::messages::GetHistory>();
    assert_eq!(request.offset_id, 0);
    let mut photo = message(11, peer(8), peer(8), server::UNIX as i32, "");
    if let tl::enums::Message::Message(message) = &mut photo {
        message.media = Some(tl::enums::MessageMedia::Photo(
            tl::types::MessageMediaPhoto {
                spoiler: false,
                live_photo: false,
                photo: None,
                ttl_seconds: None,
                video: None,
            },
        ));
    }
    let mut file = message(10, peer(8), peer(8), server::UNIX as i32 - 10, "");
    if let tl::enums::Message::Message(message) = &mut file {
        message.media = Some(tl::enums::MessageMedia::Document(
            tl::types::MessageMediaDocument {
                nopremium: false,
                spoiler: false,
                video: false,
                round: false,
                voice: false,
                alt_documents: None,
                video_cover: None,
                video_timestamp: None,
                ttl_seconds: None,
                document: Some(tl::enums::Document::Document(tl::types::Document {
                    id: 1,
                    access_hash: 2,
                    file_reference: vec![],
                    date: 0,
                    mime_type: "text/plain".into(),
                    size: 5,
                    thumbs: None,
                    video_thumbs: None,
                    dc_id: 2,
                    attributes: vec![tl::enums::DocumentAttribute::Filename(
                        tl::types::DocumentAttributeFilename {
                            file_name: "notes.txt".into(),
                        },
                    )],
                })),
            },
        ));
    }
    server.reply(
        &mut cx,
        id,
        tl::types::messages::MessagesSlice {
            inexact: false,
            count: 3,
            next_rate: None,
            offset_id_offset: None,
            search_flood: None,
            messages: vec![photo, file],
            topics: vec![],
            chats: vec![],
            users: vec![],
        }
        .boxed(),
    );
    assert!(cx.has_text("photo"));
    assert!(cx.has_text("file notes.txt"));
    let timestamp = view::design::date(server::UNIX as u64 * 1000);
    assert!(cx.has_text(&format!("Bob · {timestamp}")));
    let mut wheel =
        view::wire::interactivity::ScrollWheel::from(&view::ScrollWheelEvent::default());
    wheel.delta = view::wire::mouse::ScrollDelta::Lines { x: 0., y: 1. };
    cx.simulate_scroll_wheel("history", wheel.into_gpui());
    server.pump(&mut cx);
    let (id, request) = server.take::<tl::functions::messages::GetHistory>();
    assert_eq!(request.offset_id, 10);
    server.reply(
        &mut cx,
        id,
        tl::types::messages::Messages {
            messages: vec![message(
                9,
                peer(8),
                peer(8),
                server::UNIX as i32 - 20,
                "An older message",
            )],
            topics: vec![],
            chats: vec![],
            users: vec![],
        }
        .boxed(),
    );
    assert!(cx.has_text("An older message"));
    assert!(cx.has_text("photo"));
    let texts = cx.texts();
    assert!(
        texts.iter().position(|text| text == "An older message")
            < texts.iter().position(|text| text == "photo")
    );
    assert!(cx.find("older").is_none());
}

#[test]
fn a_history_answer_does_not_reopen_a_chat_the_reader_left() {
    let (mut cx, mut server) = listed();
    cx.simulate_click("chat-u8");
    server.pump(&mut cx);
    let (id, _) = server.take::<tl::functions::messages::GetHistory>();
    cx.simulate_click("back");
    server.reply(&mut cx, id, history());
    assert!(cx.find("history").is_none());
    assert!(cx.find("chat-u8").is_some());
}

#[test]
fn a_chat_can_be_opened_in_the_touch_layout() {
    let (mut cx, mut server) = opened();
    cx.simulate_touch(true);
    cx.simulate_resize(360., 780.);
    server.pump(&mut cx);
    assert!(cx.has_text("Hello from Bob"));
    assert!(cx.find("back").is_some());
}
