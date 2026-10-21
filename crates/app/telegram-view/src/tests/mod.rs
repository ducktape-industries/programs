mod chats;
mod history;
mod login;
mod protocol;
mod security;
mod send;
mod server;
mod updates;
use super::*;
use server::Boxed;
#[test]
fn a_connection_refusal_is_explained() {
    for code in ["undeclared_host", "consent_refused"] {
        let mut cx = view::testing::TestAppContext::new();
        cx.host().handle::<HostRandom>(|n| Ok(vec![7; n as usize]));
        cx.host().handle::<ChainStatus>(|_| {
            Ok(view::methods::NodeStatus {
                time: 1_800_000_000_000,
                ..Default::default()
            })
        });
        cx.host().refuse::<SocketConnect>(code, "refused");
        cx.open::<Telegram>();
        server::seat(&mut cx, "a key");
        assert!(
            cx.texts().iter().any(|text| text.contains("refused")),
            "{:?}",
            cx.texts()
        );
    }
}

#[test]
fn the_obfuscated_dh_exchange_establishes_the_servers_key() {
    let mut cx = server::Server::context();
    let mut server = server::Server::default();
    server.open(&mut cx);
    let session = cx.host().kept::<Session>("session").unwrap();
    assert_eq!(session.keys[&2].key, server.key.unwrap());
    assert_eq!(server.headers, 1);
    assert!(cx.has_text(words::CONNECTED), "{:?}", cx.texts());
    for _ in 0..30 {
        cx.host().stream::<ClockTicks>().send(());
        cx.run_until_parked();
    }
    server.pump(&mut cx);
    assert!(!server.acks.is_empty());
}

#[test]
fn a_bad_salt_resends_and_flood_wait_uses_protocol_ticks() {
    use grammers_tl_types::{self as tl, Serializable};
    let mut cx = server::Server::context();
    let mut server = server::Server::default();
    let entity = server.open(&mut cx);
    let body = tl::functions::updates::GetState {}.to_bytes();
    cx.update(&entity, |view, _, cx| {
        view.live
            .connection
            .as_mut()
            .unwrap()
            .request(7, body.clone());
        view.flush(cx);
    });
    server.pump(&mut cx);
    let call = server.calls.pop().unwrap();
    assert_eq!(call.body, body);
    assert_eq!(call.sequence, 1);
    assert_ne!(call.salt, 1234);
    server.send(
        &mut cx,
        tl::types::BadServerSalt {
            bad_msg_id: call.id,
            bad_msg_seqno: call.sequence,
            error_code: 48,
            new_server_salt: 1234,
        }
        .boxed(),
    );
    server.pump(&mut cx);
    let retry = server.calls.pop().unwrap();
    assert_eq!(retry.body, body);
    assert_eq!(retry.salt, 1234);
    assert_eq!(
        cx.host().kept::<Session>("session").unwrap().keys[&2].salt,
        1234
    );
    server.reply(
        &mut cx,
        retry.id,
        tl::types::RpcError {
            error_code: 420,
            error_message: "FLOOD_WAIT_3".into(),
        }
        .boxed(),
    );
    assert!(server.calls.is_empty());
    for _ in 0..2 {
        cx.host().stream::<ClockTicks>().send(());
        server.pump(&mut cx);
    }
    assert!(server.calls.is_empty());
    cx.host().stream::<ClockTicks>().send(());
    server.pump(&mut cx);
    let retry = server.calls.pop().unwrap();
    assert_eq!(retry.body, body);
    server.reply(
        &mut cx,
        retry.id,
        tl::types::updates::State {
            pts: 1,
            qts: 0,
            date: server::UNIX as i32,
            seq: 1,
            unread_count: 0,
        }
        .boxed(),
    );
    assert!(!server.acks.is_empty());
}

