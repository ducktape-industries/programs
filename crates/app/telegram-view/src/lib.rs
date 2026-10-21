//! A personal Telegram connection from this device, deployed later by SetView.
mod auth;
mod chats;
mod config;
mod history;
mod login;
mod protocol;
mod random;
mod rpc;
mod send;
mod transport;
mod ui;
mod updates;
mod words;

use borsh::{BorshDeserialize, BorshSerialize};
use protocol::{Connection, DcKey, Event};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use view::methods::{
    ChainHeads, ChainStatus, ClockTicks, HostRandom, HostSession, SocketConnect, SocketItem,
    SocketOpen, SocketSend, SocketSendMessage,
};
use view::prelude::*;

const HOSTS: &[&str] = &[
    "pluto.web.telegram.org",
    "venus.web.telegram.org",
    "aurora.web.telegram.org",
    "vesta.web.telegram.org",
    "flora.web.telegram.org",
];

#[derive(BorshDeserialize, BorshSerialize)]
struct Session {
    home: u8,
    keys: BTreeMap<u8, DcKey>,
    user: Option<i64>,
}
impl Default for Session {
    fn default() -> Self {
        Self {
            home: {
                const DEFAULT_TELEGRAM_DC: u8 = 2;
                DEFAULT_TELEGRAM_DC
            },
            keys: BTreeMap::new(),
            user: None,
        }
    }
}

#[derive(BorshDeserialize, BorshSerialize)]
struct Account {
    home: u8,
    user: Option<i64>,
}

impl Default for Account {
    fn default() -> Self {
        Self {
            home: Session::default().home,
            user: None,
        }
    }
}

#[derive(Default)]
struct Live {
    session: Session,
    keys_in_memory: bool,
    unconfirmed_keys_removal: Option<login::UnconfirmedKeysRemoval>,
    scope: Option<(String, String)>,
    bootstrap: Option<Task<()>>,
    loaded: bool,
    connection: Option<Connection>,
    socket: u64,
    dc: Option<u8>,
    retry_delay: u32,
    subscription: Option<Task<()>>,
    ticks: Option<Task<()>>,
    unix: i64,
    clock_set: bool,
    reconnect: Option<u32>,
    ready: bool,
    initialized: bool,
    calls: BTreeMap<u64, rpc::Call>,
    next_tag: u64,
    transfer: Option<(
        u8,
        u64,
        grammers_tl_types::types::auth::ExportedAuthorization,
    )>,
    phone: String,
    code_hash: String,
    password: Option<grammers_tl_types::types::account::Password>,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Telegram {
    #[serde(skip)]
    live: Live,
    #[serde(skip)]
    chats: chats::Chats,
    #[serde(skip)]
    sync: updates::Sync,
    #[serde(skip)]
    history: Option<history::History>,
    #[serde(skip)]
    status: String,
    #[serde(skip)]
    screen: login::Screen,
    #[serde(skip)]
    phone: TextField,
    #[serde(skip)]
    code: TextField,
    #[serde(skip)]
    password: TextField,
    #[serde(skip)]
    compose: TextField,
    #[serde(skip)]
    failed: Option<String>,
    #[serde(skip)]
    notice: Option<String>,
}

impl View for Telegram {
    const NAME: &'static str = words::TITLE;
    const DESCRIPTION: &'static str = words::DESCRIPTION;
    const CAPABILITIES: &'static [Capability] = &[
        Capability::Host,
        Capability::Chain,
        Capability::Clock,
        Capability::Store,
        Capability::Socket,
    ];
    const HOSTS: &'static [&'static str] = HOSTS;
    const MIN_WINDOW_WIDTH: u32 = {
        const SINGLE_COLUMN_SMALL_PHONE_WIDTH: u32 = 320;
        SINGLE_COLUMN_SMALL_PHONE_WIDTH
    };

    fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.status = words::LOCKED.into();
        cx.follow::<HostSession>((), |view, session, cx| {
            match session {
                Ok(session) => {
                    let scope = (session.chain_id, session.signer);
                    if view.live.scope.as_ref() != Some(&scope) {
                        let locked = scope.1.is_empty();
                        view.cancel_reads_and_sockets_then_clear_device();
                        view.live.scope = Some(scope);
                        if locked {
                            view.status = words::LOCKED.into();
                        } else {
                            view.load_device(cx);
                        }
                    }
                }
                Err(error) => {
                    view.cancel_reads_and_sockets_then_clear_device();
                    view.failed = Some(design::sentence(&error));
                }
            }
            cx.notify();
        })
        .detach();
        cx.follow::<ChainHeads>((), |view, head, _| {
            if let Ok(head) = head {
                let chain_clock_is_not_set = !view.live.clock_set;
                if chain_clock_is_not_set {
                    view.live.unix = (head.time / 1000) as i64;
                    if let Some(connection) = &mut view.live.connection {
                        connection.unix = view.live.unix;
                    }
                }
            }
        })
        .detach();
    }
}

