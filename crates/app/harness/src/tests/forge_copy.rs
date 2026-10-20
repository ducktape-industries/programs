//! `forge_commit` items that land what forge already holds: a copy of a path (a file keeps its
//! mode, a directory lands as its tree) and a git blob by id, across repositories of one hash,
//! with no file in the pushed pack; what forge doesn't hold as named is refused in the tool's words.
use std::collections::BTreeMap;

use ::program::BlobId;
use forge::EntryKind::{Executable, File};
use gitcore::{Hash, Kind, pack};

use super::forge_commit::{forged, log};
use super::*;

const TIME_MS: u64 = 1_760_000_000_000;

fn turn(chain: &MockChain, height: u64, calls: &[(&str, Value)]) {
    chain.at(height, TIME_MS);
    let uses: Vec<Value> = calls
        .iter()
        .map(|(id, input)| tool_use(id, "forge_commit", input.clone()))
        .collect();
    model_says(chain, Value::Array(uses), "tool_use");
    chain.at(height + 1, TIME_MS);
    for (id, _) in calls {
        relay_back(chain, id);
    }
}

fn committed(chain: &MockChain, thread: u64, call: &str) -> String {
    let (text, failed) = result_of(chain, thread, call);
    assert!(!failed, "{call}: {text}");
    let said: Value = serde_json::from_str(&text).unwrap();
    said["commit"].as_str().unwrap().to_owned()
}

fn listed(
    chain: &MockChain,
    repo: &str,
    at: &str,
    path: &str,
) -> BTreeMap<String, (forge::EntryKind, String)> {
    let asked = forge::Query::Tree {
        repo: repo.into(),
        at: at.into(),
        path: path.as_bytes().to_vec(),
        page: PageRequest::first(16),
    };
    let forge::Reply::Tree { page, .. } = chain.query(forge::PROGRAM, &asked).unwrap() else {
        panic!("a tree answers Tree")
    };
    page.items
        .into_iter()
        .map(|entry| {
            let name = String::from_utf8(entry.name).unwrap();
            (name, (entry.kind, entry.oid))
        })
        .collect()
}

fn packs(chain: &MockChain) -> BTreeMap<BlobId, Vec<u8>> {
    let host = chain.host(PROGRAM).borrow();
    host.blobs
        .iter()
        .filter(|(_, blob)| blob.kind == forge::PACK_KIND)
        .map(|(id, blob)| (*id, blob.body.clone()))
        .collect()
}

#[test]
fn a_copy_lands_the_files_and_trees_forge_holds_and_its_pack_carries_no_file() {
    let world = forged(10_000);
    let chain = &world.chain;
    let thread = world.opened("m1", "move things", Some(WINDOW));
    let start = json!({
        "repo": "notes",
        "files": [
            { "path": "src/a.rs", "content": "pub fn a() {}\n" },
            { "path": "src/b.rs", "content": "pub fn b() {}\n" },
            { "path": "bin/run.sh", "content": "echo ok\n", "executable": true },
        ],
        "message": "Start",
    });
    turn(chain, 3, &[("c1", start)]);
    let first = committed(chain, thread, "c1");
    let root = listed(chain, "notes", &first, "");
    let src = listed(chain, "notes", &first, "src");
    let bin = listed(chain, "notes", &first, "bin");
    let before = packs(chain);

    let moved = json!({
        "repo": "notes",
        "files": [
            { "path": "lib/a.rs", "copy": { "path": "src/a.rs" } },
            { "path": "run.sh", "copy": { "path": "bin/run.sh" } },
            { "path": "crate", "copy": { "path": "src/" } },
        ],
        "remove": ["src/a.rs"],
        "message": "Move",
    });
    turn(chain, 5, &[("c2", moved)]);
    let second = committed(chain, thread, "c2");
    let after = listed(chain, "notes", &second, "");
    assert_eq!(after["crate"], root["src"]);
    assert_eq!(after["run.sh"], bin["run.sh"]);
    assert_eq!(after["run.sh"].0, Executable);
    assert_eq!(listed(chain, "notes", &second, "lib")["a.rs"], src["a.rs"]);
    let left: Vec<String> = listed(chain, "notes", &second, "src").into_keys().collect();
    assert_eq!(left, ["b.rs"]);

    let pushed: Vec<Vec<u8>> = packs(chain)
        .into_iter()
        .filter(|(id, _)| !before.contains_key(id))
        .map(|(_, body)| body)
        .collect();
    let [pushed] = pushed.try_into().unwrap();
    let objects = pack::read(&pushed, Hash::Sha1, &pack::Limits::generous(), |_| Ok(None)).unwrap();
    let files = objects
        .iter()
        .filter(|(_, object)| matches!(object.kind, Kind::Blob))
        .count();
    assert_eq!(
        (files, objects.len()),
        (0, 4),
        "the pack holds the new root, lib and src trees and the commit, and no file"
    );
}

