//! The suite against RFC 9180's own vectors (A.2.1) and the XChaCha20-Poly1305 draft's (A.3.1),
//! what each context binds a wrap and a body to, and the wire bytes of a wrap and a body.
use crate::{
    BodyContext, KEY, NONCE, Refused, WRAPPED, WrapContext, device_public, hpke, open_body,
    seal_body, unwrap, wrap,
};

fn bytes(text: &str) -> Vec<u8> {
    let digits = text.as_bytes();
    (digits.chunks(2))
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn hex<const N: usize>(text: &str) -> [u8; N] {
    <[u8; N]>::try_from(bytes(text)).unwrap()
}

#[test]
fn rfc_9180_a_2_1_base_mode_of_the_suite() {
    let ikm_e = hex::<32>("909a9b35d3dc4713a5e72a4da274b55d3d3821a37e5d099e74a647db583a904b");
    let sk_e = hpke::derive(&ikm_e);
    assert_eq!(
        sk_e,
        hex("f4ec9b33b792c372c1d2c2063507b684ef925b8c75a42dbcbf57d63ccd381600")
    );
    assert_eq!(
        hpke::public(&sk_e),
        hex("1afa08d3dec047a643885163f1180476fa7ddb54c6a8029ea33f95796bf2ac4a")
    );
    let sk_r = hpke::derive(&hex(
        "1ac01f181fdf9f352797655161c58b75c656a6cc2716dcb66372da835542e1df",
    ));
    assert_eq!(
        sk_r,
        hex("8057991eef8f1f1af18f4a9491d16a1ce333f695d4db8e38da75975c4478e0fb")
    );
    let pk_r = hpke::public(&sk_r);
    assert_eq!(
        pk_r,
        hex("4310ee97d88cc1f088a5576c77ab0cf5c3ac797f3d95139c6c84b5429c59662a")
    );
    let (enc, shared_secret) = hpke::encap(&pk_r, &ikm_e).unwrap();
    assert_eq!(
        enc,
        hex("1afa08d3dec047a643885163f1180476fa7ddb54c6a8029ea33f95796bf2ac4a")
    );
    assert_eq!(
        shared_secret,
        hex("0bbe78490412b4bbea4812666f7916932b828bba79942424abb65244930d69a7")
    );
    let info = bytes("4f6465206f6e2061204772656369616e2055726e");
    let (key, base_nonce) = hpke::schedule(&shared_secret, &info);
    assert_eq!(
        key,
        hex("ad2744de8e17f4ebba575b3f5f5a8fa1f69c2a07f6e7500bc60ca6e3e3ec1c91")
    );
    assert_eq!(base_nonce, hex("5c4d98150661b848853b547f"));
    let aad = bytes("436f756e742d30");
    let pt = bytes("4265617574792069732074727574682c20747275746820626561757479");
    let ct = hpke::aead_seal(&key, &base_nonce, &aad, &pt).unwrap();
    let want = bytes(
        "1c5250d8034ec2b784ba2cfd69dbdb8af406cfe3ff938e131f0def8c8b60b4db21993c62ce81883d2dd1b51a28",
    );
    assert!(ct == want, "the first encryption differs from A.2.1's");
    let sealed = hpke::seal(&pk_r, &info, &aad, &pt, &ikm_e).unwrap();
    assert!(sealed == [enc.as_slice(), &want].concat(), "enc ‖ ct");
    assert_eq!(hpke::open(&sk_r, &sealed, &info, &aad).unwrap(), pt);
}

#[test]
fn xchacha20_poly1305_draft_a_3_1() {
    let key = hex::<KEY>("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f");
    let nonce = hex::<NONCE>("404142434445464748494a4b4c4d4e4f5051525354555657");
    let aad = bytes("50515253c0c1c2c3c4c5c6c7");
    let pt = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
    let want = bytes(concat!(
        "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb",
        "731c7f1b0b4aa6440bf3a82f4eda7e39ae64c6708c54c216cb96b72e1213b452",
        "2f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff9",
        "21f9664c97637da9768812f615c68b13b52e",
        "c0875924c1c7987947deafd8780acf49",
    ));
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    let cipher = chacha20poly1305::XChaCha20Poly1305::new(&chacha20poly1305::Key::from(key));
    let payload = Payload {
        msg: pt.as_slice(),
        aad: &aad,
    };
    let sealed = cipher.encrypt(&chacha20poly1305::XNonce::from(nonce), payload);
    assert!(sealed.unwrap() == want, "the draft's ciphertext and tag");
}

fn at(channel_id: &str, epoch: u64) -> WrapContext<'_> {
    WrapContext { channel_id, epoch }
}

