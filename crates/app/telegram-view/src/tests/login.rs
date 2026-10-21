use super::server::{self, Boxed, Server};
use crate::{Session, Telegram, auth::decode, words};
use grammers_tl_types::{self as tl, Identifiable, Serializable};
use view::{methods::*, testing::TestAppContext};

pub(super) fn user(id: i64, name: &str, access: i64) -> tl::enums::User {
    let mut bytes = tl::types::User::CONSTRUCTOR_ID.to_le_bytes().to_vec();
    3u32.serialize(&mut bytes);
    0u32.serialize(&mut bytes);
    id.serialize(&mut bytes);
    access.serialize(&mut bytes);
    name.to_owned().serialize(&mut bytes);
    decode(&bytes).unwrap()
}

pub(super) fn authorized() -> Vec<u8> {
    tl::types::auth::Authorization {
        setup_password_required: false,
        otherwise_relogin_days: None,
        tmp_sessions: None,
        future_auth_token: None,
        user: user(7, "Me", 99),
    }
    .boxed()
}

pub(super) fn sent_code() -> Vec<u8> {
    tl::types::auth::SentCode {
        r#type: tl::enums::auth::SentCodeType::App(tl::types::auth::SentCodeTypeApp { length: 5 }),
        phone_code_hash: "code-hash".into(),
        next_type: None,
        timeout: None,
    }
    .boxed()
}

pub(super) fn phone_to_code(cx: &mut TestAppContext, server: &mut Server) {
    cx.simulate_input("phone", "+9996612345");
    cx.simulate_click("login");
    server.pump(cx);
    let (id, request) = server.take::<tl::functions::auth::SendCode>();
    assert_eq!(request.phone_number, "+9996612345");
    server.reply(cx, id, sent_code());
    assert!(cx.has_text(words::CODE_SENT), "{:?}", cx.texts());
}

pub(super) fn sign_in(cx: &mut TestAppContext, server: &mut Server) {
    phone_to_code(cx, server);
    cx.simulate_input("code", "12345");
    cx.simulate_submit("code");
    server.pump(cx);
    let (id, request) = server.take::<tl::functions::auth::SignIn>();
    assert_eq!(request.phone_code.as_deref(), Some("12345"));
    assert_eq!(request.phone_code_hash, "code-hash");
    server.reply(cx, id, authorized());
    let (id, _) = server.take::<tl::functions::updates::GetState>();
    server.reply(
        cx,
        id,
        tl::types::updates::State {
            pts: 10,
            qts: 0,
            date: super::server::UNIX as i32,
            seq: 1,
            unread_count: 0,
        }
        .boxed(),
    );
    assert!(cx.has_text(words::SIGNED_IN), "{:?}", cx.texts());
}

#[test]
fn phone_then_code_signs_in_and_logout_removes_device_keys() {
    let mut cx = Server::context();
    let mut server = Server::default();
    server.open(&mut cx);
    sign_in(&mut cx, &mut server);
    assert_eq!(cx.host().kept::<Session>("session").unwrap().user, Some(7));
    assert!(
        !cx.snapshot()
            .unwrap()
            .windows(10)
            .any(|bytes| bytes == b"9996612345")
    );
    cx.simulate_click("logout");
    server.pump(&mut cx);
    let (id, _) = server.take::<tl::functions::auth::LogOut>();
    assert_eq!(cx.host().kept::<Session>("session").unwrap().user, Some(7));
    assert!(!cx.has_text(words::SIGNED_OUT));
    server.reply(
        &mut cx,
        id,
        tl::types::auth::LoggedOut {
            future_auth_token: None,
        }
        .boxed(),
    );
    for key in ["session", "peers", "updates"] {
        assert!(cx.host().kept::<Vec<u8>>(key).is_none());
    }
    assert!(cx.has_text(words::SIGNED_OUT));
    assert!(!cx.has_text(words::LOCAL_KEYS_REMOVED));
    cx.simulate_click("connect");
    cx.run_until_parked();
    server.opened(&mut cx);
    sign_in(&mut cx, &mut server);
    assert_eq!(server.headers, 2);
    for (_, value) in cx.host().requests::<StoreSet>() {
        if let Some(bytes) = value {
            assert!(
                !bytes
                    .windows(b"+9996612345".len())
                    .any(|part| part == b"+9996612345")
            );
            assert!(!bytes.windows(5).any(|part| part == b"12345"));
        }
    }
}

