# seal

The crypto behind sealed rooms in chat (J-377, J-389). chat itself never links it: chat stores opaque bytes and checks bookkeeping, and the reader's device does the crypto (chat-view, and later the CLI and bridges).

## What it does
- **Wraps** a room's 32-byte epoch key to one device's X25519 public key with HPKE base mode, RFC 9180, suite DHKEM(X25519, HKDF-SHA256) / HKDF-SHA256 / ChaCha20-Poly1305, single shot. A wrap is `enc ‖ ct`, `WRAPPED` = 80 bytes. Its HPKE `info` is `WrapContext::info()`, the borsh of `("ducktape-seal/wrap/v1", channel_id, epoch)`. The device is bound by HPKE itself, since the recipient's key is in the KEM context.
- **Seals a body** under the room key with XChaCha20-Poly1305 and a 24-byte nonce. The AAD is `BodyContext::aad()`, the borsh of `("ducktape-seal/body/v1", channel_id, message_id, epoch, author)`, where `author` is the account number. A body copied to another room, message, epoch or author doesn't open.
- What chat-view seals is the borsh of the message's `Vec<chat::Block>`. seal only sees bytes.

## What it doesn't do
- It draws no randomness. The caller passes the ephemeral seed for each wrap (the ephemeral key is `DeriveKeyPair(seed)`, RFC 9180 §7.1.3), each body's nonce, and the device and room keys. A view takes them from `host.random`.
- It keeps nothing. The device secret lives in the view's secret store.

## API
`device_public`, `wrap`, `unwrap`, `seal_body`, `open_body`, with `Refused::{LowOrderKey, Unopenable, TooLong}`.

## Proof
`src/tests.rs` runs RFC 9180 A.2.1 (DeriveKeyPair for both keys, enc, shared_secret, key, base_nonce, the first encryption) and the XChaCha20-Poly1305 draft's A.3.1 vector, plus what each context binds.