#[test]
fn a_wrap_opens_only_for_its_device_its_room_and_its_epoch() {
    let (ada, bo) = ([1u8; KEY], [2u8; KEY]);
    let room_key = [7u8; KEY];
    let wrapped = wrap(&device_public(&ada), &room_key, &at("dm-1-2", 3), &[9; KEY]).unwrap();
    assert_eq!(wrapped.len(), WRAPPED);
    assert_eq!(unwrap(&ada, &wrapped, &at("dm-1-2", 3)), Ok(room_key));
    for (secret, context) in [
        (bo, at("dm-1-2", 3)),
        (ada, at("dm-1-3", 3)),
        (ada, at("dm-1-2", 4)),
    ] {
        assert_eq!(
            unwrap(&secret, &wrapped, &context),
            Err(Refused::Unopenable),
            "{context:?}"
        );
    }
    let mut tampered = wrapped.clone();
    tampered[WRAPPED - 1] ^= 1;
    assert_eq!(
        unwrap(&ada, &tampered, &at("dm-1-2", 3)),
        Err(Refused::Unopenable)
    );
    assert_eq!(
        unwrap(&ada, &wrapped[..WRAPPED - 1], &at("dm-1-2", 3)),
        Err(Refused::Unopenable)
    );
}

#[test]
fn each_wrap_takes_its_own_ephemeral_key() {
    let device = device_public(&[1; KEY]);
    let first = wrap(&device, &[7; KEY], &at("dm-1-2", 0), &[9; KEY]).unwrap();
    let second = wrap(&device, &[7; KEY], &at("dm-1-2", 0), &[10; KEY]).unwrap();
    assert!(first[..32] != second[..32], "enc follows the seed");
}

#[test]
fn a_low_order_device_key_takes_no_wrap() {
    let refused = wrap(&[0; KEY], &[7; KEY], &at("dm-1-2", 0), &[9; KEY]);
    assert_eq!(refused, Err(Refused::LowOrderKey));
}

#[test]
fn a_body_opens_only_under_its_own_room_message_epoch_and_author() {
    let room_key = [7u8; KEY];
    let nonce = [5u8; NONCE];
    let at = BodyContext {
        channel_id: "dm-1-2",
        message_id: "m1",
        epoch: 0,
        author: 1,
    };
    let sealed = seal_body(&room_key, &nonce, &at, b"hello").unwrap();
    assert_eq!(
        open_body(&room_key, &nonce, &at, &sealed).unwrap(),
        b"hello"
    );
    for other in [
        BodyContext {
            channel_id: "dm-1-3",
            ..at
        },
        BodyContext {
            message_id: "m2",
            ..at
        },
        BodyContext { epoch: 1, ..at },
        BodyContext { author: 2, ..at },
    ] {
        let opened = open_body(&room_key, &nonce, &other, &sealed);
        assert_eq!(opened, Err(Refused::Unopenable), "{other:?}");
    }
    assert_eq!(
        open_body(&[8; KEY], &nonce, &at, &sealed),
        Err(Refused::Unopenable)
    );
    assert_eq!(
        open_body(&room_key, &[6; NONCE], &at, &sealed),
        Err(Refused::Unopenable)
    );
}

#[test]
fn a_wrap_and_a_body_are_the_bytes_an_independent_reference_computes() {
    let secret = [1u8; KEY];
    let wrapped = wrap(
        &device_public(&secret),
        &[7; KEY],
        &at("dm-1-2", 3),
        &[9; KEY],
    )
    .unwrap();
    let want = bytes(concat!(
        "de8923a675d02180cdd7d0097ba404b9e1e2930d5665cba58d1765006361793b",
        "b955ccab4f6dcecebc3572b7022f14f0cd6173524f77310e90e97cfb5af1cd87",
        "4286d4e840770f0a655a2b1fe59b5148",
    ));
    assert!(wrapped == want, "the wrap's bytes moved");
    assert_eq!(unwrap(&secret, &want, &at("dm-1-2", 3)), Ok([7; KEY]));
    let body = BodyContext {
        channel_id: "dm-1-2",
        message_id: "m1",
        epoch: 0,
        author: 1,
    };
    let sealed = seal_body(&[7; KEY], &[5; NONCE], &body, b"hello").unwrap();
    let want = bytes("99df1981725df1c0cb80714a0e74f1f5e04445ccfc");
    assert!(sealed == want, "the body's bytes moved");
    assert_eq!(
        open_body(&[7; KEY], &[5; NONCE], &body, &want).unwrap(),
        b"hello"
    );
}

#[test]
fn each_context_is_the_borsh_of_its_label_and_fields() {
    let wrap = at("dm-1-2", 3);
    let want = borsh::to_vec(&("ducktape-seal/wrap/v1", "dm-1-2", 3u64)).unwrap();
    assert_eq!(wrap.info(), want);
    let body = BodyContext {
        channel_id: "dm-1-2",
        message_id: "m1",
        epoch: 3,
        author: 9,
    };
    let want = borsh::to_vec(&("ducktape-seal/body/v1", "dm-1-2", "m1", 3u64, 9u64)).unwrap();
    assert_eq!(body.aad(), want);
}
