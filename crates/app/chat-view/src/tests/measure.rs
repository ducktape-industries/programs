//! What a session of rooms keeps on a phone's pane and what opening one again reads:
//! `CHAT_MEASURE=1` prints each room's kept rows and bytes, the total, and the reads.
use super::*;
use crate::composer::Place;
use std::cell::Cell;
use std::rc::Rc;

const ROOMS: u64 = 8;
const HEAD: u64 = 2_000;
const REPLIES: u64 = 40;

fn past(after: &[u8]) -> u64 {
    match <[u8; 8]>::try_from(after) {
        Ok(seq) => u64::from_le_bytes(seq),
        Err(_) => {
            let tail: [u8; 8] = after[after.len() - 8..].try_into().expect("a chat cursor");
            u64::from_be_bytes(tail)
        }
    }
}

#[test]
fn a_session_of_rooms_measured() {
    if std::env::var_os("CHAT_MEASURE").is_none() {
        return;
    }
    let served = Rc::new(Cell::new(0usize));
    let counted = served.clone();
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Channels { .. } => Reply::Channels(page(
                    (0..=ROOMS)
                        .map(|n| {
                            let head = if n == ROOMS { 0 } else { HEAD };
                            channel(&format!("room-{n}"), &format!("Room {n}"), head)
                        })
                        .collect(),
                )),
                Query::Roots {
                    channel_id,
                    page: asked,
                    ..
                } => {
                    let empty = channel_id == format!("room-{ROOMS}");
                    let top = match asked.after.as_deref() {
                        Some(after) => u64::MAX - past(after) - 1,
                        None if empty => 0,
                        None => HEAD,
                    };
                    let limit = asked.limit().min(top);
                    counted.set(counted.get() + limit as usize);
                    let oldest = top + 1 - limit;
                    let room = channel_id.clone();
                    let items = (oldest..=top).rev().map(|seq| MsgRow {
                        channel_id: room.clone(),
                        ..marked_row(seq)
                    });
                    Reply::Roots(::chat::PageResponse {
                        height: 1,
                        items: items.collect(),
                        next: (limit < top).then(|| chat::roots_below(&channel_id, oldest)),
                    })
                }
                Query::Thread {
                    channel_id,
                    root_seq,
                    page: asked,
                    ..
                } => {
                    let from = asked.after.as_deref().map_or(0, past);
                    let mut since = (1..=REPLIES)
                        .map(|reply| 100_000 + reply)
                        .filter(|seq| *seq > from);
                    let items: Vec<MsgRow> = since
                        .by_ref()
                        .take(asked.limit() as usize)
                        .map(|seq| MsgRow {
                            channel_id: channel_id.clone(),
                            thread: Some(root_seq),
                            ..row(seq, 8, "a reply that says a sentence or two, as replies do")
                        })
                        .collect();
                    counted.set(counted.get() + items.len());
                    let more = since.next().is_some();
                    let last = items.last().map_or(from, |reply| reply.seq);
                    Reply::Thread {
                        root: Some(marked_row(root_seq)),
                        replies: ::chat::PageResponse {
                            height: 1,
                            items,
                            next: more.then(|| last.to_le_bytes().to_vec()),
                        },
                    }
                }
                Query::MessagesAround { .. } => Reply::Messages(Vec::new()),
                query => chat_reply(query),
            })
        }));
    let view = cx.open::<Chat>();
    cx.run_until_parked();
    cx.simulate_touch(true);
    cx.simulate_resize(393., 852.);
    seat(&mut cx);
    let pane = Place {
        x: 0.,
        y: 0.,
        width: 393.,
        height: 852.,
    };
    cx.update(&view, |chat, _, cx| {
        chat.layout.pane = Some(pane);
        cx.notify();
    });
    let choose = |cx: &mut TestAppContext, room: String| {
        cx.update(&view, |chat, _, cx| {
            chat.choose(room, cx);
            cx.notify();
        });
        cx.run_until_parked();
    };
    for n in 0..ROOMS {
        choose(&mut cx, format!("room-{n}"));
        for _ in 0..4 {
            cx.update(&view, |chat, _, cx| {
                chat.load_older(cx);
                cx.notify();
            });
            cx.run_until_parked();
        }
        cx.update(&view, |chat, _, cx| {
            chat.open_thread(HEAD - 3, cx);
            cx.notify();
        });
        cx.run_until_parked();
    }
    choose(&mut cx, format!("room-{ROOMS}"));
    let bytes = |rows: &[&MsgRow]| -> usize {
        let each = rows.iter().map(|row| borsh::to_vec(*row).unwrap().len());
        each.sum()
    };
    let total = view.read(|chat| {
        let mut total = 0;
        for n in 0..ROOMS {
            let kept = &chat.visited[&format!("room-{n}")];
            let rows: Vec<&MsgRow> = kept.rows.iter().collect();
            let replies: Vec<&MsgRow> = kept.thread.iter().flat_map(|thread| &thread.replies).collect();
            let reactors = borsh::to_vec(&kept.reactors).unwrap().len();
            let room = bytes(&rows) + bytes(&replies) + reactors;
            total += room;
            eprintln!(
                "measure: room-{n} keeps {} rows, {} replies, {reactors} bytes of who reacted: {room} bytes",
                rows.len(),
                replies.len()
            );
        }
        total
    });
    eprintln!(
        "measure: {ROOMS} rooms keep {total} bytes, {} a room",
        total / ROOMS as usize
    );
    let (queries, read) = (asked_of(&cx), served.get());
    cx.update(&view, |chat, _, cx| {
        chat.choose("room-3".into(), cx);
        cx.notify();
    });
    let shown = view.read(|chat| {
        let room = chat.room.as_ref().unwrap();
        room.messages.ready().map_or(0, Vec::len)
    });
    cx.run_until_parked();
    eprintln!(
        "measure: back in room-3: {} reads, {} rows read, {shown} rows shown at once",
        asked_of(&cx) - queries,
        served.get() - read
    );
}

fn asked_of(cx: &TestAppContext) -> usize {
    cx.host().requests::<Ask<::chat::Chat>>().len()
}
