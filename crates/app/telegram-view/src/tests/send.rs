use super::{history::opened, server::Boxed};
use grammers_tl_types as tl;
use view::methods::*;

fn sent(id: i32) -> Vec<u8> {
    tl::types::UpdateShortSentMessage {
        out: true,
        id,
        pts: id,
        pts_count: 1,
        date: super::server::UNIX as i32,
        media: None,
        entities: None,
        ttl_period: None,
    }
    .boxed()
}

#[test]
fn sending_text_uses_random_ids_and_draws_the_confirmed_message() {
    let (mut cx, mut server) = opened();
    cx.simulate_input("compose", "Hello Telegram");
    cx.simulate_click("send");
    server.pump(&mut cx);
    let (id, request) = server.take::<tl::functions::messages::SendMessage>();
    assert_eq!(request.message, "Hello Telegram");
    assert_eq!(
        request.peer,
        tl::enums::InputPeer::User(tl::types::InputPeerUser {
            user_id: 8,
            access_hash: 88
        })
    );
    server.reply(&mut cx, id, sent(11));
    assert!(cx.has_text("Hello Telegram"));
    cx.simulate_input("compose", "Second message");
    cx.simulate_submit("compose");
    server.pump(&mut cx);
    let (id, second) = server.take::<tl::functions::messages::SendMessage>();
    assert_ne!(request.random_id, second.random_id);
    server.reply(&mut cx, id, sent(12));
    assert!(cx.has_text("Second message"));
    assert_eq!(cx.host().requests::<HostRandom>(), [32]);
    cx.simulate_click("back");
    assert!(cx.has_text("Second message"));
}

#[test]
fn a_send_retried_after_reconnect_keeps_its_random_id() {
    let (mut cx, mut server) = opened();
    cx.simulate_input("compose", "Send once");
    cx.simulate_click("send");
    server.pump(&mut cx);
    let (_, request) = server.take::<tl::functions::messages::SendMessage>();
    cx.host()
        .stream::<SocketConnect>()
        .send(SocketItem::Closed {
            code: 1006,
            reason: "connection lost".into(),
        });
    cx.run_until_parked();
    for _ in 0..5 {
        cx.host().stream::<ClockTicks>().send(());
        cx.run_until_parked();
    }
    server.opened(&mut cx);
    let (id, retry) = server.take::<tl::functions::messages::SendMessage>();
    assert_eq!(request.random_id, retry.random_id);
    assert_eq!(request.message, retry.message);
    server.reply(&mut cx, id, sent(11));
    assert!(cx.has_text("Send once"));
}

#[test]
fn a_refused_send_restores_the_text_for_the_reader() {
    let (mut cx, mut server) = opened();
    cx.simulate_input("compose", "Keep this text");
    cx.simulate_click("send");
    server.pump(&mut cx);
    let (id, _) = server.take::<tl::functions::messages::SendMessage>();
    server.reply(
        &mut cx,
        id,
        tl::types::RpcError {
            error_code: 403,
            error_message: "CHAT_WRITE_FORBIDDEN".into(),
        }
        .boxed(),
    );
    assert!(cx.has_text("CHAT_WRITE_FORBIDDEN"));
    cx.simulate_click("send");
    server.pump(&mut cx);
    let (_, request) = server.take::<tl::functions::messages::SendMessage>();
    assert_eq!(request.message, "Keep this text");
}

#[test]
fn signed_in_dc_migration_imports_authorization_and_retries_send_without_restoring_draft() {
    let (mut cx, mut server) = opened();
    cx.simulate_input("compose", "Across DCs");
    cx.simulate_click("send");
    server.pump(&mut cx);
    let (id, original) = server.take::<tl::functions::messages::SendMessage>();
    server.reply(
        &mut cx,
        id,
        tl::types::RpcError {
            error_code: 303,
            error_message: "USER_MIGRATE_4".into(),
        }
        .boxed(),
    );
    let (id, request) = server.take::<tl::functions::auth::ExportAuthorization>();
    assert_eq!(request.dc_id, 4);
    server.reply(
        &mut cx,
        id,
        tl::types::auth::ExportedAuthorization {
            id: 7,
            bytes: vec![1, 2, 3],
        }
        .boxed(),
    );
    assert_eq!(
        cx.host().requests::<SocketConnect>().last().unwrap().url,
        "wss://vesta.web.telegram.org/apiws_test"
    );
    server.opened(&mut cx);
    let (id, request) = server.take::<tl::functions::auth::ImportAuthorization>();
    assert_eq!((request.id, request.bytes), (7, vec![1, 2, 3]));
    assert!(!server.calls.iter().any(|call| call.body.starts_with(
        &<tl::functions::messages::SendMessage as tl::Identifiable>::CONSTRUCTOR_ID.to_le_bytes()
    )));
    assert_eq!(cx.host().kept::<crate::Session>("session").unwrap().home, 2);
    server.reply(&mut cx, id, super::login::authorized());
    assert_eq!(cx.host().kept::<crate::Session>("session").unwrap().home, 4);
    assert_eq!(
        server
            .messages
            .iter()
            .filter(|call| call.body.starts_with(
                &<tl::functions::updates::GetDifference as tl::Identifiable>::CONSTRUCTOR_ID
                    .to_le_bytes()
            ))
            .count(),
        1,
        "Import starts GetDifference once"
    );
    let (id, retry) = server.take::<tl::functions::messages::SendMessage>();
    assert_eq!(retry.random_id, original.random_id);
    server.reply(&mut cx, id, sent(11));
    assert!(cx.has_text("Across DCs"));
    cx.simulate_click("send");
    server.pump(&mut cx);
    assert!(!server.calls.iter().any(|call| call.body.starts_with(
        &<tl::functions::messages::SendMessage as tl::Identifiable>::CONSTRUCTOR_ID.to_le_bytes()
    )));
}

