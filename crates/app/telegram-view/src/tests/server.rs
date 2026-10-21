//! Test-only Telegram server. Its RSA private key is a public test fixture;
//! production builds trust only Telegram's pinned test-server public key.
use crate::auth::{decode, padded_key};
use grammers_crypto::{AuthKey, ObfuscatedCipher, aes, generate_key_data_from_nonce};
use grammers_tl_types::{self as tl, Deserializable, Identifiable, Serializable};
use num_bigint::BigUint;
use sha1::{Digest as _, Sha1};
use sha2::{Digest as _, Sha256};
use view::{methods::*, testing::TestAppContext};

pub(super) const UNIX: i64 = 1_800_000_000;

pub(super) fn seat(cx: &mut TestAppContext, signer: &str) {
    cx.host().stream::<HostSession>().send(Session {
        chain_id: "test#1".into(),
        signer: signer.into(),
        ..Default::default()
    });
    cx.run_until_parked();
}

pub(super) fn prime() -> Vec<u8> {
    grammers_crypto::hex::from_hex(include_str!("prime.txt").trim())
}

pub(super) fn hash(parts: &[&[u8]]) -> [u8; 32] {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update(part);
    }
    hash.finalize().into()
}

fn aes_key(auth: &[u8; 256], msg: &[u8; 16], x: usize) -> ([u8; 32], [u8; 32]) {
    let a = hash(&[msg, &auth[x..x + 36]]);
    let b = hash(&[&auth[40 + x..76 + x], msg]);
    let mut key = [0; 32];
    key[..8].copy_from_slice(&a[..8]);
    key[8..24].copy_from_slice(&b[8..24]);
    key[24..].copy_from_slice(&a[24..]);
    let mut iv = [0; 32];
    iv[..8].copy_from_slice(&b[..8]);
    iv[8..24].copy_from_slice(&a[8..24]);
    iv[24..].copy_from_slice(&b[24..]);
    (key, iv)
}

#[derive(Clone, Debug)]
pub(super) struct Call {
    pub(super) id: i64,
    pub(super) sequence: i32,
    pub(super) salt: i64,
    pub(super) session: i64,
    pub(super) initialized: bool,
    pub(super) body: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AuthFault {
    Generator,
    PublicOne,
    PrimeSubgroup,
    ServerNonce,
    NewNonceHash,
    FinalNonce,
    AnswerHash,
    InnerNonce,
    PqNonce,
}

pub(super) struct Server {
    cipher: Option<ObfuscatedCipher>,
    consumed: usize,
    nonce: [u8; 16],
    server_nonce: [u8; 16],
    new_nonce: [u8; 32],
    secret: BigUint,
    pub(super) key: Option<[u8; 256]>,
    pub(super) session: i64,
    pub(super) message: i64,
    pub(super) calls: Vec<Call>,
    pub(super) messages: Vec<Call>,
    pub(super) acks: Vec<i64>,
    pub(super) headers: usize,
    pub(super) fault: Option<AuthFault>,
    pub(super) client_dh: usize,
}

impl Default for Server {
    fn default() -> Self {
        Self {
            cipher: None,
            consumed: 0,
            nonce: [0; 16],
            server_nonce: [37; 16],
            new_nonce: [0; 32],
            secret: BigUint::from_bytes_be(&[71; 256]),
            key: None,
            session: 0,
            message: (UNIX << 32) | 1,
            calls: Vec::new(),
            messages: Vec::new(),
            acks: Vec::new(),
            headers: 0,
            fault: None,
            client_dh: 0,
        }
    }
}

impl Server {
    pub(super) fn context() -> TestAppContext {
        let cx = TestAppContext::new();
        cx.host().handle::<HostRandom>(|n| Ok(vec![19; n as usize]));
        cx.host().handle::<ChainStatus>(|_| {
            Ok(NodeStatus {
                time: UNIX as u64 * 1000,
                ..Default::default()
            })
        });
        cx
    }

    pub(super) fn open(&mut self, cx: &mut TestAppContext) -> view::Entity<crate::Telegram> {
        let entity = cx.open::<crate::Telegram>();
        seat(cx, "a key");
        self.opened(cx);
        entity
    }

    pub(super) fn opened(&mut self, cx: &mut TestAppContext) {
        self.cipher = None;
        let previous_socket_frames_queued_before_opened =
            cx.host().requests::<SocketSendMessage>().len();
        self.consumed = previous_socket_frames_queued_before_opened;
        cx.host()
            .stream::<SocketConnect>()
            .send(SocketItem::Opened {
                protocol: Some("binary".into()),
            });
        cx.run_until_parked();
        self.pump(cx);
    }

