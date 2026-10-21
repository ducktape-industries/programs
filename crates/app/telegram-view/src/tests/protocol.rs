use super::{
    history, login,
    security::connected,
    server::{self, Boxed, Server},
};
use crate::{updates::Position, words};
use grammers_tl_types::{self as tl, Serializable};
use view::methods::*;

#[test]
fn old_messages_are_ignored_without_disconnecting() {
    let (mut cx, mut server) = history::opened();
    server.message -= 400i64 << 32;
    server.send(
        &mut cx,
        tl::types::UpdateShort {
            date: server::UNIX as i32,
            update: tl::enums::Update::NewMessage(tl::types::UpdateNewMessage {
                message: super::chats::message(
                    11,
                    super::chats::peer(8),
                    super::chats::peer(8),
                    server::UNIX as i32,
                    "Too old",
                ),
                pts: 11,
                pts_count: 1,
            }),
        }
        .boxed(),
    );
    server.pump(&mut cx);
    assert!(!cx.has_text("Too old"));
    assert!(cx.has_text(words::SIGNED_IN));
    assert_eq!(cx.host().kept::<Position>("updates").unwrap().pts, 10);
}

#[test]
fn service_constructors_do_not_reach_updates() {
    use tl::Identifiable;
    let (mut cx, mut server) = connected();
    for constructor in [
        tl::types::MsgsStateReq::CONSTRUCTOR_ID,
        tl::types::MsgsStateInfo::CONSTRUCTOR_ID,
        tl::types::MsgsAllInfo::CONSTRUCTOR_ID,
        tl::types::MsgDetailedInfo::CONSTRUCTOR_ID,
        tl::types::MsgNewDetailedInfo::CONSTRUCTOR_ID,
        tl::types::MsgResendReq::CONSTRUCTOR_ID,
        tl::types::FutureSalts::CONSTRUCTOR_ID,
        tl::types::DestroySessionOk::CONSTRUCTOR_ID,
        tl::types::DestroySessionNone::CONSTRUCTOR_ID,
    ] {
        server.send(&mut cx, constructor.to_le_bytes().to_vec());
        server.pump(&mut cx);
        assert!(cx.has_text(words::CONNECTED));
        assert!(!cx.texts().iter().any(|text| text.contains("constructor")));
    }
}

#[test]
fn retryable_bad_messages_resend_with_fresh_ids_and_correct_parity() {
    for code in [18, 19, 20, 34, 35, 64, 32, 33] {
        let (mut cx, mut server) = connected();
        let deliberately_unhandled_pending_call = server.calls.pop().unwrap();
        server.send(
            &mut cx,
            tl::types::BadMsgNotification {
                bad_msg_id: deliberately_unhandled_pending_call.id,
                bad_msg_seqno: 1,
                error_code: code,
            }
            .boxed(),
        );
        server.pump(&mut cx);
        let retry = server.calls.pop().expect("pending request resent");
        assert_eq!(retry.body, deliberately_unhandled_pending_call.body);
        assert!(retry.id > deliberately_unhandled_pending_call.id);
        if matches!(code, 32 | 33) {
            assert_ne!(retry.session, deliberately_unhandled_pending_call.session);
            assert_eq!(retry.sequence, 1);
        }
        assert_eq!(retry.sequence % 2, if code == 34 { 0 } else { 1 });
        assert!(cx.has_text(words::CONNECTED));
    }
}

#[test]
fn new_session_requests_difference_when_signed_in() {
    let mut cx = Server::context();
    let mut server = Server::default();
    server.open(&mut cx);
    login::sign_in(&mut cx, &mut server);
    server.send(
        &mut cx,
        tl::types::NewSessionCreated {
            first_msg_id: 1,
            unique_id: 2,
            server_salt: 3,
        }
        .boxed(),
    );
    server.pump(&mut cx);
    let (_, request) = server.take::<tl::functions::updates::GetDifference>();
    assert_eq!(request.pts, 10);
}