#[test]
fn repeated_import_errors_fail_send_until_explicit_connect_opens_old_home_without_resending() {
    let (mut cx, mut server) = opened();
    cx.simulate_input("compose", "Across DCs");
    cx.simulate_click("send");
    server.pump(&mut cx);
    let (id, _) = server.take::<tl::functions::messages::SendMessage>();
    server.reply(
        &mut cx,
        id,
        tl::types::RpcError {
            error_code: 303,
            error_message: "USER_MIGRATE_4".into(),
        }
        .boxed(),
    );
    let (id, request) = server.take::<tl::functions::auth::ExportAuthorization>();
    assert_eq!(request.dc_id, 4);
    server.reply(
        &mut cx,
        id,
        tl::types::auth::ExportedAuthorization {
            id: 7,
            bytes: vec![1, 2, 3],
        }
        .boxed(),
    );
    assert_eq!(
        cx.host().requests::<SocketConnect>().last().unwrap().url,
        "wss://vesta.web.telegram.org/apiws_test"
    );
    server.opened(&mut cx);
    let (id, request) = server.take::<tl::functions::auth::ImportAuthorization>();
    assert_eq!((request.id, request.bytes), (7, vec![1, 2, 3]));
    assert!(!server.calls.iter().any(|call| call.body.starts_with(
        &<tl::functions::messages::SendMessage as tl::Identifiable>::CONSTRUCTOR_ID.to_le_bytes()
    )));
    assert_eq!(cx.host().kept::<crate::Session>("session").unwrap().home, 2);
    server.reply(
        &mut cx,
        id,
        tl::types::RpcError {
            error_code: 400,
            error_message: "AUTH_BYTES_INVALID".into(),
        }
        .boxed(),
    );
    assert_eq!(cx.host().kept::<crate::Session>("session").unwrap().home, 2);
    for _ in 0..60 {
        cx.host().stream::<ClockTicks>().send(());
        server.pump(&mut cx);
        for call in std::mem::take(&mut server.calls) {
            let constructor = u32::from_le_bytes(call.body[..4].try_into().unwrap());
            let body = match constructor {
                <tl::functions::messages::SendMessage as tl::Identifiable>::CONSTRUCTOR_ID => {
                    tl::types::RpcError {
                        error_code: 303,
                        error_message: "USER_MIGRATE_4".into(),
                    }
                    .boxed()
                }
                <tl::functions::auth::ExportAuthorization as tl::Identifiable>::CONSTRUCTOR_ID => {
                    tl::types::auth::ExportedAuthorization {
                        id: 7,
                        bytes: vec![1, 2, 3],
                    }
                    .boxed()
                }
                <tl::functions::auth::ImportAuthorization as tl::Identifiable>::CONSTRUCTOR_ID => {
                    tl::types::RpcError {
                        error_code: 400,
                        error_message: "AUTH_BYTES_INVALID".into(),
                    }
                    .boxed()
                }
                _ => continue,
            };
            server.reply(&mut cx, call.id, body);
        }
    }
    let count = |constructor: u32| {
        server
            .messages
            .iter()
            .filter(|call| call.body.starts_with(&constructor.to_le_bytes()))
            .count()
    };
    assert_eq!(
        count(<tl::functions::auth::ExportAuthorization as tl::Identifiable>::CONSTRUCTOR_ID),
        1
    );
    assert_eq!(
        count(<tl::functions::auth::ImportAuthorization as tl::Identifiable>::CONSTRUCTOR_ID),
        1
    );
    assert_eq!(
        cx.host().requests::<SocketConnect>().len(),
        2,
        "no third socket after Import fails"
    );
    assert!(cx.has_text("AUTH_BYTES_INVALID"));
    assert!(
        cx.has_text("Across DCs"),
        "the failed Send is back in the composer"
    );
    assert!(cx.has_text(crate::words::CONNECT));
    cx.simulate_click("connect");
    cx.run_until_parked();
    server.key = Some(cx.host().kept::<crate::Session>("session").unwrap().keys[&2].key);
    server.opened(&mut cx);
    assert!(!server.calls.iter().any(|call| call.body.starts_with(
        &<tl::functions::messages::SendMessage as tl::Identifiable>::CONSTRUCTOR_ID.to_le_bytes()
    )));
    cx.simulate_click("send");
    server.pump(&mut cx);
    let (_, request) = server.take::<tl::functions::messages::SendMessage>();
    assert_eq!(request.message, "Across DCs");
}