    pub(super) fn pump(&mut self, cx: &mut TestAppContext) {
        loop {
            cx.run_until_parked();
            let sent = cx.host().requests::<SocketSendMessage>();
            if self.consumed == sent.len() {
                break;
            }
            for message in &sent[self.consumed..] {
                let mut bytes = message.body.clone();
                if self.cipher.is_none() {
                    assert!(bytes.len() >= 64);
                    let header: [u8; 64] = bytes[..64].try_into().unwrap();
                    assert_ne!(header[0], 0xef);
                    assert_ne!(&header[4..8], &[0; 4]);
                    assert!(
                        ![b"HEAD".as_slice(), b"POST", b"GET ", b"OPTI"].contains(&&header[..4])
                    );
                    let reverse = std::array::from_fn(|i| header[63 - i]);
                    let mut cipher = ObfuscatedCipher::new(&reverse);
                    let mut decoded_header = header;
                    cipher.decrypt(&mut decoded_header);
                    assert_eq!(&decoded_header[56..60], &[0xee; 4]);
                    self.cipher = Some(cipher);
                    self.headers += 1;
                    bytes.drain(..64);
                }
                self.cipher.as_mut().unwrap().decrypt(&mut bytes);
                let length = i32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
                assert_eq!(length, bytes.len() - 4);
                assert_eq!(length % 4, 0);
                let packet = bytes[4..].to_vec();
                if packet[..8] == [0; 8] {
                    self.auth(cx, &packet[20..]);
                } else {
                    self.encrypted(cx, packet);
                }
            }
            self.consumed = sent.len();
        }
    }

    fn auth(&mut self, cx: &mut TestAppContext, body: &[u8]) {
        let constructor = u32::from_le_bytes(body[..4].try_into().unwrap());
        let reply = match constructor {
            tl::functions::ReqPqMulti::CONSTRUCTOR_ID => {
                let request: tl::functions::ReqPqMulti = decode(&body[4..]).unwrap();
                self.nonce = request.nonce;
                tl::types::ResPq {
                    nonce: if self.fault == Some(AuthFault::PqNonce) {
                        [0; 16]
                    } else {
                        self.nonce
                    },
                    server_nonce: self.server_nonce,
                    pq: (499u64 * 547).to_be_bytes().to_vec(),
                    server_public_key_fingerprints: vec![0x54455354],
                }
                .boxed()
            }
            tl::functions::ReqDhParams::CONSTRUCTOR_ID => {
                let request: tl::functions::ReqDhParams = decode(&body[4..]).unwrap();
                assert_eq!(request.nonce, self.nonce);
                assert_eq!(request.server_nonce, self.server_nonce);
                let n =
                    BigUint::parse_bytes(include_str!("rsa_n.txt").trim().as_bytes(), 10).unwrap();
                let d =
                    BigUint::parse_bytes(include_str!("rsa_d.txt").trim().as_bytes(), 10).unwrap();
                let decrypted =
                    padded_key(&BigUint::from_bytes_be(&request.encrypted_data).modpow(&d, &n));
                let digest = hash(&[&decrypted[32..]]);
                let temporary = std::array::from_fn(|i| decrypted[i] ^ digest[i]);
                let mut encrypted = decrypted[32..].to_vec();
                aes::ige_decrypt(&mut encrypted, &temporary, &[0; 32]);
                let data: Vec<_> = encrypted[..192].iter().rev().copied().collect();
                assert_eq!(&encrypted[192..], &hash(&[&temporary, &data]));
                let mut cursor = tl::Cursor::from_slice(&data);
                let tl::enums::PQInnerData::Data(inner) =
                    tl::enums::PQInnerData::deserialize(&mut cursor).unwrap()
                else {
                    panic!("PQ inner")
                };
                assert_eq!(inner.nonce, self.nonce);
                assert_eq!(inner.server_nonce, self.server_nonce);
                self.new_nonce = inner.new_nonce;
                let mut p = prime();
                if self.fault == Some(AuthFault::PrimeSubgroup) {
                    let prime_minus_two_divisible_by_three_failing_generator_three_subgroup_with_valid_size_and_public_bound =
                        p.last_mut().unwrap();
                    *prime_minus_two_divisible_by_three_failing_generator_three_subgroup_with_valid_size_and_public_bound -= 2;
                }
                let mut ga = BigUint::from(3u32)
                    .modpow(&self.secret, &BigUint::from_bytes_be(&p))
                    .to_bytes_be();
                if self.fault == Some(AuthFault::PublicOne) {
                    ga = vec![1];
                }
                let inner = tl::types::ServerDhInnerData {
                    nonce: self.nonce,
                    server_nonce: if self.fault == Some(AuthFault::InnerNonce) {
                        [0; 16]
                    } else {
                        self.server_nonce
                    },
                    g: if self.fault == Some(AuthFault::Generator) {
                        1
                    } else {
                        3
                    },
                    dh_prime: p,
                    g_a: ga,
                    server_time: UNIX as i32,
                }
                .boxed();
                let mut encrypted = Sha1::digest(&inner).to_vec();
                if self.fault == Some(AuthFault::AnswerHash) {
                    encrypted[0] ^= 1;
                }
                encrypted.extend(inner);
                encrypted.resize(encrypted.len().next_multiple_of(16), 83);
                let (key, iv) = generate_key_data_from_nonce(&self.server_nonce, &self.new_nonce);
                aes::ige_encrypt(&mut encrypted, &key, &iv);
                tl::types::ServerDhParamsOk {
                    nonce: self.nonce,
                    server_nonce: if self.fault == Some(AuthFault::ServerNonce) {
                        [0; 16]
                    } else {
                        self.server_nonce
                    },
                    encrypted_answer: encrypted,
                }
                .boxed()
            }
            tl::functions::SetClientDhParams::CONSTRUCTOR_ID => {
                self.client_dh += 1;
                let request: tl::functions::SetClientDhParams = decode(&body[4..]).unwrap();
                let (key, iv) = generate_key_data_from_nonce(&self.server_nonce, &self.new_nonce);
                let mut encrypted = request.encrypted_data;
                aes::ige_decrypt(&mut encrypted, &key, &iv);
                let mut cursor = tl::Cursor::from_slice(&encrypted[20..]);
                let tl::enums::ClientDhInnerData::Data(inner) =
                    tl::enums::ClientDhInnerData::deserialize(&mut cursor).unwrap();
                assert_eq!(
                    Sha1::digest(&encrypted[20..20 + cursor.pos()])[..],
                    encrypted[..20]
                );
                assert_eq!(inner.nonce, self.nonce);
                assert_eq!(inner.server_nonce, self.server_nonce);
                let key = padded_key(
                    &BigUint::from_bytes_be(&inner.g_b)
                        .modpow(&self.secret, &BigUint::from_bytes_be(&prime())),
                );
                self.key = Some(key);
                let mut confirmation =
                    AuthKey::from_bytes(key).calc_new_nonce_hash(&self.new_nonce, 1);
                if self.fault == Some(AuthFault::NewNonceHash) {
                    confirmation[0] ^= 1;
                }
                tl::types::DhGenOk {
                    nonce: self.nonce,
                    server_nonce: if self.fault == Some(AuthFault::FinalNonce) {
                        [0; 16]
                    } else {
                        self.server_nonce
                    },
                    new_nonce_hash1: confirmation,
                }
                .boxed()
            }
            _ => panic!("unexpected auth {constructor:x}"),
        };
        let mut packet = Vec::new();
        0i64.serialize(&mut packet);
        self.message.serialize(&mut packet);
        (reply.len() as i32).serialize(&mut packet);
        packet.extend(reply);
        self.frame(cx, packet);
    }

