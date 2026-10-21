use super::{
    history, login,
    server::{self, AuthFault, Boxed, Server},
    session,
};
use crate::{
    Account, Session, Telegram,
    chats::{Peer, PeerInfo},
    protocol::DcKey,
    updates::Position,
    words,
};
use grammers_tl_types::{self as tl, Identifiable, Serializable};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use view::methods::{
    HostRandom, SocketConnect, SocketItem, StoreGet, StoreSecretGet, StoreSecretSet, StoreSet,
};
use view::testing::TestAppContext;

fn keep_another_accounts_checkpoint(cx: &TestAppContext) {
    cx.host().keep(
        "account",
        &Account {
            home: 2,
            user: Some(99),
        },
    );
    cx.host().keep(
        "updates",
        &Position {
            pts: 9000,
            qts: 13,
            date: server::UNIX as i32,
            seq: 42,
            channels: [(77, 8000)].into(),
        },
    );
    cx.host().keep(
        "peers",
        &BTreeMap::from([(Peer::User(8), ("Old".to_owned(), Some(88i64)))]),
    );
}

fn sign_in_before_the_checkpoint(cx: &mut TestAppContext, server: &mut Server) {
    login::phone_to_code(cx, server);
    cx.simulate_input("code", "12345");
    cx.simulate_submit("code");
    server.pump(cx);
    let (id, _) = server.take::<tl::functions::auth::SignIn>();
    server.reply(cx, id, login::authorized());
}

fn receive_one_update(cx: &mut TestAppContext, server: &mut Server) {
    server.send(
        cx,
        tl::types::UpdateShort {
            update: tl::enums::Update::NewMessage(tl::types::UpdateNewMessage {
                message: super::chats::message(
                    11,
                    super::chats::peer(7),
                    super::chats::peer(8),
                    server::UNIX as i32,
                    "Before reopening",
                ),
                pts: 11,
                pts_count: 1,
            }),
            date: server::UNIX as i32,
        }
        .boxed(),
    );
    server.pump(cx);
    assert_eq!(cx.host().kept::<Position>("updates").unwrap().pts, 11);
}

#[test]
fn a_sign_in_after_the_keys_are_lost_starts_its_own_checkpoint() {
    let mut cx = Server::context();
    keep_another_accounts_checkpoint(&cx);
    let mut server = Server::default();
    let entity = server.open(&mut cx);
    sign_in_before_the_checkpoint(&mut cx, &mut server);
    if let Some((_, request)) = server.find::<tl::functions::updates::GetDifference>() {
        panic!(
            "new user 7 resumes old user 99 checkpoint: pts {}, qts {}, date {}",
            request.pts, request.qts, request.date
        );
    }
    assert!(
        server.calls[0]
            .body
            .starts_with(&tl::functions::updates::GetState::CONSTRUCTOR_ID.to_le_bytes())
    );
    assert!(cx.host().kept::<Position>("updates").is_none());
    let (id, _) = server.take::<tl::functions::updates::GetState>();
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
    let position = cx.host().kept::<Position>("updates").unwrap();
    assert_eq!((position.pts, position.qts, position.seq), (10, 0, 1));
    assert!(position.channels.is_empty());
    let peers = cx
        .host()
        .kept::<BTreeMap<Peer, PeerInfo>>("peers")
        .unwrap_or_default();
    assert!(!peers.contains_key(&Peer::User(8)));
    cx.update(&entity, |view, _, _| {
        assert!(!view.chats.peers.contains_key(&Peer::User(8)));
    });
}

#[test]
fn a_load_without_a_home_key_drops_the_old_peers_and_position() {
    let mut cx = Server::context();
    keep_another_accounts_checkpoint(&cx);
    let entity = cx.open::<Telegram>();
    server::seat(&mut cx, "a key");
    for key in ["updates", "peers"] {
        assert_eq!(
            cx.host()
                .requests::<StoreSet>()
                .iter()
                .filter(|(removed, value)| removed == key && value.is_none())
                .count(),
            1,
            "load without a home key did not remove {key} exactly once"
        );
    }
    assert!(!cx.has_text("Old"));
    cx.update(&entity, |view, _, _| {
        assert_eq!(view.live.session.user, None);
        assert!(view.chats.peers.is_empty());
        assert!(view.sync.position.is_none());
        assert_eq!(view.peer_name(Peer::User(8)), words::user(8));
    });
}

