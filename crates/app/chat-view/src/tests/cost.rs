//! What a tick costs in a full room, per input a reader makes: renders,
//! nodes lowered and changed, patches, bytes and time, printed as a table
//! (`cargo test --release -p chat-view cost -- --ignored --nocapture`).
use super::*;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

const ROOMS: usize = 30;

fn busy_room(newest: Rc<Cell<u64>>) -> (TestAppContext, Entity<Chat>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            let top = newest.get();
            Ok(match query {
                Query::Channels { .. } => {
                    let mut rooms = vec![channel("general", "General", top)];
                    let named = |n: usize| channel(&format!("room-{n}"), &format!("Room {n}"), 1);
                    rooms.extend((0..ROOMS).map(named));
                    Reply::Channels(page(rooms))
                }
                Query::Roots { channel_id, .. } if channel_id == "general" => {
                    Reply::Roots(::chat::PageResponse {
                        height: 1,
                        items: (top + 1 - WINDOW as u64..=top).map(marked_row).collect(),
                        next: Some(vec![1]),
                    })
                }
                query => chat_reply(query),
            })
        }));
    cx.simulate_resize(1200., 800.);
    let view = cx.open::<Chat>();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    cx.simulate_range("chat-message-list", WINDOW - 20..WINDOW);
    (cx, view)
}

#[derive(Default)]
struct Cost {
    runs: u32,
    ticks: usize,
    renders: usize,
    nodes: usize,
    lowered: usize,
    changed: usize,
    patches: usize,
    bytes: usize,
    render: Duration,
    lower: Duration,
    diff: Duration,
    encode: Duration,
}

impl Cost {
    fn row(&self, input: &str) -> String {
        let runs = self.runs as usize;
        let each = |time: Duration| format!("{:.0}", time.as_secs_f64() * 1e6 / runs as f64);
        format!(
            "| {input} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            self.ticks / runs,
            self.renders / runs,
            self.nodes / runs,
            self.lowered / runs,
            self.changed / runs,
            self.patches / runs,
            self.bytes / runs,
            each(self.render),
            each(self.lower),
            each(self.diff),
            each(self.encode),
        )
    }
}

fn changed(before: &wire::Node, after: &wire::Node) -> usize {
    let (mut before, mut after) = (before.clone(), after.clone());
    wire::diff(&mut before, &mut after)
        .iter()
        .map(|patch| match patch {
            wire::Patch::Replace { node, .. } | wire::Patch::Insert { node, .. } => node.count(),
            _ => 1,
        })
        .sum()
}

fn measure(
    cx: &mut TestAppContext,
    runs: u32,
    mut input: impl FnMut(&mut TestAppContext, u32),
) -> Cost {
    let mut cost = Cost {
        runs,
        ..Cost::default()
    };
    for run in 0..runs {
        let before = cx.root().clone();
        input(cx, run);
        for tick in cx.reports() {
            cost.ticks += 1;
            cost.renders += usize::from(tick.rendered);
            cost.lowered += tick.lowered;
            cost.patches += tick.patches;
            cost.bytes += tick.bytes;
            cost.render += tick.render;
            cost.lower += tick.lower;
            cost.diff += tick.diff;
            cost.encode += tick.encode;
        }
        cost.nodes += cx.root().count();
        cost.changed += changed(&before, cx.root());
    }
    cost
}

#[test]
fn a_hover_keeps_every_row_it_did_not_touch() {
    let (mut cx, _) = busy_room(Rc::new(Cell::new(WINDOW as u64)));
    cx.memo_checks(false);
    cx.simulate_hover("chat-message-m250-row", true);
    cx.simulate_hover("chat-message-m250-row", false);
    let seq = WINDOW;
    cx.simulate_hover(&format!("chat-message-m{seq}-row"), true);
    let tick = cx.reports()[0];
    assert!(tick.rendered);
    let hovered = format!("chat-message-m{seq}-");
    let timeline = list_in(cx.node("chat-message-list")).expect("the timeline");
    let untouched = timeline
        .children()
        .iter()
        .filter(|row| !row.key().unwrap_or_default().starts_with(&hovered));
    let mut kept: usize = untouched.map(|row| row.count()).sum();
    let rooms = cx.node("chat-sidebar-rooms-list").children().iter();
    let room = |row: &&wire::Node| {
        let key = row.key().unwrap_or_default();
        key.starts_with("chat-sidebar-channel-") || key.starts_with("chat-sidebar-dm-")
    };
    kept += rooms.filter(room).map(|row| row.count()).sum::<usize>();
    let whole = cx.root().count();
    let anew = cx.memos_lowered();
    assert_eq!(anew, [format!("chat-message-m{seq}-drawn")], "lowered anew");
    assert!(
        kept > whole / 2,
        "the rows are most of the tree: {kept} of {whole}"
    );
    assert!(
        tick.lowered < whole / 4,
        "{} lowered, {kept} of {whole} kept",
        tick.lowered
    );
}

