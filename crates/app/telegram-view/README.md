# Telegram view

A personal Telegram client running on the device, as a view on the view SDK.
It connects directly to Telegram's test WebSocket servers. It does not send
Telegram keys, phone numbers, login codes or messages through ducktape's network.

The login page asks for a phone number, then its Telegram code, then a two-step
verification password if Telegram requires one. The chats page shows names and
last-message previews, newest first, with More chats and Refresh chats. Opening
a chat reads its history; scrolling up or pressing Older messages reads older
pages. Text can be sent from the message field. Media appears as a short label.
Live updates change the history and previews; missing updates are fetched after
a gap or reconnect, including a channel's separate update counter.

`session`, `updates` and `peers` in the device store hold DC auth keys and salts,
clock offsets, Telegram user ID, update checkpoints and peer access hashes.
The store is scoped by the host to the network and seated ducktape key. Changing
either cancels the connection and reads the new store. Passwords exist only
while computing the SRP proof; snapshots contain no session or message content.
Log out calls `auth.logOut` and removes all three store keys. During reconnection
it removes them locally; Connect can also retry immediately.

This is a test-server build. The single API credential pair in `src/config.rs`
awaits eddy's ducktape registration at my.telegram.org. Host names and the
`/apiws_test` path follow J-133's ruled design §1, citing TDLib's
`ConnectionCreator.cpp` (1245–1256). Production routing, QR login, sign-up,
secret chats, media downloads, mirroring, a notifier and keychain storage are
outside this build. It connects from an app release that carries the socket host
(app #104).

Build the deployable module with `make wasm-telegram-view CARGO='cargo --locked'`.
It uses the same release profile, path remapping, optimization and ABI gate as
`wasm-views`. The crate stays outside `VIEWS` and genesis; it deploys by SetView.
`view-wasm-check` checks its normal dependency graph and builds the deployable
module through `make wasm-telegram-view`. The same backend flags work with
`make dev V=telegram-view` and `make wasm-why V=telegram-view`.

`host.random` seeds ChaCha20 once per mount/store load. On wasm32 that generator
also supplies getrandom 0.4's custom backend, selected by `.cargo/config.toml`
and shared by the Makefile's `WASM_RUSTFLAGS`. The build removes
Rust's export of the backend symbol before applying the normal view gate.
No JavaScript randomness or runtime is linked. Time comes from chain status and
heads until Telegram sets its offset; after that only protocol ticks advance it.
Clock ticks serve only keep-alive, server flood waits, authentication timeout
and reconnect.

`cargo test --locked -p telegram-view` runs against the SDK fake host. Its
scripted server decrypts the obfuscated transport, performs RSA/DH, decrypts
client envelopes and emits encrypted TL replies and updates. The RSA private
key under `src/tests` is a public test fixture trusted only by test builds.
