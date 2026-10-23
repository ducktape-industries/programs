//! Snapshot-pinned file patches through the harness tool and the real files program.
use super::forge_tools::{answered, turn};
use super::*;

const PATH: &str = "/shared/review.md";

fn bounded(blob_bytes: u64, merge_cost: u64) -> World {
    let mut world = super::world();
    world.chain.seat::<forge::Forge>(forge::PROGRAM);
    let bounds = forge::Bounds {
        blob_bytes,
        merge_cost,
        ..super::forge_commit::bounds(10_000)
    };
    world.chain.init(forge::PROGRAM, &bounds).unwrap();
    world
}

fn world() -> World {
    bounded(1 << 20, 100_000)
}

fn changed(chain: &MockChain, changes: Vec<files::Change>) -> BlobId {
    let files::Reply::Head(head) = chain.query(files::PROGRAM, &files::Query::Head).unwrap() else {
        panic!("head")
    };
    let op = files::Op::Commit {
        base: head.map(|row| row.id),
        message: "review snapshots".into(),
        changes,
    };
    abi::decode(&chain.submit(signed(ADA), files::PROGRAM, &op).unwrap()).unwrap()
}

fn put(chain: &MockChain, bytes: &[u8], exec: bool, meta: &[(&str, &str)]) -> BlobId {
    let chunks = bytes
        .chunks(files::CHUNK_SIZE as usize)
        .map(|chunk| chain.put_blob(files::CHUNK, chunk.to_vec()).unwrap())
        .collect();
    changed(
        chain,
        vec![files::Change::Put {
            path: PATH.into(),
            exec,
            meta: meta
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
            content: files::Content::Chunks {
                size: bytes.len() as u64,
                chunks,
            },
        }],
    )
}

fn input(from: &BlobId, to: &BlobId) -> Value {
    json!({ "path": PATH, "from": abi::hex(from.digest()), "to": abi::hex(to.digest()) })
}

fn refused(chain: &MockChain, thread: u64, call: &str, code: &str) {
    let (text, failed) = result_of(chain, thread, call);
    assert!(failed, "{call}: {text}");
    assert!(text.starts_with(code), "{call}: {text}");
}

#[test]
fn a_file_diff_reads_both_named_snapshots_and_only_the_changed_lines() {
    let world = world();
    let chain = &world.chain;
    let from = put(chain, b"# Notes\nstable\nold\nbetween\nlast\n", false, &[]);
    let to = put(
        chain,
        b"# Notes\nstable\nnew\nbetween\nlast\nextra\n",
        false,
        &[],
    );
    put(chain, b"not either version\n", false, &[]);
    let thread = world.opened("m1", "review changes", Some(WINDOW));
    turn(chain, 3, &[("d1", "files_diff", input(&from, &to))]);
    let answer = answered(chain, thread, "d1");
    assert_eq!(answer["from"], json!(abi::hex(from.digest())));
    assert_eq!(answer["to"], json!(abi::hex(to.digest())));
    assert_eq!(answer["changed"], json!(true));
    assert_eq!(answer["binary"], json!(false));
    assert_eq!(
        answer["patch"]["text"],
        json!("@@ -3 +3 @@\n-old\n+new\n@@ -5,0 +6 @@\n+extra\n")
    );
    assert_eq!(
        answer["before"]["size"],
        json!(b"# Notes\nstable\nold\nbetween\nlast\n".len())
    );
    assert_eq!(
        answer["after"]["size"],
        json!(b"# Notes\nstable\nnew\nbetween\nlast\nextra\n".len())
    );
}

#[test]
fn adding_and_removing_a_file_keep_the_absent_side_distinct_from_an_empty_file() {
    let world = world();
    let chain = &world.chain;
    let missing = commit(chain, ADA, "/shared/other.md", "other\n");
    let present = put(chain, b"first\n", false, &[]);
    let removed = changed(chain, vec![files::Change::Remove { path: PATH.into() }]);
    let empty = put(chain, b"", false, &[]);
    let thread = world.opened("m1", "review additions", Some(WINDOW));
    turn(
        chain,
        3,
        &[
            ("add", "files_diff", input(&missing, &present)),
            ("remove", "files_diff", input(&present, &removed)),
            ("empty", "files_diff", input(&removed, &empty)),
            ("absent", "files_diff", input(&missing, &removed)),
        ],
    );
    let add = answered(chain, thread, "add");
    assert_eq!(add["before"], Value::Null);
    assert_eq!(add["after"]["kind"], json!("file"));
    assert_eq!(add["patch"]["text"], json!("@@ -0,0 +1 @@\n+first\n"));
    let remove = answered(chain, thread, "remove");
    assert_eq!(remove["after"], Value::Null);
    assert_eq!(remove["patch"]["text"], json!("@@ -1 +0,0 @@\n-first\n"));
    let empty = answered(chain, thread, "empty");
    assert_eq!(empty["changed"], json!(true));
    assert_eq!(empty["before"], Value::Null);
    assert_eq!(empty["after"]["size"], json!(0));
    assert_eq!(empty["patch"]["text"], json!(""));
    refused(chain, thread, "absent", code::NOT_FOUND);
}