const TRANSPORT_AUTH_KEY_NOT_FOUND: i32 = -404;

impl Telegram {
    fn load_device(&mut self, cx: &mut Context<Self>) {
        self.status = words::STARTING.into();
        let host = cx.host();
        let store = cx.store();
        store.remove("session");
        self.live.bootstrap = Some(cx.land(
            async move {
                let bytes = host.ask::<HostRandom>(32).await?;
                let seed: [u8; 32] = bytes
                    .try_into()
                    .map_err(|_| Error::new("host_fault", words::RANDOM_LENGTH))?;
                let status = host.ask::<ChainStatus>(()).await?;
                let mut warnings = Vec::new();
                let account = stored::<Account>(&host, "account", &mut warnings)
                    .await?
                    .unwrap_or_default();
                let (keys, keys_in_memory) =
                    match store.secret_get::<BTreeMap<u8, DcKey>>("keys").await {
                        Ok(keys) => (keys.unwrap_or_default(), false),
                        Err(error) => match error.code.as_str() {
                            "no_secret_store" => {
                                warnings.push(words::NO_KEY_STORE.into());
                                (BTreeMap::new(), true)
                            }
                            "unknown_request" => {
                                return Err(Error::new(error.code, words::UPDATE_DUCKTAPE));
                            }
                            _ => return Err(error),
                        },
                    };
                let mut session = Session {
                    home: account.home,
                    user: account.user,
                    keys,
                };
                let mut peers =
                    stored::<BTreeMap<chats::Peer, chats::PeerInfo>>(&host, "peers", &mut warnings)
                        .await?
                        .unwrap_or_default();
                let mut position =
                    stored::<updates::Position>(&host, "updates", &mut warnings).await?;
                if !session.keys.contains_key(&session.home) {
                    session.user = None;
                    peers.clear();
                    position = None;
                    store.remove("peers");
                    store.remove("updates");
                }
                Ok((
                    seed,
                    status.time,
                    session,
                    keys_in_memory,
                    peers,
                    position,
                    warnings,
                ))
            },
            |view, result, cx| {
                match result {
                    Ok((seed, time, session, keys_in_memory, peers, position, warnings)) => {
                        random::seed(seed);
                        view.notice = (!warnings.is_empty()).then(|| warnings.join(" "));
                        view.live.loaded = true;
                        view.live.unix = (time / 1000) as i64;
                        view.live.session = session;
                        view.live.keys_in_memory = keys_in_memory;
                        view.chats.peers = peers;
                        view.sync.position = position;
                        if view.live.session.user.is_some() {
                            view.screen = login::Screen::Chats;
                        }
                        view.connect(cx);
                    }
                    Err(error) => view.failed = Some(design::sentence(&error)),
                }
                view.live.bootstrap = None;
                cx.notify();
            },
        ));
    }

    fn persist(&mut self, cx: &mut Context<Self>) {
        cx.store().set(
            "account",
            &Account {
                home: self.live.session.home,
                user: self.live.session.user,
            },
        );
        if self.live.keys_in_memory {
            return;
        }
        let written = cx.store().secret_set("keys", Some(&self.live.session.keys));
        cx.land(written, Self::keys_written).detach();
    }

