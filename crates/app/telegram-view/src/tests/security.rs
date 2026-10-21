use super::{
    history,
    server::{self, AuthFault, Boxed, Server},
};
use crate::{Session, Telegram, words};
use grammers_tl_types::{self as tl, Serializable};
use view::methods::{HostRandom, SocketConnect, StoreGet, StoreSet};

#[test]
fn a_locked_ducktape_reads_no_telegram_store_and_opens_no_socket() {
    let mut cx = Server::context();
    cx.open::<Telegram>();
    assert!(cx.host().requests::<StoreGet>().is_empty());
    assert!(cx.host().requests::<SocketConnect>().is_empty());
    assert!(cx.has_text(words::LOCKED));
    assert!(cx.find("login").is_none());
    assert!(cx.find("connect").is_none());

    server::seat(&mut cx, "");
    assert!(cx.host().requests::<HostRandom>().is_empty());
    assert!(cx.host().requests::<StoreGet>().is_empty());
    assert!(cx.host().requests::<StoreSet>().is_empty());
    assert!(cx.host().requests::<SocketConnect>().is_empty());
    assert!(cx.has_text(words::LOCKED));
    for field in ["phone", "code", "password", "login", "connect"] {
        assert!(cx.find(field).is_none());
    }
}

#[test]
fn locking_drops_the_signed_in_session_and_unlocking_restores_it() {
    let (mut cx, mut server) = history::opened();
    assert!(cx.has_text("Hello from Bob"));
    let sockets = cx.host().requests::<SocketConnect>();
    let reads = cx.host().requests::<StoreGet>().len();
    let writes = cx.host().requests::<StoreSet>().len();
    server::seat(&mut cx, "");
    assert_eq!(cx.host().requests::<SocketConnect>(), sockets);
    assert_eq!(cx.host().requests::<StoreGet>().len(), reads);
    assert_eq!(cx.host().requests::<StoreSet>().len(), writes);
    assert!(cx.has_text(words::LOCKED));
    assert!(!cx.has_text("Hello from Bob"));
    assert!(!cx.has_text("Bob"));
    for node in [
        "history", "dialogs", "compose", "logout", "login", "connect",
    ] {
        assert!(cx.find(node).is_none());
    }
    assert!(!cx.host().stream::<SocketConnect>().subscribed());
    assert!(!cx.host().stream::<view::methods::ClockTicks>().subscribed());
    assert_eq!(cx.host().kept::<Session>("session").unwrap().user, Some(7));

    server::seat(&mut cx, "a key");
    let restored = cx.host().requests::<SocketConnect>();
    assert_eq!(restored.len(), sockets.len() + 1);
    assert!(restored.last().unwrap().socket > sockets.last().unwrap().socket);
    server.opened(&mut cx);
    assert!(cx.has_text(words::SIGNED_IN));
    assert!(cx.find("dialogs").is_some());
    assert!(cx.find("logout").is_some());
    assert!(cx.find("login").is_none());
}

#[test]
fn a_click_on_connect_while_locked_opens_nothing() {
    for seated in [true, false] {
        let mut cx = Server::context();
        let entity = cx.open::<Telegram>();
        if seated {
            server::seat(&mut cx, "");
        }
        cx.update(&entity, |view, _, cx| view.connect(cx));
        cx.run_until_parked();
        assert!(cx.host().requests::<SocketConnect>().is_empty());
        assert!(cx.host().requests::<StoreGet>().is_empty());
    }
}

fn refused_dh(fault: AuthFault) {
    let mut cx = Server::context();
    let mut server = Server::default();
    server.fault = Some(fault);
    server.open(&mut cx);
    assert_eq!(server.client_dh, 0, "SetClientDhParams must not be sent");
    assert!(cx.has_text(words::RECONNECTING), "{:?}", cx.texts());
    assert!(cx.host().kept::<Session>("session").is_none());
}

#[test]
fn dh_public_one_is_refused() {
    refused_dh(AuthFault::PublicOne);
}
#[test]
fn dh_prime_with_wrong_subgroup_is_refused() {
    refused_dh(AuthFault::PrimeSubgroup);
}
#[test]
fn wrong_server_nonce_is_refused() {
    refused_dh(AuthFault::ServerNonce);
}
#[test]
fn dh_answer_hash_is_checked() {
    refused_dh(AuthFault::AnswerHash);
}
#[test]
fn dh_inner_nonce_is_checked() {
    refused_dh(AuthFault::InnerNonce);
}
#[test]
fn pq_nonce_is_checked() {
    refused_dh(AuthFault::PqNonce);
}

#[test]
fn flipped_key_confirmation_is_never_stored() {
    let mut cx = Server::context();
    let mut server = Server::default();
    server.fault = Some(AuthFault::NewNonceHash);
    server.open(&mut cx);
    assert_eq!(server.client_dh, 1);
    assert!(cx.host().kept::<Session>("session").is_none());
    assert!(cx.has_text(words::RECONNECTING));
}

