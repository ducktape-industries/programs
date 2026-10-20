//! The `chat` program: channels, messages, threads, reactions, and members.
//!
//! A write is an [`Op`], a read a [`Query`] answered by a [`Reply`], all
//! borsh, the same types `chat-view` links. The acting [`Principal`] is the
//! sender the host resolved (`ctx.sender()`): a signed frame is the account its
//! key holds (a key that holds none writes nothing). The layout, in reading order:
//!
//! - `lib.rs` (here): the types on the wire and the rows they carry
//! - `program.rs`: [`Chat`], the program: the signer resolved, then one match
//!   over every op and one over every query
//! - `state.rs`: every table the program keeps, declared once, and how a
//!   message row is stored
//! - `rules.rs`: the checks an op passes before it writes
//! - `ops.rs`: one short function per op
//! - `origin.rs`: the identity role's roster
//! - `queries.rs`: one short function per question
//! - `text.rs`: what search and tags read out of a message, at read time
//! - `mentions.rs`: a typed `@name` made the mention of the one account
//!   that holds the name, as a message is posted or edited
//! - `view.rs`: the reads a view makes of chat, folded into [`view::Names`]
//! - `description.rs`: [`describe()`], an op in a person's words
//!
//! The program runs over `program`'s contexts, so a native test runs it over
//! [`program::MockHost`] exactly as the host does. The `program` feature adds
//! its wasm exports.
mod adopt;
pub mod attributed;
mod description;
mod examples;
mod gate;
mod leases;
mod listen;
mod mentions;
pub mod message;
pub mod notice;
mod ops;
mod order;
mod origin;
mod program;
mod queries;
mod rules;
mod state;
#[cfg(test)]
mod tests;
mod text;
pub mod view;

use std::collections::BTreeMap;

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use serde::{Deserialize, Serialize};

pub use ::program::{AccountNumber, Principal};
pub use abi::hex;
pub use abi::role::identity::{Category, Kind, Profile, Standing};
pub use description::describe;
pub use gate::message_key;
pub use message::{Block, ListMarker, Mark, Notice, Span, list_item, parse_message};
pub use program::Chat;
pub use queries::{replies_after, roots_below};
pub use store::{Cursor, PageRequest, PageResponse};
pub use text::{notice_text, plain_text, tag_ranges, tags};

/// Every table chat keeps, as a block's writes name them: what a follower
/// of `program.changes` plays back in a test to say which a block touched.
pub mod tables {
    pub use crate::state::{
        ANSWERED, CHANNEL_META, CHANNELS, DRAFT_PIECES, DRAFTS, HEADS, LEASES, LISTENERS,
        LISTENING, MEMBERS, MENTIONED, MESSAGE_IDS, MESSAGES, NOTICES, ORDERS, PARTICIPANTS,
        PLACE_WATCHERS, POSTED, POSTED_IN, REACTIONS, REPLIES, ROOTS,
    };
}

/// The name this program runs under.
pub const PROGRAM: &str = "chat";

// The channel meta key a room's rules sit under: the text itself, or a files path whose file
// holds them (README, "Channel meta"). Readers respect them; nothing enforces them.
pub const RULES: &str = "rules";

