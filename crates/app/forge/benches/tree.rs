use std::hint::black_box;

use criterion::{Criterion, SamplingMode, Throughput, criterion_group, criterion_main};
use forge::{Bounds, Forge, PageRequest, Principal, Query, Reply, Repo, Settings, tables};
use gitcore::{Hash, Mode, Oid, Tree, TreeEntry};
use program::{Blob, HashKind, MockHost, Program};

fn put_tree(host: &MockHost, entries: Vec<TreeEntry>) -> Oid {
    let body = Tree { entries }.serialize();
    let id = program::blob_id(HashKind::Sha1, "tree", &body).unwrap();
    let oid = gitcore::oid_of(Hash::Sha1, gitcore::Kind::Tree, &body).unwrap();
    host.borrow_mut().blobs.insert(
        id,
        Blob {
            kind: "tree".into(),
            body,
        },
    );
    oid
}

fn host() -> MockHost {
    let host = MockHost::default();
    let ctx = host.exec(MockHost::env("forge"));
    tables::BOUNDS.put(
        &ctx,
        &Bounds {
            max_objects: 10_000,
            max_delta_depth: 64,
            max_object_size: 1 << 20,
            push_walk: 1_000,
            fetch_walk: 1_000,
            merge_cost: 100_000,
            page_size: 1_000,
            log_walk: 1_000,
            tree_walk: 256,
            diff_bytes: 4 << 20,
            blob_bytes: 1 << 20,
            record_bytes: 64 << 10,
        },
    );
    tables::REPOS.put(
        &ctx,
        &"bench".to_owned(),
        &Repo {
            hash: HashKind::Sha1,
            owner: Principal::Account(7),
            settings: Settings::default(),
            refs_count: 0,
            last_activity: 1,
        },
    );
    host
}

fn tree(c: &mut Criterion) {
    let host = host();
    let file = gitcore::oid_of(Hash::Sha1, gitcore::Kind::Blob, b"file content\n").unwrap();
    let flat = put_tree(
        &host,
        (0..1_000)
            .map(|i| TreeEntry {
                mode: Mode::Regular,
                name: format!("file-{i:04}.rs").into_bytes(),
                id: file,
            })
            .collect(),
    );
    let mut nested = put_tree(
        &host,
        vec![TreeEntry {
            mode: Mode::Regular,
            name: b"leaf.rs".to_vec(),
            id: file,
        }],
    );
    for level in (0..8).rev() {
        nested = put_tree(
            &host,
            vec![TreeEntry {
                mode: Mode::Directory,
                name: format!("level-{level}").into_bytes(),
                id: nested,
            }],
        );
    }
    let path = (0..8)
        .map(|level| format!("level-{level}"))
        .collect::<Vec<_>>()
        .join("/")
        .into_bytes();
    let ctx = host.query(MockHost::env("forge"));
    let mut group = c.benchmark_group("forge_tree_read");
    group.sampling_mode(SamplingMode::Flat);
    for (name, root, path, files) in [
        ("flat_1000", flat, Vec::new(), 1_000),
        ("nested_8", nested, path, 1),
    ] {
        let query = Query::Tree {
            repo: "bench".into(),
            at: root.to_hex(),
            path,
            page: PageRequest::first(1_000),
        };
        let Reply::Tree { page, .. } = Forge::query(&ctx, query.clone()).unwrap() else {
            panic!("expected tree reply")
        };
        assert_eq!(page.items.len(), files);
        assert!(page.next.is_none());
        group.throughput(Throughput::Elements(files as u64));
        group.bench_function(name, |b| {
            b.iter(|| black_box(Forge::query(black_box(&ctx), black_box(query.clone())).unwrap()))
        });
    }
    group.finish();
}

criterion_group!(benches, tree);
criterion_main!(benches);
