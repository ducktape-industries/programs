//! Messages: posting, threads, editing, deleting, and the reads over them.
use super::channels::edit;
use super::*;
use crate::{replies_after, roots_below};

#[test]
fn posts_land_as_roots_newest_first_and_page_older() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    for n in 1..=5 {
        chat.post(&BO, &format!("m{n}"), &format!("note {n}"), None);
    }
    let roots = |after| {
        let Reply::Roots(page) = chat.ask(Query::Roots {
            channel_id: "general".into(),
            viewer: vec![],
            page: PageRequest {
                after,
                limit: Some(2),
            },
        }) else {
            panic!("roots answer roots");
        };
        page
    };
    let newest = roots(None);
    assert_eq!(seqs(&newest.items), [5, 4]);
    assert_eq!(seqs(&roots(newest.next).items), [3, 2]);
    assert_eq!(seqs(&roots(Some(roots_below("general", 2))).items), [1]);
    assert_eq!(chat.message(5).author, BO);
}

#[test]
fn a_thread_read_past_its_newest_reply_answers_only_the_replies_since() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&ADA, "m1", "question", None);
    chat.post(&BO, "m2", "answer", Some(1));
    chat.post(&CY, "m3", "another", Some(1));
    chat.post(&BO, "m4", "a root", None);
    chat.post(&ADA, "m5", "and more", Some(1));
    let Reply::Thread { replies, .. } = chat.ask(Query::Thread {
        channel_id: "general".into(),
        root_seq: 1,
        viewer: vec![],
        page: PageRequest {
            after: Some(replies_after("general", 1, 3)),
            limit: None,
        },
    }) else {
        panic!("a thread answers a thread");
    };
    assert_eq!(seqs(&replies.items), [5]);
}

#[test]
fn a_message_id_is_unique_and_a_colon_id_is_its_programs_alone() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&BO, "m1", "hi", None);
    let again = chat.refused(&CY, post("general", "m1", "hi", None));
    assert_eq!(again, code::ALREADY_EXISTS);
    let squat = chat.refused(&BO, post("general", "forge:0001", "hi", None));
    assert_eq!(squat, code::UNAUTHORIZED);
    chat.ok(&FORGE, post("general", "forge:0001", "opened", None));
    let Reply::Message(Some(row)) = chat.ask(Query::MessageById {
        message_id: "forge:0001".into(),
    }) else {
        panic!("the id finds its message");
    };
    assert_eq!(row.seq, 2);
    let nowhere = chat.refused(&BO, post("nowhere", "m9", "hi", None));
    assert_eq!(nowhere, code::NOT_FOUND);
}

#[test]
fn a_reply_joins_its_root_and_the_authors_attention() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&ADA, "m1", "question", None);
    chat.post(&BO, "m2", "answer", Some(1));
    chat.post(&CY, "m3", "another", Some(1));
    let root = chat.message(1);
    assert_eq!((root.reply_count, root.last_reply_seq), (2, Some(3)));
    let Reply::Thread { root, replies } = chat.ask(Query::Thread {
        channel_id: "general".into(),
        root_seq: 1,
        viewer: vec![],
        page: PageRequest::default(),
    }) else {
        panic!("a thread answers a thread");
    };
    assert_eq!((root.unwrap().seq, seqs(&replies.items)), (1, vec![2, 3]));
    assert_eq!(attention(&chat, ADA).map(|row| row.seq), Some(1));
    assert_eq!(attention(&chat, BO), None);
    let orphan = chat.refused(&CY, post("general", "m4", "nowhere", Some(99)));
    assert_eq!(orphan, code::NOT_FOUND);
}

#[test]
fn a_reply_to_a_reply_lands_under_its_root_and_counts_there_once() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&ADA, "m1", "question", None);
    chat.post(&BO, "m2", "answer", Some(1));
    let before = chat.message(1).reply_count;
    chat.post(&CY, "m3", "to the answer", Some(2));
    assert_eq!(chat.message(3).thread, Some(1), "it threads under the root");
    let root = chat.message(1);
    assert_eq!(
        (root.reply_count, root.last_reply_seq),
        (before + 1, Some(3))
    );
    let named = chat.message(2);
    assert_eq!(
        (named.reply_count, named.last_reply_seq),
        (0, None),
        "the reply it named opens no thread"
    );
    let Reply::Thread { replies, .. } = chat.ask(Query::Thread {
        channel_id: "general".into(),
        root_seq: 1,
        viewer: vec![],
        page: PageRequest::default(),
    }) else {
        panic!("a thread answers a thread");
    };
    assert_eq!(seqs(&replies.items), [2, 3]);
}

