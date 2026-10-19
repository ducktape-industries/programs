//! The program natively over `MockHost`: commits, reads at the head and at older
//! snapshots, chunked files, paging, diffs and pins.
use std::collections::BTreeMap;

use ::program::{BlobId, Env, Error, HashKind, MockHost, Program, code};
use abi::role::identity::{Kind, Profile};

use crate::*;

mod lists;
mod watchers;

fn profile(number: u64, name: &str) -> Profile {
    Profile {
        number,
        name: name.into(),
        kind: Kind::User,
    }
}

fn host() -> MockHost {
    let host = MockHost::default();
    host.identity(vec![profile(1, "ada"), profile(2, "bo")]);
    host
}

fn ada() -> Env {
    MockHost::env(PROGRAM)
}

fn bo() -> Env {
    MockHost::env(PROGRAM).signed(vec![2; 32], Some(2))
}

/// `bytes` as the network holds a file's bytes: chunk blobs, put apart
/// from any transaction.
fn chunks(host: &MockHost, bytes: &[u8]) -> Content {
    let chunks = bytes
        .chunks(CHUNK_SIZE as usize)
        .map(|chunk| {
            host.exec(ada())
                .blob_put(HashKind::Sha256, CHUNK, chunk)
                .unwrap()
        })
        .collect();
    Content::Chunks {
        size: bytes.len() as u64,
        chunks,
    }
}

fn put(host: &MockHost, path: &str, text: &str) -> Change {
    Change::Put {
        path: path.into(),
        exec: false,
        meta: BTreeMap::new(),
        content: chunks(host, text.as_bytes()),
    }
}

fn commit(
    host: &MockHost,
    env: Env,
    base: Option<BlobId>,
    changes: Vec<Change>,
) -> Result<BlobId, Error> {
    let op = Op::Commit {
        base,
        message: "work".into(),
        changes,
    };
    host.attempt(|| Files::execute(&host.exec(env), op))?;
    Ok(abi::decode(&host.take_output()).unwrap())
}

fn refused(host: &MockHost, env: Env, op: Op) -> Error {
    host.refused(|| Files::execute(&host.exec(env), op))
}

fn ask(host: &MockHost, query: Query) -> Result<Reply, Error> {
    Files::query(&host.query(MockHost::env(PROGRAM)), query)
}

fn head(host: &MockHost) -> Option<BlobId> {
    match ask(host, Query::Head).unwrap() {
        Reply::Head(row) => row.map(|row| row.id),
        other => panic!("{other:?}"),
    }
}

fn names(host: &MockHost, path: &str, at: Option<BlobId>) -> Vec<String> {
    let Reply::Listing { entries, .. } = ask(
        host,
        Query::List {
            path: path.into(),
            at,
            page: PageRequest::default(),
        },
    )
    .unwrap() else {
        panic!("not a listing")
    };
    entries.items.into_iter().map(|entry| entry.path).collect()
}

fn text(host: &MockHost, path: &str, at: Option<BlobId>) -> Result<String, Error> {
    let Reply::Contents(contents) = ask(
        host,
        Query::Read {
            path: path.into(),
            at,
            offset: 0,
            len: u64::MAX,
        },
    )?
    else {
        panic!("not contents")
    };
    Ok(String::from_utf8(contents.bytes).unwrap())
}