fn list_in(node: &wire::Node) -> Option<&wire::Node> {
    match node {
        wire::Node::List { .. } => Some(node),
        node => node.children().iter().find_map(list_in),
    }
}

#[test]
#[ignore = "measurement: prints what each input costs a full room, asserts nothing of it"]
fn a_full_room_measured() {
    const RUNS: u32 = 40;
    let newest = Rc::new(Cell::new(WINDOW as u64));
    let (mut cx, view) = busy_room(newest.clone());
    cx.memo_checks(false);
    let changes = cx.host().stream::<Changes<::chat::Chat>>();
    let mut rows = vec![
        "| input | ticks | renders | nodes in tree | nodes lowered | nodes changed | patches | bytes sent | render µs | lower µs | diff µs | encode µs |".to_owned(),
        "|---|---|---|---|---|---|---|---|---|---|---|---|".to_owned(),
    ];
    let top = WINDOW as u64;
    let cost = measure(&mut cx, RUNS, |cx, run| {
        let seq = top - u64::from(run / 2 % 10);
        cx.simulate_hover(&format!("chat-message-m{seq}-row"), run % 2 == 0);
    });
    rows.push(cost.row("hover a row in / out"));
    let cost = measure(&mut cx, RUNS, |cx, run| {
        let text = "x".repeat(run as usize + 1);
        cx.simulate_input("draft-general/editor", &text);
    });
    rows.push(cost.row("type a letter"));
    let cost = measure(&mut cx, RUNS, |cx, run| {
        let seq = top - u64::from(run % 10);
        cx.update(&view, |chat, _, cx| {
            let rows = chat.room.as_mut().unwrap().messages.ready_mut().unwrap();
            let row = rows.iter_mut().find(|row| row.seq == seq).unwrap();
            row.reactions[0].count += 1;
            cx.notify();
        });
        cx.run_until_parked();
    });
    rows.push(cost.row("a reaction on one row"));
    let cost = measure(&mut cx, RUNS, |cx, run| {
        let end = WINDOW - (run % 2) as usize;
        cx.simulate_range("chat-message-list", end - 20..end);
    });
    rows.push(cost.row("scroll by one row"));
    let cost = measure(&mut cx, RUNS / 4, |cx, run| {
        let room = match run % 2 {
            0 => "chat-sidebar-channel-room-0",
            _ => "chat-sidebar-channel-general",
        };
        cx.simulate_click(room);
    });
    rows.push(cost.row("open another room"));
    let cost = measure(&mut cx, RUNS / 4, |cx, run| cx.simulate_theme(run % 2 == 0));
    rows.push(cost.row("switch the theme"));
    let mut height = 10;
    let cost = measure(&mut cx, RUNS, |cx, _| {
        height += 1;
        changes.send(block(height, posted("general")));
        cx.run_until_parked();
    });
    rows.push(cost.row("a block that changes nothing"));
    let cost = measure(&mut cx, RUNS, |cx, _| {
        newest.set(newest.get() + 1);
        height += 1;
        changes.send(block(height, posted("general")));
        cx.run_until_parked();
    });
    rows.push(cost.row("a message lands"));
    eprintln!("{}", rows.join("\n"));
    fn regions(node: &wire::Node, depth: usize, out: &mut Vec<String>) {
        if node.count() <= 20 || depth > 8 {
            return;
        }
        if let Some(key) = node.key() {
            out.push(format!("{}{key}: {}", "  ".repeat(depth), node.count()));
        }
        for child in node.children() {
            regions(child, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    regions(cx.root(), 0, &mut out);
    eprintln!("{}", out.join("\n"));
}
