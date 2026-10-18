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
//! - `state.rs`: every table and index the program keeps, declared once
//! - `rules.rs`: the checks an op passes before it writes
//! - `ops.rs`: one short function per op
//! - `origin.rs`: the identity role's roster
//! - `queries.rs`: one short function per question
//! - `text.rs`: what search and tags read out of a message
//! - `mentions.rs`: a typed `@name` made the mention of the one account
//!   that holds the name, as a message is posted or edited
//! - `view.rs`: the reads a view makes of chat, folded into [`view::Names`]
//! - `description.rs`: [`describe()`], an op in a person's words
//!
//! The program runs over `program`'s contexts, so a native test runs it over
//! [`program::MockHost`] exactly as the host does. The `program` feature adds
//! its wasm exports.
pub mod attributed;
mod description;
mod gate;
mod mentions;
pub mod message;
mod ops;
mod origin;
mod program;
mod queries;
mod rules;
mod state;
#[cfg(test)]
mod tests;
mod text;
pub mod view;

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use serde::{Deserialize, Serialize};

pub use ::program::{AccountNumber, Principal};
pub use abi::hex;
pub use abi::role::identity::{Category, Kind, Profile, Standing};
pub use description::describe;
pub use gate::message_key;
pub use message::{Block, ListMarker, Mark, Span, list_item, parse_message};
pub use program::Chat;
pub use queries::roots_below;
pub use store::{Cursor, PageRequest, PageResponse};
pub use text::{plain_text, tags, tokens};

/// Every table chat keeps, as a block's writes name them: what a follower
/// of `program.changes` plays back in a test to say which a block touched.
pub mod tables {
    pub use crate::state::{
        ANSWERED, CHANNEL_TAGS, CHANNELS, HEADS, MEMBERS, MESSAGE_IDS, MESSAGES, PARTICIPANTS,
        REACTIONS, REPLIES, ROOTS, TAGS, WORDS,
    };
}

/// The name this program runs under.
pub const PROGRAM: &str = "chat";

