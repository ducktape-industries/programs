//! The timeline as a reader scrolls it: the list's rows through a re-read,
//! the paging at both ends of a landed window, and what a hover costs.
use super::*;
use crate::listing::{OVERDRAW, splices};
use std::cell::Cell;
use std::rc::Rc;

fn list_of(node: &wire::Node) -> Option<&wire::Node> {
    if matches!(node, wire::Node::List { .. }) {
        return Some(node);
    }
    node.children().iter().find_map(list_of)
}

fn timeline(cx: &TestAppContext) -> wire::Node {
    list_of(cx.find("chat-message-list").expect("the timeline"))
        .cloned()
        .expect("the timeline is a list")
}

fn window(cx: &TestAppContext) -> (String, usize, Vec<String>) {
    let wire::Node::List {
        id,
        range_start,
        children,
        ..
    } = timeline(cx)
    else {
        unreachable!()
    };
    let keys = children
        .iter()
        .map(|row| row.key().unwrap_or_default().to_owned())
        .collect();
    let wire::ElementIdWire::Name(id) = id else {
        panic!("the list is named")
    };
    (id.to_string(), range_start, keys)
}

fn commands(cx: &TestAppContext) -> Vec<wire::ListCommand> {
    let wire::Node::List { commands, .. } = timeline(cx) else {
        unreachable!()
    };
    commands
}

fn full_room(newest: Rc<Cell<u64>>) -> (TestAppContext, Entity<Chat>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Accounts { .. } => Reply::Accounts(page(Vec::new())),
                Query::Channels { .. } => {
                    Reply::Channels(page(vec![channel("general", "General", newest.get())]))
                }
                Query::Roots { .. } => {
                    let top = newest.get();
                    Reply::Roots(::chat::PageResponse {
                        height: 1,
                        items: (top + 1 - ROOM_ROWS as u64..=top).map(marked_row).collect(),
                        next: Some(vec![1]),
                    })
                }
                Query::Members { .. } => Reply::Members(page(Vec::new())),
                Query::Reactors { .. } => Reply::Reactors(Vec::new()),
                Query::PostedIn { .. } => Reply::PostedIn(page(Vec::new())),
                Query::Listening { .. } => Reply::Listening(page(Vec::new())),
                query => panic!("unexpected chat query: {query:?}"),
            })
        }));
    let view = cx.open::<Chat>();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    (cx, view)
}

// A pane 63 rows tall: a screenful of 64 rows, the page these fixtures' seqs are drawn at.
fn tall(cx: &mut TestAppContext) {
    let row = view::design::height::ROW as f32;
    cx.simulate_resize(view::testing::VIEWPORT.0, 63. * row);
}

#[test]
fn a_post_above_a_reader_scrolled_up_adds_its_row_alone() {
    let newest = Rc::new(Cell::new(ROOM_ROWS as u64));
    let (mut cx, _) = full_room(newest.clone());
    let changes = cx.host().stream::<Changes<::chat::Chat>>();
    cx.simulate_range("chat-message-list", 100..130);
    let (id, start, shown) = window(&cx);
    assert_eq!(id, "chat-message-rows");
    newest.set(ROOM_ROWS as u64 + 1);
    changes.send(block(4, posted("general")));
    cx.run_until_parked();
    assert_eq!(
        commands(&cx),
        [wire::ListCommand::Splice {
            start: ROOM_ROWS,
            end: ROOM_ROWS,
            count: 1,
        },]
    );
    let (_, after, kept) = window(&cx);
    assert_eq!(after, start, "the rows on screen stay where they were");
    assert_eq!(kept, shown, "the rows on screen are the rows it holds");
}

