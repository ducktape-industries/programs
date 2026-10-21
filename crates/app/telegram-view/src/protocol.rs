//! The small sender: device time, auth keys, encrypted envelopes and service messages.
use crate::words;
use crate::{
    auth::{Handshake, decode},
    random,
    transport::{Failure, Wire},
};
use grammers_crypto::{AuthKey, DequeBuffer};
use grammers_mtproto::mtp::{Mtp, Plain};
use grammers_tl_types::{self as tl, Deserializable, Identifiable, Serializable};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::Read,
};

#[derive(Clone, borsh::BorshDeserialize, borsh::BorshSerialize)]
pub(crate) struct DcKey {
    pub(crate) key: [u8; 256],
    pub(crate) salt: i64,
    pub(crate) offset: i64,
}

pub(crate) enum Event {
    Ready,
    KeyChanged(Box<DcKey>),
    Rpc(u64, Result<Vec<u8>, tl::types::RpcError>),
    Update(Vec<u8>),
    Wait(u32),
    NewSession,
}

struct Pending {
    tag: u64,
    body: Vec<u8>,
    sequence: i32,
    wait: Option<u32>,
}

pub(crate) struct Connection {
    wire: Wire,
    handshake: Option<(Handshake, u8)>,
    pub(crate) key: Option<DcKey>,
    session: i64,
    sequence: i32,
    last_id: i64,
    seen: BTreeSet<i64>,
    pending: BTreeMap<i64, Pending>,
    pub(crate) unix: i64,
    pub(crate) outgoing: Vec<Vec<u8>>,
    pub(crate) events: Vec<Event>,
    elapsed: u32,
    ping: Option<(i64, u32)>,
}

impl Connection {
    pub(crate) fn new(key: Option<DcKey>, unix: i64) -> Self {
        let mut connection = Self {
            wire: Wire::new(),
            handshake: None,
            key,
            session: i64::from_le_bytes(random::bytes()),
            sequence: 0,
            last_id: 0,
            seen: BTreeSet::new(),
            pending: BTreeMap::new(),
            unix,
            outgoing: Vec::new(),
            events: Vec::new(),
            elapsed: 0,
            ping: None,
        };
        if let Some(key) = &connection.key {
            connection
                .events
                .push(Event::KeyChanged(Box::new(key.clone())));
            connection.events.push(Event::Ready);
        } else {
            let (handshake, body) = Handshake::new();
            connection.handshake = Some((handshake, 0));
            connection.plain(&body);
        }
        connection
    }

    fn plain(&mut self, body: &[u8]) {
        let mut buffer = DequeBuffer::with_capacity(body.len(), 20);
        Plain::new().push(&mut buffer, body);
        let host_message_id_replacing_plain_zero = self.next_id();
        buffer[8..16].copy_from_slice(&host_message_id_replacing_plain_zero.to_le_bytes());
        self.outgoing.push(self.wire.pack(buffer.as_ref()));
    }

    fn reset_plain_ids_after_dh_clock_correction(&mut self) {
        self.last_id = 0;
    }

    fn next_id(&mut self) -> i64 {
        let offset = self.key.as_ref().map_or(0, |key| key.offset);
        let message_id_with_nonzero_fraction_divisible_by_four =
            ((self.unix + offset).saturating_mul(1i64 << 32) & !3) | 4;
        self.last_id =
            message_id_with_nonzero_fraction_divisible_by_four.max(self.last_id.saturating_add(4));
        self.last_id
    }

    pub(crate) fn request(&mut self, tag: u64, body: Vec<u8>) {
        let id = self.next_id();
        let sequence = self.sequence * 2 + 1;
        self.sequence += 1;
        self.send(id, sequence, &body);
        self.pending.insert(
            id,
            Pending {
                tag,
                body,
                sequence,
                wait: None,
            },
        );
    }