#[test]
fn partial_frames_containers_and_gzip_preserve_message_boundaries() {
    use grammers_tl_types::{self as tl, Serializable};
    use std::io::Write;
    let mut cx = server::Server::context();
    let mut server = server::Server::default();
    let entity = server.open(&mut cx);
    cx.update(&entity, |view, _, cx| {
        view.live
            .connection
            .as_mut()
            .unwrap()
            .request(8, tl::functions::updates::GetState {}.to_bytes());
        view.flush(cx);
    });
    server.pump(&mut cx);
    let body = tl::types::NewSessionCreated {
        first_msg_id: 1,
        unique_id: 2,
        server_salt: 9876,
    }
    .boxed();
    let mut container = 0x73f1f8dcu32.to_le_bytes().to_vec();
    Serializable::serialize(&1i32, &mut container);
    Serializable::serialize(&((server::UNIX << 32) + 3), &mut container);
    Serializable::serialize(&1i32, &mut container);
    Serializable::serialize(&(body.len() as i32), &mut container);
    container.extend(body);
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gzip.write_all(&container).unwrap();
    let mut packed = 0x3072cfa1u32.to_le_bytes().to_vec();
    Serializable::serialize(&gzip.finish().unwrap(), &mut packed);
    let packet = server.envelope(packed);
    let bytes = server.framed(packet);
    for chunk in bytes.chunks(3) {
        cx.host()
            .stream::<SocketConnect>()
            .send(SocketItem::Binary(chunk.to_vec()));
        cx.run_until_parked();
    }
    server.pump(&mut cx);
    assert_eq!(
        cx.host().kept::<Session>("session").unwrap().keys[&2].salt,
        9876
    );
}

#[test]
fn startup_can_retry_a_failed_random_source_before_opening_any_socket() {
    let mut cx = server::Server::context();
    cx.host()
        .refuse::<HostRandom>("host_fault", "random source unavailable");
    cx.open::<Telegram>();
    server::seat(&mut cx, "a key");
    assert!(cx.has_text("random source unavailable"));
    assert!(cx.host().requests::<SocketConnect>().is_empty());
    cx.host().handle::<HostRandom>(|n| Ok(vec![23; n as usize]));
    cx.simulate_click("connect");
    cx.run_until_parked();
    let mut server = server::Server::default();
    server.opened(&mut cx);
    assert!(cx.has_text(words::CONNECTED));
    assert_eq!(cx.host().requests::<HostRandom>(), [32, 32]);
}

#[test]
fn switching_to_a_store_without_telegram_keys_or_peer_hashes_clears_session_and_draft() {
    let (mut cx, mut server) = history::opened();
    cx.simulate_input("compose", "Private draft");
    cx.host().handle::<view::methods::StoreGet>(|_| Ok(None));
    cx.host()
        .stream::<view::methods::HostSession>()
        .send(view::methods::Session {
            chain_id: "test#1".into(),
            signer: "another key".into(),
            ..Default::default()
        });
    cx.run_until_parked();
    assert!(!cx.has_text("Hello from Bob"));
    assert!(!cx.has_text("Private draft"));
    server.opened(&mut cx);
    assert!(cx.has_text(words::CONNECTED));
    assert_eq!(cx.host().kept::<Session>("session").unwrap().user, None);
    assert_eq!(server.headers, 2);
}

#[test]
fn a_bad_message_time_corrects_the_offset_and_retries_the_rpc() {
    use grammers_tl_types::{self as tl, Serializable};
    for packed in [false, true] {
        let mut cx = server::Server::context();
        let mut server = server::Server::default();
        let entity = server.open(&mut cx);
        cx.update(&entity, |view, _, cx| {
            view.live.unix -= 600;
            let connection = view.live.connection.as_mut().unwrap();
            connection.unix = view.live.unix;
            connection.request(77, tl::functions::updates::GetState {}.to_bytes());
            view.flush(cx);
        });
        server.pump(&mut cx);
        let call = server.calls.pop().unwrap();
        let mut body = tl::types::BadMsgNotification {
            bad_msg_id: call.id,
            bad_msg_seqno: call.sequence,
            error_code: 16,
        }
        .boxed();
        if packed {
            let mut container = 0x73f1f8dcu32.to_le_bytes().to_vec();
            Serializable::serialize(&1i32, &mut container);
            Serializable::serialize(&((server::UNIX << 32) + 3), &mut container);
            Serializable::serialize(&1i32, &mut container);
            Serializable::serialize(&(body.len() as i32), &mut container);
            container.extend(body);
            body = container;
        }
        server.send(&mut cx, body);
        server.pump(&mut cx);
        let retry = server.calls.pop().unwrap();
        assert_eq!(retry.body, call.body);
        assert_eq!(retry.id >> 32, server::UNIX);
        assert_eq!(
            cx.host().kept::<Session>("session").unwrap().keys[&2].offset,
            600
        );
    }
}