    fn keys_written(&mut self, result: Result<(), Error>, cx: &mut Context<Self>) {
        match result {
            Ok(()) => self.live.unconfirmed_keys_removal = None,
            Err(error) if error.code == "no_secret_store" => {
                self.live.keys_in_memory = true;
                self.notice = Some(words::NO_KEY_STORE.into());
            }
            Err(error) => {
                self.failed = Some(design::sentence(&error));
            }
        }
        cx.notify();
    }

    fn cancel_reads_and_sockets_then_clear_device(&mut self) {
        let socket = self.live.socket;
        self.live = Live {
            socket,
            ..Default::default()
        };
        self.chats = Default::default();
        self.sync = Default::default();
        self.history = None;
        self.screen = Default::default();
        self.phone.reset("");
        self.code.reset("");
        self.password.reset("");
        self.compose.reset("");
        self.failed = None;
        self.notice = None;
    }

    fn connect(&mut self, cx: &mut Context<Self>) {
        if self
            .live
            .scope
            .as_ref()
            .is_none_or(|(_, signer)| signer.is_empty())
        {
            return;
        }
        if !self.live.loaded {
            self.load_device(cx);
            return;
        }
        self.live.subscription = None;
        self.live.connection = None;
        self.live.ready = false;
        self.live.initialized = false;
        self.live.calls.retain(|_, call| {
            !matches!(
                call.kind,
                rpc::Rpc::State | rpc::Rpc::Difference | rpc::Rpc::ChannelDifference(_)
            )
        });
        self.live.reconnect = None;
        self.live.socket += 1;
        self.status = words::CONNECTING.into();
        let socket = self.live.socket;
        let home = self.live.dc.unwrap_or(self.live.session.home);
        self.live.dc = Some(home);
        let Some(host) = home.checked_sub(1).and_then(|i| HOSTS.get(i as usize)) else {
            self.failed = Some(words::UNKNOWN_DC.into());
            return;
        };
        self.live.subscription = Some(cx.follow::<SocketConnect>(
            SocketOpen {
                socket,
                url: format!("wss://{host}/apiws_test"),
                protocols: vec!["binary".into()],
            },
            move |view, item, cx| {
                if view.live.socket != socket {
                    return;
                }
                match item {
                    Ok(SocketItem::Opened { .. }) => {
                        let key = view.live.session.keys.get(&home).cloned();
                        view.live.connection = Some(Connection::new(key, view.live.unix));
                        view.flush(cx);
                    }
                    Ok(SocketItem::Binary(bytes)) => {
                        if let Some(connection) = &mut view.live.connection {
                            match connection.feed(bytes) {
                                Ok(()) => view.flush(cx),
                                Err(why) => view.transport_failed(why, cx),
                            }
                        }
                    }
                    Ok(SocketItem::Text(_)) => view.disconnected(words::TEXT_FRAME.into(), cx),
                    Ok(SocketItem::Closed { reason, .. }) => view.disconnected(reason, cx),
                    Err(error) => {
                        view.failed = Some(design::sentence(&error));
                        view.live.connection = None;
                        view.live.ready = false;
                        if ["undeclared_host", "consent_refused", "unknown_request"]
                            .contains(&error.code.as_str())
                        {
                            view.live.subscription = None;
                            view.live.ticks = None;
                        } else {
                            view.disconnected(design::sentence(&error), cx);
                        }
                    }
                }
                cx.notify();
            },
        ));
        if self.live.ticks.is_none() {
            self.live.ticks = Some(cx.follow::<ClockTicks>(1000, |view, tick, cx| {
                if let Err(error) = tick {
                    view.failed = Some(design::sentence(&error));
                    cx.notify();
                    return;
                }
                view.live.unix += 1;
                if let Some(wait) = &mut view.live.reconnect {
                    *wait = wait.saturating_sub(1);
                    if *wait == 0 {
                        view.connect(cx);
                        cx.notify();
                    }
                } else if let Some(connection) = &mut view.live.connection {
                    match connection.tick() {
                        Ok(()) => view.flush(cx),
                        Err(why) => view.disconnected(why, cx),
                    }
                }
            }));
        }
        cx.notify();
    }