    fn send(&mut self, id: i64, sequence: i32, body: &[u8]) {
        let key = self.key.as_ref().expect("encrypted messages follow DH");
        let mut buffer = DequeBuffer::with_capacity(body.len() + 64, 24);
        key.salt.serialize(&mut buffer);
        self.session.serialize(&mut buffer);
        id.serialize(&mut buffer);
        sequence.serialize(&mut buffer);
        (body.len() as i32).serialize(&mut buffer);
        buffer.extend(body);
        grammers_crypto::encrypt_data_v2(&mut buffer, &AuthKey::from_bytes(key.key));
        self.outgoing.push(self.wire.pack(buffer.as_ref()));
    }

    fn service(&mut self, body: Vec<u8>) {
        let id = self.next_id();
        self.send(id, self.sequence * 2, &body);
    }

    pub(crate) fn feed(&mut self, bytes: Vec<u8>) -> Result<(), Failure> {
        for mut packet in self.wire.feed(bytes)? {
            if let Some((handshake, step)) = &mut self.handshake {
                let replies = Plain::new()
                    .deserialize(&mut packet)
                    .map_err(|e| e.to_string())?;
                let grammers_mtproto::mtp::Deserialization::RpcResult(reply) = &replies[0] else {
                    return Err(words::UNEXPECTED_AUTH.into());
                };
                let body = match *step {
                    0 => handshake.pq(&reply.body)?,
                    1 => handshake.dh(&reply.body, self.unix)?,
                    _ => {
                        let (key, salt) = handshake.finish(&reply.body)?;
                        let key = DcKey {
                            key,
                            salt,
                            offset: handshake.offset,
                        };
                        self.key = Some(key.clone());
                        self.reset_plain_ids_after_dh_clock_correction();
                        self.handshake = None;
                        self.events.push(Event::KeyChanged(Box::new(key)));
                        self.events.push(Event::Ready);
                        continue;
                    }
                };
                *step += 1;
                self.plain(&body);
            } else {
                self.encrypted(&mut packet)?;
            }
        }
        Ok(())
    }