#[test]
fn an_older_page_lands_only_in_the_room_it_was_read_for() {
    let (mut cx, view) = full_room(Rc::new(Cell::new(ROOM_ROWS as u64)));
    let older = Rc::new(Cell::new(0));
    let paged = older.clone();
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Roots {
                    page: ::chat::PageRequest { after: None, .. },
                    ..
                } => Reply::Roots(::chat::PageResponse {
                    height: 1,
                    items: (1..=ROOM_ROWS as u64).map(marked_row).collect(),
                    next: Some(vec![1]),
                }),
                Query::Roots { .. } => {
                    paged.set(paged.get() + 1);
                    Reply::Roots(::chat::PageResponse {
                        height: 1,
                        items: vec![marked_row(1_000 + paged.get())],
                        next: (paged.get() < 4).then(|| vec![1]),
                    })
                }
                Query::Members { .. } => Reply::Members(page(Vec::new())),
                Query::Channels { .. } => Reply::Channels(page(Vec::new())),
                Query::Reactors { .. } => Reply::Reactors(Vec::new()),
                query => panic!("unexpected chat query: {query:?}"),
            })
        }));
    cx.update(&view, |chat, _, cx| {
        chat.load_older(cx);
        chat.open("dm-7-8".into(), cx);
        cx.notify();
    });
    cx.run_until_parked();
    view.read(|chat| {
        let room = chat.room.as_ref().unwrap();
        assert_eq!(room.id, "dm-7-8");
        assert_eq!(room.messages.ready().map(Vec::len), Some(ROOM_ROWS));
    });
    assert_eq!(older.get(), 4, "the older page was read through");
}

fn keys(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|key| (*key).to_owned()).collect()
}

#[test]
fn a_list_is_edited_by_its_keys() {
    let old = keys(&["a", "b", "c", "d", "e"]);
    let edits = |new: &[&str]| splices(&old, &keys(new));
    assert_eq!(edits(&["b", "c", "d", "e", "f"]), [(0..1, 0), (5..5, 1)]);
    assert_eq!(edits(&["c", "d", "e", "f", "g"]), [(0..2, 0), (5..5, 2)]);
    assert_eq!(edits(&["x", "y", "a", "b", "c", "d", "e"]), [(0..0, 2)]);
    assert_eq!(edits(&["a", "b", "c", "d", "e", "f"]), [(5..5, 1)]);
    assert_eq!(edits(&["a", "b", "d", "e"]), [(2..3, 0)]);
    assert_eq!(edits(&["a", "b", "c", "d", "e"]), []);
    assert_eq!(edits(&["v", "w", "x", "y", "z"]), [(0..5, 5)]);
    assert_eq!(
        edits(&["b", "x", "d", "e", "f"]),
        [(0..1, 0), (2..3, 1), (5..5, 1)]
    );
}

#[test]
fn a_list_changed_all_through_is_edited_to_its_new_keys() {
    let old: Vec<String> = (0..40).map(|at| at.to_string()).collect();
    let alternating: Vec<String> = old
        .iter()
        .enumerate()
        .map(|(at, key)| match at % 2 {
            0 => format!("x{key}"),
            _ => key.clone(),
        })
        .collect();
    let reversed: Vec<String> = old.iter().rev().cloned().collect();
    for new in [alternating, reversed] {
        let edits = splices(&old, &new);
        let mut rows = old.clone();
        let mut shift = 0isize;
        for (gone, count) in &edits {
            let at = gone.start.checked_add_signed(shift).unwrap();
            rows.splice(at..at + gone.len(), new[at..at + count].iter().cloned());
            shift += *count as isize - gone.len() as isize;
        }
        assert_eq!(rows, new, "{edits:?}");
    }
}

#[test]
fn the_timeline_is_laid_out_past_the_screen() {
    let (cx, _) = opened();
    let wire::Node::List { overdraw, .. } = timeline(&cx) else {
        unreachable!()
    };
    assert_eq!(overdraw, OVERDRAW);
    assert!(overdraw >= 600., "several notches of the wheel: {overdraw}");
}