    fn transport_failed(&mut self, failure: transport::Failure, cx: &mut Context<Self>) {
        match failure {
            transport::Failure::Status(TRANSPORT_AUTH_KEY_NOT_FOUND) => {
                let dc = self.live.dc.unwrap_or(self.live.session.home);
                self.live.session.keys.remove(&dc);
                let home = dc == self.live.session.home;
                if home {
                    let keys = std::mem::take(&mut self.live.session.keys);
                    let ticks = self.live.ticks.take();
                    let retry_delay = self.live.retry_delay;
                    self.forget(None, cx);
                    self.live.ticks = ticks;
                    self.live.retry_delay = retry_delay;
                    self.live.session.home = dc;
                    self.live.session.keys = keys;
                }
                self.persist(cx);
                self.disconnected(words::transport_status(-404), cx);
                self.failed = None;
                self.notice = Some(
                    if home {
                        words::HOME_KEY_LOST
                    } else {
                        words::DC_KEY_LOST
                    }
                    .into(),
                );
            }
            transport::Failure::Status(status) => {
                self.disconnected(words::transport_status(status), cx)
            }
            transport::Failure::Invalid(why) => self.disconnected(why, cx),
        }
    }

    fn disconnected(&mut self, why: String, cx: &mut Context<Self>) {
        self.failed = Some(why);
        self.live.connection = None;
        self.live.ready = false;
        self.live.subscription = None;
        let doubled_retry_delay_for_transport_flood_without_retry_after_or_host_fault_until_successful_rpc =
            self.live.retry_delay.saturating_mul(2);
        self.live.retry_delay = doubled_retry_delay_for_transport_flood_without_retry_after_or_host_fault_until_successful_rpc.clamp(5, 300);
        self.live.reconnect = Some(self.live.retry_delay);
        self.status = words::RECONNECTING.into();
        cx.notify();
    }

    fn flush(&mut self, cx: &mut Context<Self>) {
        let Some(connection) = &mut self.live.connection else {
            return;
        };
        let socket = self.live.socket;
        let outgoing = std::mem::take(&mut connection.outgoing);
        let events = std::mem::take(&mut connection.events);
        for body in outgoing {
            cx.host().notify::<SocketSendMessage>(SocketSend {
                socket: self.live.socket,
                body,
            });
        }
        for event in events {
            let reply_migrated_socket_or_forgot_session =
                self.live.socket != socket || self.live.connection.is_none();
            if reply_migrated_socket_or_forgot_session {
                break;
            }
            match event {
                Event::KeyChanged(key) => {
                    self.live.clock_set = true;
                    self.live
                        .session
                        .keys
                        .insert(self.live.dc.unwrap_or(self.live.session.home), *key);
                    self.persist(cx);
                }
                Event::Ready => {
                    if !self.live.ready {
                        self.status = if self.live.session.user.is_some() {
                            words::SIGNED_IN
                        } else {
                            words::CONNECTED
                        }
                        .into();
                        self.failed = None;
                        self.live.ready = true;
                        self.resume(cx);
                        if self.live.session.user.is_some() {
                            self.start_updates(cx);
                        }
                    }
                    cx.notify();
                }
                Event::NewSession => self.start_updates(cx),
                Event::Rpc(tag, result) => self.rpc(tag, result, cx),
                Event::Update(bytes) => match auth::decode(&bytes) {
                    Ok(updates) => self.receive_updates(updates, cx),
                    Err(why) => self.failed = Some(why),
                },
                Event::Wait(seconds) => {
                    self.status = words::wait(seconds);
                    cx.notify();
                }
            }
        }
    }
}

impl Render for Telegram {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::render(self, window, cx)
    }
}
export_view!(Telegram);

#[cfg(test)]
mod tests;

async fn stored<T: BorshDeserialize>(
    host: &view::host::Host,
    key: &str,
    warnings: &mut Vec<String>,
) -> Result<Option<T>, Error> {
    let Some(bytes) = host.ask::<view::methods::StoreGet>(key.to_owned()).await? else {
        return Ok(None);
    };
    match T::try_from_slice(&bytes) {
        Ok(value) => Ok(Some(value)),
        Err(_) => {
            view::store::remove(host, key);
            warnings.push(words::replaced_store(key));
            Ok(None)
        }
    }
}