    fn encrypted(&mut self, packet: &mut [u8]) -> Result<(), String> {
        let auth = AuthKey::from_bytes(self.key.as_ref().ok_or(words::NO_KEY)?.key);
        let plain = grammers_crypto::decrypt_data_v2(packet, &auth).map_err(|e| e.to_string())?;
        if plain.len() < 32 {
            return Err(words::SHORT_ENVELOPE.into());
        }
        let mut cursor = tl::Cursor::from_slice(plain);
        let _salt = read::<i64>(&mut cursor)?;
        if read::<i64>(&mut cursor)? != self.session {
            return Err(words::BAD_SESSION.into());
        }
        let id = read::<i64>(&mut cursor)?;
        let sequence = read::<i32>(&mut cursor)?;
        let length = read::<i32>(&mut cursor)?;
        let message_length_is_invalid =
            length < 4 || length % 4 != 0 || length as usize > plain.len() - 32;
        if message_length_is_invalid {
            return Err(words::BAD_MESSAGE_LENGTH.into());
        }
        let end = 32 + length as usize;
        let mtproto_v2_padding_length_is_invalid = !(12..=1024).contains(&(plain.len() - end));
        if mtproto_v2_padding_length_is_invalid {
            return Err(words::BAD_PADDING.into());
        }
        let mut queue = VecDeque::from([(id, sequence, plain[32..end].to_vec(), Vec::new())]);
        let mut acks = Vec::new();
        while let Some((id, sequence, body, parents)) = queue.pop_front() {
            let body = unpack(body)?;
            let server_message_id_or_sequence_is_invalid = id <= 0 || id % 2 != 1 || sequence < 0;
            if server_message_id_or_sequence_is_invalid {
                return Err(words::BAD_MESSAGE_ID.into());
            }
            let now = self.unix.saturating_add(self.key.as_ref().unwrap().offset);
            let authenticated_message_seconds_ahead = (id >> 32) - now;
            let authenticated_message_can_correct_clock_within_300_seconds =
                (31..=300).contains(&authenticated_message_seconds_ahead);
            if authenticated_message_can_correct_clock_within_300_seconds {
                self.key.as_mut().unwrap().offset = (id >> 32) - self.unix;
                self.changed_key();
            }
            let container = body.starts_with(&0x73f1f8dcu32.to_le_bytes());
            let message_without_time_correction_is_outside_its_or_parents_time_window = !container
                && !time_correction(&body)
                && (!self.in_replay_window_after_authenticated_clock_correction(id)
                    || parents.iter().any(|id| {
                        !self.in_replay_window_after_authenticated_clock_correction(*id)
                    }));
            if message_without_time_correction_is_outside_its_or_parents_time_window {
                continue;
            }
            let content_message_is_in_time_for_acknowledgment =
                sequence % 2 == 1 && self.in_replay_window_after_authenticated_clock_correction(id);
            if content_message_is_in_time_for_acknowledgment {
                acks.push(id);
            }
            if !self.seen.insert(id) {
                continue;
            }
            let mut cursor = tl::Cursor::from_slice(&body);
            let constructor = read::<u32>(&mut cursor)?;
            match constructor {
                0x73f1f8dc => {
                    let count = read::<i32>(&mut cursor)?;
                    let container_count_is_invalid = count < 0 || count as usize > body.len() / 16;
                    if container_count_is_invalid {
                        return Err(words::BAD_CONTAINER.into());
                    }
                    for _ in 0..count {
                        let inner_id = read::<i64>(&mut cursor)?;
                        let seq = read::<i32>(&mut cursor)?;
                        let len = read::<i32>(&mut cursor)?;
                        let inner_id_or_sequence_exceeds_container =
                            inner_id >= id || seq > sequence;
                        if inner_id_or_sequence_exceeds_container {
                            return Err(words::BAD_CONTAINER_ID.into());
                        }
                        let container_message_length_is_invalid =
                            len < 4 || len % 4 != 0 || len as usize > body.len() - cursor.pos();
                        if container_message_length_is_invalid {
                            return Err(words::BAD_CONTAINER_LENGTH.into());
                        }
                        let mut inner = vec![0; len as usize];
                        cursor.read_exact(&mut inner).map_err(|e| e.to_string())?;
                        let inner = unpack(inner)?;
                        let mut parents = parents.clone();
                        parents.push(id);
                        if time_correction(&inner) {
                            queue.push_front((inner_id, seq, inner, parents));
                        } else {
                            queue.push_back((inner_id, seq, inner, parents));
                        }
                    }
                    if cursor.pos() != body.len() {
                        return Err(words::TRAILING_CONTAINER.into());
                    }
                }
                0xf35c6d01 => {
                    let request_id = read::<i64>(&mut cursor)?;
                    let mut result = body[cursor.pos()..].to_vec();
                    result = unpack(result)?;
                    let error =
                        if result.starts_with(&tl::types::RpcError::CONSTRUCTOR_ID.to_le_bytes()) {
                            Some(decode::<tl::enums::RpcError>(&result)?)
                        } else {
                            None
                        };
                    if let Some(tl::enums::RpcError::Error(error)) = error {
                        if let Some(seconds) = error
                            .error_message
                            .strip_prefix("FLOOD_WAIT_")
                            .and_then(|n| n.parse::<u32>().ok())
                        {
                            if let Some(pending) = self.pending.get_mut(&request_id) {
                                pending.wait = Some(seconds);
                                self.events.push(Event::Wait(seconds));
                            }
                        } else if let Some(pending) = self.pending.remove(&request_id) {
                            self.events.push(Event::Rpc(pending.tag, Err(error)));
                        }
                    } else if let Some(pending) = self.pending.remove(&request_id) {
                        self.events.push(Event::Rpc(pending.tag, Ok(result)));
                    }
                }
                0xedab447b => {
                    let tl::enums::BadMsgNotification::BadServerSalt(bad) = decode(&body)? else {
                        unreachable!()
                    };
                    self.key.as_mut().unwrap().salt = bad.new_server_salt;
                    self.changed_key();
                    self.retry(bad.bad_msg_id);
                }
                0xa7eff811 => {
                    let tl::enums::BadMsgNotification::Notification(bad) = decode(&body)? else {
                        unreachable!()
                    };
                    match bad.error_code {
                        16 | 17 => {
                            self.key.as_mut().unwrap().offset = (id >> 32) - self.unix;
                            self.last_id = 0;
                            self.changed_key();
                            self.retry(bad.bad_msg_id);
                        }
                        32 | 33 => self.restart_session(),
                        18 | 19 | 20 | 64 => self.retry(bad.bad_msg_id),
                        34 | 35 => {
                            if let Some(pending) = self.pending.get_mut(&bad.bad_msg_id) {
                                pending.sequence = if bad.error_code == 34 { 0 } else { 1 };
                            }
                            self.retry(bad.bad_msg_id);
                        }
                        code => {
                            return Err(words::protocol_rejection(code));
                        }
                    }
                }
                0x9ec20908 => {
                    let tl::enums::NewSession::Created(session) = decode(&body)?;
                    self.key.as_mut().unwrap().salt = session.server_salt;
                    self.changed_key();
                    self.events.push(Event::NewSession);
                }
                0x347773c5 => {
                    let tl::enums::Pong::Pong(pong) = decode(&body)?;
                    if self.ping.is_some_and(|(id, _)| id == pong.ping_id) {
                        self.ping = None;
                        self.pending.retain(|_, pending| pending.tag != 0);
                    }
                }
                0x62d6b459 => {
                    let _: tl::enums::MsgsAck = decode(&body)?;
                }
                tl::types::MsgsStateReq::CONSTRUCTOR_ID
                | tl::types::MsgsStateInfo::CONSTRUCTOR_ID
                | tl::types::MsgsAllInfo::CONSTRUCTOR_ID
                | tl::types::MsgDetailedInfo::CONSTRUCTOR_ID
                | tl::types::MsgNewDetailedInfo::CONSTRUCTOR_ID
                | tl::types::MsgResendReq::CONSTRUCTOR_ID
                | tl::types::FutureSalts::CONSTRUCTOR_ID
                | tl::types::DestroySessionOk::CONSTRUCTOR_ID
                | tl::types::DestroySessionNone::CONSTRUCTOR_ID => {}
                tl::types::UpdatesTooLong::CONSTRUCTOR_ID
                | tl::types::Updates::CONSTRUCTOR_ID
                | tl::types::UpdatesCombined::CONSTRUCTOR_ID
                | tl::types::UpdateShort::CONSTRUCTOR_ID
                | tl::types::UpdateShortMessage::CONSTRUCTOR_ID
                | tl::types::UpdateShortChatMessage::CONSTRUCTOR_ID
                | tl::types::UpdateShortSentMessage::CONSTRUCTOR_ID => {
                    self.events.push(Event::Update(body))
                }
                _ => {}
            }
        }
        if !acks.is_empty() {
            self.service(tl::enums::MsgsAck::Ack(tl::types::MsgsAck { msg_ids: acks }).to_bytes());
        }
        Ok(())
    }

