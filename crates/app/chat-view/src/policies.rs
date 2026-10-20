//! Who can edit the open room and the message whose menu is open, as the ownership program says
//! (through `policy_view::item`): each read while something on screen asks it, and what the reader
//! may do by it. An item with no record keeps the rule it had before policies (Q8).
use ownership::Party;
use policy_view::{Editing, Item, Sheet, item};
use view::design::Ladder;
use view::methods::Change;
use view::prelude::*;
use view::{Div, Stateful};

use crate::{Chat, Pane};

const ROOM: &str = "chat-room-policy";
const MESSAGE: &str = "chat-message-policy";

pub(crate) fn room_policy(chat: &mut Chat) -> &mut Editing {
    &mut chat.room_policy
}

pub(crate) fn message_policy(chat: &mut Chat) -> &mut Editing {
    &mut chat.message_policy
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Offered {
    pub(crate) react: bool,
    pub(crate) edit: bool,
    pub(crate) delete: bool,
}

impl Chat {
    fn reader(&self) -> Option<Party> {
        self.my_account().map(Party::Account)
    }

    fn room_item(&self) -> Option<Item> {
        let info = self.room_info()?;
        Some(Item {
            program: chat::PROGRAM.to_owned(),
            key: chat::tables::CHANNELS.key(&info.channel.id),
            label: format!("#{}", info.channel.name),
        })
    }

    pub(crate) fn message_item(&self, pane: Pane, seq: u64) -> Option<Item> {
        let row = self.shown(pane).find(|row| row.seq == seq)?;
        Some(Item {
            program: chat::PROGRAM.to_owned(),
            key: chat::tables::MESSAGE_IDS.key(&row.message_id),
            label: "this message".to_owned(),
        })
    }

    pub(crate) fn settle_policies(&mut self, cx: &mut Context<Self>) {
        let reader = self.reader();
        let archived = self.room_info().is_some_and(|info| info.channel.archived);
        let asked = self.details.is_some() || self.menu.is_some() || archived;
        match self.room_item().filter(|_| asked) {
            Some(room) => item::hold(self, room_policy, room, reader.clone(), cx),
            None => item::let_go(self, room_policy),
        }
        let menu = self.menu.as_ref().map(|menu| (menu.pane, menu.seq));
        match menu.and_then(|(pane, seq)| self.message_item(pane, seq)) {
            Some(message) => item::hold(self, message_policy, message, reader, cx),
            None if self.message_policy.sheet.is_none() => item::let_go(self, message_policy),
            None => {}
        }
    }

    pub(crate) fn may_change_room(&self) -> bool {
        let read = self
            .room_item()
            .and_then(|room| self.room_policy.admits(&room));
        read.unwrap_or_else(|| {
            self.room_info()
                .is_some_and(|info| Some(&info.channel.owner) == self.me().as_ref())
        })
    }

    pub(crate) fn may_edit(&self, pane: Pane, seq: u64) -> bool {
        let read = self
            .message_item(pane, seq)
            .and_then(|message| self.message_policy.admits(&message));
        read.unwrap_or_else(|| self.wrote(pane, seq))
    }

    pub(crate) fn offered(&self, pane: Pane, seq: u64) -> Offered {
        let standing = self.shown(pane).any(|row| row.seq == seq && !row.deleted);
        let writes = standing && self.may_write();
        Offered {
            react: writes,
            edit: writes && self.editable(pane, seq) && self.may_edit(pane, seq),
            delete: standing && self.may_delete(pane, seq),
        }
    }

    pub(crate) fn who_can_edit(&mut self, pane: Pane, seq: u64, cx: &mut Context<Self>) {
        let Some(message) = self.message_item(pane, seq) else {
            return;
        };
        let reader = self.reader();
        self.menu = None;
        item::hold(self, message_policy, message, reader, cx);
        item::open_sheet(self, message_policy, Sheet::About, cx);
    }

    pub(crate) fn ownership_moved(&mut self, change: Option<Change>, cx: &mut Context<Self>) {
        item::moved(self, room_policy, change.as_ref(), cx);
        item::moved(self, message_policy, change.as_ref(), cx);
    }
}

pub(crate) fn room_row(
    chat: &Chat,
    cx: &mut Context<Chat>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<Stateful<Div>> {
    let dm = chat
        .room_info()
        .is_some_and(|info| chat::dm_peers(&info.channel.id).is_some());
    if dm {
        return None;
    }
    item::row(&chat.room_policy, ROOM, room_policy, cx, ladder, theme)
}

pub(crate) fn sheet(
    chat: &Chat,
    cx: &mut Context<Chat>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<AnyElement> {
    item::sheet(
        &chat.message_policy,
        MESSAGE,
        message_policy,
        cx,
        ladder,
        theme,
    )
    .or_else(|| item::sheet(&chat.room_policy, ROOM, room_policy, cx, ladder, theme))
}