#[test]
fn chain_heads_cannot_overwrite_telegram_time() {
    let mut cx = Server::context();
    let mut server = Server::default();
    let entity = server.open(&mut cx);
    cx.host().stream::<ChainHeads>().send(Head {
        height: 1,
        time: (server::UNIX as u64 + 600) * 1000,
        id: [0; 32],
    });
    cx.run_until_parked();
    cx.update(&entity, |view, _, _| {
        assert_eq!(view.live.unix, server::UNIX);
        assert_eq!(view.live.connection.as_ref().unwrap().unix, server::UNIX);
    });
    login::sign_in(&mut cx, &mut server);
    assert!(cx.has_text(words::SIGNED_IN));
}

#[test]
fn unfactorable_pq_is_a_refusal() {
    use crate::auth::{Handshake, decode};
    crate::random::seed([19; 32]);
    let (mut handshake, request) = Handshake::new();
    let request: tl::functions::ReqPqMulti = decode(&request[4..]).unwrap();
    assert!(
        handshake
            .pq(&tl::types::ResPq {
                nonce: request.nonce,
                server_nonce: [1; 16],
                pq: 101u64.to_be_bytes().to_vec(),
                server_public_key_fingerprints: vec![0x54455354],
            }
            .boxed())
            .is_err()
    );
}

#[test]
fn client_ids_sequences_session_and_initialization_follow_the_connection() {
    use crate::{Telegram, rpc::Rpc};
    use tl::Identifiable;
    let mut cx = Server::context();
    let mut server = Server::default();
    let entity = server.open(&mut cx);
    cx.update(&entity, |view: &mut Telegram, _, cx| {
        for _ in 0..3 {
            view.call(
                Rpc::State,
                tl::functions::updates::GetState {}.to_bytes(),
                cx,
            );
        }
    });
    server.pump(&mut cx);
    assert_eq!(
        server
            .calls
            .iter()
            .map(|call| call.sequence)
            .collect::<Vec<_>>(),
        [1, 3, 5]
    );
    assert_eq!(
        server
            .calls
            .iter()
            .map(|call| call.initialized)
            .collect::<Vec<_>>(),
        [true, false, false]
    );
    server.send(
        &mut cx,
        tl::types::NewSessionCreated {
            first_msg_id: 1,
            unique_id: 2,
            server_salt: 3,
        }
        .boxed(),
    );
    server.pump(&mut cx);
    for _ in 0..30 {
        cx.host().stream::<ClockTicks>().send(());
    }
    server.pump(&mut cx);
    assert!(server.messages.iter().any(|call| {
        call.body
            .starts_with(&tl::functions::PingDelayDisconnect::CONSTRUCTOR_ID.to_le_bytes())
    }));
    for pair in server.messages.windows(2) {
        assert!(pair[1].id > pair[0].id);
        assert_ne!(pair[1].id & 0xffff_ffff, 0);
        assert!(pair[1].sequence >= pair[0].sequence);
        assert_eq!(pair[1].session, pair[0].session);
    }
    for call in &server.messages {
        let ack = call
            .body
            .starts_with(&tl::types::MsgsAck::CONSTRUCTOR_ID.to_le_bytes());
        assert_eq!(call.sequence % 2, i32::from(!ack));
    }
}

#[test]
fn a_skewed_initial_chain_clock_is_corrected_by_telegram() {
    let mut cx = Server::context();
    cx.host().handle::<ChainStatus>(|_| {
        Ok(NodeStatus {
            time: (server::UNIX as u64 + 600) * 1000,
            ..Default::default()
        })
    });
    let mut server = Server::default();
    server.open(&mut cx);
    login::sign_in(&mut cx, &mut server);
    assert_eq!(
        cx.host().kept::<crate::Session>("session").unwrap().keys[&2].offset,
        -600
    );
    assert!(
        server
            .messages
            .iter()
            .all(|call| call.id >> 32 == server::UNIX)
    );
}