pub(super) fn connected() -> (view::testing::TestAppContext, Server) {
    let mut cx = Server::context();
    let mut server = Server::default();
    let entity = server.open(&mut cx);
    cx.update(&entity, |view, _, cx| {
        view.live
            .connection
            .as_mut()
            .unwrap()
            .request(0, vec![0; 4]);
        view.flush(cx);
    });
    server.pump(&mut cx);
    (cx, server)
}

#[test]
fn wrong_envelope_session_is_refused() {
    let (mut cx, mut server) = connected();
    server.session ^= 1;
    server.send(
        &mut cx,
        tl::types::NewSessionCreated {
            first_msg_id: 1,
            unique_id: 2,
            server_salt: 3,
        }
        .boxed(),
    );
    cx.run_until_parked();
    assert!(cx.has_text(words::RECONNECTING));
}

fn refused_padding(padding: usize) {
    let (mut cx, mut server) = connected();
    let mut body = if padding == 8 {
        let mut empty_container_8_bytes_before_envelope_and_padding =
            0x73f1f8dcu32.to_le_bytes().to_vec();
        0i32.serialize(&mut empty_container_8_bytes_before_envelope_and_padding);
        empty_container_8_bytes_before_envelope_and_padding
    } else {
        let mut unsolicited_rpc_result_16_bytes_without_pending_request =
            0xf35c6d01u32.to_le_bytes().to_vec();
        0i64.serialize(&mut unsolicited_rpc_result_16_bytes_without_pending_request);
        true.serialize(&mut unsolicited_rpc_result_16_bytes_without_pending_request);
        unsolicited_rpc_result_16_bytes_without_pending_request
    };
    let packet = server.envelope_padded(std::mem::take(&mut body), Some(padding));
    server.frame(&mut cx, packet);
    cx.run_until_parked();
    assert!(cx.has_text(words::RECONNECTING));
}
#[test]
fn eight_byte_padding_is_refused() {
    refused_padding(8);
}
#[test]
fn oversized_padding_is_refused() {
    refused_padding(1040);
}

#[test]
fn duplicated_zero_pts_count_packet_is_acked_twice_with_one_checkpoint_and_visible_row() {
    use view::methods::StoreSet;
    let (mut cx, mut server) = history::opened();
    let body = tl::types::UpdateShort {
        update: tl::enums::Update::NewMessage(tl::types::UpdateNewMessage {
            message: super::chats::message(
                11,
                super::chats::peer(8),
                super::chats::peer(8),
                server::UNIX as i32,
                "Only once",
            ),
            pts: 10,
            pts_count: 0,
        }),
        date: server::UNIX as i32,
    }
    .boxed();
    let packet = server.envelope(body);
    let id = server.message;
    server.frame(&mut cx, packet.clone());
    server.pump(&mut cx);
    let writes = cx.host().requests::<StoreSet>().len();
    server.frame(&mut cx, packet);
    server.pump(&mut cx);
    assert_eq!(cx.host().requests::<StoreSet>().len(), writes);
    assert_eq!(server.acks.iter().filter(|ack| **ack == id).count(), 2);
    assert_eq!(
        cx.texts()
            .iter()
            .filter(|text| text.as_str() == "Only once")
            .count(),
        1
    );
}

#[test]
fn container_inner_id_must_precede_its_container() {
    let (mut cx, mut server) = connected();
    let body = tl::types::NewSessionCreated {
        first_msg_id: 1,
        unique_id: 2,
        server_salt: 3,
    }
    .boxed();
    let mut container = 0x73f1f8dcu32.to_le_bytes().to_vec();
    1i32.serialize(&mut container);
    (server.message + 4).serialize(&mut container);
    1i32.serialize(&mut container);
    (body.len() as i32).serialize(&mut container);
    container.extend(body);
    server.send(&mut cx, container);
    cx.run_until_parked();
    assert!(cx.has_text(words::RECONNECTING));
}

#[test]
fn dh_generator_outside_supported_range_is_refused() {
    refused_dh(AuthFault::Generator);
}

#[test]
fn final_server_nonce_is_checked() {
    let mut cx = Server::context();
    let mut server = Server::default();
    server.fault = Some(AuthFault::FinalNonce);
    server.open(&mut cx);
    assert!(cx.host().kept::<Session>("session").is_none());
    assert!(cx.has_text(words::RECONNECTING));
}

#[test]
fn even_server_message_id_is_refused() {
    let (mut cx, mut server) = connected();
    server.message &= !1;
    server.send(
        &mut cx,
        tl::types::NewSessionCreated {
            first_msg_id: 1,
            unique_id: 2,
            server_salt: 3,
        }
        .boxed(),
    );
    cx.run_until_parked();
    assert!(cx.has_text(words::RECONNECTING));
}
