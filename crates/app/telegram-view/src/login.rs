use crate::{
    Session, Telegram, auth::check_public, config, protocol::DcKey, random, rpc::Rpc, words,
};
use grammers_tl_types::{self as tl, Serializable};
use num_bigint::BigUint;
use std::collections::BTreeMap;
use view::prelude::*;

#[derive(Default)]
pub(crate) enum Screen {
    #[default]
    Phone,
    Code,
    Password,
    Chats,
}

pub(crate) struct UnconfirmedKeysRemoval {
    confirmed_status: Option<&'static str>,
    pub(crate) refused: bool,
}

impl Telegram {
    pub(crate) fn login_busy(&self) -> bool {
        self.live.calls.values().any(|call| {
            !matches!(
                call.kind,
                Rpc::Dialogs
                    | Rpc::History(_, _)
                    | Rpc::Send { .. }
                    | Rpc::State
                    | Rpc::Difference
                    | Rpc::ChannelDifference(_)
            )
        })
    }

    pub(crate) fn submit_login(&mut self, cx: &mut Context<Self>) {
        let login_is_unavailable_or_busy = !self.live.ready || self.login_busy();
        if login_is_unavailable_or_busy {
            return;
        }
        self.failed = None;
        match self.screen {
            Screen::Phone => {
                let phone = self.phone.text().trim().to_owned();
                if phone.is_empty() {
                    return;
                }
                self.live.phone = phone.clone();
                self.phone.reset("");
                self.call(Rpc::SendCode, config::send_code(phone), cx);
            }
            Screen::Code => {
                let code = self.code.text().trim().to_owned();
                self.code.reset("");
                if code.is_empty() {
                    return;
                }
                let body = tl::functions::auth::SignIn {
                    phone_number: self.live.phone.clone(),
                    phone_code_hash: self.live.code_hash.clone(),
                    phone_code: Some(code),
                    email_verification: None,
                }
                .to_bytes();
                self.call(Rpc::SignIn, body, cx);
            }
            Screen::Password => {
                let password = self.password.text();
                self.password.reset("");
                if password.is_empty() {
                    return;
                }
                match self.srp_proof_without_queued_password(password.as_bytes()) {
                    Ok(body) => self.call(Rpc::CheckPassword, body, cx),
                    Err(why) => {
                        self.failed = Some(why);
                        cx.notify();
                    }
                }
            }
            Screen::Chats => {}
        }
    }

    fn srp_proof_without_queued_password(&self, password: &[u8]) -> Result<Vec<u8>, String> {
        let params = self
            .live
            .password
            .as_ref()
            .ok_or(words::NO_PASSWORD_PARAMETERS)?;
        let Some(tl::enums::PasswordKdfAlgo::Sha256Sha256Pbkdf2Hmacsha512iter100000Sha256ModPow(
            algo,
        )) = &params.current_algo
        else {
            return Err(words::PASSWORD_ALGORITHM.into());
        };
        let password_prime_or_generator_is_invalid = !(2..=7).contains(&algo.g)
            || !grammers_crypto::two_factor_auth::check_p_and_g(&algo.p, &algo.g);
        if password_prime_or_generator_is_invalid {
            return Err(words::BAD_PASSWORD_PARAMETERS.into());
        }
        let b = params.srp_b.as_ref().ok_or(words::NO_PASSWORD_PUBLIC)?;
        let prime = BigUint::from_bytes_be(&algo.p);
        let number = BigUint::from_bytes_be(b);
        let srp_b_including_k_times_v_is_outside_zero_and_prime =
            b.len() > 256 || number == BigUint::from(0u32) || number >= prime;
        if srp_b_including_k_times_v_is_outside_zero_and_prime {
            return Err(words::BAD_PASSWORD_PUBLIC.into());
        }
        let secret = random::bytes::<256>();
        let (m1, a) = grammers_crypto::two_factor_auth::calculate_2fa(
            &algo.salt1,
            &algo.salt2,
            &algo.p,
            &algo.g,
            b.clone(),
            secret.to_vec(),
            password,
        );
        check_public(&BigUint::from_bytes_be(&a), &prime)?;
        Ok(tl::functions::auth::CheckPassword {
            password: tl::enums::InputCheckPasswordSrp::Srp(tl::types::InputCheckPasswordSrp {
                srp_id: params.srp_id.ok_or(words::NO_PASSWORD_SESSION)?,
                a: a.to_vec(),
                m1: m1.to_vec(),
            }),
        }
        .to_bytes())
    }

