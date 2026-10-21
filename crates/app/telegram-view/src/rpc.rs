//! Queued RPC calls, retry routing and replies for login, chats and updates.
use crate::{Telegram, auth::decode, config, login::Screen, words};
use grammers_tl_types::{self as tl, Serializable};
use view::prelude::*;

#[derive(Clone)]
pub(crate) enum Rpc {
    SendCode,
    SignIn,
    PasswordParameters,
    CheckPassword,
    Logout,
    Export(u8, u64),
    Import,
    Dialogs,
    State,
    Difference,
    ChannelDifference(i64),
    History(crate::chats::Peer, i32),
    Send {
        peer: crate::chats::Peer,
        text: String,
    },
}

#[derive(Clone)]
pub(crate) struct Call {
    pub(crate) kind: Rpc,
    pub(crate) body: Vec<u8>,
}

impl Telegram {
    pub(crate) fn call(&mut self, kind: Rpc, body: Vec<u8>, cx: &mut Context<Self>) {
        self.live.next_tag += 1;
        let tag = self.live.next_tag;
        self.live.calls.insert(
            tag,
            Call {
                kind,
                body: body.clone(),
            },
        );
        self.send_call(tag, &body);
        self.flush(cx);
        cx.notify();
    }

    pub(crate) fn send_call(&mut self, tag: u64, body: &[u8]) {
        let call_must_wait_for_connection_or_authorization_import = !self.live.ready
            || (self.live.transfer.is_some()
                && !self
                    .live
                    .calls
                    .get(&tag)
                    .is_some_and(|call| matches!(call.kind, Rpc::Import)));
        if call_must_wait_for_connection_or_authorization_import {
            return;
        }
        let body = if self.live.initialized {
            body.to_vec()
        } else {
            self.live.initialized = true;
            config::initialize(body)
        };
        self.live.connection.as_mut().unwrap().request(tag, body);
    }

    pub(crate) fn resume(&mut self, cx: &mut Context<Self>) {
        if let Some((_, _, transfer)) = &self.live.transfer {
            if let Some((tag, call)) = self
                .live
                .calls
                .iter()
                .find(|(_, call)| matches!(call.kind, Rpc::Import))
            {
                let (tag, body) = (*tag, call.body.clone());
                self.send_call(tag, &body);
                self.flush(cx);
            } else {
                let body = tl::functions::auth::ImportAuthorization {
                    id: transfer.id,
                    bytes: transfer.bytes.clone(),
                }
                .to_bytes();
                self.call(Rpc::Import, body, cx);
            }
            return;
        }
        let calls: Vec<_> = self
            .live
            .calls
            .iter()
            .map(|(tag, call)| (*tag, call.body.clone()))
            .collect();
        for (tag, body) in calls {
            self.send_call(tag, &body);
        }
        self.flush(cx);
    }

    pub(crate) fn rpc(
        &mut self,
        tag: u64,
        result: Result<Vec<u8>, tl::types::RpcError>,
        cx: &mut Context<Self>,
    ) {
        let Some(call) = self.live.calls.remove(&tag) else {
            return;
        };
        match result {
            Err(error) => {
                if matches!(call.kind, Rpc::Import) {
                    self.import_failed();
                    self.failed = Some(error.error_message);
                    cx.notify();
                    return;
                }
                if let Some(dc) = ["PHONE_MIGRATE_", "USER_MIGRATE_", "NETWORK_MIGRATE_"]
                    .iter()
                    .find_map(|prefix| {
                        error
                            .error_message
                            .strip_prefix(prefix)
                            .and_then(|dc| dc.parse::<u8>().ok())
                    })
                {
                    let migration_dc_is_supported_and_different = (1..=crate::HOSTS.len() as u8)
                        .contains(&dc)
                        && dc != self.live.session.home;
                    if migration_dc_is_supported_and_different {
                        self.live.calls.insert(tag, call);
                        if self.live.session.user.is_some() {
                            self.call(
                                Rpc::Export(dc, tag),
                                tl::functions::auth::ExportAuthorization {
                                    dc_id: i32::from(dc),
                                }
                                .to_bytes(),
                                cx,
                            );
                        } else {
                            self.live.session.home = dc;
                            self.live.dc = Some(dc);
                            self.connect(cx);
                        }
                        return;
                    }
                }
                if let Rpc::Send { peer, text } = &call.kind
                    && self
                        .history
                        .as_ref()
                        .is_some_and(|history| history.peer == *peer)
                    && self.compose.text().is_empty()
                {
                    self.compose.reset(text);
                }
                match error.error_message.as_str() {
                    "SESSION_REVOKED" | "AUTH_KEY_UNREGISTERED" | "SESSION_EXPIRED" => {
                        self.forget(cx);
                        self.failed = Some(words::SESSION_ENDED.into());
                    }
                    "SESSION_PASSWORD_NEEDED" => self.call(
                        Rpc::PasswordParameters,
                        tl::functions::account::GetPassword {}.to_bytes(),
                        cx,
                    ),
                    "PHONE_NUMBER_UNOCCUPIED" | "PHONE_NUMBER_NOT_REGISTERED" => {
                        self.failed = Some(words::SIGNUP.into())
                    }
                    "PHONE_CODE_INVALID" => self.failed = Some(words::WRONG_CODE.into()),
                    "PHONE_CODE_EXPIRED" => {
                        self.screen = Screen::Phone;
                        self.failed = Some(words::EXPIRED_CODE.into());
                    }
                    "PASSWORD_HASH_INVALID" => self.failed = Some(words::WRONG_PASSWORD.into()),
                    "SRP_ID_INVALID" => self.call(
                        Rpc::PasswordParameters,
                        tl::functions::account::GetPassword {}.to_bytes(),
                        cx,
                    ),
                    _ => self.failed = Some(error.error_message),
                }
            }
            Ok(body) => {
                self.live.retry_delay = 0;
                let importing = matches!(call.kind, Rpc::Import);
                if let Err(why) = self.reply(call.kind, &body, cx) {
                    if importing {
                        self.import_failed();
                    }
                    self.failed = Some(why);
                }
            }
        }
        cx.notify();
    }