#[test]
fn a_wrong_code_can_be_corrected_without_losing_the_phone() {
    let mut cx = Server::context();
    let mut server = Server::default();
    server.open(&mut cx);
    phone_to_code(&mut cx, &mut server);
    cx.simulate_input("code", "00000");
    cx.simulate_click("login");
    server.pump(&mut cx);
    let (id, _) = server.take::<tl::functions::auth::SignIn>();
    server.reply(
        &mut cx,
        id,
        tl::types::RpcError {
            error_code: 400,
            error_message: "PHONE_CODE_INVALID".into(),
        }
        .boxed(),
    );
    assert!(cx.has_text(words::WRONG_CODE));
    cx.simulate_input("code", "12345");
    cx.simulate_click("login");
    server.pump(&mut cx);
    let (id, request) = server.take::<tl::functions::auth::SignIn>();
    assert_eq!(request.phone_number, "+9996612345");
    server.reply(&mut cx, id, authorized());
    assert_eq!(cx.host().kept::<Session>("session").unwrap().user, Some(7));
}

#[test]
fn phone_migration_reconnects_to_the_named_test_dc_and_retries() {
    let mut cx = Server::context();
    let mut server = Server::default();
    server.open(&mut cx);
    cx.simulate_input("phone", "+9996612345");
    cx.simulate_click("login");
    server.pump(&mut cx);
    let (id, _) = server.take::<tl::functions::auth::SendCode>();
    server.reply(
        &mut cx,
        id,
        tl::types::RpcError {
            error_code: 303,
            error_message: "PHONE_MIGRATE_4".into(),
        }
        .boxed(),
    );
    let opened = cx.host().requests::<SocketConnect>();
    assert_eq!(
        opened.last().unwrap().url,
        "wss://vesta.web.telegram.org/apiws_test"
    );
    assert_ne!(opened[0].socket, opened[1].socket);
    server.opened(&mut cx);
    let (id, request) = server.take::<tl::functions::auth::SendCode>();
    assert_eq!(request.phone_number, "+9996612345");
    server.reply(&mut cx, id, sent_code());
    let session = cx.host().kept::<Session>("session").unwrap();
    assert_eq!(session.home, 4);
    assert_eq!(session.keys.len(), 2);
}

#[test]
fn a_restart_reads_the_device_store_and_uses_the_same_auth_key() {
    let mut cx = Server::context();
    let mut server = Server::default();
    server.open(&mut cx);
    sign_in(&mut cx, &mut server);
    let key = cx.host().kept::<Session>("session").unwrap().keys[&2].key;
    let snapshot = cx.snapshot().unwrap();
    cx.restore::<Telegram>(&snapshot).unwrap();
    server::seat(&mut cx, "a key");
    server.opened(&mut cx);
    assert_eq!(
        cx.host().kept::<Session>("session").unwrap().keys[&2].key,
        key
    );
    cx.simulate_click("logout");
    server.pump(&mut cx);
    assert_eq!(server.headers, 2);
    let (_, _) = server.take::<tl::functions::auth::LogOut>();
}

fn password_algo(p: Vec<u8>) -> tl::enums::PasswordKdfAlgo {
    tl::enums::PasswordKdfAlgo::Sha256Sha256Pbkdf2Hmacsha512iter100000Sha256ModPow(
        tl::types::PasswordKdfAlgoSha256Sha256Pbkdf2Hmacsha512iter100000Sha256ModPow {
            salt1: vec![1, 2, 3],
            salt2: vec![4, 5, 6],
            g: 3,
            p,
        },
    )
}

fn password_screen(
    cx: &mut TestAppContext,
    server: &mut Server,
    algo: tl::enums::PasswordKdfAlgo,
    srp_b: Vec<u8>,
) {
    phone_to_code(cx, server);
    cx.simulate_input("code", "12345");
    cx.simulate_click("login");
    server.pump(cx);
    let (id, _) = server.take::<tl::functions::auth::SignIn>();
    server.reply(
        cx,
        id,
        tl::types::RpcError {
            error_code: 401,
            error_message: "SESSION_PASSWORD_NEEDED".into(),
        }
        .boxed(),
    );
    let (id, _) = server.take::<tl::functions::account::GetPassword>();
    server.reply(
        cx,
        id,
        tl::types::account::Password {
            has_recovery: false,
            has_secure_values: false,
            has_password: true,
            current_algo: Some(algo),
            srp_b: Some(srp_b),
            srp_id: Some(123),
            hint: None,
            email_unconfirmed_pattern: None,
            new_algo: tl::enums::PasswordKdfAlgo::Unknown,
            new_secure_algo: tl::enums::SecurePasswordKdfAlgo::Unknown,
            secure_random: vec![],
            pending_reset_date: None,
            login_email_pattern: None,
        }
        .boxed(),
    );
}

