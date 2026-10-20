//! A room's history as it grows: it opens at its newest on a page, a block reads again only the
//! rows it wrote, its first row in view reads the page before, and a room left keeps one page and
//! where it stood; a list follows its end only where the reader is (and on their own post).
use super::*;
use crate::composer::Place;
use std::cell::Cell;
use std::rc::Rc;
use view::{ListOffset, px};

fn history(head: Rc<Cell<u64>>) -> (TestAppContext, Entity<Chat>) {
    paged(head, 1)
}

fn paged(head: Rc<Cell<u64>>, replies: u64) -> (TestAppContext, Entity<Chat>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
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
                    let below = asked
                        .after
                        .as_deref()
                        .map(|after| u64::MAX - cursor_seq(after));
                    let top = below.map_or(head.get(), |below| below - 1);
                    let limit = asked.limit().min(top);
                    let oldest = top + 1 - limit;
                    Reply::Roots(::chat::PageResponse {
                        height: 1,
                        items: (oldest..=top).rev().map(marked_row).collect(),
                        next: (limit < top).then(|| chat::roots_below("general", oldest)),
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
                Query::Thread {
                    root_seq,
                    page: asked,
                    ..
                } => {
                    let from = asked.after.as_deref().map_or(0, cursor_seq);
                    let under = (1..=replies).map(|reply| 10_000 + root_seq * 100 + reply);
                    let mut since = under.filter(|seq| *seq > from);
                    let items: Vec<MsgRow> = since
                        .by_ref()
                        .take(asked.limit() as usize)
                        .map(|seq| MsgRow {
                            thread: Some(root_seq),
                            ..row(seq, 8, "a reply")
                        })
                        .collect();
                    let last = items.last().map_or(from, |reply| reply.seq);
                    let more = since.next().is_some();
                    Reply::Thread {
                        root: Some(marked_row(root_seq)),
                        replies: ::chat::PageResponse {
                            height: 1,
                            items,
                            next: more.then(|| chat::replies_after("general", root_seq, last)),
                        },
                    }
                }
                Query::Reactors { .. } => Reply::Reactors(Vec::new()),
                query => chat_reply(query),
            })
        }));
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
        chat.layout.placed(pane);
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
fn before_any_pane_is_placed_a_room_reads_a_screenful_of_the_window() {
    let (mut cx, view) = history(Rc::new(Cell::new(600)));
    cx.simulate_resize(766., 640.);
    cx.update(&view, |chat, _, cx| {
        chat.layout.pane = None;
        chat.layout.tallest = 0.;
        cx.notify();
    });
    let before = asked(&cx).len();
    open(&mut cx, "chat-sidebar-channel-general");
    let limits: Vec<_> = asked(&cx)[before..]
        .iter()
        .filter_map(|query| match query {
            Query::Roots { page, .. } => Some(page.limit),
            _ => None,
        })
        .collect();
    assert_eq!(limits, [Some(screenful(640.))], "the window's height");
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
    let around = |query: &Query| matches!(query, Query::MessagesAround { seq: 595, page, .. } if page.limit == Some(1));
    assert!(
        since.iter().any(around),
        "the row it wrote, alone: {since:?}"
    );
    let reactors = |query: &Query| matches!(query, Query::Reactors { seqs, .. } if seqs == &[595]);
    assert!(
        since.iter().any(reactors),
        "who chose on it alone: {since:?}"
    );
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
    let replies = state(&view, Pane::Thread);
    let top = replies.logical_scroll_top().item_ix;
    assert_eq!(top, replies.item_count(), "it opens at its last reply");
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

fn stood(cx: &mut TestAppContext, name: &str, item_ix: usize, offset: f32) {
    let Some(wire::Node::List {
        scroll_handler: Some(handler),
        item_count,
        ..
    }) = list_named(cx.root(), name).cloned()
    else {
        panic!("no list {name} that hears its scroll");
    };
    cx.simulate_event(wire::Event::ListScroll {
        handler,
        event: wire::ListScroll {
            visible_start: item_ix,
            visible_end: item_ix + 5,
            count: item_count,
            is_scrolled: true,
            is_following_tail: false,
            offset: wire::ListOffset {
                item_ix,
                offset_in_item: offset,
            },
        },
    });
}

fn thread_open(cx: &mut TestAppContext, view: &Entity<Chat>, root: u64) {
    cx.update(view, |chat, _, cx| {
        chat.open_thread(root, cx);
        cx.notify();
    });
    cx.run_until_parked();
}

#[test]
fn a_room_and_a_thread_open_at_their_newest() {
    let (mut cx, view) = paged(Rc::new(Cell::new(600)), 40);
    open(&mut cx, "chat-sidebar-channel-general");
    let rows = state(&view, Pane::Timeline);
    assert!(follows(&cx, "chat-message-rows"));
    assert_eq!(
        rows.logical_scroll_top().item_ix,
        rows.item_count(),
        "at its newest"
    );
    let before = asked(&cx).len();
    thread_open(&mut cx, &view, 595);
    let reads: Vec<_> = asked(&cx)[before..]
        .iter()
        .filter_map(|query| match query {
            Query::Thread { page, .. } => Some(page.limit),
            _ => None,
        })
        .collect();
    assert_eq!(reads, [None], "every reply in one read");
    let replies = state(&view, Pane::Thread);
    assert_eq!(replies.item_count(), 41, "the root and its forty replies");
    assert_eq!(
        replies.logical_scroll_top().item_ix,
        41,
        "at its last reply"
    );
}

fn older_reads(cx: &TestAppContext, since: usize) -> usize {
    let older = |query: &&Query| matches!(query, Query::Roots { page, .. } if page.after.is_some());
    asked(cx)[since..].iter().filter(older).count()
}

#[test]
fn the_first_row_in_view_reads_the_page_before_it_once() {
    let (mut cx, view) = history(Rc::new(Cell::new(600)));
    open(&mut cx, "chat-sidebar-channel-general");
    let before = asked(&cx).len();
    reach_first(&mut cx, &view);
    assert_eq!(older_reads(&cx, before), 1);
    assert_eq!(held(&view), (22, 579, 600), "a screenful above the first");
    let Some(wire::Node::List { commands, .. }) = list_named(cx.root(), "chat-message-rows") else {
        panic!("no timeline");
    };
    let above = wire::ListCommand::Splice {
        start: 0,
        end: 0,
        count: 11,
    };
    assert_eq!(commands, &[above], "nothing but the page above moves");
    cx.run_until_parked();
    assert_eq!(older_reads(&cx, before), 1, "once");
}

#[test]
fn a_list_at_its_end_reads_nothing_older() {
    let (mut cx, _) = history(Rc::new(Cell::new(600)));
    open(&mut cx, "chat-sidebar-channel-general");
    let before = asked(&cx).len();
    let count = match list_named(cx.root(), "chat-message-rows") {
        Some(wire::Node::List { item_count, .. }) => *item_count,
        _ => panic!("no timeline"),
    };
    cx.simulate_range("chat-message-list", count - 5..count);
    assert_eq!(older_reads(&cx, before), 0);
    assert!(
        !cx.has_text("Load older messages"),
        "the list reads older rows itself"
    );
}

#[test]
fn a_first_page_shorter_than_its_pane_fills_it() {
    let (mut cx, view) = history(Rc::new(Cell::new(30)));
    open(&mut cx, "chat-sidebar-channel-general");
    let before = asked(&cx).len();
    let mut reached = 0;
    loop {
        cx.simulate_range("chat-message-list", 0..10);
        let sensor = view.read(|chat| crate::listing::first_row(chat, Pane::Timeline));
        if cx.find(&sensor).is_none() {
            break;
        }
        cx.simulate_bounds(&sensor, (0., 0.), (400., 26.));
        cx.run_until_parked();
        reached += 1;
        assert!(reached <= 2, "two pages before the first hold the room");
    }
    assert_eq!(older_reads(&cx, before), 2);
    assert_eq!(held(&view), (30, 1, 30), "the room whole");
    view.read(|chat| {
        assert!(
            !chat.room.as_ref().unwrap().has_older,
            "nothing before seq 1"
        )
    });
}

fn kept(view: &Entity<Chat>, room: &str) -> (usize, u64, u64) {
    view.read(|chat| {
        let rows = &chat.visited[room].rows;
        (rows.len(), rows[0].seq, rows[rows.len() - 1].seq)
    })
}

#[test]
fn a_room_left_keeps_its_newest_page_however_far_up_it_was_read() {
    let (mut cx, view) = history(Rc::new(Cell::new(600)));
    open(&mut cx, "chat-sidebar-channel-general");
    reach_first(&mut cx, &view);
    reach_first(&mut cx, &view);
    assert_eq!(held(&view).0, 33, "two pages above the first");
    open(&mut cx, "chat-sidebar-channel-random");
    assert_eq!(kept(&view, "general"), (11, 590, 600));
    view.read(|chat| assert!(chat.visited["general"].has_older));
}

#[test]
fn rows_streamed_into_a_kept_room_slide_its_page() {
    let head = Rc::new(Cell::new(600));
    let (mut cx, view) = history(head.clone());
    let changes = cx.host().stream::<Changes<::chat::Chat>>();
    let stream = |cx: &mut TestAppContext, rows: u64| {
        for _ in 0..rows {
            head.set(head.get() + 1);
            changes.send(block(head.get(), posted("general")));
            cx.run_until_parked();
        }
    };
    open(&mut cx, "chat-sidebar-channel-general");
    open(&mut cx, "chat-sidebar-channel-random");
    stream(&mut cx, 20);
    assert_eq!(
        kept(&view, "general"),
        (11, 590, 600),
        "nothing read while away"
    );
    let before = asked(&cx).len();
    open(&mut cx, "chat-sidebar-channel-general");
    let pages = asked(&cx)[before..]
        .iter()
        .filter(|query| matches!(query, Query::Roots { .. }))
        .count();
    assert_eq!(pages, 1, "one read of what came");
    stream(&mut cx, 30);
    open(&mut cx, "chat-sidebar-channel-random");
    assert_eq!(
        kept(&view, "general"),
        (11, 640, 650),
        "as many rows, the newest"
    );
}

#[test]
fn a_room_returned_to_opens_its_thread_where_it_stood() {
    let (mut cx, view) = paged(Rc::new(Cell::new(600)), 40);
    open(&mut cx, "chat-sidebar-channel-general");
    thread_open(&mut cx, &view, 595);
    stood(&mut cx, "chat-message-rows", 3, 12.);
    stood(&mut cx, "chat-thread-rows", 30, 7.);
    open(&mut cx, "chat-sidebar-channel-random");
    let before = asked(&cx).len();
    open(&mut cx, "chat-sidebar-channel-general");
    view.read(|chat| {
        let thread = chat.room.as_ref().unwrap().thread.as_ref().unwrap();
        assert_eq!(thread.root, 595);
        assert_eq!(
            thread.replies.ready().map(Vec::len),
            Some(11),
            "its newest page"
        );
        assert!(thread.earlier);
    });
    let reads = asked(&cx)[before..]
        .iter()
        .filter(|query| matches!(query, Query::Roots { .. } | Query::Thread { .. }))
        .count();
    assert_eq!(reads, 0, "drawn from what was kept");
    let at = |pane| {
        let top = state(&view, pane).logical_scroll_top();
        (top.item_ix, f32::from(top.offset_in_item))
    };
    assert_eq!(at(Pane::Timeline), (3, 12.), "the room's row");
    assert_eq!(
        at(Pane::Thread),
        (1, 7.),
        "the reply, under the root, now the kept page's first"
    );
}

#[test]
fn a_room_left_above_its_newest_page_lands_where_the_reader_stood() {
    let (mut cx, view) = history(Rc::new(Cell::new(600)));
    open(&mut cx, "chat-sidebar-channel-general");
    reach_first(&mut cx, &view);
    stood(&mut cx, "chat-message-rows", 4, 0.);
    open(&mut cx, "chat-sidebar-channel-random");
    open(&mut cx, "chat-sidebar-channel-general");
    view.read(|chat| {
        let room = chat.room.as_ref().unwrap();
        assert!(room.landed);
        assert_eq!(room.target, 583);
    });
}
