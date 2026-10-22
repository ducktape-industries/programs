//! RFC 9180 base mode, single shot, for one suite: DHKEM(X25519, HKDF-SHA256), HKDF-SHA256 and
//! ChaCha20-Poly1305 (KEM 0x0020, KDF 0x0001, AEAD 0x0003).
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::Refused;

pub(crate) const ENC: usize = 32;
pub(crate) const TAG: usize = 16;
const KEM: &[u8] = b"KEM\x00\x20";
const SUITE: &[u8] = b"HPKE\x00\x20\x00\x01\x00\x03";
const VERSION: &[u8] = b"HPKE-v1";

pub(crate) fn extract(suite: &[u8], salt: &[u8], label: &[u8], ikm: &[u8]) -> [u8; 32] {
    let labeled = [VERSION, suite, label, ikm].concat();
    let (prk, _) = Hkdf::<Sha256>::extract(Some(salt), &labeled);
    prk.into()
}

pub(crate) fn expand<const L: usize>(
    prk: &[u8; 32],
    suite: &[u8],
    label: &[u8],
    info: &[u8],
) -> [u8; L] {
    let length = (L as u16).to_be_bytes();
    let labeled = [&length, VERSION, suite, label, info].concat();
    let mut out = [0u8; L];
    let hkdf = Hkdf::<Sha256>::from_prk(prk).expect("a SHA-256 PRK is 32 bytes");
    (hkdf.expand(&labeled, &mut out)).expect("an HPKE output is far under HKDF's 255 blocks");
    out
}

pub(crate) fn derive(ikm: &[u8; 32]) -> [u8; 32] {
    let prk = extract(KEM, &[], b"dkp_prk", ikm);
    expand(&prk, KEM, b"sk", &[])
}

pub(crate) fn public(secret: &[u8; 32]) -> [u8; 32] {
    PublicKey::from(&StaticSecret::from(*secret)).to_bytes()
}

fn dh(secret: &[u8; 32], public: &[u8; 32]) -> Result<[u8; 32], Refused> {
    let shared = StaticSecret::from(*secret).diffie_hellman(&PublicKey::from(*public));
    match shared.was_contributory() {
        true => Ok(shared.to_bytes()),
        false => Err(Refused::LowOrderKey),
    }
}

fn shared_secret(dh: &[u8; 32], enc: &[u8; 32], recipient: &[u8; 32]) -> [u8; 32] {
    let prk = extract(KEM, &[], b"eae_prk", dh);
    expand(
        &prk,
        KEM,
        b"shared_secret",
        &[enc.as_slice(), recipient].concat(),
    )
}

pub(crate) fn encap(
    recipient: &[u8; 32],
    seed: &[u8; 32],
) -> Result<([u8; 32], [u8; 32]), Refused> {
    let ephemeral = derive(seed);
    let enc = public(&ephemeral);
    let dh = dh(&ephemeral, recipient)?;
    Ok((enc, shared_secret(&dh, &enc, recipient)))
}

fn decap(secret: &[u8; 32], enc: &[u8; 32]) -> Result<[u8; 32], Refused> {
    let dh = dh(secret, enc)?;
    Ok(shared_secret(&dh, enc, &public(secret)))
}

pub(crate) fn schedule(shared_secret: &[u8; 32], info: &[u8]) -> ([u8; 32], [u8; 12]) {
    let psk_id_hash = extract(SUITE, &[], b"psk_id_hash", &[]);
    let info_hash = extract(SUITE, &[], b"info_hash", info);
    let context = [&[0u8][..], &psk_id_hash, &info_hash].concat();
    let secret = extract(SUITE, shared_secret, b"secret", &[]);
    let key = expand(&secret, SUITE, b"key", &context);
    let base_nonce = expand(&secret, SUITE, b"base_nonce", &context);
    (key, base_nonce)
}

pub(crate) fn aead_seal(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, Refused> {
    let cipher = ChaCha20Poly1305::new(&Key::from(*key));
    let payload = Payload {
        msg: plaintext,
        aad,
    };
    (cipher.encrypt(&Nonce::from(*nonce), payload)).map_err(|_| Refused::TooLong)
}

pub(crate) fn seal(
    recipient: &[u8; 32],
    info: &[u8],
    aad: &[u8],
    plaintext: &[u8],
    seed: &[u8; 32],
) -> Result<Vec<u8>, Refused> {
    let (enc, shared_secret) = encap(recipient, seed)?;
    let (key, nonce) = schedule(&shared_secret, info);
    let sealed = aead_seal(&key, &nonce, aad, plaintext)?;
    Ok([enc.as_slice(), &sealed].concat())
}

pub(crate) fn open(
    secret: &[u8; 32],
    sealed: &[u8],
    info: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, Refused> {
    let (enc, ct) = sealed.split_at_checked(ENC).ok_or(Refused::Unopenable)?;
    let enc = <[u8; ENC]>::try_from(enc).map_err(|_| Refused::Unopenable)?;
    let shared_secret = decap(secret, &enc).map_err(|_| Refused::Unopenable)?;
    let (key, nonce) = schedule(&shared_secret, info);
    let cipher = ChaCha20Poly1305::new(&Key::from(key));
    let payload = Payload { msg: ct, aad };
    (cipher.decrypt(&Nonce::from(nonce), payload)).map_err(|_| Refused::Unopenable)
}