#[test]
fn two_step_login_checks_srp_for_a_known_password_and_keeps_no_password() {
    use crate::auth::padded_key;
    use num_bigint::BigUint;
    use server::hash;
    let mut cx = Server::context();
    let mut server = Server::default();
    server.open(&mut cx);
    let p = server::prime();
    let prime = BigUint::from_bytes_be(&p);
    let g = padded_key(&BigUint::from(3u32));
    let python_hashlib_ph2_correct_horse_salts_1_2_3_and_4_5_6 = BigUint::from_bytes_be(
        &grammers_crypto::hex::from_hex(include_str!("srp_x.txt").trim()),
    );
    let v = BigUint::from(3u32).modpow(
        &python_hashlib_ph2_correct_horse_salts_1_2_3_and_4_5_6,
        &prime,
    );
    let b = BigUint::from_bytes_be(&[17; 256]);
    let k = BigUint::from_bytes_be(&hash(&[&p, &g]));
    let public_b = padded_key(&((k * &v + BigUint::from(3u32).modpow(&b, &prime)) % &prime));
    password_screen(
        &mut cx,
        &mut server,
        password_algo(p.clone()),
        public_b.to_vec(),
    );
    assert!(matches!(
        cx.node("password"),
        view::wire::Node::Field { secure: true, .. }
    ));
    cx.simulate_input("password", "correct horse");
    assert!(
        !cx.snapshot()
            .unwrap()
            .windows(13)
            .any(|bytes| bytes == b"correct horse")
    );
    cx.simulate_click("login");
    server.pump(&mut cx);
    let (id, request) = server.take::<tl::functions::auth::CheckPassword>();
    let tl::enums::InputCheckPasswordSrp::Srp(proof) = request.password else {
        panic!("SRP proof")
    };
    assert_eq!(proof.srp_id, 123);
    let u = BigUint::from_bytes_be(&hash(&[&proof.a, &public_b]));
    let shared = (BigUint::from_bytes_be(&proof.a) * v.modpow(&u, &prime)).modpow(&b, &prime);
    let key = hash(&[&padded_key(&shared)]);
    let hp = hash(&[&p]);
    let hg = hash(&[&g]);
    let xor: [u8; 32] = std::array::from_fn(|i| hp[i] ^ hg[i]);
    let expected = hash(&[
        &xor,
        &hash(&[&[1, 2, 3]]),
        &hash(&[&[4, 5, 6]]),
        &proof.a,
        &public_b,
        &key,
    ]);
    assert_eq!(proof.m1, expected);
    server.reply(&mut cx, id, authorized());
    assert_eq!(cx.host().kept::<Session>("session").unwrap().user, Some(7));
    for (_, bytes) in cx.host().requests::<StoreSet>() {
        if let Some(bytes) = bytes {
            assert!(!bytes.windows(13).any(|bytes| bytes == b"correct horse"));
        }
    }
}

#[test]
fn replacing_a_connection_stops_its_event_batch() {
    for logout in [false, true] {
        let mut cx = Server::context();
        let mut server = Server::default();
        server.open(&mut cx);
        let (id, reply) = if logout {
            sign_in(&mut cx, &mut server);
            cx.simulate_click("logout");
            server.pump(&mut cx);
            let (id, _) = server.take::<tl::functions::auth::LogOut>();
            (
                id,
                tl::types::auth::LoggedOut {
                    future_auth_token: None,
                }
                .boxed(),
            )
        } else {
            cx.simulate_input("phone", "+9996612345");
            cx.simulate_click("login");
            server.pump(&mut cx);
            let (id, _) = server.take::<tl::functions::auth::SendCode>();
            (
                id,
                tl::types::RpcError {
                    error_code: 303,
                    error_message: "PHONE_MIGRATE_4".into(),
                }
                .boxed(),
            )
        };
        let mut result = 0xf35c6d01u32.to_le_bytes().to_vec();
        id.serialize(&mut result);
        result.extend(reply);
        server.container(
            &mut cx,
            vec![
                result,
                tl::types::NewSessionCreated {
                    first_msg_id: 1,
                    unique_id: 2,
                    server_salt: 3,
                }
                .boxed(),
            ],
        );
        if logout {
            for key in ["session", "peers", "updates"] {
                assert!(cx.host().kept::<Vec<u8>>(key).is_none());
            }
        } else {
            let session = cx.host().kept::<Session>("session").unwrap();
            assert_eq!(session.home, 2);
            assert!(!session.keys.contains_key(&4));
            server.opened(&mut cx);
            let (_, request) = server.take::<tl::functions::auth::SendCode>();
            assert_eq!(request.phone_number, "+9996612345");
        }
    }
}