    pub(crate) fn authorized(
        &mut self,
        authorization: tl::enums::auth::Authorization,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let tl::enums::auth::Authorization::Authorization(auth) = authorization else {
            return Err(words::SIGNUP.into());
        };
        let id = match auth.user {
            tl::enums::User::User(user) => user.id,
            tl::enums::User::Empty(user) => user.id,
        };
        if self.live.session.user != Some(id) {
            self.sync = Default::default();
            self.chats.peers.clear();
            cx.store().remove("updates");
            cx.store().remove("peers");
        }
        self.live.session.user = Some(id);
        self.live.phone.clear();
        self.live.code_hash.clear();
        self.live.password = None;
        self.phone.reset("");
        self.code.reset("");
        self.password.reset("");
        self.screen = Screen::Chats;
        self.status = words::SIGNED_IN.into();
        self.failed = None;
        self.persist(cx);
        self.start_updates(cx);
        Ok(())
    }

    pub(crate) fn logout(&mut self, cx: &mut Context<Self>) {
        if !self.live.ready {
            self.forget(Some(words::LOCAL_KEYS_REMOVED), cx);
            cx.notify();
        } else if !self.login_busy() {
            self.call(Rpc::Logout, tl::functions::auth::LogOut {}.to_bytes(), cx);
        }
    }

    pub(crate) fn forget(
        &mut self,
        confirmed_status: Option<&'static str>,
        cx: &mut Context<Self>,
    ) {
        self.live.subscription = None;
        self.live.ticks = None;
        self.live.connection = None;
        self.live.ready = false;
        self.live.initialized = false;
        self.live.reconnect = None;
        self.live.dc = None;
        self.live.clock_set = false;
        self.live.retry_delay = 0;
        self.live.session = Session::default();
        self.chats = Default::default();
        self.sync = Default::default();
        self.live.transfer = None;
        self.history = None;
        self.compose.reset("");
        self.live.calls.clear();
        self.live.password = None;
        self.live.phone.clear();
        self.live.code_hash.clear();
        self.phone.reset("");
        self.code.reset("");
        self.password.reset("");
        for key in ["account", "updates", "peers", "session"] {
            cx.store().remove(key);
        }
        self.screen = Screen::Phone;
        self.notice = None;
        if self.live.keys_in_memory {
            self.live.unconfirmed_keys_removal = None;
            if let Some(status) = confirmed_status {
                self.status = status.into();
            }
        } else {
            self.live.unconfirmed_keys_removal = Some(UnconfirmedKeysRemoval {
                confirmed_status,
                refused: false,
            });
            self.remove_keys_from_key_store(cx);
        }
    }

    pub(crate) fn remove_keys_from_key_store(&mut self, cx: &mut Context<Self>) {
        let Some(removal) = &mut self.live.unconfirmed_keys_removal else {
            return;
        };
        removal.refused = false;
        self.failed = None;
        self.status = words::REMOVING_KEYS.into();
        let removed = cx.store().secret_set::<BTreeMap<u8, DcKey>>("keys", None);
        cx.land(removed, Self::keys_removed).detach();
        cx.notify();
    }

    fn keys_removed(&mut self, result: Result<(), Error>, cx: &mut Context<Self>) {
        let Some(removal) = &mut self.live.unconfirmed_keys_removal else {
            return;
        };
        match result {
            Ok(()) => {
                if let Some(status) = removal.confirmed_status {
                    self.status = status.into();
                }
                self.live.unconfirmed_keys_removal = None;
            }
            Err(error) => {
                removal.refused = true;
                self.failed = Some(design::sentence(&error));
            }
        }
        cx.notify();
    }
}