#[test]
fn with_no_key_store_a_second_run_never_resumes_the_first_runs_counters() {
    let mut cx = Server::context();
    cx.host().handle::<StoreSecretGet>(|_| {
        Err(view::host::Error::new("no_secret_store", "No key store."))
    });
    cx.host().handle::<StoreSecretSet>(|_| {
        Err(view::host::Error::new("no_secret_store", "No key store."))
    });
    let mut server = Server::default();
    server.open(&mut cx);
    login::sign_in(&mut cx, &mut server);
    receive_one_update(&mut cx, &mut server);
    let mut reopened = Server::default();
    reopened.open(&mut cx);
    sign_in_before_the_checkpoint(&mut cx, &mut reopened);
    assert!(
        reopened.calls[0]
            .body
            .starts_with(&tl::functions::updates::GetState::CONSTRUCTOR_ID.to_le_bytes()),
        "second memory-only run resumed the first run's counters"
    );
}

#[test]
fn a_reopen_with_the_home_key_kept_resumes_its_checkpoint() {
    let mut cx = Server::context();
    let mut server = Server::default();
    let entity = server.open(&mut cx);
    login::sign_in(&mut cx, &mut server);
    receive_one_update(&mut cx, &mut server);
    cx.update(&entity, |view, _, cx| {
        view.cache_peers(vec![login::user(8, "Kept", 88)], vec![], cx);
        view.channel_position(77, 8000, cx);
    });
    cx.run_until_parked();
    let mut reopened = Server::default();
    reopened.key = server.key;
    let entity = reopened.open(&mut cx);
    let position = cx.host().kept::<Position>("updates").unwrap();
    assert_eq!(position.pts, 11);
    assert_eq!(position.channels[&77], 8000);
    assert!(
        reopened.calls[0]
            .body
            .starts_with(&tl::functions::updates::GetDifference::CONSTRUCTOR_ID.to_le_bytes())
    );
    let (_, request) = reopened.take::<tl::functions::updates::GetDifference>();
    assert_eq!(
        (request.pts, request.qts, request.date),
        (11, 0, position.date)
    );
    cx.update(&entity, |view, _, _| {
        assert_eq!(view.peer_name(Peer::User(8)), "Kept");
        assert_eq!(view.sync.position.as_ref().unwrap().channels[&77], 8000);
    });
}

struct KeyStore {
    keys: Option<Vec<u8>>,
    refuses: bool,
}

impl KeyStore {
    fn secret(&self, key: &str) -> Option<BTreeMap<u8, DcKey>> {
        assert_eq!(key, "keys");
        self.keys
            .as_ref()
            .map(|bytes| view::methods::decode(bytes).unwrap())
    }
}

fn a_key_store_that_can_refuse(cx: &TestAppContext) -> Rc<RefCell<KeyStore>> {
    let keys = cx.host().secret::<BTreeMap<u8, DcKey>>("keys");
    let store = Rc::new(RefCell::new(KeyStore {
        keys: keys.as_ref().map(view::methods::encode),
        refuses: true,
    }));
    let read = store.clone();
    cx.host().handle::<StoreSecretGet>(move |key| {
        assert_eq!(key, "keys");
        Ok(read.borrow().keys.clone())
    });
    let written = store.clone();
    cx.host().handle::<StoreSecretSet>(move |(key, value)| {
        assert_eq!(key, "keys");
        let mut store = written.borrow_mut();
        if store.refuses {
            return Err(view::host::Error::new(
                "secret_store_refused",
                "The key store is locked. Unlock it and try again.",
            ));
        }
        store.keys = value;
        Ok(())
    });
    store
}