#[test]
fn an_edit_moves_search_with_it_and_a_deleted_message_takes_none() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&BO, "m1", "hello #launch", None);
    chat.ok(&BO, edit(1, "goodbye"));
    let row = chat.message(1);
    assert_eq!(
        (row.rev, row.edited, row.text.as_str()),
        (1, true, "goodbye")
    );
    assert_eq!(
        (chat.search("hello"), chat.search("goodbye")),
        (vec![], vec![1])
    );
    assert!(tagged(&chat, "launch").is_empty());
    chat.ok(&BO, delete(1));
    assert_eq!(chat.refused(&BO, edit(1, "undead")), code::WRONG_STATE);
}

#[test]
fn a_row_keeps_its_posts_block_time_through_an_edit() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&BO, "m1", "hello", None);
    let posted = chat.message(1);
    assert!(posted.time > 0);
    assert_eq!(posted.time, posted.height * 1000, "the env's block time");
    chat.ok(&BO, edit(1, "goodbye"));
    let edited = chat.message(1);
    assert_eq!((edited.height, edited.time), (posted.height, posted.time));
    assert!(edited.edited_at.is_some_and(|at| at > posted.time));
}

#[test]
fn a_delete_leaves_a_tombstone_that_holds_its_thread() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&BO, "m1", "hello #launch", None);
    chat.post(&BO, "m2", "hello again", Some(1));
    chat.ok(&ADA, delete(1));
    let tombstone = chat.message(1);
    assert!(tombstone.deleted && tombstone.blocks.is_empty() && tombstone.text.is_empty());
    assert_eq!(tombstone.reply_count, 1, "the thread under it stands");
    assert_eq!(chat.search("hello"), [2]);
    assert!(tagged(&chat, "launch").is_empty());
    assert_eq!(attention(&chat, BO), None);
    let once = chat.store.borrow().state.clone();
    chat.ok(&BO, delete(1));
    assert_eq!(
        chat.store.borrow().state,
        once,
        "deleting twice changes nothing"
    );
    chat.ok(&BO, delete(2));
}

#[test]
fn search_needs_every_word_and_tags_page_newest_first() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&BO, "m1", "Hello #World", None);
    chat.post(&BO, "m2", "hello there #world", None);
    chat.post(&BO, "m3", "there", None);
    assert_eq!(chat.search("HELLO"), [2, 1]);
    assert_eq!(chat.search("hello there"), [2]);
    let empty = crate::Chat::query(
        &chat.reads(),
        text_search(" \t ", None, PageRequest::default()),
    );
    assert_eq!(empty.unwrap_err().code, code::INVALID_INPUT);
    assert_eq!(tagged(&chat, "#WORLD"), [2, 1]);
}

#[test]
fn search_finds_a_term_inside_a_word_and_needs_every_term() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&BO, "m1", "검색인덱스는 지웠다", None);
    chat.post(&BO, "m2", "책 한 권", None);
    chat.post(&BO, "m3", "the **CAFE\u{301}** opens", None);
    chat.post(&BO, "m4", "검색 책", None);
    assert_eq!(chat.search("검색"), [4, 1]);
    assert_eq!(chat.search("책"), [4, 2]);
    assert_eq!(chat.search("검색 책"), [4]);
    assert_eq!(chat.search("인덱스 책"), Vec::<u64>::new());
    assert_eq!(chat.search("café OPENS"), [3], "NFC and case folded");
    assert_eq!(chat.search("the cafe"), Vec::<u64>::new());
}

#[test]
fn search_is_newest_first_in_a_channel_and_across_them_and_keeps_its_limit() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.ok(&ADA, create("random", PostPolicy::Open));
    chat.post(&BO, "g1", "ship it", None);
    chat.ok(&BO, post("random", "r1", "ship it", None));
    chat.post(&BO, "g2", "nothing", None);
    chat.ok(&BO, post("random", "r2", "ship that", None));
    chat.post(&BO, "g3", "ship again", None);
    let everywhere = [("general", 3), ("random", 2), ("random", 1), ("general", 1)];
    assert_eq!(
        hits(&chat, "ship", None, None),
        (places(&everywhere), false)
    );
    let random = hits(&chat, "ship", Some("random"), None);
    assert_eq!(random, (places(&everywhere[1..3]), false));
    let first_two = hits(&chat, "ship", None, Some(2));
    assert_eq!(first_two, (places(&everywhere[..2]), true));
    assert_eq!(
        hits(&chat, "ship", None, Some(4)),
        (places(&everywhere), false)
    );
    let one = hits(&chat, "ship", Some("general"), Some(1));
    assert_eq!(one, (places(&[("general", 3)]), true));
}