#[test]
fn a_login_flood_wait_resends_the_same_phone_request_after_ticks() {
    let mut cx = Server::context();
    let mut server = Server::default();
    server.open(&mut cx);
    cx.simulate_input("phone", "+9996612345");
    cx.simulate_click("login");
    server.pump(&mut cx);
    let (id, original) = server.take::<tl::functions::auth::SendCode>();
    server.reply(
        &mut cx,
        id,
        tl::types::RpcError {
            error_code: 420,
            error_message: "FLOOD_WAIT_3".into(),
        }
        .boxed(),
    );
    for _ in 0..2 {
        cx.host().stream::<ClockTicks>().send(());
        server.pump(&mut cx);
    }
    assert!(server.calls.is_empty());
    cx.host().stream::<ClockTicks>().send(());
    server.pump(&mut cx);
    let (retry_id, retry) = server.take::<tl::functions::auth::SendCode>();
    assert!(retry_id > id);
    assert_eq!(retry, original);
    server.reply(&mut cx, retry_id, sent_code());
    assert!(cx.has_text(words::CODE_SENT));
}

#[test]
fn session_revoked_removes_all_stored_telegram_data() {
    let (mut cx, mut server) = super::history::opened();
    cx.simulate_input("compose", "Revoked request");
    cx.simulate_click("send");
    server.pump(&mut cx);
    let (id, _) = server.take::<tl::functions::messages::SendMessage>();
    server.reply(
        &mut cx,
        id,
        tl::types::RpcError {
            error_code: 401,
            error_message: "SESSION_REVOKED".into(),
        }
        .boxed(),
    );
    for key in ["session", "peers", "updates"] {
        assert!(cx.host().kept::<Vec<u8>>(key).is_none());
    }
    assert!(cx.has_text(words::SESSION_ENDED));
}

#[test]
fn unsupported_srp_generator_is_a_refusal() {
    let mut cx = Server::context();
    let mut server = Server::default();
    let entity = server.open(&mut cx);
    cx.update(&entity, |view, _, cx| {
        view.screen = crate::login::Screen::Password;
        view.live.password = Some(tl::types::account::Password {
            has_recovery: false,
            has_secure_values: false,
            has_password: true,
            current_algo: Some(
                tl::enums::PasswordKdfAlgo::Sha256Sha256Pbkdf2Hmacsha512iter100000Sha256ModPow(
                    tl::types::PasswordKdfAlgoSha256Sha256Pbkdf2Hmacsha512iter100000Sha256ModPow {
                        salt1: vec![],
                        salt2: vec![],
                        g: 1,
                        p: server::prime(),
                    },
                ),
            ),
            srp_b: Some(vec![1]),
            srp_id: Some(1),
            hint: None,
            email_unconfirmed_pattern: None,
            new_algo: tl::enums::PasswordKdfAlgo::Unknown,
            new_secure_algo: tl::enums::SecurePasswordKdfAlgo::Unknown,
            secure_random: vec![],
            pending_reset_date: None,
            login_email_pattern: None,
        });
        cx.notify();
    });
    cx.run_until_parked();
    cx.simulate_input("password", "private");
    cx.simulate_click("login");
    server.pump(&mut cx);
    assert!(cx.has_text(words::BAD_PASSWORD_PARAMETERS));
    assert!(server.calls.is_empty());
}

#[test]
fn a_password_challenge_with_b_out_of_range_is_refused() {
    for srp_b in [vec![0], server::prime()] {
        let mut cx = Server::context();
        let mut server = Server::default();
        server.open(&mut cx);
        password_screen(&mut cx, &mut server, password_algo(server::prime()), srp_b);
        cx.simulate_input("password", "private");
        cx.simulate_click("login");
        server.pump(&mut cx);
        assert!(
            server
                .find::<tl::functions::auth::CheckPassword>()
                .is_none()
        );
        assert!(cx.has_text(words::BAD_PASSWORD_PUBLIC), "{:?}", cx.texts());
        assert!(
            matches!(cx.node("password"), view::wire::Node::Field { value, .. } if value.is_empty())
        );
    }
}

#[test]
fn a_password_challenge_with_an_unsafe_prime_is_refused() {
    let mut p = server::prime();
    *p.last_mut().unwrap() -= 2;
    assert!(!grammers_crypto::two_factor_auth::check_p_and_g(&p, &3));
    let mut cx = Server::context();
    let mut server = Server::default();
    server.open(&mut cx);
    password_screen(&mut cx, &mut server, password_algo(p), vec![17; 256]);
    cx.simulate_input("password", "private");
    cx.simulate_click("login");
    server.pump(&mut cx);
    assert!(
        server
            .find::<tl::functions::auth::CheckPassword>()
            .is_none()
    );
    assert!(
        cx.has_text(words::BAD_PASSWORD_PARAMETERS),
        "{:?}",
        cx.texts()
    );
    assert!(
        matches!(cx.node("password"), view::wire::Node::Field { value, .. } if value.is_empty())
    );
}