    fn encrypted(&mut self, cx: &mut TestAppContext, mut packet: Vec<u8>) {
        let auth = self.key.unwrap();
        assert_eq!(packet[..8], Sha1::digest(auth)[12..20]);
        let msg: [u8; 16] = packet[8..24].try_into().unwrap();
        let (key, iv) = aes_key(&auth, &msg, 0);
        aes::ige_decrypt(&mut packet[24..], &key, &iv);
        let plain = &packet[24..];
        assert_eq!(msg, hash(&[&auth[88..120], plain])[8..24]);
        let salt = i64::from_le_bytes(plain[..8].try_into().unwrap());
        self.session = i64::from_le_bytes(plain[8..16].try_into().unwrap());
        let id = i64::from_le_bytes(plain[16..24].try_into().unwrap());
        assert_eq!(id % 4, 0);
        let sequence = i32::from_le_bytes(plain[24..28].try_into().unwrap());
        let length = i32::from_le_bytes(plain[28..32].try_into().unwrap()) as usize;
        assert!((12..=1024).contains(&(plain.len() - 32 - length)));
        let mut body = plain[32..32 + length].to_vec();
        let initialized =
            body.starts_with(&tl::functions::InvokeWithLayer::<()>::CONSTRUCTOR_ID.to_le_bytes());
        if initialized {
            let mut cursor = tl::Cursor::from_slice(&body[4..]);
            assert_eq!(i32::deserialize(&mut cursor).unwrap(), tl::LAYER);
            assert_eq!(
                u32::deserialize(&mut cursor).unwrap(),
                tl::functions::InitConnection::<()>::CONSTRUCTOR_ID
            );
            assert_eq!(u32::deserialize(&mut cursor).unwrap(), 0);
            let _api_id = i32::deserialize(&mut cursor).unwrap();
            for _ in 0..6 {
                let _ = String::deserialize(&mut cursor).unwrap();
            }
            body = body[4 + cursor.pos()..].to_vec();
        }
        let call = Call {
            id,
            sequence,
            salt,
            session: self.session,
            initialized,
            body: body.clone(),
        };
        self.messages.push(call.clone());
        let constructor = u32::from_le_bytes(body[..4].try_into().unwrap());
        match constructor {
            tl::types::MsgsAck::CONSTRUCTOR_ID => {
                let tl::enums::MsgsAck::Ack(ack) = decode(&body).unwrap();
                self.acks.extend(ack.msg_ids);
            }
            tl::functions::PingDelayDisconnect::CONSTRUCTOR_ID => {
                let ping: tl::functions::PingDelayDisconnect = decode(&body[4..]).unwrap();
                self.send(
                    cx,
                    tl::types::Pong {
                        msg_id: id,
                        ping_id: ping.ping_id,
                    }
                    .boxed(),
                );
            }
            _ => self.calls.push(call),
        }
    }

