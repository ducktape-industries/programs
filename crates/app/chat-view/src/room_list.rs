//! The rooms the reader can open: the channels, then the direct rooms held, newest first on
//! the desktop; the phone adds everyone else, whose room opens on the first message
//! ([`Chat::dm_to_open`]). A program's own rooms (`forge:…` review threads) are its to show.
use std::cmp::Reverse;

use chat::view::Names;
use chat::{Category, ChannelInfo, Kind, Principal};
use view::prelude::*;

use crate::Chat;
use crate::names::dm_peer_of;
use crate::queries;

/// A direct room in the list: the room, once it is open; the other
/// person; the room's id.
pub(crate) struct DmEntry<'a> {
    pub info: Option<&'a ChannelInfo>,
    pub peer: u64,
    pub id: String,
}

pub(crate) struct RoomList<'a> {
    pub channels: Vec<&'a ChannelInfo>,
    pub dms: Vec<DmEntry<'a>>,
}

impl RoomList<'_> {
    /// Every room's id, in list order: what the arrows walk.
    pub(crate) fn ids(&self) -> Vec<String> {
        let channels = self.channels.iter().map(|info| info.channel.id.clone());
        channels
            .chain(self.dms.iter().map(|dm| dm.id.clone()))
            .collect()
    }

    /// The direct rooms already open (they lead the direct messages).
    pub(crate) fn opened(&self) -> usize {
        self.dms.iter().filter(|dm| dm.info.is_some()).count()
    }
}

pub(crate) struct DmChoice {
    pub peer: u64,
    pub name: String,
    pub agent: bool,
}

impl DmChoice {
    fn matches(&self, query: &str) -> bool {
        query.is_empty()
            || self.name.to_lowercase().contains(query)
            || self.peer.to_string().contains(query)
    }
}

fn person_or_agent(names: &Names, peer: u64) -> bool {
    matches!(
        names.kind(&Principal::Account(peer)),
        Some(
            Kind::User
                | Kind::Program {
                    category: Category::Agent,
                    ..
                }
        )
    )
}

impl Chat {
    pub(crate) fn room_list(&self) -> RoomList<'_> {
        let mut list = self.held_rooms();
        let (Some(mine), Some(names)) = (self.my_account(), self.names.ready()) else {
            return list;
        };
        let mut rest: Vec<(String, u64)> = names
            .people()
            .filter(|peer| *peer != mine && !list.dms.iter().any(|dm| dm.peer == *peer))
            .map(|peer| (names.member(&Principal::Account(peer)).to_lowercase(), peer))
            .collect();
        rest.sort();
        list.dms.extend(rest.into_iter().map(|(_, peer)| DmEntry {
            info: None,
            peer,
            id: chat::dm_channel_id(mine, peer),
        }));
        list
    }

    pub(crate) fn sidebar_rooms(&self) -> RoomList<'_> {
        let mut list = self.held_rooms();
        list.dms.sort_by_key(|dm| Reverse(self.recency(dm)));
        list
    }

    fn recency(&self, dm: &DmEntry) -> u64 {
        let created = dm.info.map_or(0, |info| info.channel.created_at);
        self.dm_heads
            .get(&dm.id)
            .and_then(|head| head.time)
            .unwrap_or(created)
    }

    fn held_rooms(&self) -> RoomList<'_> {
        let mine = self.my_account();
        let mut channels = Vec::new();
        let mut dms = Vec::new();
        for info in self.channels.ready().into_iter().flatten() {
            let id = info.channel.id.as_str();
            if chat::namespace::program(id).is_some() {
                continue;
            }
            if chat::dm_peers(id).is_none() {
                channels.push(info);
                continue;
            }
            dms.extend(
                mine.and_then(|mine| dm_peer_of(mine, id))
                    .map(|peer| DmEntry {
                        info: Some(info),
                        peer,
                        id: info.channel.id.clone(),
                    }),
            );
        }
        RoomList { channels, dms }
    }

    pub(crate) fn settle_dm_heads(&mut self, cx: &mut Context<Self>) {
        let Some(mine) = self.my_account() else {
            return;
        };
        let heads: Vec<(String, u64)> = self
            .channels
            .ready()
            .into_iter()
            .flatten()
            .filter(|info| dm_peer_of(mine, &info.channel.id).is_some())
            .map(|info| (info.channel.id.clone(), info.head_seq))
            .collect();
        self.dm_heads
            .retain(|id, _| heads.iter().any(|(held, _)| held == id));
        let viewer = self.viewer();
        for (id, seq) in heads {
            let head = self.dm_heads.entry(id.clone()).or_default();
            let moved = seq > 0 && head.seq != seq;
            if !moved {
                continue;
            }
            head.seq = seq;
            let newest = queries::newest(cx.host(), id.clone(), seq, viewer.clone());
            head.reading = Some(cx.land(newest, move |chat, time, cx| {
                let Some(head) = chat.dm_heads.get_mut(&id).filter(|head| head.seq == seq) else {
                    return;
                };
                head.reading = None;
                match time {
                    Ok(time) => {
                        head.time = time.or(head.time);
                        cx.notify();
                    }
                    Err(refusal) => cx.log_refused("a direct room's newest message", &refusal),
                }
            }));
        }
    }

    pub(crate) fn dm_choices(&self, query: &str) -> Vec<DmChoice> {
        let (Some(mine), Some(names)) = (self.my_account(), self.names.ready()) else {
            return Vec::new();
        };
        let query = query.trim().to_lowercase();
        let mut choices: Vec<DmChoice> = names
            .people()
            .filter(|peer| *peer != mine && person_or_agent(names, *peer))
            .map(|peer| {
                let principal = Principal::Account(peer);
                DmChoice {
                    peer,
                    name: names.member(&principal),
                    agent: crate::message::agent(names, &principal),
                }
            })
            .filter(|choice| choice.matches(&query))
            .collect();
        choices.sort_by_key(|choice| (choice.name.to_lowercase(), choice.peer));
        choices
    }

    pub(crate) fn begin_dm(&mut self) {
        self.dm_pick.get_or_insert_default();
    }

    pub(crate) fn open_dm(&mut self, peer: u64, cx: &mut Context<Self>) {
        let Some(mine) = self.my_account() else {
            return;
        };
        self.dm_pick = None;
        self.choose(chat::dm_channel_id(mine, peer), cx);
    }

    pub(crate) fn open_first_dm(&mut self, cx: &mut Context<Self>) {
        let first = self
            .dm_pick
            .as_ref()
            .map(|pick| self.dm_choices(&pick.query.text()))
            .and_then(|choices| choices.first().map(|choice| choice.peer));
        let Some(peer) = first else {
            return;
        };
        self.open_dm(peer, cx);
    }

    /// The row the room list's arrows are on: where they were left, else
    /// the open room, else the first.
    pub(crate) fn rooms_at(&self, ids: &[String]) -> usize {
        let open = self.room.as_ref().map(|room| room.id.as_str());
        self.rooms_cursor
            .as_deref()
            .or(open)
            .and_then(|id| ids.iter().position(|room| room == id))
            .unwrap_or(0)
    }
}