/// A write. The enum only grows at its end (see `op_variants_only_append`).
#[derive(
    BorshSerialize, BorshDeserialize, BorshSchema, Debug, Clone, PartialEq, Eq, ::program::Described,
)]
#[tool_about(
    "Channels, direct rooms, messages, threads, reactions and who hears each room. A message is markdown blocks; read one back as `text`."
)]
pub enum Op {
    #[tool_op(description = "Open a channel you own. channel_id has no '/', and an id with ':' belongs to the program its prefix names; `dm-` ids open only through CreateDmChannel. Open lets anyone post; MembersOnly lets you and its members.", example = examples::create)]
    CreateChannel {
        channel_id: String,
        name: String,
        post_policy: PostPolicy,
    },
    /// A members-only room between the actor's account and `counterpart`,
    /// id [`dm_channel_id`]; creating it twice is a no-op. The only way a
    /// dm id opens.
    #[tool_op(description = "Open the members-only room between you and counterpart, id `dm-<lower>-<higher>`. Opening it again does nothing.", example = examples::dm)]
    CreateDmChannel {
        counterpart: AccountNumber,
        name: String,
    },
    #[tool_op(description = "Rename a channel. Its owner, or whom the owner lists; never a direct room.", example = examples::rename)]
    RenameChannel { channel_id: String, name: String },
    #[tool_op(description = "Archive a channel (no one posts there) or bring it back. Archiving ends every hold and draft in it. Its owner, or whom the owner lists; never a direct room.", example = examples::archive)]
    SetChannelArchived { channel_id: String, archived: bool },
    /// `thread` names the root this message replies to.
    #[tool_op(description = "Post in a channel, or reply in a thread when thread is its root's seq. message_id is yours to choose, unique across chat, no '/'; a second post with it is refused. blocks are markdown as Paragraph spans with marks (Bold, Italic, Code, Link, Mention), Quote, Code, Divider. Posting needs the room open to you and not archived.", example = examples::post)]
    PostMessage {
        channel_id: String,
        message_id: String,
        blocks: Vec<Block>,
        thread: Option<u64>,
    },
    /// `base_rev` is what the editor started from, recorded, never judged.
    #[tool_op(description = "Replace a message's blocks. Its author, or whom the author lists. base_rev is the rev you edited from, recorded and never judged.", example = examples::edit)]
    EditMessage {
        channel_id: String,
        seq: u64,
        blocks: Vec<Block>,
        base_rev: Option<u32>,
    },
    #[tool_op(description = "Delete a message, leaving a tombstone. Its author, or the channel's owner.", example = examples::delete)]
    DeleteMessage { channel_id: String, seq: u64 },
    #[tool_op(description = "React to a message with an emoji, as yourself.", example = examples::react)]
    AddReaction {
        channel_id: String,
        seq: u64,
        emoji: String,
    },
    #[tool_op(description = "Take back your own reaction.", example = examples::unreact)]
    RemoveReaction {
        channel_id: String,
        seq: u64,
        emoji: String,
    },
    #[tool_op(description = "Seat or unseat a principal in a channel's members. Unseating someone who may then no longer post there ends their holds and draft in it. Its owner, or whom the owner lists; never a direct room.", example = examples::seat)]
    SetMembership {
        channel_id: String,
        principal: Principal,
        member: bool,
    },
    #[tool_op(description = "CreateChannel with its meta set at once: text keys to text values, such as rules.", example = examples::create_with_meta)]
    CreateChannelWithMeta {
        channel_id: String,
        name: String,
        post_policy: PostPolicy,
        meta: BTreeMap<String, String>,
    },
    #[tool_op(description = "Set one meta key of a channel, such as rules: how to talk there, as the text or a files path holding it. Whoever may rename it.", example = examples::set_meta)]
    SetChannelMeta {
        channel_id: String,
        key: String,
        value: String,
    },
    #[tool_op(description = "Remove one meta key. Whoever may rename the channel.", example = examples::remove_meta)]
    RemoveChannelMeta { channel_id: String, key: String },
    // moved `channel_id`'s messages off the layout chat kept before `Stored`; every channel has
    // moved, so it writes nothing now, and refuses only a channel that does not exist
    #[tool_op(description = "Retired: every channel has moved to the current layout, so it writes nothing and refuses only a channel that does not exist.", example = examples::migrate)]
    Migrate { channel_id: String },
    #[tool_op(description = "Hear every new message of a channel (or stop). Listening takes off what SetNotices cut there.", example = examples::listen)]
    Listen { channel_id: String, listening: bool },
    // migrated the first `limit` channels `Query::Unmigrated` listed; none is left, so it writes
    // nothing
    #[tool_op(description = "Retired with Migrate: none is left to migrate, so it writes nothing.", example = examples::migrate_some)]
    MigrateSome { limit: u64 },
    /// What of `channel_id` still tells the sender (`None`: what chat tells
    /// by default). It stops their listening there, as `Listen` takes this
    /// off: the two are one choice.
    #[tool_op(description = "Cut what a channel tells you to Mentions (mentions, and direct rooms) or Off; null goes back to chat's default. It stops your listening there.", example = examples::notices)]
    SetNotices {
        channel_id: String,
        notices: Option<Notices>,
    },
    // one-time (J-011 ⑥): records owners for chat's owned rows from before the guard
    #[tool_op(description = "One-time backfill of owners for chat's rows written before ownership was recorded, a page at a time; returns the key to resume after, or null when done.", returns = Option<Vec<u8>>, example = examples::adopt)]
    AdoptOwners {
        kind: String,
        after: Option<Vec<u8>>,
        limit: u64,
    },
    // as `Listen`, for `account`: the account itself or its manager in identity
    #[tool_op(description = "Listen, for another account: the account itself or its manager in identity.", example = examples::listen_for)]
    ListenFor {
        account: AccountNumber,
        channel_id: String,
        listening: bool,
    },
    // as `SetNotices`, for `account`: the account itself or its manager in identity
    #[tool_op(description = "SetNotices, for another account: the account itself or its manager in identity.", example = examples::notices_for)]
    SetNoticesFor {
        account: AccountNumber,
        channel_id: String,
        notices: Option<Notices>,
    },
    // hears every new message of `channel_id` for the sender's wait (a `watch` relation), leaving
    // what it chose for the room as it is
    #[tool_op(description = "Hear every new message of a channel for a wait, leaving what you chose for the room as it is.", example = examples::watch)]
    Watch { channel_id: String, watching: bool },
    // the sender answering in a room before it posts: `text` joins its draft there (`anew`: in
    // place of the text so far), `doing` (when given) replaces what it says it is doing, and
    // `thread` is the root it answers under
    #[tool_op(description = "Show that you are answering in a channel before you post: text joins your draft there (anew: in place of the text so far), doing (when given) replaces the line on what you are doing, and thread is the root you answer under. Your post in the channel takes the draft's place. Allowed where you may post; nobody is told.", example = examples::drafting)]
    Drafting {
        channel_id: String,
        thread: Option<u64>,
        text: String,
        anew: bool,
        doing: Option<String>,
    },
    #[tool_op(description = "Take down your draft in a channel.", example = examples::undraft)]
    Undraft { channel_id: String },
    // the sender's own order of its rooms and folders in the lists, written whole: an empty order
    // goes back to chat's
    #[tool_op(description = "Put your rooms and folders in your own order in the lists: a room by its channel id, a folder by its name (the text before a channel name's first '/'). Writes your whole order; an empty list goes back to chat's order.", example = examples::room_order)]
    SetRoomOrder { order: Vec<Entry> },
    // `PostMessage` with its blocks parsed here from `text` (`parse_message`), for a writer that
    // has text in hand
    #[tool_op(description = "Post text in a channel, or reply in a thread when thread is its root's seq. text is markdown as chat's composer writes it: **bold**, *italic*, `code`, fenced code, > quotes, --- dividers, links, and <@N> to mention account N; each line is its own paragraph. message_id is yours to choose, unique across chat, no '/'; a second post with it is refused. Posting needs the room open to you and not archived.", example = examples::post_text)]
    Post {
        channel_id: String,
        message_id: String,
        text: String,
        thread: Option<u64>,
    },
    #[tool_op(description = "Choose for several accounts at once what a channel tells them: listening true hears every message, else notices cuts it as SetNotices does (null: chat's default). Each account is you or one you manage in identity; all are written or none, and listening with notices set is refused.", example = examples::choose_for_many)]
    ChooseForMany {
        accounts: Vec<AccountNumber>,
        channel_id: String,
        listening: bool,
        notices: Option<Notices>,
    },
    #[tool_op(description = "Post a notice: a module's status line (a pod ended, a check failed, a change merged) told only to the accounts it concerns, and to a wait watching the room; everyone else sees it in the room and is not told. notice is kind (the module's word for what it reports, such as pod), group (notices of one author in one room under one group are one thing's states, such as a run's start and end), subject (what it is about, in words), link (a duck:// link to it, or null), status (Note, Running, Passed or Failed), detail (one line) and concerns (account numbers). A notice is never edited: post its next state as a new notice under the same group. message_id and thread are as PostMessage's, and it needs the room open to you and not archived.", example = examples::notify)]
    Notify {
        channel_id: String,
        message_id: String,
        thread: Option<u64>,
        notice: Notice,
    },
    #[tool_op(description = "Hold a place: a channel's foot, or a thread when thread is its root's seq. Granted while no one else holds that exact place or has a draft open there; taking one you already hold is ok, and one anyone else holds is refused busy, naming who. Allowed where you may post. Your post there, Release, Break, an archive of the room or an unseat that takes your posting there ends it, and it never stops anyone posting.", example = examples::take)]
    Take {
        channel_id: String,
        thread: Option<u64>,
    },
    #[tool_op(description = "Let go of a place you hold: a channel's foot, or a thread when thread is its root's seq. Letting go of one you do not hold does nothing.", example = examples::release)]
    Release {
        channel_id: String,
        thread: Option<u64>,
    },
    #[tool_op(description = "From a program only: be told each time a hold or a draft ends in any place (a Release, a Break, an archive or an unseat that ends them, an Undraft, a post that ends its author's hold or draft there, a draft moving to another thread), each as a message of deliver followed by PlaceEvent::Cleared { channel_id, thread } in borsh. An empty deliver stops it.", example = examples::watch_places)]
    WatchPlaces { deliver: Vec<u8> },
    #[tool_op(description = "End whatever holds a place, a channel's foot or a thread when thread is its root's seq: its hold, whoever took it, and every draft open there. Whoever may rename the channel.", example = examples::break_place)]
    Break {
        channel_id: String,
        thread: Option<u64>,
    },
}

