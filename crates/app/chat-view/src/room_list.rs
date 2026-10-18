//! The rooms the reader can open, one list for both trees: the channels, then the direct rooms
//! the reader holds, newest message first. A program's own rooms (`forge:…` review threads)
//! are its to show.
use std::cmp::Reverse;

use chat::view::Names;
use chat::{Category, ChannelInfo, Kind, Principal};
use view::prelude::*;

use crate::Chat;
use crate::names::dm_peer_of;
use crate::queries;

// A direct room in the list: the room and the other person.
pub(crate) struct DmEntry<'a> {
    pub info: &'a ChannelInfo,
    pub peer: u64,
}

pub(crate) struct RoomList<'a> {
    pub channels: Vec<&'a ChannelInfo>,
    pub dms: Vec<DmEntry<'a>>,
    // the rooms left out because they are archived
    pub archived: usize,
}

impl RoomList<'_> {
    /// Every room's id, in list order: what the arrows walk.
    pub(crate) fn ids(&self) -> Vec<String> {
        let channels = self.channels.iter().map(|info| info.channel.id.clone());
        channels
            .chain(self.dms.iter().map(|dm| dm.info.channel.id.clone()))
            .collect()
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
        list.dms.sort_by_key(|dm| Reverse(self.recency(dm)));
        list
    }

    fn recency(&self, dm: &DmEntry) -> u64 {
        let created = dm.info.channel.created_at;
        self.dm_heads
            .get(&dm.info.channel.id)
            .and_then(|head| head.time)
            .unwrap_or(created)
    }

    fn held_rooms(&self) -> RoomList<'_> {
        let mine = self.my_account();
        let mut channels = Vec::new();
        let mut dms = Vec::new();
        let mut archived = 0;
        for info in self.channels.ready().into_iter().flatten() {
            let id = info.channel.id.as_str();
            if chat::namespace::program(id).is_some() {
                continue;
            }
            // an archived room leaves the lists, as it is archived; a link
            // or the open room still reaches it through `channels`
            if info.channel.archived {
                archived += 1;
                continue;
            }
            if chat::dm_peers(id).is_none() {
                channels.push(info);
                continue;
            }
            dms.extend(
                mine.and_then(|mine| dm_peer_of(mine, id))
                    .map(|peer| DmEntry { info, peer }),
            );
        }
        RoomList {
            channels,
            dms,
            archived,
        }
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
        let Some(id) = self.rooms_cursor.as_deref().or(open) else {
            return 0;
        };
        let at = |room: &str| ids.iter().position(|listed| listed == room);
        at(id).or_else(|| self.in_place_of(id, &at)).unwrap_or(0)
    }

    // A room the list leaves out (an archived channel) hands its row to the
    // next listed channel after it in the program's order, else to the last
    // one before it.
    fn in_place_of(&self, id: &str, at: &impl Fn(&str) -> Option<usize>) -> Option<usize> {
        let channels: Vec<&str> = self
            .channels
            .ready()?
            .iter()
            .map(|info| info.channel.id.as_str())
            .filter(|room| chat::dm_peers(room).is_none())
            .collect();
        let place = channels.iter().position(|room| *room == id)?;
        let (before, after) = channels.split_at(place);
        let next = after.iter().copied().find_map(at);
        let last = || before.iter().rev().copied().find_map(at);
        next.or_else(last)
    }
}
