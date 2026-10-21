pub(crate) const LOCKED: &str = "Unlock ducktape to use Telegram.";
pub(crate) const NO_KEY_STORE: &str =
    "This device has no key store, so Telegram forgets this sign-in when ducktape closes.";
pub(crate) const UPDATE_DUCKTAPE: &str = "Update ducktape to use Telegram.";
pub(crate) const STARTING: &str = "Reading this device's Telegram session…";
pub(crate) const CONNECTING: &str = "Connecting to Telegram…";
pub(crate) const CONNECTED: &str = "Connected to Telegram";
pub(crate) const IMPORT_FAILED: &str =
    "Authorization transfer failed. Connect to return to your previous data centre.";
pub(crate) const RECONNECTING: &str = "Reconnecting to Telegram…";
pub(crate) const DEVICE: &str = "Your session stays on this device. Telegram test servers.";
pub(crate) const PHONE: &str = "Phone number, including country code";
pub(crate) const CODE: &str = "Telegram login code";
pub(crate) const PASSWORD: &str = "Telegram two-step verification password";
pub(crate) const SIGNUP: &str =
    "New Telegram accounts cannot be created here. Register in Telegram first.";
pub(crate) const WRONG_CODE: &str = "That code is incorrect. Try again.";
pub(crate) const EXPIRED_CODE: &str = "That code expired. Request a new one.";
pub(crate) const WRONG_PASSWORD: &str = "That password is incorrect. Try again.";
pub(crate) const CODE_SENT: &str = "Enter the code Telegram sent you.";
pub(crate) const PASSWORD_NEEDED: &str = "Enter your Telegram two-step verification password.";
pub(crate) const SIGNED_IN: &str = "Signed in to Telegram";
pub(crate) const LOCAL_KEYS_REMOVED: &str = "This device's Telegram keys have been removed locally. Telegram still lists this device. End its session from Telegram's Settings > Devices on another device.";
pub(crate) const SIGNED_OUT: &str = "Logged out. This device's Telegram keys have been removed.";
pub(crate) const REMOVING_KEYS: &str = "Removing this device's Telegram keys…";
pub(crate) const REMOVE_KEYS_AGAIN: &str = "Remove the keys again";

pub(crate) const CONNECT: &str = "Connect";
pub(crate) const HOME_KEY_LOST: &str =
    "Telegram no longer knows this device's home key. Sign in again.";
pub(crate) const DC_KEY_LOST: &str =
    "Telegram no longer knows this data centre's key. Creating a new key.";
pub(crate) fn transport_status(status: i32) -> String {
    format!("Telegram transport status: {status}")
}

pub(crate) fn replaced_store(key: &str) -> String {
    format!("This device's Telegram {key} could not be read and was removed. Starting it again.")
}

pub(crate) const TITLE: &str = "Telegram";
pub(crate) const DESCRIPTION: &str = "Your Telegram chats, connected from this device.";
pub(crate) const READING_CHATS: &str = "Reading chats…";
pub(crate) const NO_CHATS: &str = "No Telegram chats yet.";
pub(crate) const NO_CHATS_DETAIL: &str = "Start a chat in Telegram, then choose Refresh chats.";
pub(crate) const REFRESH_CHATS: &str = "Refresh chats";
pub(crate) const MORE_CHATS: &str = "More chats";
pub(crate) const LOGOUT: &str = "Log out";
pub(crate) const CONTINUE: &str = "Continue";
pub(crate) const READING_MESSAGES: &str = "Reading messages…";
pub(crate) const NO_MESSAGES: &str = "No messages yet.";
pub(crate) const NO_MESSAGES_DETAIL: &str = "Messages in this chat will appear here.";
pub(crate) const CHATS: &str = "Chats";
pub(crate) const OLDER_MESSAGES: &str = "Older messages";
pub(crate) const MESSAGE: &str = "Message";
pub(crate) const SEND: &str = "Send";
pub(crate) const RANDOM_LENGTH: &str = "host.random returned the wrong length";
pub(crate) const UNKNOWN_DC: &str = "Telegram named an unknown data centre";
pub(crate) const TEXT_FRAME: &str = "Telegram sent a text WebSocket frame";
pub(crate) const NO_PASSWORD_PARAMETERS: &str = "Telegram has not supplied password parameters";
pub(crate) const PASSWORD_ALGORITHM: &str = "This Telegram password algorithm is not supported";
pub(crate) const BAD_PASSWORD_PARAMETERS: &str = "Telegram password parameters did not verify";
pub(crate) const NO_PASSWORD_PUBLIC: &str = "Telegram omitted the password public value";
pub(crate) const BAD_PASSWORD_PUBLIC: &str = "Telegram password public value did not verify";
pub(crate) const NO_PASSWORD_SESSION: &str = "Telegram omitted the password session";
pub(crate) const SESSION_ENDED: &str = "The Telegram session ended. Sign in again.";
pub(crate) const CODE_DELIVERY: &str = "Telegram requires a different code delivery method";
pub(crate) const NO_CHANNEL_HASH: &str =
    "Telegram has not supplied this channel's access hash. Refresh chats.";
