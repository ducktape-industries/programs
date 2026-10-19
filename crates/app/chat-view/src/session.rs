//! Who reads: the session the host hands over (the seated key and the
//! account it holds), and what that lets them do in the open room.
use std::collections::BTreeMap;

use chat::view::Names;
use chat::{Kind, Principal};
use view::prelude::*;

use crate::Chat;
use crate::notifier::{Choice, choices};
use crate::queries::{channels, ownerships, roster};
use futures::FutureExt;

/// Why the reader may not write in the open room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Gate {
    /// every write is an account's: a key that holds none only reads
    NoAccount,
    Archived,
    /// members-only, and they are not on the roster
    NotMember,
}

impl Chat {
    pub(crate) fn session_changed(&mut self, next: Session, cx: &mut Context<Self>) {
        if next == self.session {
            return;
        }
        cx.notify();
        let prev = std::mem::replace(&mut self.session, next);
        let reader_changed = self.session.signer != prev.signer
            || self.session.endpoint != prev.endpoint
            || self.session.chain_id != prev.chain_id;
        if reader_changed {
            self.load_names(cx);
            self.choices = Loadable::Idle;
            self.managed_choices = Loadable::Idle;
            if self.details.is_some() {
                self.load_choices(cx);
            }
        }
        if reader_changed || (prev.connected && !self.session.connected) {
            for draft in self.drafts.values_mut() {
                draft.retire_device_requests();
            }
            if let Some(room) = &mut self.room {
                room.pending.clear();
            }
            self.reads.cursors.clear();
            self.reads.kept = None;
            self.create = None;
        }
        if self.session.connected && (reader_changed || !prev.connected) {
            self.load_kept(cx);
        }
        if !prev.connected && self.session.connected {
            self.load_channels(cx);
            self.refresh(cx);
        }
    }

    pub(crate) fn visibility_changed(&mut self, visible: bool, cx: &mut Context<Self>) {
        if !visible && self.create.take().is_some() {
            cx.notify();
        }
        if self.reads.visible == visible {
            return;
        }
        self.reads.visible = visible;
        self.reads.entering = visible;
        if visible && self.session.connected {
            self.reread_channels(cx);
        }
    }

    /// The roster's names and whom its agents answer, read again with the
    /// ones on screen kept: the roster is read once, for both.
    pub(crate) fn load_names(&mut self, cx: &mut Context<Self>) {
        let host = cx.host();
        let names = roster(host.clone()).shared();
        cx.load(self, names.clone(), |chat| &mut chat.names);
        let answers = async move { ownerships(host, &names.await?).await };
        cx.load(self, answers, |chat| &mut chat.ownerships);
    }

    /// What the reader chose for each room's notices, read again.
    pub(crate) fn load_choices(&mut self, cx: &mut Context<Self>) {
        let Some(me) = self.my_account() else {
            self.choices = Loadable::Idle;
            self.managed_choices = Loadable::Idle;
            return;
        };
        cx.load(self, choices(cx.host(), me), |chat| &mut chat.choices);
        let known = self.names.ready().map(|names| managed_by(names, me));
        let managed = managed_choices(cx.host(), me, known);
        cx.load(self, managed, |chat| &mut chat.managed_choices);
    }

    pub(crate) fn load_channels(&mut self, cx: &mut Context<Self>) {
        // a link that came back: the sidebar says the rooms are loading
        self.channels = Loadable::Idle;
        cx.load(self, channels(cx.host()), |chat| &mut chat.channels);
    }

    /// The network the session is on, by its label (`<label>#<salt>`).
    pub(crate) fn network(&self) -> &str {
        let chain = self.session.chain_id.as_str();
        chain.split_once('#').map_or(chain, |(label, _)| label)
    }

    /// The reader's account number, as the host resolved it.
    pub(crate) fn my_account(&self) -> Option<u64> {
        self.session.account
    }

    /// The principal chat writes the reader as: their account; none while their
    /// key holds none, since only an account writes.
    pub(crate) fn me(&self) -> Option<Principal> {
        Principal::writer(self.my_account())
    }

    /// The reader's account name for the first page's avatar, as the
    /// roster names it (Expo `chat/index.tsx:110`, `connection.account.name`);
    /// empty until the session and the roster both say who the reader is.
    pub(crate) fn reader_name(&self) -> String {
        let me = self.me();
        me.and_then(|me| self.names.ready()?.name(&me).map(str::to_owned))
            .unwrap_or_default()
    }

    /// The reader, as a query's `viewer`.
    pub(crate) fn viewer(&self) -> Vec<Principal> {
        self.me().into_iter().collect()
    }

    pub(crate) fn holds_account(&self) -> bool {
        self.me().is_some()
    }

    /// Why the reader may not write in the open room; none when they may.
    pub(crate) fn write_gate(&self) -> Option<Gate> {
        if !self.holds_account() {
            return Some(Gate::NoAccount);
        }
        let info = self.room_info()?;
        if info.channel.archived {
            return Some(Gate::Archived);
        }
        let me = self.me()?;
        let seated = self
            .room
            .as_ref()
            .and_then(|room| room.members.ready())
            .is_some_and(|members| members.iter().any(|m| m.principal == me));
        (!info.channel.admits(&me, seated)).then_some(Gate::NotMember)
    }

    pub(crate) fn may_write(&self) -> bool {
        self.write_gate().is_none()
    }

    /// Why the agent `principal` would give the reader no answer
    /// ([`crate::names::unanswered`]).
    // an agent harness doesn't run: what it answers is no rule of harness's
    pub(crate) fn outside_harness(&self, principal: &Principal) -> bool {
        let Some(names) = self.names.ready() else {
            return false;
        };
        crate::message::agent(names, principal) && !crate::queries::run_by_harness(names, principal)
    }

    pub(crate) fn unanswered(&self, principal: &Principal) -> Option<&'static str> {
        let names = self.names.ready()?;
        let ownerships = self.ownerships.ready()?;
        crate::names::unanswered(names, ownerships, principal, self.my_account()?)
    }
}

// the accounts `reader` manages in identity, in number order: it chooses their notices too
fn managed_by(names: &Names, reader: u64) -> Vec<u64> {
    let managed = |number: &u64| {
        matches!(
            names.kind(&Principal::Account(*number)),
            Some(Kind::Program { manager, .. }) if *manager == reader
        )
    };
    names.people().filter(managed).collect()
}

// what each account `reader` manages chose, room by room; `known` names them once the roster is
// read, else the roster is read for them
async fn managed_choices(
    host: Host,
    reader: u64,
    known: Option<Vec<u64>>,
) -> Result<Vec<(u64, BTreeMap<String, Choice>)>, Error> {
    let accounts = match known {
        Some(accounts) => accounts,
        None => managed_by(&roster(host.clone()).await?, reader),
    };
    let mut chosen = Vec::new();
    for account in accounts {
        chosen.push((account, choices(host.clone(), account).await?));
    }
    Ok(chosen)
}
