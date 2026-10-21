use crate::{
    Telegram,
    auth::decode,
    chats::{Dialog, Peer, message_date, message_peer, message_text},
    history::Message,
    random,
    rpc::Rpc,
};
use grammers_tl_types::{self as tl, Serializable};
use view::prelude::*;

impl Telegram {
    pub(crate) fn send_text(&mut self, cx: &mut Context<Self>) {
        let Some(history) = &self.history else {
            return;
        };
        let send_is_unavailable_or_signed_out =
            !self.live.ready || self.live.session.user.is_none();
        if send_is_unavailable_or_signed_out {
            return;
        }
        let text = self.compose.text();
        if text.trim().is_empty() {
            return;
        }
        let peer = history.peer;
        let input = match self.input_peer(peer) {
            Ok(input) => input,
            Err(why) => {
                self.failed = Some(why);
                cx.notify();
                return;
            }
        };
        let body = tl::functions::messages::SendMessage {
            no_webpage: false,
            silent: false,
            background: false,
            clear_draft: false,
            noforwards: false,
            update_stickersets_order: false,
            invert_media: false,
            allow_paid_floodskip: false,
            peer: input,
            reply_to: None,
            message: text.clone(),
            random_id: i64::from_le_bytes(random::bytes()),
            reply_markup: None,
            entities: None,
            schedule_date: None,
            schedule_repeat_period: None,
            send_as: None,
            quick_reply_shortcut: None,
            effect: None,
            allow_paid_stars: None,
            suggested_post: None,
            rich_message: None,
        }
        .to_bytes();
        self.compose.reset("");
        self.failed = None;
        self.call(Rpc::Send { peer, text }, body, cx);
    }

    pub(crate) fn sent(
        &mut self,
        peer: Peer,
        text: String,
        body: &[u8],
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let updates = decode::<tl::enums::Updates>(body)?;
        if let tl::enums::Updates::UpdateShortSentMessage(sent) = &updates {
            self.upsert_dialog(Dialog {
                peer,
                id: sent.id,
                date: sent.date,
                text: text.clone(),
            });
            if let Some(history) = &mut self.history {
                let history_is_for_sent_peer = history.peer == peer;
                if history_is_for_sent_peer {
                    history.scroll.scroll_to_bottom();
                    history.messages.insert(
                        sent.id,
                        Message {
                            id: sent.id,
                            sender: Peer::User(self.live.session.user.unwrap()),
                            author: None,
                            date: sent.date,
                            text: text.clone(),
                        },
                    );
                }
            }
        }
        self.receive_updates(updates, cx);
        if let Some(history) = &self.history {
            let history_is_for_sent_peer = history.peer == peer;
            if history_is_for_sent_peer {
                history.scroll.scroll_to_bottom();
            }
        }
        cx.notify();
        Ok(())
    }

    pub(crate) fn show_message(&mut self, message: tl::enums::Message) {
        if let Some(peer) = message_peer(&message, self.live.session.user) {
            self.upsert_dialog(Dialog {
                peer,
                id: message.id(),
                date: message_date(&message),
                text: message_text(&message),
            });
            self.cache_message(message);
        }
    }
}