#[test]
fn unchanged_text_still_reports_mode_and_metadata_changes() {
    let world = world();
    let chain = &world.chain;
    let from = put(chain, b"same\n", false, &[("review", "draft")]);
    let to = put(chain, b"same\n", true, &[("review", "ready")]);
    let thread = world.opened("m1", "review metadata", Some(WINDOW));
    turn(
        chain,
        3,
        &[
            ("same", "files_diff", input(&from, &from)),
            ("meta", "files_diff", input(&from, &to)),
        ],
    );
    let same = answered(chain, thread, "same");
    assert_eq!(same["changed"], json!(false));
    assert_eq!(same["patch"]["text"], json!(""));
    let meta = answered(chain, thread, "meta");
    assert_eq!(meta["changed"], json!(true));
    assert_eq!(meta["patch"]["text"], json!(""));
    assert_eq!(meta["before"]["executable"], json!(false));
    assert_eq!(meta["after"]["executable"], json!(true));
    assert_eq!(meta["before"]["meta"]["review"], json!("draft"));
    assert_eq!(meta["after"]["meta"]["review"], json!("ready"));
}

#[test]
fn unicode_and_missing_final_newlines_are_kept_while_binary_contents_are_not_patched() {
    let world = world();
    let chain = &world.chain;
    let from = put(chain, "é".as_bytes(), false, &[]);
    let to = put(chain, "🦆".as_bytes(), false, &[]);
    let invalid_utf8 = put(chain, b"\xff", false, &[]);
    let nul = put(chain, b"text\0", false, &[]);
    let thread = world.opened("m1", "review bytes", Some(WINDOW));
    turn(
        chain,
        3,
        &[
            ("text", "files_diff", input(&from, &to)),
            ("binary", "files_diff", input(&from, &invalid_utf8)),
            ("nul", "files_diff", input(&to, &nul)),
        ],
    );
    assert_eq!(
        answered(chain, thread, "text")["patch"]["text"],
        json!("@@ -1 +1 @@\n-é\n\\ No newline at end of file\n+🦆\n\\ No newline at end of file\n")
    );
    for call in ["binary", "nul"] {
        let answer = answered(chain, thread, call);
        assert_eq!(answer["binary"], json!(true));
        assert_eq!(answer["changed"], json!(true));
        assert_eq!(answer["patch"], Value::Null);
    }
}

#[test]
fn a_long_patch_is_reachable_by_diff_lines_or_an_explicit_whole_read() {
    let world = world();
    let chain = &world.chain;
    let from = put(chain, b"", false, &[]);
    let line = "é🦆\n";
    let text = line.repeat(crate::tools::WHOLE_UP_TO as usize / line.len() + 1);
    let to = put(chain, text.as_bytes(), false, &[]);
    let mut whole = input(&from, &to);
    whole["whole"] = json!(true);
    let mut range = input(&from, &to);
    range["lines"] = json!([2, 3]);
    let mut conflict = whole.clone();
    conflict["lines"] = json!([2, 3]);
    let thread = world.opened("m1", "read a long patch", Some(WINDOW));
    turn(
        chain,
        3,
        &[
            ("summary", "files_diff", input(&from, &to)),
            ("whole", "files_diff", whole),
            ("range", "files_diff", range),
            ("conflict", "files_diff", conflict),
        ],
    );
    let summary = answered(chain, thread, "summary");
    assert_eq!(summary["patch"]["text"], Value::Null);
    assert!(summary["patch"]["left_out"].is_string());
    let full = answered(chain, thread, "whole");
    let patch = full["patch"]["text"].as_str().unwrap();
    assert_eq!(summary["patch"]["size"], json!(patch.len()));
    assert_eq!(
        summary["patch"]["total_lines"],
        json!(text.lines().count() + 1)
    );
    assert_eq!(full["patch"]["eof"], json!(true));
    assert!(patch.ends_with("+é🦆\n"));
    let range = answered(chain, thread, "range");
    assert_eq!(range["patch"]["text"], json!("+é🦆\n+é🦆\n"));
    assert_eq!(range["patch"]["lines"], json!([2, 3]));
    assert_eq!(range["patch"]["eof"], json!(false));
    refused(chain, thread, "conflict", code::INVALID_INPUT);
}

#[test]
fn a_small_change_after_a_files_chunk_is_compared_without_the_unchanged_prefix() {
    let world = bounded(2 * files::CHUNK_SIZE, 100_000);
    let chain = &world.chain;
    let prefix = format!("{}é🦆\n", "x".repeat(files::CHUNK_SIZE as usize - 2));
    let from = put(chain, format!("{prefix}old\n").as_bytes(), false, &[]);
    let to = put(chain, format!("{prefix}new\n").as_bytes(), false, &[]);
    let thread = world.opened("m1", "review after a chunk", Some(WINDOW));
    turn(chain, 3, &[("d1", "files_diff", input(&from, &to))]);
    let answer = answered(chain, thread, "d1");
    assert_eq!(answer["patch"]["text"], json!("@@ -2 +2 @@\n-old\n+new\n"));
}

