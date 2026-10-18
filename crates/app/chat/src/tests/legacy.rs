//! Chat before [`Stored`](crate::state::Stored): old rows and their postings, read until each
//! channel is migrated, then moved by `Op::Migrate` with every read answering the same.
use super::channels::edit;
use super::*;
use crate::legacy::{CHANNEL_TAGS, OLD_MESSAGES, TAGS, WORDS, postings};
use crate::state::{MESSAGES, Stored};
use crate::{MessageHits, MsgRow};

// the store as chat kept it before `Stored`: every row whole, with its search and tag postings
pub(super) fn age(chat: &Chat) {
    let ctx = chat.store.exec(chat.env(Origin::Root, None));
    for (key, stored) in MESSAGES.all(&ctx).unwrap() {
        let row = stored.row();
        postings(&ctx, &row, true);
        OLD_MESSAGES.put(&ctx, &key, &row);
        MESSAGES.remove(&ctx, &key);
    }
}

// every message, old rows read as new ones, by channel then seq
pub(super) fn every_row(chat: &Chat) -> Vec<MsgRow> {
    let reads = chat.reads();
    let old = OLD_MESSAGES.all(&reads).unwrap().into_iter();
    let new = MESSAGES.all(&reads).unwrap().into_iter();
    let mut rows: Vec<MsgRow> = (old.map(|(_, row)| Stored::from(row).row()))
        .chain(new.map(|(_, row)| row.row()))
        .collect();
    rows.sort_by_key(|row| (row.channel_id.clone(), row.seq));
    rows
}

pub(super) fn legacy_is_empty(chat: &Chat) -> bool {
    let reads = chat.reads();
    OLD_MESSAGES.all(&reads).unwrap().is_empty()
        && WORDS.all(&reads).unwrap().is_empty()
        && TAGS.all(&reads).unwrap().is_empty()
        && CHANNEL_TAGS.all(&reads).unwrap().is_empty()
}

pub(super) fn migrate(channel: &str) -> Op {
    Op::Migrate {
        channel_id: channel.into(),
    }
}

fn unmigrated(chat: &Chat) -> Vec<String> {
    let page = PageRequest::default();
    let Reply::ChannelIds(page) = chat.ask(Query::Unmigrated { page }) else {
        panic!("Unmigrated answers channel ids");
    };
    page.items
}

fn carried(reply: Reply) -> Vec<MsgRow> {
    match reply {
        Reply::Roots(page) | Reply::TagHits(page) => page.items,
        Reply::Messages(rows) | Reply::Hits(MessageHits { hits: rows, .. }) => rows,
        Reply::Thread { root, replies } => root.into_iter().chain(replies.items).collect(),
        Reply::Message(row) => row.into_iter().collect(),
        other => panic!("no rows in {other:?}"),
    }
}

// what every read of messages answers, as the rows it carries
fn answers(chat: &Chat) -> Vec<Vec<MsgRow>> {
    let viewer = || vec![ADA];
    let mut asked = Vec::new();
    for channel in ["general", "ops"] {
        let channel_id = || channel.to_owned();
        asked.extend([
            Query::Roots {
                channel_id: channel_id(),
                viewer: viewer(),
                page: PageRequest::default(),
            },
            Query::MessagesAround {
                channel_id: channel_id(),
                seq: 3,
                viewer: viewer(),
                page: PageRequest::first(50),
            },
            Query::Thread {
                channel_id: channel_id(),
                root_seq: 1,
                viewer: viewer(),
                page: PageRequest::default(),
            },
        ]);
    }
    for text in ["alpha", "beta", "gamma"] {
        asked.push(Query::Search {
            text: text.into(),
            viewer: viewer(),
            channel_id: None,
            page: PageRequest::default(),
        });
    }
    for tag in ["ship", "it"] {
        asked.push(Query::TagSearch {
            tag: tag.into(),
            viewer: viewer(),
            channel_id: None,
            page: PageRequest::default(),
        });
    }
    for message_id in ["g1", "g3", "o1"] {
        let message_id = message_id.into();
        asked.push(Query::MessageById { message_id });
    }
    asked.into_iter().map(|q| carried(chat.ask(q))).collect()
}

