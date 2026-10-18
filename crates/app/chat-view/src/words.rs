//! What chat says, once: every sentence, status word, title and label
//! that more than one place shows, in either tree. The Expo app types
//! these in its chat screens (it has no chat `words.ts`); here a tree that
//! shows one reads it from this file. The model keeps the sentences only
//! it says: a room's intro (`names.rs`), the composer's hint (`room.rs`),
//! and the notices a write or a read leaves (`actions.rs`, `room.rs`,
//! `search.rs`, `watch.rs`).
use chat::ChannelRow;

#[cfg(not(feature = "notifier"))]
use crate::session::Gate;

// ---------- labels both trees type ----------

pub(crate) const SEND: &str = "Send";
pub(crate) const CANCEL: &str = "Cancel";
pub(crate) const DELETE: &str = "Delete";
pub(crate) const DISMISS: &str = "Dismiss";
/// Puts a refused send's words back in the composer.
pub(crate) const RESTORE: &str = "Restore";
/// Under a message changed since it was posted.
pub(crate) const EDITED: &str = "edited";
pub(crate) const NAME: &str = "Name";
pub(crate) const RENAME: &str = "Rename";
pub(crate) const PEOPLE: &str = "People";
pub(crate) const NEW_CHANNEL: &str = "New channel";

// ---------- the rooms ----------

/// The program's name, over its rooms and on its root.
pub(crate) const CHAT: &str = "Chat";
pub(crate) const CHANNELS: &str = "Channels";
pub(crate) const DIRECT_MESSAGES: &str = "Direct messages";
pub(crate) const LOADING_ROOMS: &str = "Loading rooms…";
pub(crate) const SEARCH_MESSAGES: &str = "Search messages";
/// What an agent's badge says.
pub(crate) const AGENT: &str = "Agent";

pub(crate) const FIND_SOMEONE: &str = "Find a person or agent";
pub(crate) const SEARCH_PEOPLE: &str = "Search by name or number";
pub(crate) const NO_ONE_MATCHES: &str = "Nobody matches that name or number";
pub(crate) const LOADING_PEOPLE: &str = "Loading people…";
pub(crate) const DM_NEEDS_ACCOUNT: &str = "Create an account to message someone";

/// Who may post in a channel, as its caption, its chip and its details say.
pub(crate) const OPEN: &str = "Open";
pub(crate) const MEMBERS_ONLY: &str = "Members only";
pub(crate) const ARCHIVED: &str = "Archived";
/// The new-channel form's policy choice: its label and its name.
pub(crate) const WHO_MAY_POST: &str = "Who may post";

/// Who may post in `channel`: an archived one takes no posts at all.
pub(crate) fn policy(channel: &ChannelRow) -> &'static str {
    match (channel.archived, channel.members_only()) {
        (true, _) => ARCHIVED,
        (false, true) => MEMBERS_ONLY,
        (false, false) => OPEN,
    }
}

/// An account by its number, where no name is known or beside the name.
pub(crate) fn account(number: u64) -> String {
    format!("account {number}")
}

// ---------- a room ----------

pub(crate) const CHANNEL_DETAILS: &str = "Channel details";
pub(crate) const THREAD: &str = "Thread";
pub(crate) const LOADING_MESSAGES: &str = "Loading messages…";
pub(crate) const LOADING_OLDER: &str = "Loading older messages…";
pub(crate) const NO_REPLIES: &str = "No replies yet";

/// A read of the room refused: the desktop's notice, the phone's line at
/// the list's end.
pub(crate) fn unreadable_room(why: &str) -> String {
    format!("Couldn’t read this room: {why}")
}

/// What a deleted message says in its place.
pub(crate) const MESSAGE_DELETED: &str = "Message deleted";

#[cfg(not(feature = "notifier"))]
/// Where the composer would be, why the reader may not write here.
pub(crate) fn gate(gate: Gate) -> &'static str {
    match gate {
        Gate::Archived => {
            "This channel is archived. It keeps its history and takes no new messages."
        }
        Gate::NotMember => {
            "This channel is members-only and your key is not on its roster. Ask a member to add your key from Channel details."
        }
        Gate::NoAccount => {
            "To send messages, create or join an account in Account. You can read this channel without an account."
        }
    }
}

/// A program's post's way to the program.
pub(crate) fn open_in(program: &str) -> String {
    format!("Open in {program}")
}

/// A root's thread button, as assistive technology names it.
pub(crate) fn open_thread(replies: &str) -> String {
    format!("Open thread, {replies}")
}

// ---------- the composer ----------

/// What the composer's field is, by what it writes.
pub(crate) const NEW_MESSAGE: &str = "New message";
pub(crate) const REPLY: &str = "Reply";
pub(crate) const EDITED_MESSAGE: &str = "Edited message";
/// Over a refused send's words, put back to try again.
pub(crate) const UNSENT: &str = "An earlier message wasn’t sent";
/// What a send's words are about when the direct message it opens first
/// did not open: the message itself never left.
pub(crate) const OPENING_DM: &str = "Opening the conversation";

// ---------- a message's actions and the picker ----------

pub(crate) const MESSAGE_ACTIONS: &str = "Message actions";
pub(crate) const ADD_REACTION: &str = "Add reaction";
pub(crate) const REPLY_IN_THREAD: &str = "Reply in thread";
pub(crate) const COPY_LINK: &str = "Copy link";
pub(crate) const EDIT_MESSAGE: &str = "Edit message";
pub(crate) const DELETE_MESSAGE: &str = "Delete message";
pub(crate) const DELETE_QUESTION: &str = "Delete this message?";
/// What the clipboard took, as the confirmation names it.
pub(crate) const MESSAGE_LINK: &str = "message link";
pub(crate) const EMOJI_CATEGORIES: &str = "Emoji categories";
pub(crate) const FREQUENTLY_USED: &str = "Frequently used";
pub(crate) const NO_EMOJI_MATCH: &str = "No emoji match";
pub(crate) const SEARCH_EMOJI: &str = "Search emoji";

/// An emoji cell, as assistive technology names it.
pub(crate) fn react_with(emoji: &str) -> String {
    format!("React with {emoji}")
}

// ---------- channel details ----------

pub(crate) const MEMBERS: &str = "Members";
pub(crate) const NO_MEMBERS: &str = "No members added. An open channel needs none.";
pub(crate) const ARCHIVE_CHANNEL: &str = "Archive channel";
pub(crate) const UNARCHIVE_CHANNEL: &str = "Unarchive channel";
/// An archived room's own way back, beside its strip.
pub(crate) const UNARCHIVE: &str = "Unarchive";
pub(crate) const RENAME_CHANNEL: &str = "Rename channel";
/// The Add a member field's label and placeholder.
pub(crate) const ACCOUNT_NUMBER: &str = "Account number";