#[test]
fn a_diff_refuses_directories_bad_snapshot_ids_and_unknown_snapshots() {
    let world = world();
    let chain = &world.chain;
    let from = put(chain, b"old\n", false, &[]);
    changed(chain, vec![files::Change::Remove { path: PATH.into() }]);
    let directory = changed(chain, vec![files::Change::Mkdir { path: PATH.into() }]);
    let mut bad = input(&from, &directory);
    bad["from"] = json!("not a snapshot");
    let mut unknown = input(&from, &directory);
    unknown["to"] = json!("00".repeat(32));
    let thread = world.opened("m1", "refuse bad inputs", Some(WINDOW));
    turn(
        chain,
        3,
        &[
            ("directory", "files_diff", input(&from, &directory)),
            ("bad", "files_diff", bad),
            ("unknown", "files_diff", unknown),
        ],
    );
    refused(chain, thread, "directory", code::INVALID_INPUT);
    refused(chain, thread, "bad", code::INVALID_INPUT);
    refused(chain, thread, "unknown", code::NOT_FOUND);
}

#[test]
fn a_pruned_older_chunk_is_refused_instead_of_reading_the_head() {
    let world = world();
    let chain = &world.chain;
    let old = b"older version\n";
    let from = put(chain, old, false, &[]);
    let to = put(chain, b"current version\n", false, &[]);
    let chunk = chain.put_blob(files::CHUNK, old.to_vec()).unwrap();
    chain.drop_blob(&chunk);
    let thread = world.opened("m1", "refuse pruned bytes", Some(WINDOW));
    turn(chain, 3, &[("pruned", "files_diff", input(&from, &to))]);
    refused(chain, thread, "pruned", code::CORRUPT);
}

#[test]
fn a_side_over_forges_blob_bytes_is_answered_too_large_without_reading_it() {
    let world = bounded(64, 100_000);
    let chain = &world.chain;
    let large = "line\n".repeat(13);
    let from = put(chain, b"small\n", false, &[]);
    let to = put(chain, large.as_bytes(), false, &[]);
    let chunk = chain.put_blob(files::CHUNK, large.into_bytes()).unwrap();
    chain.drop_blob(&chunk);
    let thread = world.opened("m1", "review a large file", Some(WINDOW));
    turn(chain, 3, &[("large", "files_diff", input(&from, &to))]);
    let answer = answered(chain, thread, "large");
    assert_eq!(answer["oversize"], json!("too large"));
    assert_eq!(answer["changed"], json!(true));
    assert_eq!(answer["after"]["size"], json!(65));
    assert_eq!(answer["patch"], Value::Null);
}

#[test]
fn an_edit_past_forges_merge_cost_is_too_different_while_a_one_sided_edit_is_patched() {
    let world = bounded(1 << 20, 4);
    let chain = &world.chain;
    let empty = put(chain, b"", false, &[]);
    let from = put(chain, b"a\nb\nc\nd\ne\n", false, &[]);
    let to = put(chain, b"v\nw\nx\ny\nz\n", false, &[]);
    let thread = world.opened("m1", "review a rewrite", Some(WINDOW));
    turn(
        chain,
        3,
        &[
            ("rewrite", "files_diff", input(&from, &to)),
            ("added", "files_diff", input(&empty, &from)),
        ],
    );
    let rewrite = answered(chain, thread, "rewrite");
    assert_eq!(rewrite["oversize"], json!("too different"));
    assert_eq!(rewrite["changed"], json!(true));
    assert_eq!(rewrite["patch"], Value::Null);
    let added = answered(chain, thread, "added");
    assert_eq!(added["oversize"], Value::Null);
    let patch = added["patch"]["text"].as_str().unwrap();
    assert!(patch.ends_with("+d\n+e\n"), "{patch}");
}

#[test]
fn a_symlink_diff_compares_its_target_text_without_following_it() {
    let world = world();
    let chain = &world.chain;
    let from = changed(
        chain,
        vec![files::Change::Symlink {
            path: PATH.into(),
            target: "/shared/old".into(),
        }],
    );
    let to = changed(
        chain,
        vec![files::Change::Symlink {
            path: PATH.into(),
            target: "/shared/new".into(),
        }],
    );
    let thread = world.opened("m1", "review link targets", Some(WINDOW));
    turn(chain, 3, &[("link", "files_diff", input(&from, &to))]);
    let answer = answered(chain, thread, "link");
    assert_eq!(answer["before"]["kind"], json!("symlink"));
    assert_eq!(answer["before"]["target"], json!("/shared/old"));
    assert_eq!(answer["after"]["target"], json!("/shared/new"));
    assert_eq!(
        answer["patch"]["text"],
        json!(
            "@@ -1 +1 @@\n-/shared/old\n\\ No newline at end of file\n+/shared/new\n\\ No newline at end of file\n"
        )
    );
}
