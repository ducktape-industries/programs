//! A room's history as it grows: it opens on a screenful, a block reads again
//! only the rows it wrote, rooms and threads opened again show at once, and a
//! list follows its end only where the reader is (and on their own post).
use super::*;
use crate::composer::Place;
use std::cell::Cell;
use std::rc::Rc;
use view::{ListOffset, px};

// #general with `head` roots (marked rows, each with a reaction), its roots
// paged by the limit asked; #random with none; under any root one reply.
// Anything else as `chat_reply` answers it. The pane is 260 tall: a
// screenful is ten 26-tall rows and one cut at the edge.
fn history(head: Rc<Cell<u64>>) -> (TestAppContext, Entity<Chat>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host().handle::<Ask<::chat::Chat>>(move |query| {
        Ok(match query {
            Query::Channels { .. } => Reply::Channels(page(vec![
                channel("general", "General", head.get()),
                channel("random", "Random", 0),
            ])),
            Query::Roots {
                channel_id,
                page: asked,
                ..
            } if channel_id == "general" => {
                let top = head.get();
                let limit = asked.limit().min(top);
                Reply::Roots(::chat::PageResponse {
                    height: 1,
                    items: (top + 1 - limit..=top).rev().map(marked_row).collect(),
                    next: (limit < top).then(|| vec![1]),
                })
            }
            Query::MessagesAround {
                channel_id,
                seq,
                page: asked,
                ..
            } if channel_id == "general" => {
                let half = asked.limit() / 2;
                let lo = seq.saturating_sub(half).max(1);
                let hi = seq.saturating_add(half).min(head.get());
                Reply::Messages((lo..=hi).map(marked_row).collect())
            }
            Query::Thread { root_seq, .. } => Reply::Thread {
                root: Some(marked_row(root_seq)),
                replies: page(vec![MsgRow {
                    thread: Some(root_seq),
                    ..row(1000, 8, "a reply")
                }]),
            },
            query => chat_reply(query),
        })
    });
    let view = cx.open::<Chat>();
    cx.run_until_parked();
    seat(&mut cx);
    let pane = Place {
        x: 0.,
        y: 0.,
        width: 400.,
        height: 260.,
    };
    cx.update(&view, |chat, _, cx| {
        chat.layout.pane = Some(pane);
        cx.notify();
    });
    (cx, view)
}

fn asked(cx: &TestAppContext) -> Vec<Query> {
    cx.host().requests::<Ask<::chat::Chat>>()
}

fn open(cx: &mut TestAppContext, key: &str) {
    cx.simulate_click(key);
    cx.run_until_parked();
}

fn held(view: &Entity<Chat>) -> (usize, u64, u64) {
    view.read(|chat| {
        let rows = chat.room.as_ref().unwrap().messages.ready().unwrap();
        let (first, last) = (rows.first().unwrap().seq, rows.last().unwrap().seq);
        (rows.len(), first, last)
    })
}

#[test]
fn a_room_opens_on_one_screenful_of_its_newest_roots() {
    let (mut cx, view) = history(Rc::new(Cell::new(600)));
    let before = asked(&cx).len();
    open(&mut cx, "chat-sidebar-channel-general");
    let limits: Vec<_> = asked(&cx)[before..]
        .iter()
        .filter_map(|query| match query {
            Query::Roots { page, .. } => Some(page.limit),
            _ => None,
        })
        .collect();
    assert_eq!(limits, [Some(11)], "one page, a screenful");
    assert_eq!(held(&view), (11, 590, 600));
    view.read(|chat| assert!(chat.room.as_ref().unwrap().has_older));
}

#[test]
fn a_block_reads_again_only_the_rows_it_wrote_and_a_new_root_one_page() {
    let head = Rc::new(Cell::new(600));
    let (mut cx, view) = history(head.clone());
    let changes = cx.host().stream::<Changes<::chat::Chat>>();
    open(&mut cx, "chat-sidebar-channel-general");
    let before = asked(&cx).len();
    let chose = (
        "general".to_owned(),
        595u64,
        "👍".to_owned(),
        Principal::Account(9),
    );
    changes.send(block(4, vec![chat::tables::REACTIONS.key(&chose)]));
    cx.run_until_parked();
    let since = &asked(&cx)[before..];
    let roots = |query: &Query| matches!(query, Query::Roots { .. });
    assert!(!since.iter().any(roots), "no page of roots: {since:?}");
    let around = |query: &Query| matches!(query, Query::MessagesAround { seq: 595, .. });
    assert!(since.iter().any(around), "the row it wrote: {since:?}");
    let reactors = |query: &Query| matches!(query, Query::Reactors { seqs, .. } if seqs == &[595]);
    assert!(
        since.iter().any(reactors),
        "who chose on it alone: {since:?}"
    );
    // a root past the newest: one page, over the rows held
    head.set(601);
    let before = asked(&cx).len();
    changes.send(block(5, posted("general")));
    cx.run_until_parked();
    let pages = asked(&cx)[before..]
        .iter()
        .filter(|query| roots(query))
        .count();
    assert_eq!(pages, 1);
    assert_eq!(held(&view), (12, 590, 601), "the older rows stay");
}

