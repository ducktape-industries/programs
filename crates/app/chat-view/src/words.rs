//! What chat says, once: every sentence, status word, title and label
//! more than one place shows, in either tree. The model keeps the
//! sentences only it says (`names.rs`, `room.rs`, `actions.rs`, `search.rs`).
use chat::ChannelRow;

#[cfg(not(feature = "notifier"))]
use crate::notifier::Choice;
#[cfg(not(feature = "notifier"))]
use crate::session::Gate;
#[cfg(not(feature = "notifier"))]
use chat::Notices;

pub(crate) const SEND: &str = "Send";
pub(crate) const CANCEL: &str = "Cancel";
pub(crate) const DELETE: &str = "Delete";
pub(crate) const DISMISS: &str = "Dismiss";
pub(crate) const RESTORE: &str = "Restore";
pub(crate) const EDITED: &str = "edited";
pub(crate) const NAME: &str = "Name";
pub(crate) const RENAME: &str = "Rename";
pub(crate) const PEOPLE: &str = "People";
pub(crate) const NEW_CHANNEL: &str = "New channel";
pub(crate) const TODAY: &str = "Today";
pub(crate) const YESTERDAY: &str = "Yesterday";
pub(crate) const NOTICE_MARK: &str = "●";
pub(crate) const NOTICE_FOR: &str = "for";

pub(crate) const CHAT: &str = "Chat";
pub(crate) const CHANNELS: &str = "Channels";
pub(crate) const DIRECT_MESSAGES: &str = "Direct messages";
// what the room lists say while the reader is in no channel and the network holds some
pub(crate) const NONE_HELD: &str =
    "Rooms you open, own or listen to are listed here. Search finds the rest by name.";
// the room lists' last row: the channels the reader is not in, which it unfolds
pub(crate) fn more_channels(count: usize) -> String {
    match count {
        1 => "1 more channel".to_owned(),
        count => format!("{count} more channels"),
    }
}
// search's head over the rooms whose names match
pub(crate) const ROOMS: &str = "Rooms";
pub(crate) const LOADING_ROOMS: &str = "Loading rooms…";
pub(crate) const LOADING_CHANNELS: &str = "Loading channels…";
pub(crate) const SEARCH_MESSAGES: &str = "Search messages";
pub(crate) const MORE_RESULTS: &str = "More results";
pub(crate) const AGENT: &str = "Agent";
pub(crate) const OTHER_ANSWER: &str = "Other…";

pub(crate) fn ask_option(label: &str, default: bool) -> String {
    match default {
        true => format!("{label} (default)"),
        false => label.to_owned(),
    }
}

pub(crate) fn answered(option: Option<&str>) -> String {
    match option {
        Some(label) => format!("Answered: {label}"),
        None => "Answered in the thread".to_owned(),
    }
}

// someone answering in a room before they post: who, and what they say they are doing
pub(crate) const ANSWERING: &str = "is answering…";
pub(crate) const ANSWERING_READ: &str = "who is answering";

pub(crate) fn answering(name: &str) -> String {
    format!("{name} {ANSWERING}")
}

// who is answering under a root, on its reply row in the channel
pub(crate) fn answering_under(names: &[String]) -> Option<String> {
    match names {
        [] => None,
        [name] => Some(answering(name)),
        [names @ .., last] => Some(format!("{} and {last} are answering…", names.join(", "))),
    }
}

pub(crate) fn doing(what: &str) -> String {
    format!("{what}…")
}

pub(crate) fn busy(name: &str, height: u64) -> String {
    format!("{name} is answering here since block {height}")
}

pub(crate) fn since(height: u64) -> String {
    format!("since block {height}")
}

pub(crate) const BREAK: &str = "Break";

pub(crate) const FIND_SOMEONE: &str = "Find a person or agent";
pub(crate) const SEARCH_PEOPLE: &str = "Search by name or number";
pub(crate) const NO_ONE_MATCHES: &str = "Nobody matches that name or number";
pub(crate) const LOADING_PEOPLE: &str = "Loading people…";
pub(crate) const DM_NEEDS_ACCOUNT: &str = "Create an account to message someone";
pub(crate) const CREATE_NEEDS_ACCOUNT: &str = "Create an account to create a channel";

pub(crate) const OPEN: &str = "Open";
pub(crate) const MEMBERS_ONLY: &str = "Members only";
pub(crate) const ARCHIVED: &str = "Archived";
pub(crate) const WHO_MAY_POST: &str = "Who may post";

