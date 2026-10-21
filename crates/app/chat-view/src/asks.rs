//! An ask in a message, as its reader sees it: the options to pick while it is put to them and
//! unanswered, or what an addressee answered.
use chat::asking::{self, Ask};
use chat::{MsgRow, Principal};

use crate::message::ChatMessage;
use crate::{Chat, Pane};

/// What a message that asks the reader draws under its blocks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Shown {
    /// No addressee has replied: a button per option, and Other.
    Open(Ask),
    /// An addressee replied: the option their words name, or none for words of their own.
    Answered(Option<String>),
}

impl Chat {
    pub(crate) fn ask_shown(&self, message: &ChatMessage) -> Option<Shown> {
        let root = message.thread.is_none() && message.seq > 0;
        let live = root && !message.deleted && !message.pending;
        if !live {
            return None;
        }
        let ask = asking::read(&message.blocks)?;
        let me = self.my_account()?;
        if !ask.to.contains(&me) {
            return None;
        }
        let room = self.room.as_ref()?;
        let landed = room.answers.get(&message.seq).into_iter().flatten();
        let sent = room
            .pending
            .iter()
            .filter(|row| row.thread == Some(message.seq));
        let reply = landed.chain(sent).find(|reply| addressed(&ask, reply));
        match reply {
            Some(reply) => Some(Shown::Answered(answered(&ask, reply))),
            None if self.may_write() => Some(Shown::Open(ask)),
            None => None,
        }
    }

    pub(crate) fn option_of(&self, pane: Pane, seq: u64, at: usize) -> Option<String> {
        let row = self.shown(pane).find(|row| row.seq == seq)?;
        let ask = asking::read(&row.blocks)?;
        ask.options.get(at).map(|choice| choice.label.clone())
    }
}

/// An ask among the rows a room holds: its seq, and the accounts it is put to.
pub(crate) struct AskRow {
    seq: u64,
    to: Vec<u64>,
}

pub(crate) fn asked<'a>(rows: impl IntoIterator<Item = &'a MsgRow>) -> Vec<AskRow> {
    let roots = rows
        .into_iter()
        .filter(|row| row.thread.is_none() && !row.deleted);
    roots
        .filter_map(|row| {
            let ask = asking::read(&row.blocks)?;
            Some(AskRow {
                seq: row.seq,
                to: ask.to,
            })
        })
        .collect()
}

pub(crate) fn put_to(asks: Vec<AskRow>, reader: Option<u64>) -> Vec<u64> {
    let theirs = |ask: &AskRow| reader.is_some_and(|reader| ask.to.contains(&reader));
    asks.into_iter().filter(theirs).map(|ask| ask.seq).collect()
}

fn addressed(ask: &Ask, reply: &MsgRow) -> bool {
    matches!(reply.author, Principal::Account(account) if ask.to.contains(&account))
}

fn answered(ask: &Ask, reply: &MsgRow) -> Option<String> {
    if reply.deleted {
        return None;
    }
    let at = asking::answer(ask, &chat::plain_text(&reply.blocks))?;
    ask.options.get(at).map(|choice| choice.label.clone())
}