#[test]
fn a_message_over_300_seconds_ahead_waits_for_bad_msg_16_clock_correction() {
    use crate::{Session, protocol::DcKey};
    let mut cx = Server::context();
    cx.host().handle::<ChainStatus>(|_| {
        Ok(NodeStatus {
            time: (server::UNIX as u64 - 400) * 1000,
            ..Default::default()
        })
    });
    let key = [71; 256];
    cx.host().keep(
        "session",
        &Session {
            home: 2,
            keys: [(
                2,
                DcKey {
                    key,
                    salt: 0,
                    offset: 0,
                },
            )]
            .into(),
            user: Some(7),
        },
    );
    let mut server = Server::default();
    server.key = Some(key);
    let _entity = server.open(&mut cx);
    let (id, _) = server.take::<tl::functions::updates::GetState>();
    assert_eq!(id >> 32, server::UNIX - 400);
    server.reply(
        &mut cx,
        id,
        tl::types::updates::State {
            pts: 10,
            qts: 0,
            date: server::UNIX as i32,
            seq: 1,
            unread_count: 0,
        }
        .boxed(),
    );
    assert!(cx.host().kept::<Position>("updates").is_none());
    assert_eq!(
        cx.host().kept::<Session>("session").unwrap().keys[&2].offset,
        0
    );
}

#[test]
fn a_stored_key_recovers_from_a_chain_clock_120_seconds_behind() {
    use crate::{Session, protocol::DcKey};
    use tl::Identifiable;
    let mut cx = Server::context();
    cx.host().handle::<ChainStatus>(|_| {
        Ok(NodeStatus {
            time: (server::UNIX as u64 - 120) * 1000,
            ..Default::default()
        })
    });
    let key = [71; 256];
    cx.host().keep(
        "session",
        &Session {
            home: 2,
            keys: [(
                2,
                DcKey {
                    key,
                    salt: 0,
                    offset: 0,
                },
            )]
            .into(),
            user: Some(7),
        },
    );
    let mut server = Server::default();
    server.key = Some(key);
    let entity = server.open(&mut cx);
    let (id, _) = server.take::<tl::functions::updates::GetState>();
    assert_eq!(id >> 32, server::UNIX - 120);
    server.reply(
        &mut cx,
        id,
        tl::types::updates::State {
            pts: 10,
            qts: 0,
            date: server::UNIX as i32,
            seq: 1,
            unread_count: 0,
        }
        .boxed(),
    );
    assert_eq!(cx.host().kept::<Position>("updates").unwrap().pts, 10);
    assert_eq!(
        cx.host().kept::<Session>("session").unwrap().keys[&2].offset,
        120
    );
    for _ in 0..120 {
        server.message += 1i64 << 32;
        cx.host().stream::<ClockTicks>().send(());
        server.pump(&mut cx);
    }
    assert_eq!(
        server
            .messages
            .iter()
            .filter(|call| call
                .body
                .starts_with(&tl::functions::PingDelayDisconnect::CONSTRUCTOR_ID.to_le_bytes()))
            .count(),
        4
    );
    assert_eq!(cx.host().requests::<SocketConnect>().len(), 1);
    assert_eq!(server.headers, 1, "the stored key needs no DH");
    assert_eq!(
        cx.host().kept::<Session>("session").unwrap().keys[&2].offset,
        120
    );
    cx.update(&entity, |view, _, _| {
        assert!(view.live.ready);
        assert!(view.failed.is_none());
    });
}

#[test]
fn future_plain_ids_have_no_window_and_only_dh_server_time_sets_offset() {
    let mut cx = Server::context();
    let mut server = Server::default();
    server.message += 120i64 << 32;
    server.open(&mut cx);
    assert!(cx.has_text(words::CONNECTED));
    assert_eq!(
        cx.host().kept::<crate::Session>("session").unwrap().keys[&2].offset,
        0
    );
}

#[test]
fn a_pq_that_exhausts_the_iteration_budget_is_refused() {
    use crate::auth::{Handshake, decode};
    crate::random::seed([19; 32]);
    let (mut handshake, request) = Handshake::new();
    let request: tl::functions::ReqPqMulti = decode(&request[4..]).unwrap();
    let response = tl::types::ResPq {
        nonce: request.nonce,
        server_nonce: [1; 16],
        pq: {
            let large_prime_without_proper_factor_exceeding_rho_budget = u64::MAX - 58;
            large_prime_without_proper_factor_exceeding_rho_budget
                .to_be_bytes()
                .to_vec()
        },
        server_public_key_fingerprints: vec![0x54455354],
    }
    .boxed();
    assert_eq!(handshake.pq(&response), Err(words::UNFACTORED_PQ.into()));
}