#[test]
fn a_hover_folds_no_message_again() {
    let (mut cx, view) = opened();
    let folded = |view: &Entity<Chat>| {
        view.read(|chat| {
            chat.drawn(Pane::Timeline)
                .borrow()
                .folded
                .as_ref()
                .map(|folded| folded.messages.clone())
                .expect("the timeline folded its rows")
        })
    };
    let before = folded(&view);
    cx.simulate_hover("chat-message-m1-row", true);
    assert!(cx.find("chat-message-m1-actions").is_some());
    cx.simulate_hover("chat-message-m1-row", false);
    cx.simulate_hover("chat-message-m2-row", true);
    assert!(cx.find("chat-message-m2-actions").is_some());
    assert!(Rc::ptr_eq(&before, &folded(&view)), "no row moved");
    cx.update(&view, |chat, _, cx| {
        let rows = chat.room.as_mut().unwrap().messages.ready_mut().unwrap();
        rows[1].reactions.push(::chat::Reaction {
            emoji: "🎉".into(),
            count: 1,
            reacted_by_me: false,
        });
        cx.notify();
    });
    cx.run_until_parked();
    assert!(!Rc::ptr_eq(&before, &folded(&view)), "a reaction moved m2");
    assert!(cx.has_text("🎉"), "{:?}", cx.texts());
    let after = folded(&view);
    assert_eq!(before.len(), after.len());
    let moved: Vec<u64> = (before.iter().zip(after.iter()))
        .filter(|(was, now)| was != now)
        .map(|(_, now)| now.seq)
        .collect();
    assert_eq!(moved, [2], "the row it moved, folded again alone");
}

#[test]
fn jump_to_latest_shows_as_the_reader_leaves_the_tail() {
    let (mut cx, _) = opened();
    assert!(cx.find("chat-jump-latest").is_none());
    let wire::Node::List {
        scroll_handler: Some(handler),
        item_count,
        ..
    } = timeline(&cx)
    else {
        panic!("the timeline hears its scroll")
    };
    let scrolled = |end: usize, tail: bool| wire::Event::ListScroll {
        handler,
        event: wire::ListScroll {
            visible_start: 0,
            visible_end: end,
            count: item_count,
            is_scrolled: !tail,
            is_following_tail: tail,
            offset: wire::ListOffset::default(),
        },
    };
    cx.simulate_event(scrolled(item_count - 1, false));
    assert!(cx.find("chat-jump-latest").is_some());
    cx.simulate_event(scrolled(item_count, true));
    assert!(cx.find("chat-jump-latest").is_none());
}

fn long_room(head: Rc<Cell<u64>>) -> (TestAppContext, Entity<Chat>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            let head = head.get();
            Ok(match query {
                Query::Accounts { .. } => Reply::Accounts(page(Vec::new())),
                Query::Channels { .. } => {
                    Reply::Channels(page(vec![channel("general", "General", head)]))
                }
                Query::MessagesAround { seq, page, .. } => {
                    let half = page.limit() / 2;
                    let lo = seq.saturating_sub(half).max(1);
                    let hi = seq.saturating_add(half).min(head);
                    Reply::Messages((lo..=hi).map(marked_row).collect())
                }
                Query::Roots { .. } => Reply::Roots(::chat::PageResponse {
                    height: 1,
                    items: (head + 1 - ROOM_ROWS as u64..=head)
                        .map(marked_row)
                        .collect(),
                    next: Some(vec![1]),
                }),
                Query::Members { .. } => Reply::Members(page(Vec::new())),
                Query::Reactors { .. } => Reply::Reactors(Vec::new()),
                Query::PostedIn { .. } => Reply::PostedIn(page(Vec::new())),
                Query::Listening { .. } => Reply::Listening(page(Vec::new())),
                query => panic!("unexpected chat query: {query:?}"),
            })
        }));
    tall(&mut cx);
    let view = cx.open::<Chat>();
    seat(&mut cx);
    (cx, view)
}

fn seqs(view: &Entity<Chat>) -> (u64, u64, usize) {
    view.read(|chat| {
        let rows = chat.room.as_ref().unwrap().messages.ready().unwrap();
        (rows[0].seq, rows[rows.len() - 1].seq, rows.len())
    })
}

