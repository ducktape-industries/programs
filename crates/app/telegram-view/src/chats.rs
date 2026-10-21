use crate::words;
use crate::{Telegram, auth::decode, rpc::Rpc};
use grammers_tl_types::{self as tl, Serializable};
use std::collections::BTreeMap;
use view::prelude::*;

pub(crate) const LOCAL_FIRST_PAGE_SIZE: i32 = 50;

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
)]
pub(crate) enum Peer {
    User(i64),
    Group(i64),
    Channel(i64),
}
impl Peer {
    pub(crate) fn from_tl(peer: &tl::enums::Peer) -> Self {
        match peer {
            tl::enums::Peer::User(p) => Self::User(p.user_id),
            tl::enums::Peer::Chat(p) => Self::Group(p.chat_id),
            tl::enums::Peer::Channel(p) => Self::Channel(p.channel_id),
        }
    }
    pub(crate) fn key(self) -> String {
        match self {
            Self::User(id) => format!("u{id}"),
            Self::Group(id) => format!("g{id}"),
            Self::Channel(id) => format!("c{id}"),
        }
    }
}

#[derive(Clone, borsh::BorshDeserialize, borsh::BorshSerialize)]
pub(crate) struct PeerInfo {
    pub(crate) name: String,
    hash: Option<i64>,
}

pub(crate) struct Dialog {
    pub(crate) peer: Peer,
    pub(crate) id: i32,
    pub(crate) date: i32,
    pub(crate) text: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Cursor {
    pub(crate) peer: Peer,
    pub(crate) id: i32,
    pub(crate) date: i32,
}

#[derive(Default)]
pub(crate) struct Chats {
    pub(crate) peers: BTreeMap<Peer, PeerInfo>,
    pub(crate) dialogs: Vec<Dialog>,
    pub(crate) after: Option<Cursor>,
    pub(crate) more: bool,
    pub(crate) loaded: bool,
}

impl Telegram {
    pub(crate) fn peer_name(&self, peer: Peer) -> String {
        self.chats
            .peers
            .get(&peer)
            .map(|info| info.name.clone())
            .unwrap_or_else(|| match peer {
                Peer::User(id) => words::user(id),
                Peer::Group(id) => words::group(id),
                Peer::Channel(id) => words::channel(id),
            })
    }

    pub(crate) fn input_peer(&self, peer: Peer) -> Result<tl::enums::InputPeer, String> {
        if matches!(peer,Peer::User(id) if self.live.session.user==Some(id)) {
            return Ok(tl::enums::InputPeer::PeerSelf);
        }
        let hash = || {
            self.chats
                .peers
                .get(&peer)
                .and_then(|info| info.hash)
                .ok_or_else(|| words::NO_CHAT_HASH.to_owned())
        };
        Ok(match peer {
            Peer::User(user_id) => tl::enums::InputPeer::User(tl::types::InputPeerUser {
                user_id,
                access_hash: hash()?,
            }),
            Peer::Group(chat_id) => {
                tl::enums::InputPeer::Chat(tl::types::InputPeerChat { chat_id })
            }
            Peer::Channel(channel_id) => {
                tl::enums::InputPeer::Channel(tl::types::InputPeerChannel {
                    channel_id,
                    access_hash: hash()?,
                })
            }
        })
    }