#[test]
fn search_walks_a_long_channel_to_its_first_message() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&BO, "m1", "needle", None);
    for n in 2..=300 {
        chat.post(&BO, &format!("m{n}"), "hay", None);
    }
    chat.post(&BO, "m301", "needle", None);
    assert_eq!(chat.search("needle"), [301, 1]);
    let newest_hay = places(&[("general", 300), ("general", 299), ("general", 298)]);
    assert_eq!(hits(&chat, "hay", None, Some(3)), (newest_hay, true));
    assert_eq!(chat.search("hay").len(), 299);
}

#[test]
fn tags_page_newest_first_in_a_channel_and_across_them() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.ok(&ADA, create("random", PostPolicy::Open));
    chat.post(&BO, "g1", "#ship it", None);
    chat.ok(&BO, post("random", "r1", "#Ship it", None));
    chat.post(&BO, "g2", "#other", None);
    chat.ok(&BO, post("random", "r2", "go #ship", None));
    chat.post(&BO, "g3", "#ship again", None);
    chat.post(&BO, "g4", "no tag: `#ship`", None);
    let everywhere = [("general", 3), ("random", 2), ("random", 1), ("general", 1)];
    let pages = [places(&everywhere[..3]), places(&everywhere[3..])];
    assert_eq!(tag_pages(&chat, "#SHIP", None, 3), pages);
    assert_eq!(
        tag_pages(&chat, "ship", None, 1).concat(),
        places(&everywhere)
    );
    let general = places(&[("general", 3), ("general", 1)]);
    assert_eq!(
        tag_pages(&chat, "ship", Some("general"), 1).concat(),
        general
    );
    chat.ok(&BO, edit(3, "untagged"));
    chat.ok(&BO, delete(1));
    let left = places(&[("random", 2), ("random", 1)]);
    assert_eq!(tag_pages(&chat, "ship", None, 2).concat(), left);
    assert!(
        tag_pages(&chat, "ship", Some("general"), 2)
            .concat()
            .is_empty()
    );
}

#[test]
fn a_tag_page_cursor_stays_in_its_listing() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&BO, "m1", "#a #b", None);
    chat.post(&BO, "m2", "#a #b", None);
    let Reply::TagHits(first) = chat.ask(tag_search("a", None, PageRequest::first(1))) else {
        panic!("a tag search answers tag hits");
    };
    let elsewhere = tag_search("b", None, PageRequest::resume(first.next, 1));
    let refusal = crate::Chat::query(&chat.reads(), elsewhere).unwrap_err();
    assert_eq!(refusal.code, code::STALE);
}

#[test]
fn text_search_pages_newest_first_and_its_cursor_stays_in_its_search() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.ok(&ADA, create("random", PostPolicy::Open));
    chat.post(&BO, "g1", "ship it", None);
    chat.ok(&BO, post("random", "r1", "Ship it", None));
    chat.post(&BO, "g2", "nothing", None);
    chat.ok(&BO, post("random", "r2", "ship that", None));
    chat.post(&BO, "g3", "ship again", None);
    let everywhere = [("general", 3), ("random", 2), ("random", 1), ("general", 1)];
    let pages = [places(&everywhere[..3]), places(&everywhere[3..])];
    assert_eq!(text_pages(&chat, "SHIP", None, 3), pages);
    assert_eq!(
        text_pages(&chat, "ship", None, 1).concat(),
        places(&everywhere)
    );
    assert_eq!(
        text_pages(&chat, "ship", Some("random"), 1).concat(),
        places(&everywhere[1..3])
    );
    let both = places(&[("random", 1), ("general", 1)]);
    assert_eq!(text_pages(&chat, "ship  it", None, 1).concat(), both);
    let Reply::TextHits(first) = chat.ask(text_search("ship", None, PageRequest::first(1))) else {
        panic!("a text search answers a page of hits");
    };
    let elsewhere = text_search("it", None, PageRequest::resume(first.next, 1));
    let refusal = crate::Chat::query(&chat.reads(), elsewhere).unwrap_err();
    assert_eq!(refusal.code, code::STALE);
    let blank = text_search(" \t ", None, PageRequest::default());
    let refusal = crate::Chat::query(&chat.reads(), blank).unwrap_err();
    assert_eq!(refusal.code, code::INVALID_INPUT);
}