pub(crate) const NO_CHANNEL_STATE: &str = "Telegram omitted the channel state";
pub(crate) const NO_CHANNEL_POSITION: &str = "Telegram omitted the channel position";
pub(crate) const UNEXPECTED_AUTH: &str = "Unexpected Telegram authentication message";
pub(crate) const NO_KEY: &str = "Missing Telegram auth key";
pub(crate) const SHORT_ENVELOPE: &str = "Short Telegram envelope";
pub(crate) const BAD_SESSION: &str = "Telegram session id did not verify";
pub(crate) const BAD_MESSAGE_LENGTH: &str = "Invalid Telegram message length";
pub(crate) const BAD_PADDING: &str = "Invalid Telegram padding";
pub(crate) const BAD_MESSAGE_ID: &str = "Invalid Telegram message id or sequence";
pub(crate) const BAD_CONTAINER: &str = "Invalid Telegram container";
pub(crate) const BAD_CONTAINER_ID: &str = "Invalid Telegram container message id or sequence";
pub(crate) const BAD_CONTAINER_LENGTH: &str = "Invalid Telegram container length";
pub(crate) const TRAILING_CONTAINER: &str = "Trailing Telegram container data";
pub(crate) const AUTH_TIMEOUT: &str = "Telegram authentication timed out";
pub(crate) const PONG_TIMEOUT: &str = "Telegram stopped answering keep-alive";
pub(crate) const NO_CHAT_HASH: &str =
    "Telegram has not supplied this chat's access hash. Refresh chats.";
pub(crate) const UNAVAILABLE: &str = "Message unavailable";
pub(crate) const SERVICE: &str = "Service message";
pub(crate) const BAD_PQ_NONCE: &str = "Invalid Telegram PQ nonce or size";
pub(crate) const UNKNOWN_RSA: &str = "Telegram's RSA key is unknown";
pub(crate) const BAD_PQ: &str = "Invalid Telegram PQ";
pub(crate) const UNFACTORED_PQ: &str = "Telegram PQ could not be factored";
pub(crate) const BAD_PQ_FACTORS: &str = "Invalid Telegram PQ factors";
pub(crate) const DH_DECLINED: &str = "Telegram declined the DH exchange";
pub(crate) const BAD_DH_LENGTH: &str = "Invalid Telegram DH answer length";
pub(crate) const BAD_DH_HASH: &str = "Telegram DH hash did not verify";
pub(crate) const BAD_PRIME: &str = "Telegram's DH prime or generator did not verify";
pub(crate) const KEY_DECLINED: &str = "Telegram declined the new auth key; reconnect to try again";
pub(crate) const INCOMPLETE_DH: &str = "DH exchange is incomplete";
pub(crate) const BAD_KEY_CONFIRMATION: &str = "Telegram auth key confirmation did not verify";
pub(crate) const BAD_NONCES: &str = "Telegram DH nonces did not verify";
pub(crate) const BAD_PUBLIC: &str = "Telegram DH public value is outside the safe range";
pub(crate) const PHOTO: &str = "photo";
pub(crate) const FILE: &str = "file";
pub(crate) const LOCATION: &str = "location";
pub(crate) const CONTACT: &str = "contact";
pub(crate) const POLL: &str = "poll";
pub(crate) const STORY: &str = "story";
pub(crate) const DICE: &str = "dice";
pub(crate) const MEDIA: &str = "media";

pub(crate) fn wait(seconds: u32) -> String {
    format!("Telegram asks to wait {seconds} seconds")
}
pub(crate) fn protocol_rejection(code: i32) -> String {
    format!("Telegram rejected a protocol message ({code})")
}
pub(crate) fn invalid_frame(length: i32) -> String {
    format!("Invalid Telegram transport frame: {length}")
}
pub(crate) fn user(id: i64) -> String {
    format!("User {id}")
}
pub(crate) fn group(id: i64) -> String {
    format!("Group {id}")
}
pub(crate) fn channel(id: i64) -> String {
    format!("Channel {id}")
}
pub(crate) fn file(name: &str) -> String {
    format!("file {name}")
}
pub(crate) fn caption(text: &str, media: &str) -> String {
    format!("{text}\n{media}")
}
pub(crate) fn message_author(name: &str, date: &str) -> String {
    format!("{name} · {date}")
}