/// A read. `viewer` is the reader's principals: they decide
/// [`Reaction::reacted_by_me`]. Every list takes a [`PageRequest`] and answers a
/// [`PageResponse`] whose `next` resumes it. Each variant names the tables
/// it is answered from (`#[reads]`, [`program::Reads`]), so a view following
/// `program.changes` re-reads it only for a block that wrote to one of them;
/// a read of message rows follows `REACTIONS` too, since the viewer's own
/// reactions are read onto the rows.
#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Debug,
    Clone,
    PartialEq,
    Eq,
    ::program::Ask,
    ::program::Described,
)]
#[ask(Chat)]
pub enum Query {
    #[ask(Reply::Channels(PageResponse<ChannelInfo>))]
    #[reads(state::CHANNELS, state::HEADS, state::CHANNEL_META)]
    #[tool_query(description = "Every channel a page at a time: id, name, post policy, owner, archived, meta and head_seq (its newest message's seq). A page with limit null is every row.", example = examples::channels)]
    Channels { page: PageRequest },
    #[ask(Reply::Channel(Option<ChannelInfo>))]
    #[reads(state::CHANNELS, state::HEADS, state::CHANNEL_META)]
    #[tool_query(description = "One channel by id, with its meta (rules among them), or null.", example = examples::channel)]
    Channel { channel_id: String },
    /// The message an emitted id names (forge finds its own posts so).
    #[ask(Reply::Message(Option<MsgRow>))]
    #[reads(state::MESSAGE_IDS, state::MESSAGES, state::REACTIONS)]
    #[tool_query(description = "The message a message_id names, or null.", example = examples::by_id)]
    MessageById { message_id: String },
    /// The author's most recently answered thread in this channel, if any.
    #[ask(Reply::Attention(Option<MsgRow>))]
    #[reads(state::ANSWERED, state::MESSAGES, state::REACTIONS)]
    #[tool_query(description = "The author's most recently answered thread in a channel, or null.", example = examples::attention)]
    ThreadAttention {
        channel_id: String,
        author: Principal,
    },
    /// One page of timeline roots, newest first.
    #[ask(Reply::Roots(PageResponse<MsgRow>))]
    #[reads(state::ROOTS, state::MESSAGES, state::REACTIONS)]
    #[tool_query(description = "A channel's timeline, newest first, without thread replies. viewer is the principals reading, which decide reacted_by_me on each reaction.", example = examples::roots)]
    Roots {
        channel_id: String,
        viewer: Vec<Principal>,
        page: PageRequest,
    },
    // the seqs from seq - page.limit / 2 through seq + page.limit / 2: a window symmetric about
    // seq, so an even limit holds limit + 1 seqs; page.after is not read
    #[ask(Reply::Messages(Vec<MsgRow>))]
    #[reads(state::MESSAGES, state::REACTIONS)]
    #[tool_query(description = "The messages from seq - page.limit/2 through seq + page.limit/2 (integer halves): a window symmetric about seq, so an even limit gives up to limit + 1 messages. page.after is not read.", example = examples::around)]
    MessagesAround {
        channel_id: String,
        seq: u64,
        viewer: Vec<Principal>,
        page: PageRequest,
    },
    /// The root plus one page of replies, in post order.
    #[ask(Reply::Thread { root: Option<MsgRow>, replies: PageResponse<MsgRow> })]
    #[reads(state::REPLIES, state::MESSAGES, state::REACTIONS)]
    #[tool_query(description = "A thread's root and a page of its replies, in post order.", example = examples::thread)]
    Thread {
        channel_id: String,
        root_seq: u64,
        viewer: Vec<Principal>,
        page: PageRequest,
    },
    #[ask(Reply::Members(PageResponse<MemberRow>))]
    #[reads(state::MEMBERS)]
    #[tool_query(description = "A channel's seated members, a page at a time.", example = examples::members)]
    Members {
        channel_id: String,
        page: PageRequest,
    },
    // The messages holding every whitespace-separated term of `text` as
    // a substring (NFC, case folded), newest first, at most `page.limit`
    // hits. Answered by walking the messages: chat keeps no search index.
    #[ask(Reply::Hits(MessageHits))]
    #[reads(state::HEADS, state::MESSAGES, state::REACTIONS)]
    #[tool_query(description = "Messages holding every word of text (case folded), newest first, in one channel or all; capped says the walk stopped at page.limit hits.", example = examples::search)]
    Search {
        text: String,
        viewer: Vec<Principal>,
        channel_id: Option<String>,
        page: PageRequest,
    },
    // One page of the messages whose `#tags` hold `tag`, newest first.
    // Answered by walking the messages: chat keeps no tag index.
    #[ask(Reply::TagHits(PageResponse<MsgRow>))]
    #[reads(state::HEADS, state::MESSAGES, state::REACTIONS)]
    #[tool_query(description = "Messages whose #tags hold tag, newest first.", example = examples::tagged)]
    TagSearch {
        tag: String,
        viewer: Vec<Principal>,
        channel_id: Option<String>,
        page: PageRequest,
    },
    /// Every account's profile, ascending by number, a page at a time:
    /// the program asks the identity role, so a view links one program. It
    /// reads nothing of chat's: it moves with identity's blocks, which a
    /// view follows apart.
    #[ask(Reply::Accounts(PageResponse<Profile>))]
    #[tool_query(description = "Every account's profile by number, as identity keeps them, a page at a time.", example = examples::accounts)]
    Accounts { page: PageRequest },
    // Who chose each reaction on the messages at `seqs`: one entry per
    // message and emoji, each in key order.
    #[ask(Reply::Reactors(Vec<ReactedBy>))]
    #[reads(state::REACTIONS)]
    #[tool_query(description = "Who chose each reaction on the messages at seqs: one entry per message and emoji.", example = examples::reactors)]
    Reactors { channel_id: String, seqs: Vec<u64> },
    // the channels whose messages were not all migrated yet (`Op::Migrate`): every channel has
    // moved, so it answers none
    #[ask(Reply::ChannelIds(PageResponse<String>))]
    #[tool_query(description = "Retired with Migrate: answers no channel.", example = examples::unmigrated)]
    Unmigrated { page: PageRequest },
    #[ask(Reply::Listening(PageResponse<String>))]
    #[reads(state::LISTENING)]
    #[tool_query(description = "The channels an account listens to.", example = examples::listening)]
    Listening {
        account: AccountNumber,
        page: PageRequest,
    },
    /// Where `account` is in `channel_id`, for its notices: the seq it came
    /// into the room at, and into each thread of `roots` it is in.
    #[ask(Reply::Followed(Followed))]
    #[reads(
        state::CHANNELS,
        state::MEMBERS,
        state::MESSAGES,
        state::POSTED,
        state::PARTICIPANTS,
        state::MENTIONED
    )]
    #[tool_query(description = "Where an account is in a channel, for its notices: the seq it came into the room at, and into each thread of roots it is in.", example = examples::followed)]
    Followed {
        channel_id: String,
        account: AccountNumber,
        roots: Vec<u64>,
    },
    /// The rooms `account` cut the notices of (`Op::SetNotices`), each with
    /// what still tells it, a page at a time.
    #[ask(Reply::Notices(PageResponse<(String, Notices)>))]
    #[reads(state::NOTICES)]
    #[tool_query(description = "The channels an account cut the notices of, each with what still tells it.", example = examples::cut)]
    Notices {
        account: AccountNumber,
        page: PageRequest,
    },
    // the rooms `account` watches for a wait (`Op::Watch`)
    #[ask(Reply::Watching(PageResponse<String>))]
    #[reads(state::WATCHING)]
    #[tool_query(description = "The channels an account watches for a wait.", example = examples::watching)]
    Watching {
        account: AccountNumber,
        page: PageRequest,
    },
    // what `account` chose for `channel_id`: whether it listens there, and what of it still tells
    // it when cut
    #[ask(Reply::Chosen { listening: bool, notices: Option<Notices> })]
    #[reads(state::LISTENING, state::NOTICES)]
    #[tool_query(description = "What an account chose for a channel: whether it listens, and what still tells it when cut.", example = examples::chosen)]
    Chosen {
        account: AccountNumber,
        channel_id: String,
    },
    // who is answering in a room before they post (`Op::Drafting`), each with its text so far
    #[ask(Reply::Drafts(Vec<Draft>))]
    #[reads(state::DRAFTS, state::DRAFT_PIECES)]
    #[tool_query(description = "Who is answering in a channel right now, before they post: each draft's author, the thread root it answers under, the block and time it opened, what it says it is doing and its text so far.", example = examples::drafts)]
    Drafts { channel_id: String },
    // the names of the rooms `ids` names, each as (id, name), with nothing else of a room's: a
    // reader names the rooms it holds ids of in one read; an id no room has is left out
    #[ask(Reply::Names(Vec<(String, String)>))]
    #[reads(state::CHANNELS)]
    #[tool_query(description = "The names of the channels ids names, each as (id, name) and nothing else of the channel; an id no channel has is left out.", example = examples::names)]
    Names { ids: Vec<String> },
    // the order `account` put its rooms and folders in (`Op::SetRoomOrder`): empty for none
    #[ask(Reply::RoomOrder(Vec<Entry>))]
    #[reads(state::ORDERS)]
    #[tool_query(description = "The order an account put its rooms and folders in, or an empty list.", example = examples::room_order_of)]
    RoomOrder { account: AccountNumber },
    // the channels `account` wrote in the timeline of
    #[ask(Reply::PostedIn(PageResponse<String>))]
    #[reads(state::POSTED_IN)]
    #[tool_query(description = "The channels an account has written in.", example = examples::posted_in)]
    PostedIn {
        account: AccountNumber,
        page: PageRequest,
    },
    #[ask(Reply::ChosenBy(Vec<AccountChoice>))]
    #[reads(state::LISTENING, state::NOTICES)]
    #[tool_query(description = "What each of several accounts chose for a channel, in the order asked: whether it listens, and what still tells it when cut.", example = examples::chosen_by)]
    ChosenBy {
        channel_id: String,
        accounts: Vec<AccountNumber>,
    },
    // for each `(channel_id, since)` of `rooms`, what came after `since` that is meant for
    // `account` (`notice::why`), less what its choice for the room leaves out: the head counted to
    // and how many, in the order asked; a room no channel has counts nothing
    #[ask(Reply::Unread(Vec<UnreadCount>))]
    #[reads(
        state::HEADS,
        state::MESSAGES,
        state::CHANNELS,
        state::MEMBERS,
        state::POSTED,
        state::PARTICIPANTS,
        state::MENTIONED,
        state::LISTENING,
        state::NOTICES
    )]
    #[tool_query(description = "For each (channel_id, since) of rooms, how many messages after since are meant for account: in its direct rooms, mentioning it, replies in threads it is in, or in the timeline of rooms it is in, as its choice for each room allows. Answers each room's head counted to and the count, in the order asked.", example = examples::unread)]
    Unread {
        account: AccountNumber,
        rooms: Vec<(String, u64)>,
    },
    // One page of the messages holding every whitespace-separated term of `text` as a substring
    // (NFC, case folded), newest first: `Search` with a cursor. Answered by walking the messages.
    #[ask(Reply::TextHits(PageResponse<MsgRow>))]
    #[reads(state::HEADS, state::MESSAGES, state::REACTIONS)]
    #[tool_query(description = "Messages holding every word of text (case folded), newest first, in one channel or all, a page at a time.", example = examples::text_search)]
    TextSearch {
        text: String,
        viewer: Vec<Principal>,
        channel_id: Option<String>,
        page: PageRequest,
    },
    #[ask(Reply::Busy(Option<Holder>))]
    #[reads(state::LEASES, state::DRAFTS)]
    #[tool_query(description = "Who else is in a place, a channel's foot or a thread when thread is its root's seq: its holder (Lease, from Take), else an open draft there (Draft), by anyone but for_account, with the block each began; null when the place is free for for_account.", example = examples::busy)]
    Busy {
        channel_id: String,
        thread: Option<u64>,
        for_account: AccountNumber,
    },
}