#[test]
fn a_commit_writes_files_that_a_reader_lists_stats_and_reads() {
    let host = host();
    let id = commit(
        &host,
        ada(),
        None,
        vec![
            put(&host, "/shared/a.txt", "hello\nworld"),
            put(&host, "/home/ada/notes/n.md", "x"),
        ],
    )
    .unwrap();
    assert_eq!(head(&host), Some(id));
    assert_eq!(names(&host, "/", None), ["/home", "/shared"]);
    assert_eq!(
        names(&host, "/home/ada/notes", None),
        ["/home/ada/notes/n.md"]
    );
    assert_eq!(text(&host, "/shared/a.txt", None).unwrap(), "hello\nworld");
    let Reply::Stat { snapshot, entry } = ask(
        &host,
        Query::Stat {
            path: "/".into(),
            at: None,
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(snapshot, Some(id));
    assert!(matches!(entry.unwrap().node, Node::Dir { entries: 2, .. }));
    let Reply::Contents(middle) = ask(
        &host,
        Query::Read {
            path: "/shared/a.txt".into(),
            at: None,
            offset: 3,
            len: 4,
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(
        (middle.bytes.as_slice(), middle.size, middle.eof),
        (&b"lo\nw"[..], 11, false)
    );
    let Reply::Head(Some(row)) = ask(&host, Query::Head).unwrap() else {
        panic!()
    };
    assert_eq!(
        (row.seq, row.author, row.parent),
        (1, Principal::Account(1), None)
    );
}

#[test]
fn an_empty_filesystem_lists_its_roots_and_has_nothing_to_read() {
    let host = host();
    assert_eq!(head(&host), None);
    for root in ["/", "/home", "/home/ada", "/shared"] {
        assert!(names(&host, root, None).is_empty(), "{root}");
    }
    let missing = ask(
        &host,
        Query::List {
            path: "/shared/x".into(),
            at: None,
            page: PageRequest::default(),
        },
    )
    .unwrap_err();
    assert_eq!(missing.code, code::NOT_FOUND);
    assert_eq!(
        text(&host, "/shared/x", None).unwrap_err().code,
        code::NOT_FOUND
    );
}

#[test]
fn an_account_writes_anywhere_but_the_namespace_roots_and_a_refusal_leaves_nothing() {
    let host = host();
    commit(
        &host,
        ada(),
        None,
        vec![
            put(&host, "/etc/passwd", "x"),
            put(&host, "/home/bo/notes", "x"),
        ],
    )
    .unwrap();
    let before = head(&host);
    for path in ["/home", "/home/ada", "/shared"] {
        let op = Op::Commit {
            base: None,
            message: String::new(),
            changes: vec![put(&host, "/shared/ok", "x"), put(&host, path, "x")],
        };
        assert_eq!(refused(&host, ada(), op).code, code::UNAUTHORIZED, "{path}");
    }
    assert_eq!(head(&host), before);
    let nobody = Env {
        sender: None,
        ..ada()
    };
    let op = Op::Commit {
        base: None,
        message: String::new(),
        changes: vec![put(&host, "/shared/x", "x")],
    };
    assert_eq!(refused(&host, nobody, op).code, code::UNAUTHORIZED);
    commit(
        &host,
        ada().root(),
        None,
        vec![put(&host, "/etc/motd", "hi")],
    )
    .unwrap();
    assert_eq!(text(&host, "/etc/motd", None).unwrap(), "hi");
}

#[test]
fn a_base_is_stale_only_for_the_paths_that_changed_since() {
    let host = host();
    let first = commit(&host, ada(), None, vec![put(&host, "/shared/x", "1")]).unwrap();
    commit(&host, bo(), None, vec![put(&host, "/shared/y", "2")]).unwrap();
    let op = Op::Commit {
        base: None,
        message: String::new(),
        changes: vec![put(&host, "/shared/x", "3")],
    };
    assert_eq!(refused(&host, bo(), op).code, code::STALE);
    commit(&host, bo(), Some(first), vec![put(&host, "/shared/x", "3")]).unwrap();
    assert_eq!(text(&host, "/shared/x", None).unwrap(), "3");
    assert_eq!(text(&host, "/shared/y", None).unwrap(), "2");
    let unknown = Op::Commit {
        base: Some(BlobId::Sha256([7; 32])),
        message: String::new(),
        changes: vec![put(&host, "/shared/z", "")],
    };
    assert_eq!(refused(&host, ada(), unknown).code, code::NOT_FOUND);
}

#[test]
fn directories_are_made_moved_and_removed_and_each_rule_refuses_alone() {
    let host = host();
    commit(
        &host,
        ada(),
        None,
        vec![
            Change::Mkdir {
                path: "/shared/d".into(),
            },
            put(&host, "/shared/x", "x"),
        ],
    )
    .unwrap();
    let head_now = head(&host);
    commit(
        &host,
        ada(),
        head_now,
        vec![Change::Move {
            from: "/shared/x".into(),
            to: "/shared/d/x".into(),
        }],
    )
    .unwrap();
    assert_eq!(names(&host, "/shared", None), ["/shared/d"]);
    assert_eq!(text(&host, "/shared/d/x", None).unwrap(), "x");
    let base = head(&host);
    let attempt = |change: Change| Op::Commit {
        base,
        message: String::new(),
        changes: vec![change],
    };
    let cases = [
        (
            Change::Mkdir {
                path: "/shared/d".into(),
            },
            code::ALREADY_EXISTS,
        ),
        (
            Change::Remove {
                path: "/shared/none".into(),
            },
            code::NOT_FOUND,
        ),
        (
            Change::Move {
                from: "/shared/d".into(),
                to: "/shared/d/e".into(),
            },
            code::INVALID_INPUT,
        ),
        (
            Change::Move {
                from: "/shared/d/x".into(),
                to: "/shared/nowhere/x".into(),
            },
            code::NOT_FOUND,
        ),
        (put(&host, "/shared/d/x/y", "x"), code::WRONG_STATE),
    ];
    for (change, expected) in cases {
        assert_eq!(
            refused(&host, ada(), attempt(change.clone())).code,
            expected,
            "{change:?}"
        );
    }
    let twice = Op::Commit {
        base,
        message: String::new(),
        changes: vec![put(&host, "/shared/a", "1"), put(&host, "/shared/a", "2")],
    };
    assert_eq!(refused(&host, ada(), twice).code, code::INVALID_INPUT);
    commit(
        &host,
        ada(),
        base,
        vec![Change::Remove {
            path: "/shared/d".into(),
        }],
    )
    .unwrap();
    assert!(names(&host, "/shared", None).is_empty());
}

#[test]
fn a_large_file_names_its_chunks_and_reads_across_them() {
    let host = host();
    let full = vec![b'a'; CHUNK_SIZE as usize];
    let tail = b"tail".to_vec();
    let put = |bytes: &[u8]| -> BlobId {
        host.exec(ada())
            .blob_put(HashKind::Sha256, CHUNK, bytes)
            .unwrap()
    };
    let first = put(&full);
    let last = put(&tail);
    let size = CHUNK_SIZE + 4;
    let chunks = |chunks: Vec<BlobId>, size: u64| Change::Put {
        path: "/shared/big".into(),
        exec: true,
        meta: BTreeMap::from([("type".into(), "bytes".into())]),
        content: Content::Chunks { size, chunks },
    };
    let attempt = |change: Change| Op::Commit {
        base: None,
        message: String::new(),
        changes: vec![change],
    };
    let wrong_order = refused(&host, ada(), attempt(chunks(vec![last, first], size)));
    assert_eq!(wrong_order.code, code::INVALID_INPUT);
    let wrong_count = refused(&host, ada(), attempt(chunks(vec![first], size)));
    assert_eq!(wrong_count.code, code::INVALID_INPUT);
    let unheld = refused(
        &host,
        ada(),
        attempt(chunks(vec![first, BlobId::Sha256([1; 32])], size)),
    );
    assert_eq!(unheld.code, code::NOT_FOUND);
    commit(&host, ada(), None, vec![chunks(vec![first, last], size)]).unwrap();
    let Reply::Contents(across) = ask(
        &host,
        Query::Read {
            path: "/shared/big".into(),
            at: None,
            offset: CHUNK_SIZE - 2,
            len: 10,
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(across.bytes, b"aatail");
    assert!(across.eof);
    let Reply::Stat {
        entry: Some(entry), ..
    } = ask(
        &host,
        Query::Stat {
            path: "/shared/big".into(),
            at: None,
        },
    )
    .unwrap()
    else {
        panic!()
    };
    assert!(matches!(entry.node, Node::File { size: s, exec: true, .. } if s == size));
    assert_eq!(entry.meta["type"], "bytes");
}

#[test]
fn file_bytes_in_a_transaction_are_refused() {
    let host = host();
    let staged = refused(
        &host,
        ada(),
        Op::Stage {
            bytes: b"x".to_vec(),
        },
    );
    let inline = Op::Commit {
        base: None,
        message: String::new(),
        changes: vec![Change::Put {
            path: "/shared/x".into(),
            exec: false,
            meta: BTreeMap::new(),
            content: Content::Inline(b"x".to_vec()),
        }],
    };
    let inlined = refused(&host, ada(), inline);
    for refusal in [staged, inlined] {
        assert_eq!(refusal, carried());
    }
    assert!(names(&host, "/shared", None).is_empty());
}

#[test]
fn an_older_snapshot_keeps_its_tree_and_history_lists_newest_first() {
    let host = host();
    let first = commit(&host, ada(), None, vec![put(&host, "/shared/x", "old")]).unwrap();
    let second = commit(
        &host,
        ada(),
        Some(first),
        vec![put(&host, "/shared/x", "new")],
    )
    .unwrap();
    assert_eq!(text(&host, "/shared/x", Some(first)).unwrap(), "old");
    assert_eq!(text(&host, "/shared/x", None).unwrap(), "new");
    let Reply::History(page) = ask(
        &host,
        Query::History {
            page: PageRequest::first(1),
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(page.items[0].id, second);
    assert_eq!(page.items[0].parent, Some(first));
    let Reply::History(rest) = ask(
        &host,
        Query::History {
            page: PageRequest::resume(page.next, 1),
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!((rest.items[0].id, rest.next), (first, None));
    let Reply::Snapshot(Some(row)) = ask(&host, Query::Snapshot { id: first }).unwrap() else {
        panic!()
    };
    assert_eq!(row.seq, 1);
    let unknown = text(&host, "/shared/x", Some(BlobId::Sha256([9; 32]))).unwrap_err();
    assert_eq!(unknown.code, code::NOT_FOUND);
}

#[test]
fn find_and_grep_page_through_a_snapshot_and_keep_their_cursor_to_it() {
    let host = host();
    let files: Vec<Change> = ["/shared/a/1", "/shared/a/2", "/shared/a-b", "/shared/b/3"]
        .into_iter()
        .map(|path| put(&host, path, &format!("needle in {path}\nhay\nneedle again")))
        .collect();
    commit(&host, ada(), None, files).unwrap();
    let mut found = Vec::new();
    let mut page = PageRequest::first(2);
    loop {
        let Reply::Listing { snapshot, entries } = ask(
            &host,
            Query::Find {
                prefix: "/shared/".into(),
                at: None,
                page,
            },
        )
        .unwrap() else {
            panic!()
        };
        found.extend(entries.items.into_iter().map(|entry| entry.path));
        let Some(next) = entries.next else {
            break;
        };
        page = PageRequest::resume(Some(next), 2);
        assert!(snapshot.is_some());
    }
    assert_eq!(
        found,
        [
            "/shared/a",
            "/shared/a/1",
            "/shared/a/2",
            "/shared/a-b",
            "/shared/b",
            "/shared/b/3"
        ]
    );
    let Reply::Hits { hits, .. } = ask(
        &host,
        Query::Grep {
            pattern: "needle".into(),
            prefix: "/shared/a".into(),
            at: None,
            page: PageRequest::first(2),
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(hits.items.len(), 2);
    assert_eq!(
        (hits.items[1].path.as_str(), hits.items[1].line),
        ("/shared/a/1", 3)
    );
    let Reply::Hits { hits: more, .. } = ask(
        &host,
        Query::Grep {
            pattern: "needle".into(),
            prefix: "/shared/a".into(),
            at: None,
            page: PageRequest::resume(hits.next.clone(), 2),
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(more.items[0].path, "/shared/a/2");
    let elsewhere = ask(
        &host,
        Query::Find {
            prefix: "/shared/".into(),
            at: None,
            page: PageRequest::resume(hits.next, 2),
        },
    )
    .unwrap_err();
    assert_eq!(elsewhere.code, code::STALE);
}

#[test]
fn a_diff_names_what_was_added_removed_and_modified_under_a_prefix() {
    let host = host();
    let from = commit(
        &host,
        ada(),
        None,
        vec![
            put(&host, "/shared/keep", "k"),
            put(&host, "/shared/edit", "1"),
            put(&host, "/shared/gone/x", "x"),
        ],
    )
    .unwrap();
    let to = commit(
        &host,
        ada(),
        Some(from),
        vec![
            put(&host, "/shared/edit", "2"),
            Change::Remove {
                path: "/shared/gone".into(),
            },
            put(&host, "/shared/new", "n"),
        ],
    )
    .unwrap();
    let Reply::Diff(entries) = ask(
        &host,
        Query::Diff {
            from,
            to,
            prefix: "/shared/".into(),
        },
    )
    .unwrap() else {
        panic!()
    };
    let seen: Vec<(&str, DiffKind)> = entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry.kind))
        .collect();
    assert_eq!(
        seen,
        [
            ("/shared/edit", DiffKind::Modified),
            ("/shared/gone", DiffKind::Removed),
            ("/shared/gone/x", DiffKind::Removed),
            ("/shared/new", DiffKind::Added),
        ]
    );
}

#[test]
fn a_pin_names_a_minted_snapshot_and_its_unpin_needs_its_key() {
    let host = host();
    let id = commit(&host, ada(), None, vec![put(&host, "/shared/x", "x")]).unwrap();
    Files::execute(
        &host.exec(ada()),
        Op::Pin {
            name: "release".into(),
            snapshot: id,
        },
    )
    .unwrap();
    let again = Op::Pin {
        name: "release".into(),
        snapshot: id,
    };
    assert_eq!(refused(&host, bo(), again).code, code::ALREADY_EXISTS);
    let unminted = Op::Pin {
        name: "other".into(),
        snapshot: BlobId::Sha256([3; 32]),
    };
    assert_eq!(refused(&host, ada(), unminted).code, code::NOT_FOUND);
    let Reply::Pin(Some(pin)) = ask(
        &host,
        Query::Pin {
            name: "release".into(),
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!((pin.snapshot, pin.owner), (id, Principal::Account(1)));
    let unpin = Op::Unpin {
        name: "release".into(),
    };
    assert_eq!(
        Files::access(&host.query(bo()), &unpin).unwrap(),
        [::program::Need::delete(crate::gate::pin_key("release"))]
    );
    Files::execute(
        &host.exec(ada()),
        Op::Unpin {
            name: "release".into(),
        },
    )
    .unwrap();
    let Reply::Pins(pins) = ask(
        &host,
        Query::Pins {
            page: PageRequest::default(),
        },
    )
    .unwrap() else {
        panic!()
    };
    assert!(pins.items.is_empty());
}

#[test]
fn a_symlink_reads_as_its_target_and_a_commit_carries_a_change() {
    let host = host();
    commit(
        &host,
        ada(),
        None,
        vec![Change::Symlink {
            path: "/shared/link".into(),
            target: "/shared/elsewhere".into(),
        }],
    )
    .unwrap();
    assert_eq!(
        text(&host, "/shared/link", None).unwrap(),
        "/shared/elsewhere"
    );
    let base = head(&host);
    let empty = Op::Commit {
        base,
        message: String::new(),
        changes: Vec::new(),
    };
    assert_eq!(refused(&host, ada(), empty).code, code::INVALID_INPUT);
}
