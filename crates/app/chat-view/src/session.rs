//! Who reads: the session the host hands over (the seated key and the
//! account it holds), and what that lets them do in the open room.
use chat::view::Names;
use chat::{Kind, Principal, Standing};
use view::prelude::*;

use crate::Chat;
use crate::notifier::{Choice, choices};
use crate::queries::{channels, ownerships, roster};

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
        if !self.session_heard {
            self.session_heard = true;
            cx.notify();
        }
        if next == self.session {
            return;
        }
        cx.notify();
        let prev = std::mem::replace(&mut self.session, next);
        let reader_changed = self.session.signer != prev.signer
            || self.session.endpoint != prev.endpoint
            || self.session.chain_id != prev.chain_id;
        let account_changed = self.session.account != prev.account;
        if reader_changed || account_changed {
            self.listening.clear();
            self.posted.clear();
            self.order.clear();
            self.listening_asked = false;
            if prev.connected && self.session.connected {
                self.refresh(cx);
            }
        }
        if reader_changed {
            self.typing = Default::default();
            self.load_names(cx);
            self.load_avatars(cx);
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

    // The roster's names and every agent that narrowed whom it answers,
    // read again with the ones on screen kept.
    pub(crate) fn load_names(&mut self, cx: &mut Context<Self>) {
        let host = cx.host();
        cx.load(self, roster(host.clone()), |chat| &mut chat.names);
        cx.load(self, ownerships(host), |chat| &mut chat.ownerships);
    }

    // Each account's picture, read again; what is on screen stays until a
    // different set lands.
    pub(crate) fn load_avatars(&mut self, cx: &mut Context<Self>) {
        if self.faces.pictures.is_none() {
            self.faces.pictures = Some(cx.new(|_| view::pictures::Pictures::default()));
        }
        let host = cx.host();
        let read = identity::view::avatars(move |ask| host.query(ask));
        cx.land(read, |chat, read, cx| match read {
            Ok(avatars) => {
                if *chat.faces.avatars != avatars {
                    chat.faces.avatars = std::rc::Rc::new(avatars);
                    cx.notify();
                }
            }
            Err(refusal) => cx.log_refused("the pictures", &refusal),
        })
        .detach();
    }

    // The open direct room's other person's picture, once read.
    pub(crate) fn dm_peer_picture(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<std::sync::Arc<view::Image>> {
        let room = self.room.as_ref()?;
        let peer = crate::names::dm_peer_of(self.my_account()?, &room.id)?;
        self.faces.of(peer, cx)
    }

    /// What the reader chose for each room's notices, read again.
    pub(crate) fn load_choices(&mut self, cx: &mut Context<Self>) {
        let Some(me) = self.my_account() else {
            self.choices = Loadable::Idle;
            self.managed_choices = Loadable::Idle;
            return;
        };
        cx.load(self, choices(cx.host(), me), |chat| &mut chat.choices);
        let room = self.room_id();
        let known = self.names.ready().map(|names| managed_by(names, me));
        let managed = managed_choices(cx.host(), me, room, known);
        cx.load(self, managed, |chat| &mut chat.managed_choices);
    }

    pub(crate) fn load_channels(&mut self, cx: &mut Context<Self>) {
        // a link that came back: the sidebar says the rooms are loading
        self.channels = Loadable::Idle;
        cx.load(self, channels(cx.host()), |chat| &mut chat.channels);
        // the next frame asks again which rooms the reader listens to
        self.listening_asked = false;
    }

    // The rooms the reader listens to and wrote in, which both lists hold as theirs and `#`
    // offers, and the order the reader put them in.
    pub(crate) fn load_listening(&mut self, cx: &mut Context<Self>) {
        let Some(me) = self.my_account() else {
            return;
        };
        self.listening_asked = true;
        let read = crate::queries::listening(cx.host(), me);
        cx.land(read, move |chat, read, cx| match read {
            _ if chat.my_account() != Some(me) => {}
            Ok(rooms) => {
                let rooms: std::collections::BTreeSet<String> = rooms.into_iter().collect();
                if rooms != chat.listening {
                    chat.listening = rooms;
                    cx.notify();
                }
            }
            Err(refusal) => cx.log_refused("the rooms listened to", &refusal),
        })
        .detach();
        let posted = crate::queries::posted_in(cx.host(), me);
        cx.land(posted, move |chat, read, cx| match read {
            _ if chat.my_account() != Some(me) => {}
            Ok(rooms) => {
                let rooms: std::collections::BTreeSet<String> = rooms.into_iter().collect();
                if rooms != chat.posted {
                    chat.posted = rooms;
                    cx.notify();
                }
            }
            Err(refusal) => cx.log_refused("the rooms written in", &refusal),
        })
        .detach();
        let order = crate::queries::room_order(cx.host(), me);
        cx.land(order, move |chat, read, cx| match read {
            _ if chat.my_account() != Some(me) => {}
            Ok(order) => {
                if order != chat.order {
                    chat.order = order;
                    cx.notify();
                }
            }
            Err(refusal) => cx.log_refused("the order of the rooms", &refusal),
        })
        .detach();
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

    // why the agent `principal` would give the reader no answer (`crate::names::unanswered`)
    pub(crate) fn unanswered(&self, principal: &Principal) -> Option<&'static str> {
        let names = self.names.ready()?;
        let ownerships = self.ownerships.ready()?;
        crate::names::unanswered(names, ownerships, principal, self.my_account()?)
    }
}

fn managed_by(names: &Names, reader: u64) -> Vec<u64> {
    let managed = |number: &u64| {
        matches!(
            names.kind(&Principal::Account(*number)),
            Some(Kind::Program { manager, standing: Standing::Active, .. }) if *manager == reader
        )
    };
    names.people().filter(managed).collect()
}

async fn managed_choices(
    host: Host,
    reader: u64,
    room: String,
    known: Option<Vec<u64>>,
) -> Result<Vec<(u64, Choice)>, Error> {
    if room.is_empty() || chat::dm_peers(&room).is_some() {
        return Ok(Vec::new());
    }
    let accounts = match known {
        Some(accounts) => accounts,
        None => managed_by(&roster(host.clone()).await?, reader),
    };
    if accounts.is_empty() {
        return Ok(Vec::new());
    }
    let asked = chat::ask::ChosenBy {
        channel_id: room,
        accounts,
    };
    let chosen = host.query(asked).await?;
    Ok(chosen
        .into_iter()
        .map(|chosen| (chosen.account, Choice::of(chosen.listening, chosen.notices)))
        .collect())
}