#[derive(BorshSerialize, BorshDeserialize, BorshSchema, Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Channels(PageResponse<ChannelInfo>),
    Channel(Option<ChannelInfo>),
    Message(Option<MsgRow>),
    Attention(Option<MsgRow>),
    Roots(PageResponse<MsgRow>),
    Messages(Vec<MsgRow>),
    Thread {
        root: Option<MsgRow>,
        replies: PageResponse<MsgRow>,
    },
    Members(PageResponse<MemberRow>),
    Hits(MessageHits),
    TagHits(PageResponse<MsgRow>),
    Accounts(PageResponse<Profile>),
    Reactors(Vec<ReactedBy>),
    ChannelIds(PageResponse<String>),
    Listening(PageResponse<String>),
    Notices(PageResponse<(String, Notices)>),
    Followed(Followed),
    Watching(PageResponse<String>),
    Chosen {
        listening: bool,
        notices: Option<Notices>,
    },
    Drafts(Vec<Draft>),
    Names(Vec<(String, String)>),
    RoomOrder(Vec<Entry>),
    PostedIn(PageResponse<String>),
    ChosenBy(Vec<AccountChoice>),
    Unread(Vec<UnreadCount>),
    TextHits(PageResponse<MsgRow>),
    Busy(Option<Holder>),
}

