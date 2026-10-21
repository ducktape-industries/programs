use super::{
    login,
    server::{self, Boxed, Server},
};
use crate::auth::decode;
use grammers_tl_types::{self as tl, Identifiable, Serializable};

pub(super) fn peer(id: i64) -> tl::enums::Peer {
    tl::enums::Peer::User(tl::types::PeerUser { user_id: id })
}

pub(super) fn message(
    id: i32,
    to: tl::enums::Peer,
    from: tl::enums::Peer,
    date: i32,
    text: &str,
) -> tl::enums::Message {
    let mut bytes = tl::types::Message::CONSTRUCTOR_ID.to_le_bytes().to_vec();
    (1u32 << 8).serialize(&mut bytes);
    0u32.serialize(&mut bytes);
    id.serialize(&mut bytes);
    from.serialize(&mut bytes);
    to.serialize(&mut bytes);
    date.serialize(&mut bytes);
    text.to_owned().serialize(&mut bytes);
    decode(&bytes).unwrap()
}

pub(super) fn dialog(peer: tl::enums::Peer, id: i32) -> tl::enums::Dialog {
    tl::enums::Dialog::Dialog(tl::types::Dialog {
        pinned: false,
        unread_mark: false,
        view_forum_as_messages: false,
        peer,
        top_message: id,
        read_inbox_max_id: 0,
        read_outbox_max_id: 0,
        unread_count: 0,
        unread_mentions_count: 0,
        unread_reactions_count: 0,
        unread_poll_votes_count: 0,
        notify_settings: decode(
            &[
                tl::types::PeerNotifySettings::CONSTRUCTOR_ID.to_le_bytes(),
                [0; 4],
            ]
            .concat(),
        )
        .unwrap(),
        pts: None,
        draft: None,
        folder_id: None,
        ttl_period: None,
    })
}

pub(super) fn dialogs() -> Vec<u8> {
    tl::types::messages::Dialogs {
        dialogs: vec![dialog(peer(8), 10)],
        messages: vec![message(
            10,
            peer(8),
            peer(8),
            server::UNIX as i32,
            "Hello from Bob",
        )],
        chats: vec![],
        users: vec![login::user(8, "Bob", 88)],
    }
    .boxed()
}

pub(super) fn listed() -> (view::testing::TestAppContext, Server) {
    let mut cx = Server::context();
    let mut server = Server::default();
    server.open(&mut cx);
    login::sign_in(&mut cx, &mut server);
    let (id, _) = server.take::<tl::functions::messages::GetDialogs>();
    server.reply(&mut cx, id, dialogs());
    (cx, server)
}