fn places(at: &[(&str, u64)]) -> Vec<(String, u64)> {
    at.iter()
        .map(|(channel, seq)| (channel.to_string(), *seq))
        .collect()
}

fn hits(
    chat: &Chat,
    text: &str,
    channel: Option<&str>,
    limit: Option<u64>,
) -> (Vec<(String, u64)>, bool) {
    let page = PageRequest { after: None, limit };
    let Reply::TextHits(found) = chat.ask(text_search(text, channel, page)) else {
        panic!("a text search answers a page of hits");
    };
    let at = found
        .items
        .iter()
        .map(|row| (row.channel_id.clone(), row.seq));
    (at.collect(), found.next.is_some())
}

fn tag_search(tag: &str, channel: Option<&str>, page: PageRequest) -> Query {
    Query::TagSearch {
        tag: tag.into(),
        viewer: vec![],
        channel_id: channel.map(str::to_owned),
        page,
    }
}

fn tag_pages(chat: &Chat, tag: &str, channel: Option<&str>, limit: u64) -> Vec<Vec<(String, u64)>> {
    pages(chat, limit, |page| tag_search(tag, channel, page))
}

fn text_search(text: &str, channel: Option<&str>, page: PageRequest) -> Query {
    Query::TextSearch {
        text: text.into(),
        viewer: vec![],
        channel_id: channel.map(str::to_owned),
        page,
    }
}

fn text_pages(
    chat: &Chat,
    text: &str,
    channel: Option<&str>,
    limit: u64,
) -> Vec<Vec<(String, u64)>> {
    pages(chat, limit, |page| text_search(text, channel, page))
}

// Every page of a search, `limit` hits each, each resumed from the cursor the one before answered.
fn pages(chat: &Chat, limit: u64, ask: impl Fn(PageRequest) -> Query) -> Vec<Vec<(String, u64)>> {
    let (mut pages, mut after) = (Vec::new(), None);
    loop {
        let asked = ask(PageRequest::resume(after, limit));
        let (Reply::TagHits(page) | Reply::TextHits(page)) = chat.ask(asked) else {
            panic!("a search answers a page of hits");
        };
        pages.push(
            page.items
                .iter()
                .map(|row| (row.channel_id.clone(), row.seq))
                .collect(),
        );
        assert!(pages.len() <= 64, "the pages of a search end");
        match page.next {
            Some(next) => after = Some(next),
            None => return pages,
        }
    }
}

fn seqs(rows: &[crate::MsgRow]) -> Vec<u64> {
    rows.iter().map(|row| row.seq).collect()
}

fn attention(chat: &Chat, author: Principal) -> Option<crate::MsgRow> {
    let Reply::Attention(row) = chat.ask(Query::ThreadAttention {
        channel_id: "general".into(),
        author,
    }) else {
        panic!("attention answers attention");
    };
    row
}

pub(super) fn tagged(chat: &Chat, tag: &str) -> Vec<u64> {
    let ask = |channel_id| {
        let Reply::TagHits(page) = chat.ask(Query::TagSearch {
            tag: tag.into(),
            viewer: vec![],
            channel_id,
            page: PageRequest::default(),
        }) else {
            panic!("a tag search answers tag hits");
        };
        seqs(&page.items)
    };
    let everywhere = ask(None);
    assert_eq!(ask(Some("general".into())), everywhere);
    everywhere
}

#[test]
fn flat_text_joins_a_paragraphs_marked_runs_as_typed() {
    assert_eq!(
        crate::plain_text(&crate::parse_message(
            "the **flaky** retry in `sync`; see it"
        )),
        "the flaky retry in sync; see it"
    );
}

#[test]
fn a_post_of_text_is_the_post_of_its_parsed_blocks() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    let text = "hello <@2>, the **draft** is up\n> quoted";
    let as_text = |id: &str, thread| Op::Post {
        channel_id: "general".into(),
        message_id: id.into(),
        text: text.into(),
        thread,
    };
    chat.ok(&ADA, as_text("t1", None));
    chat.post(&ADA, "b1", text, None);
    chat.ok(&BO, as_text("t2", Some(1)));
    let (by_text, by_blocks) = (chat.message(1), chat.message(2));
    assert_eq!(by_text.blocks, parse_message(text));
    assert_eq!(
        (&by_text.blocks, &by_text.text),
        (&by_blocks.blocks, &by_blocks.text)
    );
    assert_eq!(chat.message(1).reply_count, 1);
    let again = chat.refused(&CY, as_text("t1", None));
    assert_eq!(again, code::ALREADY_EXISTS);
}
