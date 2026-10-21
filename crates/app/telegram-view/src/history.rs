use crate::{
    Telegram,
    auth::decode,
    chats::{LOCAL_FIRST_PAGE_SIZE, Peer, message_date, message_peer, message_text},
    rpc::Rpc,
};
use grammers_tl_types::{self as tl, Serializable};
use std::collections::BTreeMap;
use view::prelude::*;

pub(crate) struct Message {
    pub(crate) id: i32,
    pub(crate) sender: Peer,
    pub(crate) author: Option<String>,
    pub(crate) date: i32,
    pub(crate) text: String,
}

pub(crate) struct History {
    pub(crate) peer: Peer,
    pub(crate) messages: BTreeMap<i32, Message>,
    pub(crate) before: Option<i32>,
    pub(crate) more: bool,
    pub(crate) loaded: bool,
    pub(crate) scroll: ScrollHandle,
}

impl Telegram {
    pub(crate) fn close_chat(&mut self, cx: &mut Context<Self>) {
        self.history = None;
        cx.notify();
    }

    pub(crate) fn open_chat(&mut self, peer: Peer, cx: &mut Context<Self>) {
        self.history = Some(History {
            peer,
            messages: BTreeMap::new(),
            before: None,
            more: false,
            loaded: false,
            scroll: ScrollHandle::new(),
        });
        self.failed = None;
        self.compose.reset("");
        self.read_history(false, cx);
        cx.notify();
    }

    pub(crate) fn read_history(&mut self, older: bool, cx: &mut Context<Self>) {
        let Some(history) = &self.history else {
            return;
        };
        let history_request_is_unavailable_or_pending = !self.live.ready
            || self
                .live
                .calls
                .values()
                .any(|call| matches!(call.kind,Rpc::History(peer, _) if peer==history.peer));
        if history_request_is_unavailable_or_pending {
            return;
        }
        let older_history_is_exhausted = older && !history.more;
        if older_history_is_exhausted {
            return;
        }
        let peer = history.peer;
        let before = if older {
            history.before.unwrap_or(0)
        } else {
            0
        };
        let input = match self.input_peer(peer) {
            Ok(input) => input,
            Err(why) => {
                self.failed = Some(why);
                cx.notify();
                return;
            }
        };
        self.call(
            Rpc::History(peer, before),
            tl::functions::messages::GetHistory {
                peer: input,
                offset_id: before,
                offset_date: 0,
                add_offset: 0,
                limit: LOCAL_FIRST_PAGE_SIZE,
                max_id: 0,
                min_id: 0,
                hash: 0,
            }
            .to_bytes(),
            cx,
        );
    }

    pub(crate) fn history_reply(
        &mut self,
        peer: Peer,
        offset: i32,
        body: &[u8],
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let (messages, chats, users, total) = match decode::<tl::enums::messages::Messages>(body)? {
            tl::enums::messages::Messages::Messages(page) => {
                (page.messages, page.chats, page.users, None)
            }
            tl::enums::messages::Messages::Slice(page) => {
                (page.messages, page.chats, page.users, Some(page.count))
            }
            tl::enums::messages::Messages::ChannelMessages(page) => {
                if let Peer::Channel(id) = peer {
                    self.channel_position(id, page.pts, cx);
                }
                (page.messages, page.chats, page.users, Some(page.count))
            }
            tl::enums::messages::Messages::NotModified(_) => return Ok(()),
        };
        self.cache_peers(users, chats, cx);
        if self
            .history
            .as_ref()
            .is_none_or(|history| history.peer != peer)
        {
            return Ok(());
        }
        let count = messages.len();
        let before = messages.iter().map(tl::enums::Message::id).min();
        for message in messages {
            self.cache_message(message);
        }
        let history = self.history.as_mut().unwrap();
        let more = before.is_some()
            && (count == LOCAL_FIRST_PAGE_SIZE as usize
                || total.is_some_and(|total| total > history.messages.len() as i32));
        history.more = if offset == 0 {
            more || history.more
        } else {
            more && before != history.before
        };
        history.before = history.messages.keys().next().copied();
        if !history.loaded {
            history.scroll.scroll_to_bottom();
        }
        history.loaded = true;
        cx.notify();
        Ok(())
    }

    pub(crate) fn cache_message(&mut self, message: tl::enums::Message) {
        let Some(history) = &mut self.history else {
            return;
        };
        if message_peer(&message, self.live.session.user) != Some(history.peer) {
            return;
        }
        let (sender, author) = match &message {
            tl::enums::Message::Message(message) => (
                message
                    .from_id
                    .as_ref()
                    .map(Peer::from_tl)
                    .unwrap_or_else(|| {
                        if message.out {
                            self.live.session.user.map_or(history.peer, Peer::User)
                        } else {
                            history.peer
                        }
                    }),
                message.post_author.clone(),
            ),
            tl::enums::Message::Service(message) => (
                message.from_id.as_ref().map_or(history.peer, Peer::from_tl),
                None,
            ),
            _ => (history.peer, None),
        };
        let row = Message {
            id: message.id(),
            sender,
            author,
            date: message_date(&message),
            text: message_text(&message),
        };
        history.messages.insert(row.id, row);
    }
}