#[test]
fn a_tampered_encrypted_server_message_drops_the_connection() {
    let mut cx = server::Server::context();
    let mut server = server::Server::default();
    let entity = server.open(&mut cx);
    let mut packet = server.envelope(
        grammers_tl_types::types::NewSessionCreated {
            first_msg_id: 1,
            unique_id: 2,
            server_salt: 3,
        }
        .boxed(),
    );
    let last = packet.last_mut().unwrap();
    *last ^= 1;
    server.frame(&mut cx, packet);
    cx.run_until_parked();
    cx.update(&entity, |view, _, _| {
        assert!(view.live.connection.is_none());
        assert!(!view.live.ready);
        assert!(view.failed.is_some());
    });
    assert!(cx.has_text(words::RECONNECTING));
}

#[test]
fn unknown_transport_keys_retry_dh_without_reusing_rejected_key_only_on_affected_dc() {
    for dc in [2, 4] {
        let mut cx = server::Server::context();
        let mut server = server::Server::default();
        let entity = server.open(&mut cx);
        login::sign_in(&mut cx, &mut server);
        cx.update(&entity, |view, _, cx| {
            let key = view.live.session.keys[&2].clone();
            view.live.session.keys.insert(dc, key);
            view.live.dc = Some(dc);
            cx.store().set("session", &view.live.session);
        });
        server.frame(&mut cx, (-404i32).to_le_bytes().to_vec());
        cx.run_until_parked();
        let session = cx.host().kept::<Session>("session").unwrap();
        assert!(!session.keys.contains_key(&dc));
        if dc == 2 {
            assert_eq!(session.user, None);
            assert!(cx.has_text(words::HOME_KEY_LOST));
        } else {
            assert_eq!(session.user, Some(7));
            assert!(session.keys.contains_key(&2));
        }
        for _ in 0..5 {
            cx.host().stream::<ClockTicks>().send(());
            cx.run_until_parked();
        }
        server.opened(&mut cx);
        assert!(
            cx.host()
                .kept::<Session>("session")
                .unwrap()
                .keys
                .contains_key(&dc)
        );
        let connects = cx.host().requests::<SocketConnect>().len();
        server.frame(&mut cx, (-404i32).to_le_bytes().to_vec());
        cx.run_until_parked();
        for _ in 0..9 {
            cx.host().stream::<ClockTicks>().send(());
            cx.run_until_parked();
        }
        assert_eq!(cx.host().requests::<SocketConnect>().len(), connects);
        cx.host().stream::<ClockTicks>().send(());
        cx.run_until_parked();
        assert_eq!(cx.host().requests::<SocketConnect>().len(), connects + 1);
        server.opened(&mut cx);
        assert!(
            cx.host()
                .kept::<Session>("session")
                .unwrap()
                .keys
                .contains_key(&dc)
        );
    }
}

#[test]
fn transport_floods_back_off_and_explicit_connect_retries_immediately() {
    let mut cx = server::Server::context();
    let mut server = server::Server::default();
    let entity = server.open(&mut cx);
    for delay in [5, 10] {
        let before = cx.host().requests::<SocketConnect>().len();
        server.frame(&mut cx, (-429i32).to_le_bytes().to_vec());
        cx.run_until_parked();
        assert!(cx.has_text(words::RECONNECTING));
        for _ in 1..delay {
            cx.host().stream::<ClockTicks>().send(());
            cx.run_until_parked();
        }
        assert_eq!(cx.host().requests::<SocketConnect>().len(), before);
        cx.simulate_click("connect");
        cx.run_until_parked();
        assert_eq!(cx.host().requests::<SocketConnect>().len(), before + 1);
        server.opened(&mut cx);
        cx.update(&entity, |view, _, cx| {
            view.live
                .connection
                .as_mut()
                .unwrap()
                .request(999, vec![0; 4]);
            view.flush(cx);
        });
        server.pump(&mut cx);
    }
    login::sign_in(&mut cx, &mut server);
    server.frame(&mut cx, (-429i32).to_le_bytes().to_vec());
    cx.run_until_parked();
    let sent = cx.host().requests::<SocketSendMessage>().len();
    cx.simulate_click("logout");
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<SocketSendMessage>().len(), sent);
    for key in ["session", "peers", "updates"] {
        assert!(cx.host().kept::<Vec<u8>>(key).is_none());
    }
    assert!(cx.has_text(words::LOCAL_KEYS_REMOVED));
    assert!(!cx.has_text(words::SIGNED_OUT));
}