    fn in_replay_window_after_authenticated_clock_correction(&self, id: i64) -> bool {
        let now = self.unix.saturating_add(self.key.as_ref().unwrap().offset);
        (now.saturating_sub(300)..=now.saturating_add(30)).contains(&(id >> 32))
    }

    fn changed_key(&mut self) {
        self.events.push(Event::KeyChanged(Box::new(
            self.key.as_ref().unwrap().clone(),
        )));
    }

    fn retry(&mut self, id: i64) {
        if let Some(mut pending) = self.pending.remove(&id) {
            let fresh_message_id_after_time_correction_or_flood_wait = self.next_id();
            let content = pending.sequence % 2 != 0;
            pending.sequence = self.sequence * 2 + i32::from(content);
            self.sequence += i32::from(content);
            self.send(
                fresh_message_id_after_time_correction_or_flood_wait,
                pending.sequence,
                &pending.body,
            );
            self.pending.insert(
                fresh_message_id_after_time_correction_or_flood_wait,
                pending,
            );
        }
    }

    fn restart_session(&mut self) {
        self.session = i64::from_le_bytes(random::bytes());
        self.sequence = 0;
        self.seen.clear();
        for (_, pending) in std::mem::take(&mut self.pending) {
            self.request(pending.tag, pending.body);
        }
    }

