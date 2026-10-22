//! Sealing for chat: a room's epoch key wrapped to each device with HPKE base mode (RFC 9180) and
//! each message body under XChaCha20-Poly1305. It draws no randomness and keeps nothing: the
//! caller brings the random bytes and holds the keys.
mod hpke;
#[cfg(test)]
mod tests;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};

pub const KEY: usize = 32;
pub const NONCE: usize = 24;
pub const WRAPPED: usize = hpke::ENC + KEY + hpke::TAG;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    LowOrderKey,
    Unopenable,
    TooLong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WrapContext<'a> {
    pub channel_id: &'a str,
    pub epoch: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyContext<'a> {
    pub channel_id: &'a str,
    pub message_id: &'a str,
    pub epoch: u64,
    pub author: u64,
}

const WRAP_LABEL: &str = "ducktape-seal/wrap/v1";
const BODY_LABEL: &str = "ducktape-seal/body/v1";

fn text(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
}

impl WrapContext<'_> {
    pub fn info(&self) -> Vec<u8> {
        let mut out = Vec::new();
        text(&mut out, WRAP_LABEL);
        text(&mut out, self.channel_id);
        out.extend_from_slice(&self.epoch.to_le_bytes());
        out
    }
}

impl BodyContext<'_> {
    pub fn aad(&self) -> Vec<u8> {
        let mut out = Vec::new();
        text(&mut out, BODY_LABEL);
        text(&mut out, self.channel_id);
        text(&mut out, self.message_id);
        out.extend_from_slice(&self.epoch.to_le_bytes());
        out.extend_from_slice(&self.author.to_le_bytes());
        out
    }
}

pub fn device_public(secret: &[u8; KEY]) -> [u8; KEY] {
    hpke::public(secret)
}

pub fn wrap(
    device: &[u8; KEY],
    room_key: &[u8; KEY],
    at: &WrapContext,
    ephemeral_seed: &[u8; KEY],
) -> Result<Vec<u8>, Refused> {
    hpke::seal(device, &at.info(), &[], room_key, ephemeral_seed)
}

pub fn unwrap(secret: &[u8; KEY], wrapped: &[u8], at: &WrapContext) -> Result<[u8; KEY], Refused> {
    let opened = hpke::open(secret, wrapped, &at.info(), &[])?;
    <[u8; KEY]>::try_from(opened.as_slice()).map_err(|_| Refused::Unopenable)
}

pub fn seal_body(
    room_key: &[u8; KEY],
    nonce: &[u8; NONCE],
    at: &BodyContext,
    plaintext: &[u8],
) -> Result<Vec<u8>, Refused> {
    let cipher = XChaCha20Poly1305::new(&Key::from(*room_key));
    let aad = at.aad();
    let payload = Payload {
        msg: plaintext,
        aad: &aad,
    };
    (cipher.encrypt(&XNonce::from(*nonce), payload)).map_err(|_| Refused::TooLong)
}

pub fn open_body(
    room_key: &[u8; KEY],
    nonce: &[u8; NONCE],
    at: &BodyContext,
    sealed: &[u8],
) -> Result<Vec<u8>, Refused> {
    let cipher = XChaCha20Poly1305::new(&Key::from(*room_key));
    let aad = at.aad();
    let payload = Payload {
        msg: sealed,
        aad: &aad,
    };
    (cipher.decrypt(&XNonce::from(*nonce), payload)).map_err(|_| Refused::Unopenable)
}