// One room's count ([`Query::Unread`]): what came after the cursor asked, up to `to`, meant for
// the reader.
#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
    Debug,
    Clone,
    PartialEq,
    Eq,
)]
pub struct UnreadCount {
    pub channel_id: String,
    pub to: u64,
    pub meant: u64,
}

// One place in an account's own order of its lists (`Op::SetRoomOrder`): a room by its channel id,
// or a folder by its name, the text before a channel name's first `/`.
#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
)]
pub enum Entry {
    Room(String),
    Folder(String),
}

#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
    Debug,
    Clone,
    PartialEq,
    Eq,
)]
pub struct AccountChoice {
    pub account: AccountNumber,
    pub listening: bool,
    pub notices: Option<Notices>,
}

/// Where an account is in a room ([`Query::Followed`]): what comes after the
/// seq it came in at is meant for it.
#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
    Debug,
    Clone,
    Default,
    PartialEq,
    Eq,
)]
pub struct Followed {
    /// In the room: its first message in the timeline, or 0 for a seat in a
    /// members-only room or the room's opening.
    pub room: Option<u64>,
    /// Each thread asked about that it is in, by root: the first message it
    /// wrote or was mentioned in there.
    pub threads: Vec<(u64, u64)>,
}

#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
)]
pub enum PostPolicy {
    Open,
    MembersOnly,
}