pub(crate) fn policy(channel: &ChannelRow) -> &'static str {
    match (channel.archived, channel.members_only()) {
        (true, _) => ARCHIVED,
        (false, true) => MEMBERS_ONLY,
        (false, false) => OPEN,
    }
}

pub(crate) fn folder(name: &str) -> String {
    format!("{name} folder")
}

pub(crate) fn unread_rooms(count: usize) -> String {
    match count {
        1 => "1 unread room".to_owned(),
        count => format!("{count} unread rooms"),
    }
}

pub(crate) fn account(number: u64) -> String {
    format!("account {number}")
}

pub(crate) const CHANNEL_DETAILS: &str = "Channel details";
pub(crate) const CONVERSATION_DETAILS: &str = "Conversation details";
pub(crate) const THREAD: &str = "Thread";
pub(crate) const LOADING_MESSAGES: &str = "Loading messages…";
pub(crate) const LOADING_THREAD: &str = "Loading the thread…";
pub(crate) const LOADING_OLDER: &str = "Loading older messages…";
pub(crate) const NO_REPLIES: &str = "No replies yet";

pub(crate) fn unreadable_room(why: &str) -> String {
    format!("Couldn’t read this room: {why}")
}

pub(crate) fn unreadable_names(why: &str) -> String {
    format!("Couldn’t read who’s who, so people show as account numbers: {why}")
}

pub(crate) const MESSAGE_DELETED: &str = "Message deleted";

#[cfg(not(feature = "notifier"))]
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

pub(crate) fn open_in(program: &str) -> String {
    format!("Open in {program}")
}

pub(crate) fn open_thread(replies: &str) -> String {
    format!("Open thread, {replies}")
}

pub(crate) const NEW_MESSAGE: &str = "New message";
pub(crate) const REPLY: &str = "Reply";
pub(crate) const EDITED_MESSAGE: &str = "Edited message";
pub(crate) const UNSENT: &str = "An earlier message wasn’t sent";
pub(crate) const OPENING_DM: &str = "Opening the conversation";

pub(crate) const MESSAGE_ACTIONS: &str = "Message actions";
pub(crate) const WHO_REACTED: &str = "Who reacted";

pub(crate) fn open(name: &str) -> String {
    format!("Open {name}")
}
pub(crate) const ADD_REACTION: &str = "Add reaction";
pub(crate) const REPLY_IN_THREAD: &str = "Reply in thread";
pub(crate) const COPY_LINK: &str = "Copy link";
pub(crate) const MOVE_UP: &str = "Move up";
pub(crate) const MOVE_DOWN: &str = "Move down";
pub(crate) const EDIT_MESSAGE: &str = "Edit message";
pub(crate) const DELETE_MESSAGE: &str = "Delete message";
pub(crate) const DELETE_QUESTION: &str = "Delete this message?";
pub(crate) const MESSAGE_LINK: &str = "message link";
pub(crate) const COPY_TEXT: &str = "Copy text";
pub(crate) const MESSAGE_TEXT: &str = "message text";
pub(crate) const ROOM_ACTIONS: &str = "Room actions";
pub(crate) const ROOM_LINK: &str = "room link";
pub(crate) const EMOJI_CATEGORIES: &str = "Emoji categories";
pub(crate) const FREQUENTLY_USED: &str = "Frequently used";
pub(crate) const NO_EMOJI_MATCH: &str = "No emoji match";
pub(crate) const SEARCH_EMOJI: &str = "Search emoji";

pub(crate) fn react_with(emoji: &str) -> String {
    format!("React with {emoji}")
}

pub(crate) const MEMBERS: &str = "Members";
pub(crate) const NO_MEMBERS: &str = "No members added. An open channel needs none.";
pub(crate) const ARCHIVE_CHANNEL: &str = "Archive channel";
pub(crate) const UNARCHIVE_CHANNEL: &str = "Unarchive channel";
pub(crate) const UNARCHIVE: &str = "Unarchive";
pub(crate) const RENAME_CHANNEL: &str = "Rename channel";
pub(crate) const ACCOUNT_NUMBER: &str = "Account number";
pub(crate) const RULES: &str = "Rules";
pub(crate) const NO_RULES: &str = "No rules";
pub(crate) const SET_RULES: &str = "Set rules";
pub(crate) const CLEAR_RULES: &str = "Clear rules";
pub(crate) const HAS_RULES: &str = "This room has rules";
pub(crate) const RULES_READING: &str = "Reading the rules…";
pub(crate) const RULES_FILES: &str = "files' live heads, for the room's rules";