    pub(super) fn send(&mut self, cx: &mut TestAppContext, body: Vec<u8>) {
        let packet = self.envelope(body);
        self.frame(cx, packet);
    }

    pub(super) fn envelope(&mut self, body: Vec<u8>) -> Vec<u8> {
        self.envelope_padded(body, None)
    }

    pub(super) fn envelope_padded(&mut self, body: Vec<u8>, padding: Option<usize>) -> Vec<u8> {
        let auth = self.key.unwrap();
        let mut plain = Vec::new();
        0i64.serialize(&mut plain);
        self.session.serialize(&mut plain);
        self.message += 4;
        self.message.serialize(&mut plain);
        1i32.serialize(&mut plain);
        (body.len() as i32).serialize(&mut plain);
        plain.extend(body);
        let padded = padding.map_or_else(
            || (plain.len() + 12).next_multiple_of(16),
            |padding| plain.len() + padding,
        );
        assert_eq!(padded % 16, 0);
        plain.resize(padded, 51);
        let msg: [u8; 16] = hash(&[&auth[96..128], &plain])[8..24].try_into().unwrap();
        let (key, iv) = aes_key(&auth, &msg, 8);
        aes::ige_encrypt(&mut plain, &key, &iv);
        let mut packet = Sha1::digest(auth)[12..20].to_vec();
        packet.extend(msg);
        packet.extend(plain);
        packet
    }

    pub(super) fn framed(&mut self, packet: Vec<u8>) -> Vec<u8> {
        let mut framed = (packet.len() as i32).to_le_bytes().to_vec();
        framed.extend(packet);
        self.cipher.as_mut().unwrap().encrypt(&mut framed);
        framed
    }

    pub(super) fn frame(&mut self, cx: &mut TestAppContext, packet: Vec<u8>) {
        let framed = self.framed(packet);
        cx.host()
            .stream::<SocketConnect>()
            .send(SocketItem::Binary(framed));
    }

    pub(super) fn take<T: Deserializable + Identifiable>(&mut self) -> (i64, T) {
        self.find::<T>().expect("expected RPC")
    }

    pub(super) fn find<T: Deserializable + Identifiable>(&mut self) -> Option<(i64, T)> {
        let index = self
            .calls
            .iter()
            .position(|call| call.body.starts_with(&T::CONSTRUCTOR_ID.to_le_bytes()))?;
        let call = self.calls.remove(index);
        Some((call.id, decode(&call.body[4..]).unwrap()))
    }

    pub(super) fn container(&mut self, cx: &mut TestAppContext, bodies: Vec<Vec<u8>>) {
        let mut container = 0x73f1f8dcu32.to_le_bytes().to_vec();
        (bodies.len() as i32).serialize(&mut container);
        for body in bodies {
            self.message += 4;
            self.message.serialize(&mut container);
            1i32.serialize(&mut container);
            (body.len() as i32).serialize(&mut container);
            container.extend(body);
        }
        self.send(cx, container);
        cx.run_until_parked();
    }

    pub(super) fn reply(&mut self, cx: &mut TestAppContext, id: i64, body: Vec<u8>) {
        let mut reply = 0xf35c6d01u32.to_le_bytes().to_vec();
        id.serialize(&mut reply);
        reply.extend(body);
        self.send(cx, reply);
        self.pump(cx);
    }
}

pub(super) trait Boxed: Serializable + Identifiable {
    fn boxed(&self) -> Vec<u8> {
        let mut body = Self::CONSTRUCTOR_ID.to_le_bytes().to_vec();
        self.serialize(&mut body);
        body
    }
}
impl<T: Serializable + Identifiable> Boxed for T {}
