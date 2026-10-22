//! A working value of each chat op and query, as `#[tool_op]` and `#[tool_query]` name them: what
//! an agent edits rather than build one from the schema.
use std::collections::BTreeMap;

use crate::message::Status;
use crate::{
    Block, Mark, Notice, Notices, Op, PageRequest, Peer, PeerMessage, PeerWrite, PostPolicy,
    Principal, Query, Span,
};

const ROOM: &str = "design";

fn room() -> String {
    ROOM.into()
}

fn me() -> Vec<Principal> {
    vec![Principal::Account(21)]
}

fn every() -> PageRequest {
    PageRequest {
        after: None,
        limit: None,
    }
}

fn twenty() -> PageRequest {
    PageRequest {
        after: None,
        limit: Some(20),
    }
}

pub(crate) fn create() -> Op {
    Op::CreateChannel {
        channel_id: room(),
        name: room(),
        post_policy: PostPolicy::Open,
    }
}

pub(crate) fn dm() -> Op {
    Op::CreateDmChannel {
        counterpart: 17,
        name: "chief".into(),
    }
}

pub(crate) fn rename() -> Op {
    Op::RenameChannel {
        channel_id: room(),
        name: "design review".into(),
    }
}

pub(crate) fn archive() -> Op {
    Op::SetChannelArchived {
        channel_id: room(),
        archived: true,
    }
}

pub(crate) fn post() -> Op {
    let hello = Span::plain("hello ");
    let chief = Span {
        text: "chief".into(),
        marks: vec![Mark::Mention(Principal::Account(17))],
    };
    Op::PostMessage {
        channel_id: room(),
        message_id: "m-1".into(),
        blocks: vec![Block::Paragraph(vec![hello, chief])],
        thread: None,
    }
}

pub(crate) fn edit() -> Op {
    Op::EditMessage {
        channel_id: room(),
        seq: 4,
        blocks: vec![Block::paragraph("hello again")],
        base_rev: None,
    }
}

pub(crate) fn delete() -> Op {
    Op::DeleteMessage {
        channel_id: room(),
        seq: 4,
    }
}

pub(crate) fn react() -> Op {
    Op::AddReaction {
        channel_id: room(),
        seq: 4,
        emoji: "👍".into(),
    }
}

pub(crate) fn unreact() -> Op {
    Op::RemoveReaction {
        channel_id: room(),
        seq: 4,
        emoji: "👍".into(),
    }
}

pub(crate) fn seat() -> Op {
    Op::SetMembership {
        channel_id: room(),
        principal: Principal::Account(21),
        member: true,
    }
}

pub(crate) fn create_with_meta() -> Op {
    Op::CreateChannelWithMeta {
        channel_id: room(),
        name: room(),
        post_policy: PostPolicy::Open,
        meta: BTreeMap::from([(crate::RULES.to_owned(), "English, short.".to_owned())]),
    }
}

pub(crate) fn set_meta() -> Op {
    Op::SetChannelMeta {
        channel_id: room(),
        key: crate::RULES.into(),
        value: "English, short.".into(),
    }
}

pub(crate) fn remove_meta() -> Op {
    Op::RemoveChannelMeta {
        channel_id: room(),
        key: crate::RULES.into(),
    }
}

pub(crate) fn listen() -> Op {
    Op::Listen {
        channel_id: room(),
        listening: true,
    }
}

pub(crate) fn notices() -> Op {
    Op::SetNotices {
        channel_id: room(),
        notices: Some(Notices::Mentions),
    }
}

pub(crate) fn adopt() -> Op {
    Op::AdoptOwners {
        kind: "chat.message".into(),
        after: None,
        limit: 100,
    }
}

pub(crate) fn watch() -> Op {
    Op::Watch {
        channel_id: room(),
        watching: true,
    }
}

pub(crate) fn post_text() -> Op {
    Op::Post {
        channel_id: room(),
        message_id: "m-2".into(),
        text: "hello <@17>, the **draft** is up".into(),
        thread: None,
    }
}

pub(crate) fn channels() -> Query {
    Query::Channels { page: every() }
}

pub(crate) fn channel() -> Query {
    Query::Channel { channel_id: room() }
}

pub(crate) fn by_id() -> Query {
    Query::MessageById {
        message_id: "m-1".into(),
    }
}

pub(crate) fn attention() -> Query {
    Query::ThreadAttention {
        channel_id: room(),
        author: Principal::Account(21),
    }
}

pub(crate) fn roots() -> Query {
    Query::Roots {
        channel_id: room(),
        viewer: me(),
        page: twenty(),
    }
}

pub(crate) fn around() -> Query {
    Query::MessagesAround {
        channel_id: room(),
        seq: 40,
        viewer: me(),
        page: twenty(),
    }
}

pub(crate) fn thread() -> Query {
    Query::Thread {
        channel_id: room(),
        root_seq: 4,
        viewer: me(),
        page: every(),
    }
}

pub(crate) fn members() -> Query {
    Query::Members {
        channel_id: room(),
        page: every(),
    }
}

pub(crate) fn tagged() -> Query {
    Query::TagSearch {
        tag: "release".into(),
        viewer: me(),
        channel_id: None,
        page: twenty(),
    }
}