/// What of a room still tells an account that cut its notices
/// (`Op::SetNotices`); listening (`Op::Listen`) is the other way, every
/// message.
#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
)]
pub enum Notices {
    /// What mentions it, and what is written to it in a direct room.
    Mentions,
    /// Nothing.
    Off,
}

#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
    Debug,
    Clone,
    PartialEq,
    Eq,
)]
pub struct ChannelRow {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    pub post_policy: PostPolicy,
    pub owner: Principal,
    pub archived: bool,
}

impl ChannelRow {
    pub fn members_only(&self) -> bool {
        self.post_policy == PostPolicy::MembersOnly
    }

    /// Whether the room lets `principal` post, `seated` saying whether it holds
    /// a member seat: posting is open, or it owns the room or sits in it.
    /// Archiving aside; the program and the view ask this one rule.
    pub fn admits(&self, principal: &Principal, seated: bool) -> bool {
        !self.members_only() || self.owner == *principal || seated
    }
}

/// A channel and the seq of its newest message.
#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
    Debug,
    Clone,
    PartialEq,
    Eq,
)]
pub struct ChannelInfo {
    pub channel: ChannelRow,
    pub head_seq: u64,
    #[serde(default)]
    pub meta: BTreeMap<String, String>,
}

#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
    Debug,
    Clone,
    PartialEq,
    Eq,
)]
pub struct MemberRow {
    pub principal: Principal,
    pub height: u64,
    pub time: u64,
}