#[test]
fn a_landed_room_opens_on_its_message_and_pages_both_ways() {
    let head = Rc::new(Cell::new(1_000));
    let (mut cx, view) = long_room(head.clone());
    let routes = cx.host().stream::<view::methods::HostRoute>();
    routes.send("general/500".into());
    cx.run_until_parked();
    assert_eq!(seqs(&view), (436, 564, 129), "64 either side of 500");
    let (id, _, _) = window(&cx);
    assert_eq!(id, "chat-landed-rows");
    assert!(
        commands(&cx).contains(&wire::ListCommand::ScrollTo(wire::ListOffset {
            item_ix: 500 - 436,
            offset_in_item: 0.,
        })),
        "{:?}",
        commands(&cx)
    );
    assert!(
        !cx.has_text("Load older messages"),
        "the list reads older rows itself"
    );

    let wire::Node::List { item_count, .. } = timeline(&cx) else {
        unreachable!()
    };
    cx.simulate_range("chat-message-list", item_count - 10..item_count);
    assert_eq!(seqs(&view), (436, 629, 194), "a page below the last row");
    reach_first(&mut cx, &view);
    assert_eq!(
        seqs(&view),
        (371, 629, 259),
        "a page above, the newer end kept"
    );
    view.read(|chat| assert!(chat.room.as_ref().unwrap().landed));

    for _ in 0..8 {
        let wire::Node::List { item_count, .. } = timeline(&cx) else {
            unreachable!()
        };
        cx.simulate_range("chat-message-list", item_count - 10..item_count);
    }
    view.read(|chat| {
        let room = chat.room.as_ref().unwrap();
        assert!(!room.landed, "read down to the head, the room is live");
        assert!(room.at_tail);
    });
    let (id, _, _) = window(&cx);
    assert_eq!(id, "chat-message-rows", "kept on the newest");
    let (_, last, _) = seqs(&view);
    assert_eq!(last, 1_000);
    assert!(cx.find("chat-jump-latest").is_none());
}

fn message_reads(cx: &TestAppContext) -> Vec<Query> {
    let asked = cx.host().requests::<Ask<::chat::Chat>>().into_iter();
    asked
        .filter(|query| {
            matches!(
                query,
                Query::Roots { .. } | Query::MessagesAround { .. } | Query::Thread { .. }
            )
        })
        .collect()
}

#[test]
fn latest_in_a_landed_room_reads_only_the_rows_past_the_newest_it_holds() {
    let (mut cx, view) = long_room(Rc::new(Cell::new(600)));
    open_link(&mut cx, 500);
    assert_eq!(seqs(&view), (436, 564, 129));
    let before = message_reads(&cx).len();
    cx.simulate_click("chat-jump-latest-button");
    cx.run_until_parked();
    let asked = message_reads(&cx).split_off(before);
    let [Query::MessagesAround { seq, page, .. }] = asked.as_slice() else {
        panic!("one read of the rows past 564, not the room again: {asked:?}")
    };
    let half = page.limit() / 2;
    assert_eq!(seq - half, 565, "no row the view holds is read again");
    assert!(seq + half >= 600, "the rows up to the head are read");
    assert_eq!(
        seqs(&view),
        (436, 600, 165),
        "the held rows stay and the newer ones join them"
    );
    view.read(|chat| {
        let room = chat.room.as_ref().unwrap();
        assert!(!room.landed && room.at_tail, "the room is live at its end");
    });
    let (id, _, _) = window(&cx);
    assert_eq!(id, "chat-message-rows");
    assert!(cx.find("chat-jump-latest").is_none());
}

#[test]
fn latest_past_more_than_a_screenful_reads_the_newest_screenful_alone_and_lands_at_the_head() {
    let (mut cx, view) = long_room(Rc::new(Cell::new(1_000)));
    open_link(&mut cx, 100);
    assert_eq!(
        seqs(&view),
        (36, 164, 129),
        "a screenful of 64 either side of 100"
    );
    let before = message_reads(&cx).len();
    cx.simulate_click("chat-jump-latest-button");
    cx.run_until_parked();
    let asked = message_reads(&cx).split_off(before);
    let [Query::MessagesAround { page, .. }] = asked.as_slice() else {
        panic!("one read: {asked:?}")
    };
    assert!(
        page.limit() <= 64,
        "no read past a screenful of the gap: {page:?}"
    );
    let (first, last, held) = seqs(&view);
    assert_eq!(last, 1_000, "the room lands at its head");
    assert!(
        first > 164 && held <= 65,
        "the gap stays unread: {first}..{last}, {held} rows"
    );
    view.read(|chat| {
        let room = chat.room.as_ref().unwrap();
        assert!(!room.landed && room.at_tail && room.has_older);
    });
    assert!(cx.find("chat-jump-latest").is_none());
}