    pub(crate) fn cache_peers(
        &mut self,
        users: Vec<tl::enums::User>,
        chats: Vec<tl::enums::Chat>,
        cx: &mut Context<Self>,
    ) {
        let mut changed = false;
        for user in users {
            if let tl::enums::User::User(user) = user {
                let name = [
                    user.first_name.as_deref().unwrap_or(""),
                    user.last_name.as_deref().unwrap_or(""),
                ]
                .join(" ")
                .trim()
                .to_owned();
                let name = if name.is_empty() {
                    user.username.unwrap_or_else(|| words::user(user.id))
                } else {
                    name
                };
                changed |= self.cache_peer(
                    Peer::User(user.id),
                    name,
                    if user.min { None } else { user.access_hash },
                    user.min,
                );
            }
        }
        for chat in chats {
            let (peer, name, hash, min) = match chat {
                tl::enums::Chat::Chat(chat) => (Peer::Group(chat.id), chat.title, None, false),
                tl::enums::Chat::Forbidden(chat) => (Peer::Group(chat.id), chat.title, None, false),
                tl::enums::Chat::Channel(chat) => (
                    Peer::Channel(chat.id),
                    chat.title,
                    if chat.min { None } else { chat.access_hash },
                    chat.min,
                ),
                tl::enums::Chat::ChannelForbidden(chat) => (
                    Peer::Channel(chat.id),
                    chat.title,
                    Some(chat.access_hash),
                    false,
                ),
                _ => continue,
            };
            changed |= self.cache_peer(peer, name, hash, min);
        }
        if changed {
            cx.store().set("peers", &self.chats.peers);
        }
    }

    fn cache_peer(&mut self, peer: Peer, name: String, hash: Option<i64>, min: bool) -> bool {
        let previous = self.chats.peers.get(&peer);
        let hash = hash.or(previous.and_then(|info| info.hash));
        let name_preserving_previous_for_min_object = match previous {
            Some(info) if min => info.name.clone(),
            _ => name,
        };
        let name = name_preserving_previous_for_min_object;
        let changed = previous.is_none_or(|info| info.hash != hash || info.name != name);
        self.chats.peers.insert(peer, PeerInfo { name, hash });
        changed
    }

    pub(crate) fn read_dialogs(&mut self, older: bool, cx: &mut Context<Self>) {
        let dialogs_request_is_unavailable_or_pending = !self.live.ready
            || self.live.session.user.is_none()
            || self
                .live
                .calls
                .values()
                .any(|call| matches!(call.kind, Rpc::Dialogs));
        if dialogs_request_is_unavailable_or_pending {
            return;
        }
        let after = if older {
            self.chats.after.clone()
        } else {
            None
        };
        let (offset_date, offset_id, offset_peer) = match &after {
            Some(after) => match self.input_peer(after.peer) {
                Ok(peer) => (after.date, after.id, peer),
                Err(why) => {
                    self.failed = Some(why);
                    cx.notify();
                    return;
                }
            },
            None => (0, 0, tl::enums::InputPeer::Empty),
        };
        self.call(
            Rpc::Dialogs,
            tl::functions::messages::GetDialogs {
                exclude_pinned: older,
                folder_id: None,
                offset_date,
                offset_id,
                offset_peer,
                limit: LOCAL_FIRST_PAGE_SIZE,
                hash: 0,
            }
            .to_bytes(),
            cx,
        );
    }

    pub(crate) fn dialogs_reply(
        &mut self,
        body: &[u8],
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let (dialogs, messages, chats, users, total) =
            match decode::<tl::enums::messages::Dialogs>(body)? {
                tl::enums::messages::Dialogs::Dialogs(page) => {
                    (page.dialogs, page.messages, page.chats, page.users, None)
                }
                tl::enums::messages::Dialogs::Slice(page) => (
                    page.dialogs,
                    page.messages,
                    page.chats,
                    page.users,
                    Some(page.count),
                ),
                tl::enums::messages::Dialogs::NotModified(_) => {
                    self.chats.loaded = true;
                    return Ok(());
                }
            };
        self.cache_peers(users, chats, cx);
        let mut after = None;
        let mut pinned = false;
        for dialog in dialogs {
            let tl::enums::Dialog::Dialog(dialog) = dialog else {
                continue;
            };
            let peer = Peer::from_tl(&dialog.peer);
            if let Peer::Channel(id) = peer {
                let dialog_channel_position = dialog.pts;
                if let Some(pts) = dialog_channel_position {
                    self.channel_position(id, pts, cx);
                }
            }
            let message = messages.iter().find(|message| {
                message.id() == dialog.top_message
                    && message_peer(message, self.live.session.user) == Some(peer)
            });
            let (date, text) = message.map_or((0, String::new()), |message| {
                (message_date(message), message_text(message))
            });
            pinned |= dialog.pinned;
            if !dialog.pinned {
                after = Some(Cursor {
                    peer,
                    id: dialog.top_message,
                    date,
                });
            }
            self.upsert_dialog(Dialog {
                peer,
                id: dialog.top_message,
                date,
                text,
            });
        }
        self.chats.more = total.is_some_and(|total| (self.chats.dialogs.len() as i32) < total)
            && ((after.is_some() && after != self.chats.after) || (pinned && after.is_none()));
        self.chats.after = after;
        self.chats.loaded = true;
        cx.notify();
        Ok(())
    }

