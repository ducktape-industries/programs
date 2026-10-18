//! The rich composer: what a draft is for (a [`Target`]), the key it is
//! kept and focused under, and the chat op a committed draft becomes. The
//! draft over the host's text field is `draft`; the tree that draws it,
//! `binding`.
pub(crate) mod attachments;
mod binding;
mod draft;

pub use attachments::{Attaching, Upload};
pub use binding::{Click, Event, Outcome, view};
pub(crate) use binding::{
    Handle, Presses, chips, dropped, editor, mention_popup, opening, picks, press, presses,
};
pub use draft::{Draft, MentionChoice, Place, Send, focus};
pub(crate) use draft::{editor_id, matching_choices};

use chat::message::Attachment;
use chat::{Block, MsgRow, Op, Principal, parse_message};
use serde::{Deserialize, Serialize};
use view::host::Error;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Target {
    Post {
        channel: String,
        thread: Option<u64>,
    },
    Edit {
        channel: String,
        seq: u64,
        base_rev: u32,
    },
}

impl Target {
    pub fn channel(&self) -> &str {
        match self {
            Target::Post { channel, .. } | Target::Edit { channel, .. } => channel,
        }
    }

    /// The key the draft is kept under; its editor is `<key>/editor`.
    pub fn key(&self) -> String {
        match self {
            Target::Post {
                channel,
                thread: None,
            } => format!("draft-{channel}"),
            Target::Post {
                channel,
                thread: Some(root),
            } => format!("draft-{channel}-{root}"),
            Target::Edit { channel, seq, .. } => format!("edit-{channel}-{seq}"),
        }
    }

    /// Whether `key` is an edit's.
    pub fn edits(key: &str) -> bool {
        key.starts_with("edit-")
    }
}

/// The op a committed draft becomes, `id` naming a new message.
pub fn op(id: String, send: &Send, files: &[Attachment], target: &Target) -> Result<Op, Error> {
    let body = &send.body;
    if body.is_empty() && files.is_empty() {
        return Err(Error::new("invalid_body", "Message must not be empty"));
    }
    let mut blocks = match body.is_empty() {
        true => Vec::new(),
        false => parse_message(body),
    };
    blocks.extend(files.iter().cloned().map(Block::Attachment));
    Ok(match target.clone() {
        Target::Post { channel, thread } => Op::PostMessage {
            channel_id: channel,
            message_id: id,
            blocks,
            thread,
        },
        Target::Edit {
            channel,
            seq,
            base_rev,
        } => Op::EditMessage {
            channel_id: channel,
            seq,
            blocks,
            base_rev: Some(base_rev),
        },
    })
}

pub fn pending_row(op: &Op, author: Principal) -> Option<MsgRow> {
    let Op::PostMessage {
        message_id,
        blocks,
        thread,
        ..
    } = op
    else {
        return None;
    };
    Some(MsgRow {
        message_id: message_id.clone(),
        blocks: blocks.clone(),
        thread: *thread,
        ..MsgRow::by(author)
    })
}