#[test]
fn latest_in_a_live_room_scrolled_up_reads_nothing_and_goes_to_its_end() {
    let (mut cx, view) = full_room(Rc::new(Cell::new(ROOM_ROWS as u64)));
    let wire::Node::List {
        scroll_handler: Some(handler),
        item_count,
        ..
    } = timeline(&cx)
    else {
        panic!("the timeline hears its scroll")
    };
    cx.simulate_event(wire::Event::ListScroll {
        handler,
        event: wire::ListScroll {
            visible_start: 100,
            visible_end: 130,
            count: item_count,
            is_scrolled: true,
            is_following_tail: false,
            offset: wire::ListOffset::default(),
        },
    });
    assert!(cx.find("chat-jump-latest").is_some(), "scrolled up");
    let held = seqs(&view);
    let before = message_reads(&cx).len();
    cx.simulate_click("chat-jump-latest-button");
    cx.run_until_parked();
    let asked = message_reads(&cx).split_off(before);
    assert!(
        asked.is_empty(),
        "the view holds the newest rows, and read {asked:?}"
    );
    assert_eq!(seqs(&view), held, "the rows it holds stay");
    view.read(|chat| assert!(chat.room.as_ref().unwrap().at_tail));
    assert!(cx.find("chat-jump-latest").is_none());
}

const SEEDED_HEAD: u64 = 49;

fn seeded_room(listed: Option<u64>) -> (TestAppContext, Entity<Chat>) {
    let message = |seq: u64| MsgRow {
        thread: seq.is_multiple_of(3).then_some(1),
        height: seq,
        ..row(seq, 7 + seq % 2, &format!("row {seq}"))
    };
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Accounts { .. } => Reply::Accounts(page(Vec::new())),
                Query::Channels { .. } => {
                    let Some(head) = listed else {
                        return Err(wire::Error::new("unavailable", "the node is busy"));
                    };
                    Reply::Channels(page(vec![channel("general", "General", head)]))
                }
                Query::MessagesAround { seq, page, .. } => {
                    let half = page.limit() / 2;
                    let lo = seq.saturating_sub(half).max(1);
                    let hi = seq.saturating_add(half).min(SEEDED_HEAD);
                    Reply::Messages((lo..=hi).map(message).collect())
                }
                Query::Roots { .. } => Reply::Roots(::chat::PageResponse {
                    height: 1,
                    items: (1..=SEEDED_HEAD)
                        .filter(|seq| !seq.is_multiple_of(3))
                        .map(message)
                        .collect(),
                    next: None,
                }),
                Query::Members { .. } => Reply::Members(page(Vec::new())),
                Query::PostedIn { .. } => Reply::PostedIn(page(Vec::new())),
                Query::Listening { .. } => Reply::Listening(page(Vec::new())),
                query => panic!("unexpected chat query: {query:?}"),
            })
        }));
    tall(&mut cx);
    let view = cx.open::<Chat>();
    (cx, view)
}

fn open_link(cx: &mut TestAppContext, seq: u64) {
    cx.host()
        .stream::<view::methods::HostRoute>()
        .send(format!("general/{seq}"));
    cx.run_until_parked();
}

#[test]
fn a_link_near_the_end_lands_on_its_message_with_the_rows_above_it() {
    let (mut cx, view) = seeded_room(Some(SEEDED_HEAD));
    seat(&mut cx);
    open_link(&mut cx, SEEDED_HEAD - 2);
    view.read(|chat| assert!(chat.room.as_ref().unwrap().landed));
    let target = (1..=SEEDED_HEAD - 2)
        .filter(|seq| !seq.is_multiple_of(3))
        .count();
    let wire::Node::List {
        item_count,
        range_start,
        children,
        commands,
        ..
    } = timeline(&cx)
    else {
        unreachable!()
    };
    assert_eq!(item_count, target + 2);
    assert!(
        commands.contains(&wire::ListCommand::ScrollTo(wire::ListOffset {
            item_ix: target,
            offset_in_item: 0.,
        })),
        "{commands:?}"
    );
    assert!(
        range_start < target && range_start + children.len() == item_count,
        "the frame carries rows {range_start}..{} of {item_count}",
        range_start + children.len()
    );
}