    pub(crate) fn upsert_dialog(&mut self, dialog: Dialog) {
        if let Some(old) = self
            .chats
            .dialogs
            .iter_mut()
            .find(|old| old.peer == dialog.peer)
        {
            if (dialog.date, dialog.id) >= (old.date, old.id) {
                *old = dialog;
            }
        } else {
            self.chats.dialogs.push(dialog);
        }
        self.chats.dialogs.sort_by(|a, b| {
            b.date
                .cmp(&a.date)
                .then_with(|| b.id.cmp(&a.id))
                .then_with(|| a.peer.cmp(&b.peer))
        });
    }
}

pub(crate) fn message_peer(message: &tl::enums::Message, me: Option<i64>) -> Option<Peer> {
    let (peer, from) = match message {
        tl::enums::Message::Message(message) => (
            Some(Peer::from_tl(&message.peer_id)),
            message.from_id.as_ref(),
        ),
        tl::enums::Message::Service(message) => (
            Some(Peer::from_tl(&message.peer_id)),
            message.from_id.as_ref(),
        ),
        tl::enums::Message::Empty(message) => (message.peer_id.as_ref().map(Peer::from_tl), None),
    };
    if peer == me.map(Peer::User) {
        from.map(Peer::from_tl).or(peer)
    } else {
        peer
    }
}
pub(crate) fn message_date(message: &tl::enums::Message) -> i32 {
    match message {
        tl::enums::Message::Message(message) => message.date,
        tl::enums::Message::Service(message) => message.date,
        _ => 0,
    }
}
pub(crate) fn message_text(message: &tl::enums::Message) -> String {
    match message {
        tl::enums::Message::Empty(_) => words::UNAVAILABLE.into(),
        tl::enums::Message::Service(_) => words::SERVICE.into(),
        tl::enums::Message::Message(message) => {
            let media = message.media.as_ref().and_then(media_text);
            match (message.message.is_empty(), media) {
                (true, Some(media)) => media,
                (false, Some(media)) => words::caption(&message.message, &media),
                (_, None) => message.message.clone(),
            }
        }
    }
}
fn media_text(media: &tl::enums::MessageMedia) -> Option<String> {
    use tl::enums::MessageMedia as Media;
    Some(match media {
        Media::Empty | Media::WebPage(_) => return None,
        Media::Photo(_) => words::PHOTO.into(),
        Media::Document(media) => {
            let Some(tl::enums::Document::Document(document)) = &media.document else {
                return Some(words::FILE.into());
            };
            document
                .attributes
                .iter()
                .find_map(|attribute| match attribute {
                    tl::enums::DocumentAttribute::Filename(file) => {
                        Some(words::file(&file.file_name))
                    }
                    _ => None,
                })
                .unwrap_or_else(|| words::FILE.into())
        }
        Media::Geo(_) | Media::GeoLive(_) | Media::Venue(_) => words::LOCATION.into(),
        Media::Contact(_) => words::CONTACT.into(),
        Media::Poll(_) => words::POLL.into(),
        Media::Story(_) => words::STORY.into(),
        Media::Dice(_) => words::DICE.into(),
        _ => words::MEDIA.into(),
    })
}