pub(crate) fn accounts() -> Query {
    Query::Accounts { page: every() }
}

pub(crate) fn reactors() -> Query {
    Query::Reactors {
        channel_id: room(),
        seqs: vec![4, 5],
    }
}

pub(crate) fn clients() -> Query {
    Query::Clients {
        channel_id: room(),
        seqs: vec![4, 5],
    }
}

pub(crate) fn listening() -> Query {
    Query::Listening {
        account: 21,
        page: every(),
    }
}

pub(crate) fn followed() -> Query {
    Query::Followed {
        channel_id: room(),
        account: 21,
        roots: vec![4],
    }
}

pub(crate) fn cut() -> Query {
    Query::Notices {
        account: 21,
        page: every(),
    }
}

pub(crate) fn watching() -> Query {
    Query::Watching {
        account: 21,
        page: every(),
    }
}

pub(crate) fn chosen() -> Query {
    Query::Chosen {
        account: 21,
        channel_id: room(),
    }
}

pub(crate) fn drafting() -> Op {
    Op::Drafting {
        channel_id: room(),
        thread: None,
        text: "Looking at the build".into(),
        anew: false,
        doing: Some("running forge_read".into()),
    }
}

pub(crate) fn undraft() -> Op {
    Op::Undraft { channel_id: room() }
}

pub(crate) fn drafts() -> Query {
    Query::Drafts { channel_id: room() }
}

pub(crate) fn names() -> Query {
    Query::Names { ids: vec![room()] }
}

pub(crate) fn posted_in() -> Query {
    Query::PostedIn {
        account: 21,
        page: every(),
    }
}

pub(crate) fn choose_for_many() -> Op {
    Op::ChooseForMany {
        accounts: vec![21, 22],
        channel_id: room(),
        listening: false,
        notices: Some(Notices::Mentions),
    }
}

pub(crate) fn chosen_by() -> Query {
    Query::ChosenBy {
        channel_id: room(),
        accounts: vec![21, 22],
    }
}

pub(crate) fn unread() -> Query {
    Query::Unread {
        account: 21,
        rooms: vec![(room(), 40)],
    }
}

pub(crate) fn text_search() -> Query {
    Query::TextSearch {
        text: "deploy".into(),
        viewer: me(),
        channel_id: None,
        page: twenty(),
    }
}

pub(crate) fn notify() -> Op {
    Op::Notify {
        channel_id: room(),
        message_id: "pods-builds-1-ended".into(),
        thread: None,
        notice: ended(),
    }
}

pub(crate) fn notify_with() -> Op {
    Op::NotifyWith {
        channel_id: room(),
        message_id: "pods-builds-1-ended".into(),
        thread: None,
        notice: ended(),
        by: Some(Principal::Account(15)),
        text: Some("The log stops at `make check`.".into()),
    }
}

fn ended() -> Notice {
    Notice {
        kind: "pod".into(),
        group: "builds-1".into(),
        subject: "pod builds-1".into(),
        link: None,
        status: Status::Failed,
        detail: "exit code 1".into(),
        concerns: vec![21],
    }
}

pub(crate) fn take() -> Op {
    Op::Take {
        channel_id: room(),
        thread: Some(4),
    }
}

pub(crate) fn release() -> Op {
    Op::Release {
        channel_id: room(),
        thread: Some(4),
    }
}

pub(crate) fn watch_places() -> Op {
    Op::WatchPlaces { deliver: vec![0] }
}

pub(crate) fn busy() -> Query {
    Query::Busy {
        channel_id: room(),
        thread: Some(4),
        for_account: 21,
    }
}

pub(crate) fn holders() -> Query {
    Query::Holders {
        channel_id: room(),
        thread: Some(4),
        page: every(),
    }
}

pub(crate) fn break_place() -> Op {
    Op::Break {
        channel_id: room(),
        thread: Some(4),
    }
}

pub(crate) fn set_post_policy() -> Op {
    Op::SetPostPolicy {
        channel_id: room(),
        post_policy: PostPolicy::MembersOnly,
    }
}

pub(crate) fn seated() -> Query {
    Query::Seated {
        principal: Principal::Account(21),
        channel_ids: vec![room(), "general".into()],
    }
}

pub(crate) fn as_reader() -> Query {
    let asked = Query::Roots {
        channel_id: room(),
        viewer: me(),
        page: PageRequest::default(),
    };
    Query::As {
        reader: Principal::Account(21),
        query: abi::encode(&asked),
    }
}

pub(crate) fn repliers() -> Query {
    Query::Repliers {
        channel_id: room(),
        roots: vec![4, 9],
    }
}

pub(crate) fn link() -> Op {
    Op::Link {
        channel_id: room(),
        peer: Some(Peer {
            network: [0x22; 32],
            channel_id: room(),
        }),
    }
}

pub(crate) fn from_peer() -> Op {
    Op::FromPeer {
        channel_id: room(),
        write: PeerWrite::React {
            message: PeerMessage::There("hello-1".into()),
            emoji: "👍".into(),
            on: true,
        },
    }
}

pub(crate) fn peer() -> Query {
    Query::Peer { channel_id: room() }
}

pub(crate) fn foreigners() -> Query {
    Query::Foreigners { page: twenty() }
}