#[test]
fn a_room_opened_again_shows_at_once_and_reads_only_what_was_written_since() {
    let (mut cx, view) = history(Rc::new(Cell::new(600)));
    let changes = cx.host().stream::<Changes<::chat::Chat>>();
    open(&mut cx, "chat-sidebar-channel-general");
    open(&mut cx, "chat-sidebar-channel-random");
    let edited = ("general".to_owned(), 597u64);
    changes.send(block(4, vec![chat::tables::MESSAGES.key(&edited)]));
    cx.run_until_parked();
    let before = asked(&cx).len();
    open(&mut cx, "chat-sidebar-channel-general");
    assert_eq!(held(&view), (11, 590, 600), "the rows it showed");
    let since = &asked(&cx)[before..];
    let whole = |query: &Query| matches!(query, Query::Roots { .. } | Query::Members { .. });
    assert!(
        !since.iter().any(whole),
        "nothing read again whole: {since:?}"
    );
    let around = |query: &Query| matches!(query, Query::MessagesAround { seq: 597, .. });
    assert!(since.iter().any(around), "the row written since: {since:?}");
}

#[test]
fn a_thread_opened_again_shows_its_replies_at_once_and_reads_them_behind() {
    let (mut cx, view) = history(Rc::new(Cell::new(600)));
    open(&mut cx, "chat-sidebar-channel-general");
    cx.update(&view, |chat, _, cx| {
        chat.open_thread(595, cx);
        cx.notify();
    });
    cx.run_until_parked();
    cx.update(&view, |chat, _, cx| {
        chat.close_thread();
        cx.notify();
    });
    let before = asked(&cx).len();
    cx.update(&view, |chat, _, cx| {
        chat.open_thread(595, cx);
        cx.notify();
    });
    view.read(|chat| {
        let thread = chat.room.as_ref().unwrap().thread.as_ref().unwrap();
        let shown = thread.replies.ready().map(Vec::len);
        assert_eq!(shown, Some(1), "shown before any read lands");
    });
    cx.run_until_parked();
    let thread = |query: &&Query| matches!(query, Query::Thread { .. });
    let reads = asked(&cx)[before..].iter().filter(thread).count();
    assert_eq!(reads, 1, "read again behind");
}

// The list `name` in the frame, as the host is sent it.
fn list_named<'a>(node: &'a wire::Node, name: &str) -> Option<&'a wire::Node> {
    if let wire::Node::List {
        id: wire::ElementIdWire::Name(id),
        ..
    } = node
        && id == name
    {
        return Some(node);
    }
    node.children()
        .iter()
        .find_map(|child| list_named(child, name))
}

// Whether the host is told the list `name` follows its end.
fn follows(cx: &TestAppContext, name: &str) -> bool {
    match list_named(cx.root(), name) {
        Some(wire::Node::List { following_tail, .. }) => *following_tail,
        _ => panic!("no list {name}"),
    }
}

fn state(view: &Entity<Chat>, pane: Pane) -> view::ListState {
    view.read(|chat| chat.drawn(pane).borrow().list.as_ref().unwrap().1.clone())
}

#[test]
fn a_whole_thread_follows_its_end_and_one_with_pages_left_does_not() {
    let (mut cx, view) = history(Rc::new(Cell::new(600)));
    open(&mut cx, "chat-sidebar-channel-general");
    cx.update(&view, |chat, _, cx| {
        chat.open_thread(595, cx);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(follows(&cx, "chat-thread-rows"), "every reply is in");
    let top = state(&view, Pane::Thread).logical_scroll_top().item_ix;
    assert_eq!(top, 0, "it opens on its root");
    cx.update(&view, |chat, _, cx| {
        let room = chat.room.as_mut().unwrap();
        room.thread.as_mut().unwrap().has_more = true;
        cx.notify();
    });
    cx.run_until_parked();
    assert!(
        !follows(&cx, "chat-thread-rows"),
        "its loaded foot is not its end"
    );
}

#[test]
fn an_arrival_moves_nothing_and_the_readers_own_post_goes_to_the_end() {
    let head = Rc::new(Cell::new(600));
    let (mut cx, view) = history(head.clone());
    let changes = cx.host().stream::<Changes<::chat::Chat>>();
    open(&mut cx, "chat-sidebar-channel-general");
    assert!(follows(&cx, "chat-message-rows"), "a room at its head");
    // the reader scrolled up: where the list stands is the host's to say
    let rows = state(&view, Pane::Timeline);
    rows.scroll_to(ListOffset {
        item_ix: 0,
        offset_in_item: px(0.),
    });
    head.set(601);
    changes.send(block(5, posted("general")));
    cx.run_until_parked();
    assert_eq!(held(&view).2, 601, "it arrived");
    assert_eq!(rows.logical_scroll_top().item_ix, 0, "and moved nothing");
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    cx.host().never::<Submit<::chat::Chat>>();
    cx.simulate_focus("draft-general/editor");
    cx.simulate_input("draft-general/editor", "mine");
    cx.simulate_field_key("draft-general/editor", "enter");
    cx.run_until_parked();
    let end = rows.item_count();
    assert_eq!(
        rows.logical_scroll_top().item_ix,
        end,
        "the reader's own: to the end"
    );
}