#[test]
fn a_refused_key_delete_on_log_out_claims_nothing_and_can_be_retried() {
    let mut cx = Server::context();
    let mut server = Server::default();
    server.open(&mut cx);
    login::sign_in(&mut cx, &mut server);
    let store = a_key_store_that_can_refuse(&cx);
    cx.host()
        .stream::<SocketConnect>()
        .send(SocketItem::Closed {
            code: 1006,
            reason: "Connection lost".into(),
        });
    cx.run_until_parked();
    cx.simulate_click("logout");
    cx.run_until_parked();
    assert!(
        !cx.has_text(words::LOCAL_KEYS_REMOVED),
        "offline log out claims the keys were removed before the key store confirms"
    );
    assert!(!cx.has_text(words::SIGNED_OUT));
    assert!(cx.has_text("The key store is locked. Unlock it and try again."));
    assert_eq!(
        store.borrow().secret("keys").unwrap()[&2].key,
        server.key.unwrap()
    );
    assert!(
        cx.has_text("Remove the keys again"),
        "refused removal has no retry button"
    );
    assert!(cx.find("remove-keys-again").is_some());
    cx.host().never::<StoreSecretSet>();
    cx.simulate_click("remove-keys-again");
    cx.run_until_parked();
    assert!(cx.has_text("Removing this device's Telegram keys…"));
    assert!(!cx.has_text(words::LOCAL_KEYS_REMOVED));
    assert!(store.borrow().secret("keys").is_some());
    store.borrow_mut().refuses = false;
    let written = store.clone();
    cx.host().handle::<StoreSecretSet>(move |(key, value)| {
        assert_eq!(key, "keys");
        written.borrow_mut().keys = value;
        Ok(())
    });
    cx.host().answer_held::<StoreSecretSet>();
    cx.run_until_parked();
    assert!(store.borrow().secret("keys").is_none());
    assert!(cx.has_text(words::LOCAL_KEYS_REMOVED));
    assert!(!cx.has_text("Remove the keys again"));
    assert!(cx.find("remove-keys-again").is_none());
    assert!(!cx.has_text("The key store is locked. Unlock it and try again."));
}

#[test]
fn an_online_log_out_claims_removal_only_once_the_key_store_confirms() {
    let mut cx = Server::context();
    let mut server = Server::default();
    server.open(&mut cx);
    login::sign_in(&mut cx, &mut server);
    let store = a_key_store_that_can_refuse(&cx);
    cx.simulate_click("logout");
    server.pump(&mut cx);
    let (id, _) = server.take::<tl::functions::auth::LogOut>();
    server.reply(
        &mut cx,
        id,
        tl::types::auth::LoggedOut {
            future_auth_token: None,
        }
        .boxed(),
    );
    assert!(
        !cx.has_text(words::SIGNED_OUT),
        "online log out claims the keys were removed before the key store confirms"
    );
    assert!(cx.has_text("The key store is locked. Unlock it and try again."));
    assert!(store.borrow().secret("keys").is_some());
    assert!(cx.has_text("Remove the keys again"));
    assert!(cx.find("remove-keys-again").is_some());
    store.borrow_mut().refuses = false;
    cx.simulate_click("connect");
    server.opened(&mut cx);
    login::sign_in(&mut cx, &mut server);
    assert!(cx.has_text(words::SIGNED_IN));
    assert!(!cx.has_text(words::SIGNED_OUT));
    assert!(!cx.has_text("Remove the keys again"));
    assert_eq!(
        store.borrow().secret("keys").unwrap()[&2].key,
        server.key.unwrap()
    );
}

#[test]
fn a_memory_only_log_out_says_so_at_once_and_deletes_nothing() {
    let mut cx = Server::context();
    cx.host().handle::<StoreSecretGet>(|_| {
        Err(view::host::Error::new("no_secret_store", "No key store."))
    });
    cx.host().handle::<StoreSecretSet>(|_| {
        Err(view::host::Error::new("no_secret_store", "No key store."))
    });
    let mut server = Server::default();
    server.open(&mut cx);
    login::sign_in(&mut cx, &mut server);
    cx.host()
        .stream::<SocketConnect>()
        .send(SocketItem::Closed {
            code: 1006,
            reason: "Connection lost".into(),
        });
    cx.run_until_parked();
    cx.simulate_click("logout");
    cx.run_until_parked();
    assert!(cx.has_text(words::LOCAL_KEYS_REMOVED));
    assert!(cx.host().requests::<StoreSecretSet>().is_empty());
    assert!(!cx.has_text("Remove the keys again"));
}