/// A write. The enum only grows at its end (see `op_variants_only_append`).
#[derive(BorshSerialize, BorshDeserialize, BorshSchema, Debug, Clone, PartialEq, Eq)]
pub enum Op {
    CreateChannel {
        channel_id: String,
        name: String,
        post_policy: PostPolicy,
    },
    /// A members-only room between the actor's account and `counterpart`,
    /// id [`dm_channel_id`]; creating it twice is a no-op. The only way a
    /// dm id opens.
    CreateDmChannel {
        counterpart: AccountNumber,
        name: String,
    },
    RenameChannel {
        channel_id: String,
        name: String,
    },
    SetChannelArchived {
        channel_id: String,
        archived: bool,
    },
    /// `thread` names the root this message replies to.
    PostMessage {
        channel_id: String,
        message_id: String,
        blocks: Vec<Block>,
        thread: Option<u64>,
    },
    /// `base_rev` is what the editor started from, recorded, never judged.
    EditMessage {
        channel_id: String,
        seq: u64,
        blocks: Vec<Block>,
        base_rev: Option<u32>,
    },
    DeleteMessage {
        channel_id: String,
        seq: u64,
    },
    AddReaction {
        channel_id: String,
        seq: u64,
        emoji: String,
    },
    RemoveReaction {
        channel_id: String,
        seq: u64,
        emoji: String,
    },
    SetMembership {
        channel_id: String,
        principal: Principal,
        member: bool,
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
    BorshSerialize, BorshDeserialize, BorshSchema, Debug, Clone, PartialEq, Eq, ::program::Ask,
)]
#[ask(Chat)]
pub enum Query {
    #[ask(Reply::Channels(PageResponse<ChannelInfo>))]
    #[reads(state::CHANNELS, state::HEADS)]
    Channels { page: PageRequest },
    #[ask(Reply::Channel(Option<ChannelInfo>))]
    #[reads(state::CHANNELS, state::HEADS)]
    Channel { channel_id: String },
    /// The message an emitted id names (forge finds its own posts so).
    #[ask(Reply::Message(Option<MsgRow>))]
    #[reads(state::MESSAGE_IDS, state::MESSAGES, state::REACTIONS)]
    MessageById { message_id: String },
    /// The author's most recently answered thread in this channel, if any.
    #[ask(Reply::Attention(Option<MsgRow>))]
    #[reads(state::ANSWERED, state::MESSAGES, state::REACTIONS)]
    ThreadAttention {
        channel_id: String,
        author: Principal,
    },
    /// One page of timeline roots, newest first.
    #[ask(Reply::Roots(PageResponse<MsgRow>))]
    #[reads(state::ROOTS, state::MESSAGES, state::REACTIONS)]
    Roots {
        channel_id: String,
        viewer: Vec<Principal>,
        page: PageRequest,
    },
    /// `page.limit` messages centred on `seq`.
    #[ask(Reply::Messages(Vec<MsgRow>))]
    #[reads(state::MESSAGES, state::REACTIONS)]
    MessagesAround {
        channel_id: String,
        seq: u64,
        viewer: Vec<Principal>,
        page: PageRequest,
    },
    /// The root plus one page of replies, in post order.
    #[ask(Reply::Thread { root: Option<MsgRow>, replies: PageResponse<MsgRow> })]
    #[reads(state::REPLIES, state::MESSAGES, state::REACTIONS)]
    Thread {
        channel_id: String,
        root_seq: u64,
        viewer: Vec<Principal>,
        page: PageRequest,
    },
    #[ask(Reply::Members(PageResponse<MemberRow>))]
    #[reads(state::MEMBERS)]
    Members {
        channel_id: String,
        page: PageRequest,
    },
    /// Every token of `text`, newest first, at most `page.limit` hits.
    #[ask(Reply::Hits(MessageHits))]
    #[reads(state::WORDS, state::MESSAGES, state::REACTIONS)]
    Search {
        text: String,
        viewer: Vec<Principal>,
        channel_id: Option<String>,
        page: PageRequest,
    },
    #[ask(Reply::TagHits(PageResponse<MsgRow>))]
    #[reads(state::TAGS, state::CHANNEL_TAGS, state::MESSAGES, state::REACTIONS)]
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
    Accounts { page: PageRequest },
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
    /// the flattened text search indexes; a tombstone's is empty
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

/// The program whose own account wrote `row` in that program's own
/// `<program>:<name>` room (`author_program`: the program the author's
/// account is, from identity's profiles).
pub fn program_author<'a>(row: &'a MsgRow, author_program: Option<&str>) -> Option<&'a str> {
    namespace::program(&row.channel_id).filter(|program| author_program == Some(*program))
}

/// A program's own post in its own room ([`program_author`]), as one code
/// block in that program's language (forge's `opened`, `review 7`). The code
/// is the program's to word; a reader shows it as that program's event and
/// points to where the program itself shows the room. `(program, code)`.
pub fn program_post<'a>(
    row: &'a MsgRow,
    author_program: Option<&str>,
) -> Option<(&'a str, &'a str)> {
    let program = program_author(row, author_program)?;
    match row.blocks.as_slice() {
        [
            Block::Code {
                lang: Some(lang),
                text,
            },
        ] if lang == program => Some((program, text)),
        _ => None,
    }
}

/// Old op bytes are described with the current code (`describe`): the op
/// enum only grows at its end. Append a new variant here; never reorder.
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
        ]
    );
}

#[test]
fn a_programs_own_code_block_in_its_room_is_its_post() {
    let code = |lang: &str| Block::Code {
        lang: Some(lang.into()),
        text: "review 7".into(),
    };
    let row = |channel: &str, author: Principal, blocks| MsgRow {
        channel_id: channel.into(),
        blocks,
        ..MsgRow::by(author)
    };
    // forge's account is 9; account 1 is a user's
    let forge = || Principal::Account(9);
    let program_of = |row: &MsgRow| (row.author == forge()).then_some("forge");
    let post = row("forge:web:3", forge(), vec![code("forge")]);
    assert_eq!(
        program_post(&post, program_of(&post)),
        Some(("forge", "review 7"))
    );
    for other in [
        row("forge:web:3", Principal::Account(1), vec![code("forge")]),
        row("forge:web:3", forge(), vec![code("rust")]),
        row("forge:web:3", forge(), vec![code("forge"), Block::Divider]),
        row("general", forge(), vec![code("forge")]),
        row("chess:1", forge(), vec![code("forge")]),
    ] {
        assert_eq!(program_post(&other, program_of(&other)), None, "{other:?}");
    }
}