    pub(crate) fn tick(&mut self) -> Result<(), String> {
        self.unix += 1;
        self.elapsed += 1;
        let retry: Vec<_> = self
            .pending
            .iter_mut()
            .filter_map(|(id, pending)| {
                let wait = pending.wait.as_mut()?;
                *wait = wait.saturating_sub(1);
                if *wait == 0 {
                    pending.wait = None;
                    Some(*id)
                } else {
                    None
                }
            })
            .collect();
        for id in retry {
            self.retry(id);
        }
        let mobile_nat_probe_period_seconds = 30;
        let three_round_trip_dh_deadline_seconds = 60;
        let two_probe_periods_without_pong = 2 * mobile_nat_probe_period_seconds;
        let mobile_nat_probe_is_due =
            self.elapsed.is_multiple_of(mobile_nat_probe_period_seconds) && self.ping.is_none();
        if self.key.is_none() {
            if self.elapsed >= three_round_trip_dh_deadline_seconds {
                return Err(words::AUTH_TIMEOUT.into());
            }
        } else if self
            .ping
            .is_some_and(|(_, at)| self.elapsed - at >= two_probe_periods_without_pong)
        {
            return Err(words::PONG_TIMEOUT.into());
        } else if mobile_nat_probe_is_due {
            let ping = i64::from_le_bytes(random::bytes());
            let internal_content_related_ping_tag_for_parity_correction = 0;
            self.request(
                internal_content_related_ping_tag_for_parity_correction,
                tl::functions::PingDelayDisconnect {
                    ping_id: ping,
                    disconnect_delay: {
                        let transit_margin_after_local_pong_deadline_seconds = 15;
                        two_probe_periods_without_pong as i32
                            + transit_margin_after_local_pong_deadline_seconds
                    },
                }
                .to_bytes(),
            );
            self.ping = Some((ping, self.elapsed));
        }
        let oldest_message_id_in_300_second_replay_window =
            ((self.unix + self.key.as_ref().map_or(0, |key| key.offset) - 300) << 32).max(0);
        self.seen
            .retain(|id| *id >= oldest_message_id_in_300_second_replay_window);
        Ok(())
    }
}

fn time_correction(body: &[u8]) -> bool {
    body.starts_with(&0xa7eff811u32.to_le_bytes())
        && matches!(decode::<tl::enums::BadMsgNotification>(body),
            Ok(tl::enums::BadMsgNotification::Notification(bad)) if matches!(bad.error_code, 16 | 17))
}

fn read<T: Deserializable>(cursor: &mut tl::Cursor<'_>) -> Result<T, String> {
    T::deserialize(cursor).map_err(|e| e.to_string())
}

pub(crate) fn unpack(mut body: Vec<u8>) -> Result<Vec<u8>, String> {
    while body.starts_with(&0x3072cfa1u32.to_le_bytes()) {
        let compressed: Vec<u8> = decode(&body[4..])?;
        let mut expanded = Vec::new();
        flate2::read::GzDecoder::new(&compressed[..])
            .read_to_end(&mut expanded)
            .map_err(|e| e.to_string())?;
        body = expanded;
    }
    Ok(body)
}