// two channels: roots, a thread, tags, reactions, an edit and a delete
fn busy() -> Chat {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.ok(&ADA, create("ops", PostPolicy::Open));
    chat.post(&ADA, "g1", "alpha beta #ship", None);
    chat.post(&BO, "g2", "alpha #it", Some(1));
    chat.post(&CY, "g3", "gamma", None);
    chat.post(&ADA, "g4", "alpha #ship again", None);
    chat.ok(&CY, edit(3, "gamma #ship"));
    chat.ok(&BO, react(1, "👍", true));
    chat.ok(&ADA, react(1, "🎉", true));
    chat.ok(&ADA, delete(4));
    chat.ok(&BO, post("ops", "o1", "alpha in ops #ship", None));
    chat
}

#[test]
fn old_rows_read_the_same_before_and_after_each_channel_migrates() {
    let mut chat = busy();
    let before = answers(&chat);
    age(&chat);
    assert!(MESSAGES.all(&chat.reads()).unwrap().is_empty());
    assert_eq!(answers(&chat), before, "an old row reads as a new one");
    assert_eq!(unmigrated(&chat), ["general", "ops"]);
    chat.ok(&BO, migrate("general"));
    assert_eq!(answers(&chat), before);
    assert_eq!(unmigrated(&chat), ["ops"]);
    chat.ok(&CY, migrate("ops"));
    assert_eq!(answers(&chat), before);
    assert!(unmigrated(&chat).is_empty());
    assert!(legacy_is_empty(&chat));
}

#[test]
fn migrating_again_writes_nothing() {
    let mut chat = busy();
    age(&chat);
    chat.ok(&ADA, migrate("general"));
    let once = chat.store.borrow().state.clone();
    chat.ok(&ADA, migrate("general"));
    assert_eq!(chat.store.borrow().state, once);
    assert_eq!(chat.refused(&ADA, migrate("nowhere")), code::NOT_FOUND);
}

#[test]
fn a_half_migrated_channel_reads_what_its_messages_hold() {
    let mut chat = busy();
    age(&chat);
    chat.post(&BO, "g5", "alpha fresh #ship", None);
    chat.ok(&ADA, edit(1, "beta only"));
    chat.ok(&CY, react(1, "👀", true));
    chat.post(&CY, "g6", "alpha reply", Some(1));
    chat.ok(&CY, delete(3));
    let postings_of = |seq: u64| {
        let words = WORDS.all(&chat.reads()).unwrap();
        let at = |(_, channel, at): &(String, String, u64)| channel == "general" && *at == seq;
        words.iter().filter(|key| at(key)).count()
    };
    assert_eq!(postings_of(1), 0, "an old row rewritten takes its postings");
    assert_eq!(postings_of(3), 0, "so does a deleted one");
    assert_eq!(postings_of(2), 2, "an untouched old row keeps its own");
    let root = chat.message(1);
    assert_eq!((root.text.as_str(), root.reply_count), ("beta only", 2));
    assert_eq!(root.reactions.len(), 3);
    let mut rows = every_row(&chat);
    rows.sort_by_key(|row| std::cmp::Reverse((row.time, row.seq)));
    let newest = |held: &dyn Fn(&MsgRow) -> bool| -> Vec<u64> {
        rows.iter()
            .filter(|row| held(row))
            .map(|row| row.seq)
            .collect()
    };
    for word in ["alpha", "beta", "gamma", "fresh"] {
        let holding = newest(&|row| row.text.contains(word));
        assert_eq!(chat.search(word), holding, "search {word}");
    }
    let shipped = newest(&|row| row.tags.iter().any(|tag| tag == "ship"));
    let tag = "ship".to_owned();
    let page = PageRequest::default();
    let asked = Query::TagSearch {
        tag,
        viewer: vec![],
        channel_id: None,
        page,
    };
    let Reply::TagHits(page) = chat.ask(asked) else {
        panic!("a tag search answers tag hits");
    };
    let tagged: Vec<u64> = page.items.iter().map(|row| row.seq).collect();
    assert_eq!(tagged, shipped);
    let before = answers(&chat);
    chat.ok(&BO, migrate("general"));
    chat.ok(&BO, migrate("ops"));
    assert_eq!(answers(&chat), before);
    assert!(legacy_is_empty(&chat));
}

#[test]
fn any_account_migrates_and_the_op_names_its_channel() {
    assert!(crate::gate::needs(&migrate("general")).is_empty());
    let said = crate::describe(&migrate("general"));
    assert_eq!(said.title, "Migrate channel · #general");
}