#[test]
fn dialogs_show_users_groups_channels_and_previews_newest_first_with_paging() {
    let mut cx = Server::context();
    let mut server = Server::default();
    server.open(&mut cx);
    login::sign_in(&mut cx, &mut server);
    let (id, request) = server.take::<tl::functions::messages::GetDialogs>();
    assert_eq!(request.offset_id, 0);
    assert_eq!(request.offset_peer, tl::enums::InputPeer::Empty);
    let group = tl::enums::Peer::Chat(tl::types::PeerChat { chat_id: 9 });
    server.reply(
        &mut cx,
        id,
        tl::types::messages::DialogsSlice {
            count: 3,
            dialogs: vec![dialog(group.clone(), 20), dialog(peer(8), 10)],
            messages: vec![
                message(10, peer(8), peer(8), server::UNIX as i32, "Bob's preview"),
                message(
                    20,
                    group.clone(),
                    peer(8),
                    server::UNIX as i32 + 2,
                    "Group preview",
                ),
            ],
            chats: vec![tl::enums::Chat::Forbidden(tl::types::ChatForbidden {
                id: 9,
                title: "A group".into(),
            })],
            users: vec![login::user(8, "Bob", 88)],
        }
        .boxed(),
    );
    assert!(cx.has_text("Bob"));
    assert!(cx.has_text("A group"));
    assert!(cx.has_text("Bob's preview"));
    let texts = cx.texts();
    assert!(
        texts.iter().position(|text| text == "A group")
            < texts.iter().position(|text| text == "Bob")
    );
    cx.simulate_click("more-chats");
    server.pump(&mut cx);
    let (id, request) = server.take::<tl::functions::messages::GetDialogs>();
    assert_eq!(request.offset_id, 10);
    assert_eq!(request.offset_date, server::UNIX as i32);
    assert_eq!(
        request.offset_peer,
        tl::enums::InputPeer::User(tl::types::InputPeerUser {
            user_id: 8,
            access_hash: 88
        })
    );
    let channel = tl::enums::Peer::Channel(tl::types::PeerChannel { channel_id: 10 });
    server.reply(
        &mut cx,
        id,
        tl::types::messages::Dialogs {
            dialogs: vec![dialog(channel.clone(), 5)],
            messages: vec![message(
                5,
                channel,
                peer(8),
                server::UNIX as i32 - 5,
                "Channel preview",
            )],
            chats: vec![tl::enums::Chat::ChannelForbidden(
                tl::types::ChannelForbidden {
                    broadcast: true,
                    megagroup: false,
                    monoforum: false,
                    id: 10,
                    access_hash: 1010,
                    title: "A channel".into(),
                    until_date: None,
                },
            )],
            users: vec![],
        }
        .boxed(),
    );
    assert!(cx.has_text("A channel"));
    assert!(cx.has_text("Channel preview"));
    assert!(cx.find("more-chats").is_none());
    let peers = cx
        .host()
        .kept::<std::collections::BTreeMap<crate::chats::Peer, crate::chats::PeerInfo>>("peers")
        .unwrap();
    assert_eq!(peers.len(), 3);
}

#[test]
fn refreshing_keeps_the_chat_list_visible_until_the_answer_arrives() {
    let (mut cx, mut server) = listed();
    assert!(cx.has_text("Bob"));
    cx.simulate_click("refresh");
    server.pump(&mut cx);
    assert!(cx.has_text("Hello from Bob"));
    let (id, _) = server.take::<tl::functions::messages::GetDialogs>();
    server.reply(&mut cx, id, dialogs());
    assert!(cx.has_text("Bob"));
}

#[test]
fn a_page_of_only_pinned_chats_can_continue_to_the_unpinned_chats() {
    let mut cx = Server::context();
    let mut server = Server::default();
    server.open(&mut cx);
    login::sign_in(&mut cx, &mut server);
    let (id, _) = server.take::<tl::functions::messages::GetDialogs>();
    let mut pinned = dialog(peer(8), 10);
    if let tl::enums::Dialog::Dialog(dialog) = &mut pinned {
        dialog.pinned = true;
    }
    server.reply(
        &mut cx,
        id,
        tl::types::messages::DialogsSlice {
            count: 2,
            dialogs: vec![pinned],
            messages: vec![message(10, peer(8), peer(8), server::UNIX as i32, "Pinned")],
            chats: vec![],
            users: vec![login::user(8, "Bob", 88)],
        }
        .boxed(),
    );
    cx.simulate_click("more-chats");
    server.pump(&mut cx);
    let (id, request) = server.take::<tl::functions::messages::GetDialogs>();
    assert!(request.exclude_pinned);
    assert_eq!(request.offset_id, 0);
    server.reply(
        &mut cx,
        id,
        tl::types::messages::DialogsSlice {
            count: 2,
            dialogs: vec![dialog(peer(9), 9)],
            messages: vec![message(
                9,
                peer(9),
                peer(9),
                server::UNIX as i32 + 1,
                "Unpinned",
            )],
            chats: vec![],
            users: vec![login::user(9, "Alice", 99)],
        }
        .boxed(),
    );
    assert!(cx.has_text("Pinned"));
    assert!(cx.has_text("Unpinned"));
}