#[test]
fn no_auth_key_reaches_the_plain_store() {
    let mut cx = Server::context();
    let mut server = Server::default();
    server.open(&mut cx);
    login::sign_in(&mut cx, &mut server);
    server.send(
        &mut cx,
        tl::types::NewSessionCreated {
            first_msg_id: 1,
            unique_id: 2,
            server_salt: 1234,
        }
        .boxed(),
    );
    server.pump(&mut cx);
    let key = server.key.unwrap();
    for (name, value) in cx.host().requests::<StoreSet>() {
        if let Some(bytes) = value {
            assert!(
                !bytes.windows(key.len()).any(|part| part == key),
                "auth key reached plain store entry {name}"
            );
        }
    }
    let keys = cx.host().secret::<BTreeMap<u8, DcKey>>("keys").unwrap();
    assert_eq!(keys[&2].key, key);
    assert_eq!(keys[&2].salt, 1234);
}

#[test]
fn a_reopened_view_signs_in_from_the_key_store() {
    let mut cx = Server::context();
    let mut server = Server::default();
    server.open(&mut cx);
    login::sign_in(&mut cx, &mut server);
    let saved = session(&cx);
    assert_eq!(saved.user, Some(7));
    assert_eq!(saved.keys[&2].key, server.key.unwrap());
    let mut reopened = Server::default();
    reopened.key = Some(saved.keys[&2].key);
    let entity = reopened.open(&mut cx);
    cx.update(&entity, |view, _, _| {
        assert!(matches!(view.screen, crate::login::Screen::Chats));
    });
    assert!(cx.has_text(words::SIGNED_IN));
    assert_eq!(reopened.client_dh, 0);
    assert_eq!(reopened.headers, 1);
    assert_eq!(session(&cx).keys[&2].key, saved.keys[&2].key);
}

#[test]
fn an_old_plaintext_session_is_dropped_and_the_view_starts_signed_out() {
    let mut cx = Server::context();
    cx.host().keep(
        "session",
        &Session {
            home: 2,
            user: Some(7),
            keys: [(
                2,
                DcKey {
                    key: [71; 256],
                    salt: 1234,
                    offset: 0,
                },
            )]
            .into(),
        },
    );
    let entity = cx.open::<Telegram>();
    server::seat(&mut cx, "a key");
    assert!(cx.host().kept::<Session>("session").is_none());
    assert!(
        cx.host()
            .requests::<StoreSet>()
            .iter()
            .any(|(key, value)| key == "session" && value.is_none())
    );
    assert!(
        cx.host()
            .requests::<StoreGet>()
            .iter()
            .all(|key| key != "session")
    );
    let mut server = Server::default();
    server.opened(&mut cx);
    assert_eq!(server.client_dh, 1);
    cx.update(&entity, |view, _, _| {
        assert!(matches!(view.screen, crate::login::Screen::Phone));
        assert_eq!(view.live.session.user, None);
    });
    assert!(cx.find("phone").is_some());
    assert!(!cx.has_text(words::SIGNED_IN));
}