#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
    Debug,
    Clone,
    PartialEq,
    Eq,
)]
pub struct MsgRow {
    pub channel_id: String,
    pub seq: u64,
    pub message_id: String,
    pub author: Principal,
    pub height: u64,
    pub time: u64,
    pub blocks: Vec<Block>,
    // the blocks as one line of text ([`plain_text`]), derived at read;
    // a tombstone's is empty
    pub text: String,
    pub deleted: bool,
    pub edited: bool,
    pub rev: u32,
    pub edited_at: Option<u64>,
    /// what the last edit claimed to be based on, recorded, never judged
    pub base_rev: Option<u32>,
    /// `Some(root_seq)` marks a thread reply
    pub thread: Option<u64>,
    pub reply_count: u64,
    pub last_reply_seq: Option<u64>,
    pub reactions: Vec<Reaction>,
    // the `#tags` in the blocks ([`tags`]), derived at read
    pub tags: Vec<String>,
}

impl MsgRow {
    /// A row with nothing yet but its author: a view's pending post before
    /// the program serves it, a test's base row.
    pub fn by(author: Principal) -> MsgRow {
        MsgRow {
            channel_id: String::new(),
            seq: 0,
            message_id: String::new(),
            author,
            height: 0,
            time: 0,
            blocks: Vec::new(),
            text: String::new(),
            deleted: false,
            edited: false,
            rev: 0,
            edited_at: None,
            base_rev: None,
            thread: None,
            reply_count: 0,
            last_reply_seq: None,
            reactions: Vec::new(),
            tags: Vec::new(),
        }
    }

    pub fn notice(&self) -> Option<&Notice> {
        match self.blocks.as_slice() {
            [Block::Notice(notice)] => Some(notice),
            _ => None,
        }
    }
}

/// One emoji on a message: how many principals chose it.
#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
    Debug,
    Clone,
    PartialEq,
    Eq,
)]
pub struct Reaction {
    pub emoji: String,
    pub count: u64,
    /// filled per reader from the query's `viewer`
    pub reacted_by_me: bool,
}

// Who chose one emoji on one message.
#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
    Debug,
    Clone,
    PartialEq,
    Eq,
)]
pub struct ReactedBy {
    pub seq: u64,
    pub emoji: String,
    pub by: Vec<Principal>,
}

