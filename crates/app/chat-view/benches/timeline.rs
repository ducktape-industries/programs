use std::hint::black_box;

use chat::{Block, MsgRow, Principal, Reaction};
use chat_view::{Chat, Room};
use criterion::{
    BatchSize, BenchmarkId, Criterion, SamplingMode, Throughput, criterion_group, criterion_main,
};
use serde_json::json;
use view::methods::Query as Ask;
use view::testing::TestAppContext;

fn snapshot(count: u64) -> serde_json::Value {
    let rows: Vec<_> = (1..=count)
        .map(|seq| {
            let text = if seq.is_multiple_of(5) {
                "The timeline should keep the discussion readable while new messages arrive. Here are the details of the latest review, including the changes to paging and the next steps. ".repeat(12)
            } else {
                format!("Message {seq}: thanks, I will check the latest changes.")
            };
            MsgRow {
                channel_id: "general".into(),
                seq,
                message_id: format!("m{seq}"),
                height: seq,
                time: 1_700_000_000_000 + seq * 60_000,
                blocks: vec![Block::paragraph(&text)],
                text,
                reply_count: if seq.is_multiple_of(11) { 3 } else { 0 },
                last_reply_seq: seq.is_multiple_of(11).then_some(seq + 1),
                reactions: if seq.is_multiple_of(7) {
                    vec![Reaction { emoji: "👍".into(), count: 3, reacted_by_me: false }]
                } else {
                    Vec::new()
                },
                ..MsgRow::by(Principal::Account(7 + (seq / 3) % 8))
            }
        })
        .collect();
    let mut room = serde_json::to_value(Room::default()).unwrap();
    room["id"] = json!("general");
    room["messages"] = json!({"ready": rows});
    room["members"] = json!({"ready": []});
    room["at_tail"] = json!(true);
    room["reaches_head"] = json!(true);
    let mut state = serde_json::to_value(Chat::default()).unwrap();
    state["session"]["connected"] = json!(true);
    state["room"] = room;
    state["channels"] = json!({"ready": []});
    state
}

fn context(snapshot: &serde_json::Value) -> TestAppContext {
    let mut cx = TestAppContext::new();
    cx.host().handle::<Ask<identity::Identity>>(|_| {
        Ok(identity::Reply::Accounts(identity::PageResponse {
            height: 1,
            items: Vec::new(),
            next: None,
        }))
    });
    cx.host().handle::<Ask<chat::Chat>>(|query| match query {
        chat::Query::Channels { .. } => Ok(chat::Reply::Channels(chat::PageResponse {
            height: 1,
            items: Vec::new(),
            next: None,
        })),
        chat::Query::Accounts { .. } => Ok(chat::Reply::Accounts(chat::PageResponse {
            height: 1,
            items: Vec::new(),
            next: None,
        })),
        query => panic!("unexpected query: {query:?}"),
    });
    let entity = cx.open::<Chat>();
    cx.run_until_parked();
    let state = serde_json::from_value(snapshot.clone()).unwrap();
    cx.update(&entity, |chat, _, cx| {
        *chat = state;
        cx.notify();
    });
    cx
}

fn timeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("chat_timeline_first_frame");
    group.sampling_mode(SamplingMode::Flat);
    for count in [100, 1_000, 10_000] {
        let snapshot = snapshot(count);
        let mut cx = context(&snapshot);
        assert!(cx.tick(Vec::new()).rendered);
        assert!(cx.find("chat-timeline").is_some());
        assert!(cx.find(&format!("chat-message-m{count}-block-0")).is_some());
        drop(cx);
        group.throughput(Throughput::Elements(count));
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &snapshot,
            |b, snapshot| {
                b.iter_batched_ref(
                    || context(snapshot),
                    |cx| black_box(cx.tick(Vec::new())),
                    BatchSize::PerIteration,
                );
            },
        );
    }
    group.finish();
}

criterion_group!(benches, timeline);
criterion_main!(benches);