#[test]
fn a_copy_from_another_repository_at_a_rev_and_a_blob_by_id_land_their_objects() {
    let world = forged(10_000);
    let chain = &world.chain;
    let thread = world.opened("m1", "carry", Some(WINDOW));
    let write = |text: &str| json!({ "repo": "notes", "files": [{ "path": "a.md", "content": text }], "message": text });
    turn(chain, 3, &[("c1", write("one\n"))]);
    let one = committed(chain, thread, "c1");
    turn(chain, 5, &[("c2", write("two\n"))]);
    let two = committed(chain, thread, "c2");
    let at = |commit: &str| listed(chain, "notes", commit, "")["a.md"].1.clone();
    let carried = json!({
        "repo": "other",
        "files": [
            { "path": "old.md", "copy": { "repo": "notes", "rev": &one, "path": "a.md" } },
            { "path": "new.md", "copy": { "repo": "notes", "path": "a.md" } },
            { "path": "tool.sh", "blob": at(&two), "executable": true },
        ],
        "message": "Carry",
    });
    turn(chain, 7, &[("c3", carried)]);
    let other = listed(chain, "other", &committed(chain, thread, "c3"), "");
    assert_eq!(other["old.md"], (File, at(&one)));
    assert_eq!(other["new.md"], (File, at(&two)));
    assert_eq!(other["tool.sh"], (Executable, at(&two)));
}

#[test]
fn what_forge_does_not_hold_as_named_is_refused_and_nothing_lands() {
    let world = forged(10_000);
    let chain = &world.chain;
    let create = forge::Op::Create {
        repo: "wide".into(),
        hash: ::program::HashKind::Sha256,
    };
    chain.submit(signed(ADA), forge::PROGRAM, &create).unwrap();
    let thread = world.opened("m1", "copy badly", Some(WINDOW));
    let start = json!({
        "repo": "notes",
        "files": [{ "path": "src/a.rs", "content": "a\n" }],
        "message": "Start",
    });
    turn(chain, 3, &[("c1", start)]);
    let first = committed(chain, thread, "c1");
    let tree = listed(chain, "notes", &first, "")["src"].1.clone();
    let item = |item: Value| json!({ "repo": "notes", "files": [item], "message": "m" });
    let calls = [
        (
            "r1",
            item(json!({ "path": "b.rs", "copy": { "path": "src/nope.rs" } })),
        ),
        (
            "r2",
            item(
                json!({ "path": "b.rs", "copy": { "rev": "no-such-branch", "path": "src/a.rs" } }),
            ),
        ),
        (
            "r3",
            item(json!({ "path": "b.rs", "blob": "0".repeat(40) })),
        ),
        ("r4", item(json!({ "path": "b.rs", "blob": tree }))),
        (
            "r5",
            item(json!({ "path": "b.rs", "copy": { "path": "src/a.rs" }, "executable": true })),
        ),
        (
            "r6",
            item(json!({ "path": "b.rs", "copy": { "repo": "wide", "path": "a.rs" } })),
        ),
        (
            "r7",
            item(json!({ "path": "b.rs", "copy": { "repo": "ghost", "path": "a.rs" } })),
        ),
        (
            "r8",
            item(json!({ "path": "b.rs", "copy": { "path": "src/a.rs" }, "content": "b\n" })),
        ),
    ];
    turn(chain, 5, &calls);
    for (call, refused, says) in [
        ("r1", code::NOT_FOUND, "nothing at src/nope.rs"),
        ("r3", code::NOT_FOUND, "holds no object"),
        ("r4", code::INVALID_INPUT, "not a blob"),
        (
            "r5",
            code::INVALID_INPUT,
            "executable goes with content or blob",
        ),
        ("r6", code::INVALID_INPUT, "different hashes"),
        ("r7", code::NOT_FOUND, "no repository ghost"),
        ("r8", code::INVALID_INPUT, "give only one of"),
    ] {
        let (text, failed) = result_of(chain, thread, call);
        assert!(
            failed && text.starts_with(refused) && text.contains(says),
            "{call}: {text}"
        );
    }
    let (text, failed) = result_of(chain, thread, "r2");
    assert!(failed, "a rev forge can't resolve is refused: {text}");
    assert_eq!(log(chain, "notes").len(), 1);
}
