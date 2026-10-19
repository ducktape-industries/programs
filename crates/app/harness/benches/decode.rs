use std::hint::black_box;

use borsh::{BorshSchema, BorshSerialize, schema::BorshSchemaContainer};
use criterion::{Criterion, SamplingMode, Throughput, criterion_group, criterion_main};

#[derive(BorshSerialize, BorshSchema)]
struct Nested {
    label: String,
    values: Vec<u64>,
    child: Option<Box<Nested>>,
}

fn measure<T: BorshSerialize + BorshSchema>(c: &mut Criterion, name: &str, value: &T) {
    let schema = BorshSchemaContainer::for_type::<T>();
    let bytes = borsh::to_vec(value).unwrap();
    let declaration = T::declaration();
    borsh_json::decode(&schema, &declaration, "value", &bytes).unwrap();
    let mut group = c.benchmark_group("borsh_json_decode");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Bytes(bytes.len() as u64));
    group.bench_function(name, |b| {
        b.iter(|| {
            black_box(
                borsh_json::decode(
                    black_box(&schema),
                    black_box(&declaration),
                    "value",
                    black_box(&bytes),
                )
                .unwrap(),
            )
        });
    });
    group.finish();
}

fn decode(c: &mut Criterion) {
    measure(c, "small_op", &harness::Op::Close { thread: 42 });
    let page = chat::PageResponse {
        height: 200,
        next: None,
        items: (1..=200)
            .map(|seq| {
                let text = if seq % 5 == 0 {
                    "Review the updated implementation and its paging behavior. ".repeat(32)
                } else {
                    format!("Message {seq}: the latest changes look good.")
                };
                chat::MsgRow {
                    channel_id: "general".into(),
                    seq,
                    message_id: format!("m{seq}"),
                    height: seq,
                    time: 1_700_000_000_000 + seq * 60_000,
                    blocks: vec![chat::Block::paragraph(&text)],
                    text,
                    reply_count: if seq % 11 == 0 { 2 } else { 0 },
                    reactions: if seq % 7 == 0 {
                        vec![chat::Reaction {
                            emoji: "👍".into(),
                            count: 3,
                            reacted_by_me: false,
                        }]
                    } else {
                        Vec::new()
                    },
                    ..chat::MsgRow::by(chat::Principal::Account(7 + seq % 8))
                }
            })
            .collect(),
    };
    measure(c, "room_page_200", &page);
    let mut child = None;
    for level in (0..32).rev() {
        child = Some(Box::new(Nested {
            label: format!("level-{level}"),
            values: vec![level; 8],
            child,
        }));
    }
    measure(c, "nested_32", &child.unwrap());
}

criterion_group!(benches, decode);
criterion_main!(benches);