#[test]
fn with_no_key_store_the_sign_in_lives_in_memory_and_says_so() {
    for unavailable_when_loading in [true, false] {
        let mut cx = Server::context();
        if unavailable_when_loading {
            cx.host().handle::<StoreSecretGet>(|_| {
                Err(view::host::Error::new("no_secret_store", "No key store."))
            });
        }
        cx.host().handle::<StoreSecretSet>(|_| {
            Err(view::host::Error::new("no_secret_store", "No key store."))
        });
        let mut server = Server::default();
        let entity = server.open(&mut cx);
        login::sign_in(&mut cx, &mut server);
        assert!(cx.has_text(
            "This device has no key store, so Telegram forgets this sign-in when ducktape closes."
        ));
        cx.update(&entity, |view, _, _| {
            assert_eq!(view.live.session.user, Some(7));
            assert_eq!(view.live.session.keys[&2].key, server.key.unwrap());
        });
        let key = server.key.unwrap();
        for (_, value) in cx.host().requests::<StoreSet>() {
            if let Some(bytes) = value {
                assert!(!bytes.windows(key.len()).any(|part| part == key));
            }
        }
        assert!(cx.host().secret::<BTreeMap<u8, DcKey>>("keys").is_none());
        let writes = cx.host().requests::<StoreSecretSet>();
        assert_eq!(writes.len(), usize::from(!unavailable_when_loading));
        let mut reopened = Server::default();
        let entity = reopened.open(&mut cx);
        cx.update(&entity, |view, _, _| {
            assert!(matches!(view.screen, crate::login::Screen::Phone));
            assert_eq!(view.live.session.user, None);
        });
        assert_eq!(reopened.client_dh, 1);
        assert!(cx.find("phone").is_some());
        assert!(!cx.has_text(words::SIGNED_IN));
    }
}

#[test]
fn an_app_without_the_secret_store_opens_no_socket() {
    let mut cx = Server::context();
    cx.host().handle::<StoreSecretGet>(|_| {
        Err(view::host::Error::new(
            "unknown_request",
            "Unknown request.",
        ))
    });
    cx.open::<Telegram>();
    server::seat(&mut cx, "a key");
    assert!(cx.host().requests::<SocketConnect>().is_empty());
    assert!(cx.has_text("Update ducktape to use Telegram."));
    assert!(cx.host().requests::<StoreSecretSet>().is_empty());
    assert!(
        cx.host()
            .requests::<StoreSet>()
            .iter()
            .all(|(_, value)| value.is_none())
    );
}

#[test]
fn a_key_store_that_refuses_opens_no_socket() {
    let mut cx = Server::context();
    cx.host().handle::<StoreSecretGet>(|_| {
        Err(view::host::Error::new(
            "secret_store_refused",
            "The key store is locked. Unlock it and try again.",
        ))
    });
    cx.open::<Telegram>();
    server::seat(&mut cx, "a key");
    assert!(cx.host().requests::<SocketConnect>().is_empty());
    assert!(cx.host().requests::<StoreSecretSet>().is_empty());
    assert!(cx.has_text("The key store is locked. Unlock it and try again."));
}

#[test]
fn logging_out_removes_the_keys_from_the_key_store() {
    let mut cx = Server::context();
    let mut server = Server::default();
    server.open(&mut cx);
    login::sign_in(&mut cx, &mut server);
    assert!(cx.host().secret::<BTreeMap<u8, DcKey>>("keys").is_some());
    assert!(cx.host().kept::<Account>("account").is_some());
    cx.simulate_click("logout");
    server.pump(&mut cx);
    let (id, _) = server.take::<tl::functions::auth::LogOut>();
    server.reply(
        &mut cx,
        id,
        tl::types::auth::LoggedOut {
            future_auth_token: None,
        }
        .boxed(),
    );
    assert!(cx.host().secret::<BTreeMap<u8, DcKey>>("keys").is_none());
    assert!(
        cx.host()
            .requests::<StoreSecretSet>()
            .iter()
            .any(|(key, value)| key == "keys" && value.is_none())
    );
    for key in ["account", "peers", "updates", "session"] {
        assert!(cx.host().kept::<Vec<u8>>(key).is_none());
    }
    assert!(cx.has_text(words::SIGNED_OUT));
}

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
    assert_eq!(super::session(&cx).user, Some(7));

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
    assert!(super::session(&cx).keys.is_empty());
    assert!(cx.host().kept::<Account>("account").is_none());
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
    assert!(super::session(&cx).keys.is_empty());
    assert!(cx.host().kept::<Account>("account").is_none());
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
    assert!(super::session(&cx).keys.is_empty());
    assert!(cx.host().kept::<Account>("account").is_none());
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