    fn import_failed(&mut self) {
        if let Some((_, tag, _)) = self.live.transfer.take() {
            let failed_transfer_call = self.live.calls.remove(&tag);
            if let Some(call) = failed_transfer_call {
                let failed_transfer_call_kind = call.kind;
                if let Rpc::Send { peer, text } = failed_transfer_call_kind {
                    let failed_send_can_restore_empty_draft = self
                        .history
                        .as_ref()
                        .is_some_and(|history| history.peer == peer)
                        && self.compose.text().is_empty();
                    if failed_send_can_restore_empty_draft {
                        self.compose.reset(&text);
                    }
                }
            }
        }
        self.live.dc = Some(self.live.session.home);
        self.live.connection = None;
        self.live.subscription = None;
        self.live.ready = false;
        self.status = words::IMPORT_FAILED.into();
    }

    fn reply(&mut self, kind: Rpc, body: &[u8], cx: &mut Context<Self>) -> Result<(), String> {
        match kind {
            Rpc::SendCode => match decode::<tl::enums::auth::SentCode>(body)? {
                tl::enums::auth::SentCode::Code(code) => {
                    self.live.code_hash = code.phone_code_hash;
                    self.screen = Screen::Code;
                    self.status = words::CODE_SENT.into();
                }
                tl::enums::auth::SentCode::Success(success) => {
                    self.authorized(success.authorization, cx)?
                }
                _ => return Err(words::CODE_DELIVERY.into()),
            },
            Rpc::SignIn | Rpc::CheckPassword => self.authorized(decode(body)?, cx)?,
            Rpc::PasswordParameters => {
                let tl::enums::account::Password::Password(params) = decode(body)?;
                self.live.password = Some(params);
                self.screen = Screen::Password;
                self.status = words::PASSWORD_NEEDED.into();
            }
            Rpc::State => self.state_reply(body, cx)?,
            Rpc::Difference => self.difference_reply(body, cx)?,
            Rpc::ChannelDifference(id) => self.channel_reply(id, body, cx)?,
            Rpc::Dialogs => self.dialogs_reply(body, cx)?,
            Rpc::History(peer, offset) => self.history_reply(peer, offset, body, cx)?,
            Rpc::Send { peer, text } => self.sent(peer, text, body, cx)?,
            Rpc::Logout => {
                let _: tl::enums::auth::LoggedOut = decode(body)?;
                self.forget(cx);
            }
            Rpc::Export(dc, tag) => {
                let tl::enums::auth::ExportedAuthorization::Authorization(transfer) = decode(body)?;
                self.live.transfer = Some((dc, tag, transfer));
                self.live.dc = Some(dc);
                self.connect(cx);
            }
            Rpc::Import => {
                let authorization = decode(body)?;
                if !matches!(
                    authorization,
                    tl::enums::auth::Authorization::Authorization(_)
                ) {
                    return Err(words::SIGNUP.into());
                }
                let dc = self.live.transfer.as_ref().unwrap().0;
                self.live.session.home = dc;
                self.live.transfer = None;
                self.resume(cx);
                self.authorized(authorization, cx)?;
            }
        }
        Ok(())
    }
}