#[test]
fn host_faults_back_off_without_a_socket_open_loop() {
    let mut cx = server::Server::context();
    cx.host()
        .refuse::<SocketConnect>("host_fault", "temporarily unavailable");
    cx.open::<Telegram>();
    server::seat(&mut cx, "a key");
    for _ in 0..4 {
        cx.host().stream::<ClockTicks>().send(());
        cx.run_until_parked();
    }
    assert_eq!(cx.host().requests::<SocketConnect>().len(), 1);
    cx.host().stream::<ClockTicks>().send(());
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<SocketConnect>().len(), 2);
    for _ in 0..9 {
        cx.host().stream::<ClockTicks>().send(());
        cx.run_until_parked();
    }
    assert_eq!(cx.host().requests::<SocketConnect>().len(), 2);
}
#[test]
fn malformed_stored_values_are_removed_and_explained() {
    use view::methods::{StoreGet, StoreSet};
    for key in ["session", "peers", "updates"] {
        let mut cx = server::Server::context();
        cx.host()
            .handle::<StoreGet>(move |asked| Ok((asked == key).then(|| vec![255])));
        let mut server = server::Server::default();
        server.open(&mut cx);
        assert!(cx.has_text(&words::replaced_store(key)));
        assert!(
            cx.host()
                .requests::<StoreSet>()
                .iter()
                .any(|(removed, value)| removed == key && value.is_none())
        );
        assert!(cx.has_text(words::CONNECTED));
    }
}

#[test]
fn peer_names_and_hashes_are_persisted_only_when_changed() {
    use view::methods::StoreSet;
    let mut cx = server::Server::context();
    let mut server = server::Server::default();
    let entity = server.open(&mut cx);
    let peer_writes = |cx: &view::testing::TestAppContext| {
        cx.host()
            .requests::<StoreSet>()
            .iter()
            .filter(|(key, value)| key == "peers" && value.is_some())
            .count()
    };
    cx.update(&entity, |view, _, cx| {
        view.cache_peers(vec![login::user(8, "Bob", 88)], vec![], cx)
    });
    cx.run_until_parked();
    assert_eq!(peer_writes(&cx), 1);
    cx.update(&entity, |view, _, cx| {
        view.cache_peers(vec![login::user(8, "Bob", 88)], vec![], cx)
    });
    cx.run_until_parked();
    assert_eq!(peer_writes(&cx), 1);
    cx.update(&entity, |view, _, cx| {
        view.cache_peers(vec![login::user(8, "Robert", 88)], vec![], cx)
    });
    cx.run_until_parked();
    assert_eq!(peer_writes(&cx), 2);
    let peers = cx
        .host()
        .kept::<BTreeMap<crate::chats::Peer, crate::chats::PeerInfo>>("peers")
        .unwrap();
    assert_eq!(peers[&crate::chats::Peer::User(8)].name, "Robert");
    cx.update(&entity, |view, _, cx| {
        view.cache_peers(vec![login::user(8, "Robert", 89)], vec![], cx)
    });
    cx.run_until_parked();
    assert_eq!(peer_writes(&cx), 3);
    let grammers_tl_types::enums::User::User(mut min) = login::user(8, "Bob Group", 0) else {
        unreachable!()
    };
    min.min = true;
    cx.update(&entity, |view, _, cx| {
        view.cache_peers(vec![grammers_tl_types::enums::User::User(min)], vec![], cx)
    });
    cx.run_until_parked();
    assert_eq!(
        peer_writes(&cx),
        3,
        "a min user's name never replaces a stored one"
    );
}

#[test]
fn empty_chats_and_messages_show_their_detail_lines() {
    use grammers_tl_types as tl;
    let mut cx = server::Server::context();
    let mut server = server::Server::default();
    let entity = server.open(&mut cx);
    login::sign_in(&mut cx, &mut server);
    let (id, _) = server.take::<tl::functions::messages::GetDialogs>();
    server.reply(
        &mut cx,
        id,
        tl::types::messages::Dialogs {
            dialogs: vec![],
            messages: vec![],
            chats: vec![],
            users: vec![],
        }
        .boxed(),
    );
    assert!(cx.has_text(words::NO_CHATS));
    assert!(cx.has_text(words::NO_CHATS_DETAIL));
    cx.update(&entity, |view, _, cx| {
        view.cache_peers(vec![login::user(8, "Bob", 88)], vec![], cx);
        view.open_chat(crate::chats::Peer::User(8), cx);
    });
    server.pump(&mut cx);
    let (id, _) = server.take::<tl::functions::messages::GetHistory>();
    server.reply(
        &mut cx,
        id,
        tl::types::messages::Messages {
            messages: vec![],
            topics: vec![],
            chats: vec![],
            users: vec![],
        }
        .boxed(),
    );
    assert!(cx.has_text(words::NO_MESSAGES));
    assert!(cx.has_text(words::NO_MESSAGES_DETAIL));
}