#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
    Debug,
    Clone,
    PartialEq,
    Eq,
)]
pub struct MessageHits {
    pub hits: Vec<MsgRow>,
    pub capped: bool,
}

// Someone answering in a room before they post (`Op::Drafting`): the thread root it answers
// under, the block and time it opened, what it says it is doing and its text so far.
#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
    Debug,
    Clone,
    PartialEq,
    Eq,
)]
pub struct Draft {
    pub author: Principal,
    pub thread: Option<u64>,
    pub height: u64,
    pub time: u64,
    pub doing: String,
    pub text: String,
}

#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
    Debug,
    Clone,
    PartialEq,
    Eq,
)]
pub struct Holder {
    pub principal: Principal,
    pub kind: Holding,
    pub height: u64,
}

#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
)]
pub enum Holding {
    Lease,
    Draft,
}

#[derive(BorshSerialize, BorshDeserialize, BorshSchema, Debug, Clone, PartialEq, Eq)]
pub enum PlaceEvent {
    Cleared {
        channel_id: String,
        thread: Option<u64>,
    },
}

/// The room two accounts share: `dm-<lower>-<higher>`.
pub fn dm_channel_id(a: AccountNumber, b: AccountNumber) -> String {
    format!("dm-{}-{}", a.min(b), a.max(b))
}

/// The two accounts of a dm room id, or `None` for any other channel.
pub fn dm_peers(channel_id: &str) -> Option<(AccountNumber, AccountNumber)> {
    let (a, b) = channel_id.strip_prefix("dm-")?.split_once('-')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

pub mod namespace {
    /// `program`'s id `name`: `forge:web:3`.
    pub fn id(program: &str, name: &str) -> String {
        format!("{program}:{name}")
    }

    /// The program an id belongs to, or `None` for one people made.
    pub fn program(id: &str) -> Option<&str> {
        id.split_once(':').map(|(program, _)| program)
    }
}

// A program's own post, in any room: its own account's message of one code block in that
// program's language (`author_program`: the program the author's account is, from identity's
// profiles), such as forge's `opened` and `review 7` in a change's room, or `opened programs
// 213` in a job's room. The code is the program's to word; a reader shows it as that program's
// event and points to where the program itself shows it. `(program, code)`.
pub fn program_post<'a>(
    row: &'a MsgRow,
    author_program: Option<&str>,
) -> Option<(&'a str, &'a str)> {
    match row.blocks.as_slice() {
        [
            Block::Code {
                lang: Some(lang),
                text,
            },
        ] if author_program == Some(lang.as_str()) => Some((lang, text)),
        _ => None,
    }
}

#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        [
            "CreateChannel",
            "CreateDmChannel",
            "RenameChannel",
            "SetChannelArchived",
            "PostMessage",
            "EditMessage",
            "DeleteMessage",
            "AddReaction",
            "RemoveReaction",
            "SetMembership",
            "CreateChannelWithMeta",
            "SetChannelMeta",
            "RemoveChannelMeta",
            "Migrate",
            "Listen",
            "MigrateSome",
            "SetNotices",
            "AdoptOwners",
            "ListenFor",
            "SetNoticesFor",
            "Watch",
            "Drafting",
            "Undraft",
            "SetRoomOrder",
            "Post",
            "ChooseForMany",
            "Notify",
            "Take",
            "Release",
            "WatchPlaces",
            "Break",
        ]
    );
}

#[test]
fn a_programs_own_code_block_is_its_post_in_any_room() {
    let code = |lang: &str| Block::Code {
        lang: Some(lang.into()),
        text: "review 7".into(),
    };
    let row = |channel: &str, author: Principal, blocks| MsgRow {
        channel_id: channel.into(),
        blocks,
        ..MsgRow::by(author)
    };
    let forge = || Principal::Account(9);
    let program_of = |row: &MsgRow| (row.author == forge()).then_some("forge");
    let post = row("forge:web:3", forge(), vec![code("forge")]);
    assert_eq!(
        program_post(&post, program_of(&post)),
        Some(("forge", "review 7"))
    );
    // a job's room hears of a change there
    for elsewhere in [
        row("j-101", forge(), vec![code("forge")]),
        row("chess:1", forge(), vec![code("forge")]),
    ] {
        assert_eq!(
            program_post(&elsewhere, program_of(&elsewhere)),
            Some(("forge", "review 7"))
        );
    }
    for other in [
        row("forge:web:3", Principal::Account(1), vec![code("forge")]),
        row("forge:web:3", forge(), vec![code("rust")]),
        row("forge:web:3", forge(), vec![code("forge"), Block::Divider]),
    ] {
        assert_eq!(program_post(&other, program_of(&other)), None, "{other:?}");
    }
}