#[test]
fn a_link_to_the_newest_message_opens_the_room_at_its_head() {
    for seated_first in [true, false] {
        let (mut cx, view) = seeded_room(Some(SEEDED_HEAD));
        if seated_first {
            seat(&mut cx);
            open_link(&mut cx, SEEDED_HEAD);
        } else {
            open_link(&mut cx, SEEDED_HEAD);
            seat(&mut cx);
        }
        at_its_head(&cx, &view);
    }
    let (mut cx, view) = seeded_room(None);
    seat(&mut cx);
    open_link(&mut cx, SEEDED_HEAD);
    view.read(|chat| assert!(chat.room.as_ref().unwrap().landed, "no list yet"));
    cx.update(&view, |chat, _, cx| {
        chat.channels_arrived(vec![channel("general", "General", SEEDED_HEAD)], cx);
    });
    cx.run_until_parked();
    at_its_head(&cx, &view);
    let (mut cx, view) = seeded_room(Some(SEEDED_HEAD + 1));
    seat(&mut cx);
    open_link(&mut cx, SEEDED_HEAD);
    view.read(|chat| assert!(chat.room.as_ref().unwrap().landed));
}

fn at_its_head(cx: &TestAppContext, view: &Entity<Chat>) {
    view.read(|chat| {
        let room = chat.room.as_ref().unwrap();
        assert!(!room.landed, "the window holds the head: not a landing");
        assert!(room.at_tail);
    });
    let (id, _, _) = window(cx);
    assert_eq!(id, "chat-message-rows", "the room at its head");
    assert!(cx.find("chat-jump-latest").is_none());
}

fn threaded_room(
    head: u64,
    thread: impl Fn(u64) -> Option<u64> + 'static,
) -> (TestAppContext, Entity<Chat>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    threaded(&cx, head, thread);
    tall(&mut cx);
    let view = cx.open::<Chat>();
    seat(&mut cx);
    (cx, view)
}

fn past_100(seq: u64) -> Option<u64> {
    (seq > 100).then_some(100)
}

fn read_to_the_foot(cx: &mut TestAppContext, seq: u64) {
    open_link(cx, seq);
    let wire::Node::List { item_count, .. } = timeline(cx) else {
        unreachable!()
    };
    cx.simulate_range("chat-message-list", item_count - 10..item_count);
}

#[test]
fn newer_pages_of_replies_alone_read_on_to_the_head() {
    let (mut cx, view) = threaded_room(1_100, past_100);
    read_to_the_foot(&mut cx, 60);
    at_its_head(&cx, &view);
    assert_eq!(seqs(&view), (1, 124, 124), "the window around 60, whole");
}

#[test]
fn roots_past_a_run_of_replies_join_the_rows_the_reader_is_on() {
    let (mut cx, view) = threaded_room(510, |seq| (101..=500).contains(&seq).then_some(100));
    read_to_the_foot(&mut cx, 60);
    view.read(|chat| assert!(chat.room.as_ref().unwrap().landed));
    assert_eq!(
        seqs(&view),
        (1, 510, 185),
        "the window around 60, then the page past the run"
    );
    let wire::Node::List { item_count, .. } = timeline(&cx) else {
        unreachable!()
    };
    cx.simulate_range("chat-message-list", item_count - 10..item_count);
    at_its_head(&cx, &view);
}

#[test]
fn an_older_page_of_replies_keeps_the_rows_the_reader_is_on() {
    let (mut cx, view) = threaded_room(600, |seq| (11..=500).contains(&seq).then_some(10));
    open_link(&mut cx, 550);
    assert_eq!(seqs(&view), (486, 600, 115));
    reach_first(&mut cx, &view);
    assert_eq!(seqs(&view), (421, 600, 180), "a page above, the rest kept");
}

