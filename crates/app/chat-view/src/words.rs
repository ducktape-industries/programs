//! What chat says, once: every sentence, status word, title and label
//! that more than one place shows, in either tree. The Expo app types
//! these in its chat screens (it has no chat `words.ts`); here a tree that
//! shows one reads it from this file. The model keeps the sentences only
//! it says: a room's intro (`names.rs`), the composer's hint (`room.rs`),
//! and the notices a write or a read leaves (`actions.rs`, `room.rs`,
//! `search.rs`, `watch.rs`).
use chat::ChannelRow;

#[cfg(not(feature = "notifier"))]
use crate::notifier::Choice;
#[cfg(not(feature = "notifier"))]
use crate::session::Gate;
#[cfg(not(feature = "notifier"))]
use chat::Notices;

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
pub(crate) const COPY_TEXT: &str = "Copy text";
pub(crate) const MESSAGE_TEXT: &str = "message text";
pub(crate) const ROOM_ACTIONS: &str = "Room actions";
pub(crate) const ROOM_LINK: &str = "room link";
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
pub(crate) const CATEGORY: &str = "Category";
pub(crate) const NO_CATEGORY: &str = "No category";
pub(crate) const SET_CATEGORY: &str = "Set category";
pub(crate) const CLEAR_CATEGORY: &str = "Clear category";
pub(crate) const NOTIFICATIONS: &str = "Notifications";

/// A room's notices choice as its details offer it: its name, and what it
/// tells. A direct room's default is every message.
#[cfg(not(feature = "notifier"))]
pub(crate) fn choice(choice: Choice, dm: bool) -> (&'static str, &'static str) {
    match choice {
        Choice::Every => ("All messages", "Every new message, in every thread too"),
        Choice::Default if dm => ("On", "Every message"),
        Choice::Default => (
            "Default",
            "Mentions, threads you are in, and the room once you write in it, open it or sit in it",
        ),
        Choice::Cut(Notices::Mentions) => ("Mentions only", "Only what mentions you"),
        Choice::Cut(Notices::Off) => ("Off", "Nothing from here"),
    }
}

// ---------- a message's files ----------

pub(crate) const ATTACH: &str = "Attach files";
pub(crate) const SAVE: &str = "Save";
pub(crate) const UPLOADING: &str = "Uploading…";
// a put the host ended before the network held it
pub(crate) const PUT_CLOSED: &str = "The upload stopped before the network held the file";
// a file a message names that files does not answer for
pub(crate) const NOT_THERE: &str = "The file isn’t there";

// how far a file is up: the bytes the host sent of all it sends
pub(crate) fn uploading((sent, all): (u64, u64)) -> String {
    let percent = match all {
        0 => 100,
        all => sent.min(all) * 100 / all,
    };
    format!("Uploading {percent}%")
}

pub(crate) fn remove_file(name: &str) -> String {
    format!("Remove {name}")
}

// the message of the commit that puts a draft's file
pub(crate) fn attached(name: &str) -> String {
    format!("Attach {name} to a chat message")
}

// the message of the commit that takes a draft's file out again
pub(crate) fn unattached(name: &str) -> String {
    format!("Take {name} out of a chat draft")
}

// a send handed back because one of its files did not go up
pub(crate) fn not_up(name: &str, why: &str) -> String {
    format!("{name} didn’t upload: {why}")
}

// a count of bytes, in the unit that reads best
pub(crate) fn size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    match bytes {
        1 => "1 byte".to_owned(),
        bytes if bytes < KB => format!("{bytes} bytes"),
        bytes if bytes < MB => format!("{:.1} KB", bytes as f64 / KB as f64),
        bytes => format!("{:.1} MB", bytes as f64 / MB as f64),
    }
}

// ---------- widgets ----------

// what a widget is called until its file names it
pub(crate) const WIDGET: &str = "Widget";
pub(crate) const WIDGET_LOADING: &str = "Loading…";
pub(crate) const WIDGET_EMPTY: &str = "Nothing to show.";
pub(crate) const WIDGET_NOT_TEXT: &str = "The widget’s file is not text.";

// what the host's log says a widget's follow of `program` was refused as
pub(crate) fn widget_heads(program: &str) -> String {
    format!("{program}'s live heads for a widget")
}

pub(crate) fn widget_not_a_file(path: &str) -> String {
    format!("No widget file at {path}.")
}

pub(crate) fn widget_unreadable(why: &str) -> String {
    format!("The widget’s file doesn’t read: {why}")
}

pub(crate) fn widget_reply(why: &str) -> String {
    format!("The reply doesn’t read: {why}")
}

// a question the source program or the node did not answer
pub(crate) fn widget_asked(program: &str, why: &str) -> String {
    format!("Asking {program}: {why}")
}

// the program's entry or code, which its schema is read from, did not come
pub(crate) fn widget_code_unread(program: &str, why: &str) -> String {
    format!("Reading {program}’s code: {why}")
}

pub(crate) fn widget_no_next(path: &str) -> String {
    format!("there is no `{path}` to read the next page from")
}

pub(crate) fn widget_not_a_list(path: &str) -> String {
    format!("`{path}` is not a list")
}

pub(crate) fn widget_nothing_at(path: &str) -> String {
    format!("there is nothing at `{path}`")
}

pub(crate) fn widget_no_program(program: &str) -> String {
    format!("This network runs no program called {program}.")
}

pub(crate) fn widget_no_code(program: &str) -> String {
    format!("This node doesn’t hold {program}’s code.")
}

pub(crate) fn widget_no_schema(program: &str) -> String {
    format!("{program}’s code carries no schema, so a widget can’t read it.")
}

pub(crate) fn widget_bad_schema(program: &str) -> String {
    format!("{program}’s schema doesn’t read.")
}

pub(crate) fn close_widget(title: &str) -> String {
    format!("Close {title}")
}

// a checklist row as a screen reader says it
pub(crate) fn checked(item: &str, done: bool) -> String {
    match done {
        true => format!("{item}, done"),
        false => format!("{item}, not done"),
    }
}