pub(crate) fn rules_from(path: &str) -> String {
    format!("From {path}")
}
pub(crate) const NOTIFICATIONS: &str = "Notifications";
pub(crate) const AGENTS_NOTICES: &str = "Your agents";
pub(crate) const ALL_AGENTS: &str = "All agents";
pub(crate) const ALL_AGENTS_HEAR: &str = "What your agents hear";
pub(crate) const MIXED: &str = "Mixed";

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

#[cfg(not(feature = "notifier"))]
pub(crate) fn managed_choice(choice: Choice) -> (&'static str, &'static str) {
    match choice {
        Choice::Every => ("All messages", "Every new message, in every thread too"),
        Choice::Default => ("Default", "Mentions, and replies in threads it is in"),
        Choice::Cut(Notices::Mentions) => ("Mentions only", "Only what mentions it"),
        Choice::Cut(Notices::Off) => ("Off", "Nothing from here"),
    }
}

#[cfg(not(feature = "notifier"))]
pub(crate) fn managed_notices(name: &str) -> String {
    format!("What {name} hears")
}

pub(crate) const ATTACH: &str = "Attach files";
pub(crate) const SAVE: &str = "Save";
pub(crate) const OPEN_IN_FILES: &str = "Open in Files";
pub(crate) const UPLOADING: &str = "Uploading…";
pub(crate) const PUT_CLOSED: &str = "The upload stopped before the network held the file";
pub(crate) const NOT_THERE: &str = "The file isn’t there";
pub(crate) const FILE_READING: &str = "Reading…";
pub(crate) const OPENING: &str = "Opening…";
pub(crate) const FILE_FAILED: &str = "Couldn’t read the file";
pub(crate) const FILE_MISSING: &str = "Missing from files";
pub(crate) const NOT_LANDED: &str =
    "No block took the upload in time: remove it and attach it again";
pub(crate) const FILES_MOVED: &str = "files blocks, for the pictures not there yet";

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

pub(crate) fn attached(name: &str) -> String {
    format!("Attach {name} to a chat message")
}

pub(crate) fn unattached(name: &str) -> String {
    format!("Take {name} out of a chat draft")
}

pub(crate) fn not_up(name: &str, why: &str) -> String {
    format!("{name} didn’t upload: {why}")
}

pub(crate) fn not_up_because(why: &str, path: &str) -> String {
    why.replace(path, "this file")
}

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

pub(crate) const WIDGET: &str = "Widget";
pub(crate) const WIDGET_LOADING: &str = "Loading…";
pub(crate) const WIDGET_EMPTY: &str = "Nothing to show.";
pub(crate) const WIDGET_NOT_TEXT: &str =
    "The widget’s file is not text: save the widget file as UTF-8 text";

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

pub(crate) fn widget_asked(program: &str, why: &str) -> String {
    format!("Asking {program}: {why}")
}

pub(crate) fn widget_code_unread(program: &str, why: &str) -> String {
    format!("Reading {program}’s code: {why}")
}

pub(crate) fn widget_no_next(path: &str) -> String {
    format!(
        "there is no `{path}` to read the next page from: point the widget at the reply field containing the next cursor"
    )
}

// the file a widget reads beside its source, not there
pub(crate) fn widget_no_text(path: &str) -> String {
    format!("Nothing at {path} yet.")
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
    format!(
        "{program}’s code carries no schema, so a widget can’t read it: rebuild the program with its schema before using it in a widget"
    )
}

pub(crate) fn widget_bad_schema(program: &str) -> String {
    format!("{program}’s schema doesn’t read.")
}

pub(crate) fn close_widget(title: &str) -> String {
    format!("Close {title}")
}

pub(crate) fn checked(item: &str, done: bool) -> String {
    match done {
        true => format!("{item}, done"),
        false => format!("{item}, not done"),
    }
}

pub(crate) const ALL: &str = "All";
pub(crate) const ROOM_FILTER: &str = "Room filter";

pub(crate) fn no_tagged(tag: &str) -> String {
    format!("No #{tag} posts yet")
}