#[test]
fn older_pages_keep_the_newer_end() {
    let (mut cx, view) = threaded_room(700, |seq| (301..=600).contains(&seq).then_some(300));
    read_to_the_foot(&mut cx, 280);
    view.read(|chat| assert!(chat.room.as_ref().unwrap().landed));
    let newest = seqs(&view).1;
    for _ in 0..3 {
        reach_first(&mut cx, &view);
    }
    assert_eq!(seqs(&view).1, newest, "the newer end stays");
    view.read(|chat| {
        let rows = chat.room.as_ref().unwrap().messages.ready().unwrap();
        assert!(
            rows.iter().any(|row| row.seq == 300),
            "root 300 held throughout"
        );
    });
}

#[test]
fn a_link_opened_as_a_block_lands_opens_the_room_at_its_head() {
    let (mut cx, view) = threaded_room(120, past_100);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    cx.host()
        .stream::<Changes<::chat::Chat>>()
        .send(block(4, posted("general")));
    open_link(&mut cx, 120);
    at_its_head(&cx, &view);
    assert_eq!(seqs(&view), (56, 120, 65), "the window around 120");
}

fn a_hundred_replies(seq: u64) -> Option<u64> {
    (101..=200).contains(&seq).then_some(100)
}

#[test]
fn a_link_to_a_reply_opens_its_thread_on_it_and_the_room_on_its_root() {
    let (mut cx, view) = threaded_room(300, a_hundred_replies);
    open_link(&mut cx, 150);
    assert_eq!(seqs(&view), (86, 214, 129), "the window around 150");
    view.read(|chat| {
        let room = chat.room.as_ref().unwrap();
        assert!(room.landed);
        assert_eq!(room.thread.as_ref().map(|thread| thread.root), Some(100));
    });
    assert!(cx.find("chat-thread-pane").is_some());
    assert_eq!(scrolled_to(&view, Pane::Timeline), Some(14));
    assert_eq!(scrolled_to(&view, Pane::Thread), Some(50));
}

#[test]
fn a_link_to_a_reply_whose_root_is_off_its_window_lands_on_the_root() {
    let (mut cx, view) = threaded_room(1_000, |seq| (11..=900).contains(&seq).then_some(10));
    open_link(&mut cx, 850);
    assert_eq!(seqs(&view), (1, 74, 74), "the window around the root");
    view.read(|chat| {
        let room = chat.room.as_ref().unwrap();
        assert_eq!(room.target, 10);
        let thread = room.thread.as_ref().unwrap();
        assert_eq!(thread.root, 10);
        let replies = thread.replies.ready().unwrap();
        assert_eq!(replies.last().map(|row| row.seq), Some(900));
        assert!(!thread.has_more);
    });
    assert_eq!(scrolled_to(&view, Pane::Timeline), Some(10));
    assert_eq!(scrolled_to(&view, Pane::Thread), Some(840));
}

#[test]
fn a_link_to_a_root_closes_the_thread_beside_the_room() {
    let (mut cx, view) = threaded_room(300, a_hundred_replies);
    open_link(&mut cx, 150);
    assert!(cx.find("chat-thread-pane").is_some());
    open_link(&mut cx, 250);
    view.read(|chat| {
        let room = chat.room.as_ref().unwrap();
        assert!(room.thread.is_none());
        assert_eq!(room.target, 250);
    });
    assert!(cx.find("chat-thread-pane").is_none());
    assert_eq!(scrolled_to(&view, Pane::Timeline), Some(49));
}

#[test]
#[ignore = "measurement: prints a hover's render time over a full room, asserts nothing of it"]
fn a_hover_over_a_full_room_measured() {
    for shown in [12, 40, 64] {
        let (mut cx, _) = full_room(Rc::new(Cell::new(ROOM_ROWS as u64)));
        cx.simulate_range("chat-message-list", ROOM_ROWS - shown..ROOM_ROWS);
        let hovers = 200;
        let clock = std::time::Instant::now();
        for at in 0..hovers {
            let seq = ROOM_ROWS as u64 - (at % 10) as u64;
            cx.simulate_hover(&format!("chat-message-m{seq}-row"), true);
        }
        let each = clock.elapsed() / hovers as u32;
        let (_, start, rows) = window(&cx);
        eprintln!(
            "a hover over a {ROOM_ROWS}-row room, {} rows lowered from {start}: {each:?} per hover",
            rows.len()
        );
    }
}
