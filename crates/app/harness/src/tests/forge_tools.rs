//! J-049's forge tools with forge seated: a branch started from another and the commit id read
//! back, span edits that change only their spans, a change opened, commented on and closed by its
//! number, files read by path and lines, found and searched.
use forge::Revision;

use super::forge_commit::{forged, log};
use super::*;

const TIME_MS: u64 = 1_760_000_000_000;

pub(super) fn turn(chain: &MockChain, height: u64, calls: &[(&str, &str, Value)]) {
    chain.at(height, TIME_MS);
    let uses: Vec<Value> = calls
        .iter()
        .map(|(id, tool, input)| tool_use(id, tool, input.clone()))
        .collect();
    model_says(chain, Value::Array(uses), "tool_use");
    chain.at(height + 1, TIME_MS);
    for (id, _, _) in calls {
        relay_back(chain, id);
    }
}

pub(super) fn world() -> World {
    let seated = forged(10_000);
    let register = identity::Op::RegisterProgram {
        program: forge::PROGRAM.into(),
    };
    seated
        .chain
        .submit(Origin::Root, identity::PROGRAM, &register)
        .unwrap();
    seated
}

pub(super) fn answered(chain: &MockChain, thread: u64, call: &str) -> Value {
    let (text, failed) = result_of(chain, thread, call);
    assert!(!failed, "{call}: {text}");
    serde_json::from_str(&text).unwrap()
}

pub(super) fn refusal(chain: &MockChain, thread: u64, call: &str) -> String {
    let (text, failed) = result_of(chain, thread, call);
    assert!(failed, "{call} should fail: {text}");
    text
}

pub(super) fn head(chain: &MockChain, repo: &str, branch: &str) -> String {
    let asked = forge::Query::Log {
        repo: repo.into(),
        from: Revision::Ref(format!("refs/heads/{branch}").into_bytes()),
        exclude: None,
        page: PageRequest::first(1),
    };
    let forge::Reply::Log { tip, .. } = chain.query(forge::PROGRAM, &asked).unwrap() else {
        panic!("a log answers Log")
    };
    tip
}

fn file_at(chain: &MockChain, repo: &str, at: &str, path: &str) -> (String, String) {
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    let asked = forge::Query::Tree {
        repo: repo.into(),
        at: at.into(),
        path: dir.as_bytes().to_vec(),
        page: PageRequest::first(64),
    };
    let forge::Reply::Tree { page, .. } = chain.query(forge::PROGRAM, &asked).unwrap() else {
        panic!("a tree answers Tree")
    };
    let entry = page
        .items
        .into_iter()
        .find(|entry| entry.name == name.as_bytes())
        .unwrap();
    let asked = forge::Query::Blob {
        repo: repo.into(),
        oid: entry.oid.clone(),
        range: None,
    };
    let forge::Reply::Blob { blob, .. } = chain.query(forge::PROGRAM, &asked).unwrap() else {
        panic!("a blob answers Blob")
    };
    (String::from_utf8(blob.bytes).unwrap(), entry.oid)
}

const LIB: &str = "pub mod a;\npub mod b;\n\nfn one() {}\nfn two() {}\n";

pub(super) fn seeded(world: &World, thread: u64) -> String {
    let chain = &world.chain;
    let start = json!({
        "repo": "notes",
        "files": [
            { "path": "src/lib.rs", "content": LIB },
            { "path": "README.md", "content": "# Notes\nfirst line\n" },
        ],
        "message": "start",
    });
    turn(chain, 3, &[("s1", "forge_commit", start)]);
    answered(chain, thread, "s1");
    let dev = json!({
        "repo": "notes",
        "branch": "dev",
        "files": [{ "path": "src/a.rs", "content": "pub fn a() {}\n" }],
        "message": "dev's own",
    });
    turn(chain, 5, &[("s2", "forge_commit", dev)]);
    answered(chain, thread, "s2")["commit"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn a_branch_starts_from_the_rev_it_names_and_the_answer_names_its_commit() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "branch from dev", Some(WINDOW));
    let dev = seeded(&world, thread);
    assert_eq!(dev, head(chain, "notes", "dev"));

    let work = json!({
        "repo": "notes",
        "branch": "j-1/work",
        "from": "dev",
        "files": [{ "path": "src/b.rs", "content": "pub fn b() {}\n" }],
        "message": "work",
    });
    turn(chain, 7, &[("c1", "forge_commit", work)]);
    let said = answered(chain, thread, "c1");
    let commit = said["commit"].as_str().unwrap();
    assert_eq!(said["pushed"], json!("refs/heads/j-1/work"));
    assert_eq!(said["parent"], json!(dev));
    assert_eq!(commit, head(chain, "notes", "j-1/work"));
    assert_eq!(
        file_at(chain, "notes", commit, "src/a.rs").0,
        "pub fn a() {}\n"
    );

    let again = json!({
        "repo": "notes",
        "branch": "j-1/work",
        "from": "main",
        "files": [{ "path": "x.md", "content": "x\n" }],
        "message": "again",
    });
    turn(chain, 9, &[("c2", "forge_commit", again)]);
    let text = refusal(chain, thread, "c2");
    assert!(
        text.starts_with(code::INVALID_INPUT) && text.contains(commit),
        "{text}"
    );
    assert_eq!(commit, head(chain, "notes", "j-1/work"));
}

#[test]
fn a_span_edit_changes_its_spans_and_leaves_every_other_byte_as_it_was() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "edit by span", Some(WINDOW));
    seeded(&world, thread);
    let main = head(chain, "notes", "main");
    let (_, readme_before) = file_at(chain, "notes", &main, "README.md");

    let edit = json!({
        "repo": "notes",
        "files": [
            { "path": "src/lib.rs", "edits": [
                { "after": "pub mod b;\n", "insert": "pub mod c;\n" },
                { "replace": "fn two() {}", "with": "fn two() -> u8 {\n    2\n}" },
            ] },
            { "path": "src/c.rs", "content": "pub fn c() {}\n" },
        ],
        "message": "c, and two says 2",
    });
    turn(chain, 7, &[("e1", "forge_commit", edit)]);
    let commit = answered(chain, thread, "e1")["commit"]
        .as_str()
        .unwrap()
        .to_owned();
    let (lib, _) = file_at(chain, "notes", &commit, "src/lib.rs");
    assert_eq!(
        lib,
        "pub mod a;\npub mod b;\npub mod c;\n\nfn one() {}\nfn two() -> u8 {\n    2\n}\n"
    );
    assert_eq!(
        file_at(chain, "notes", &commit, "src/c.rs").0,
        "pub fn c() {}\n"
    );
    assert_eq!(
        file_at(chain, "notes", &commit, "README.md").1,
        readme_before
    );

    let twice = json!({
        "repo": "notes",
        "files": [{ "path": "src/lib.rs", "edits": [{ "replace": "fn ", "with": "pub fn " }] }],
        "message": "every fn",
    });
    let missing = json!({
        "repo": "notes",
        "files": [{ "path": "src/none.rs", "edits": [{ "append": "x" }] }],
        "message": "nothing there",
    });
    turn(
        chain,
        9,
        &[
            ("e2", "forge_commit", twice),
            ("e3", "forge_commit", missing),
        ],
    );
    let text = refusal(chain, thread, "e2");
    assert!(
        text.starts_with(code::INVALID_INPUT) && text.contains("2 times, at lines 5, 6"),
        "{text}"
    );
    let text = refusal(chain, thread, "e3");
    assert!(text.starts_with(code::NOT_FOUND), "{text}");
    assert_eq!(head(chain, "notes", "main"), commit);
    assert_eq!(log(chain, "notes").len(), 2);
}

#[test]
fn a_change_is_opened_commented_on_and_closed_by_its_number() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "open a change", Some(WINDOW));
    let dev = seeded(&world, thread);
    let open = json!({
        "repo": "notes",
        "from": "dev",
        "into": "main",
        "title": "dev's own",
        "body": "Why: a.",
    });
    turn(chain, 7, &[("o1", "forge_change_open", open)]);
    let said = answered(chain, thread, "o1");
    assert_eq!(said, json!({ "ok": true, "number": 1 }));

    let comment = json!({ "repo": "notes", "number": 1, "body": "Replaced by #2." });
    turn(chain, 9, &[("o2", "forge_change_comment", comment)]);
    let said = answered(chain, thread, "o2");
    assert_eq!(
        (said["number"].clone(), said["ok"].clone()),
        (json!(1), json!(true))
    );
    turn(
        chain,
        11,
        &[(
            "o3",
            "forge_change_close",
            json!({ "repo": "notes", "number": 1 }),
        )],
    );
    assert_eq!(
        answered(chain, thread, "o3"),
        json!({ "ok": true, "number": 1, "state": "closed" })
    );

    let asked = forge::Query::Change {
        repo: "notes".into(),
        n: 1,
        page: PageRequest::first(8),
    };
    let forge::Reply::Change {
        change, reviews, ..
    } = chain.query(forge::PROGRAM, &asked).unwrap()
    else {
        panic!("a change answers Change")
    };
    assert_eq!(change.from, Revision::Ref(b"refs/heads/dev".to_vec()));
    assert_eq!(change.into, b"refs/heads/main");
    assert_eq!(change.author, Principal::Account(world.agent));
    assert_eq!(change.state, forge::ChangeState::Closed);
    let [review] = reviews.items.try_into().unwrap();
    assert_eq!(review.draft.verdict, forge::Verdict::Comment);
    assert_eq!(review.draft.commit_oid, dev);
    assert_eq!(review.draft.body, "Replaced by #2.");
}

fn narrow(blob_bytes: u64) -> World {
    let mut seated = super::world();
    seated.chain.seat::<forge::Forge>(forge::PROGRAM);
    let bounds = forge::Bounds {
        blob_bytes,
        ..super::forge_commit::bounds(10_000)
    };
    seated.chain.init(forge::PROGRAM, &bounds).unwrap();
    let register = identity::Op::RegisterProgram {
        program: forge::PROGRAM.into(),
    };
    seated
        .chain
        .submit(Origin::Root, identity::PROGRAM, &register)
        .unwrap();
    seated
}

#[test]
fn a_file_over_what_forge_serves_at_once_is_read_and_searched_a_range_at_a_time() {
    let world = narrow(64);
    let chain = &world.chain;
    let thread = world.opened("m1", "big files", Some(WINDOW));
    let rows: String = (1..=60).map(|n| format!("row {n}\n")).collect();
    let mixed = format!(
        "find me\n{}\n{}find me\nend\n",
        "x".repeat(55),
        "\0".repeat(64)
    );
    let files = json!({
        "repo": "big",
        "files": [
            { "path": "rows.txt", "content": rows },
            { "path": "mixed.txt", "content": mixed },
        ],
        "message": "big files",
    });
    turn(chain, 3, &[("b1", "forge_commit", files)]);
    answered(chain, thread, "b1");
    let span = json!({ "repo": "big", "path": "rows.txt", "offset": 54, "length": 14 });
    let lines = json!({ "repo": "big", "path": "rows.txt", "lines": [11, 12] });
    let whole = json!({ "repo": "big", "path": "rows.txt" });
    let binary = json!({ "repo": "big", "path": "mixed.txt", "offset": 64, "length": 64 });
    let straddled = json!({ "repo": "big", "pattern": "row 11" });
    let found = json!({ "repo": "big", "pattern": "find me" });
    turn(
        chain,
        5,
        &[
            ("r1", "forge_read", span),
            ("r2", "forge_read", lines),
            ("r3", "forge_read", whole),
            ("r4", "forge_read", binary),
            ("g1", "forge_grep", straddled),
            ("g2", "forge_grep", found),
        ],
    );
    let text = &answered(chain, thread, "r1")["text"];
    assert_eq!(text, &json!("row 10\nrow 11\n"));
    let text = &answered(chain, thread, "r2")["text"];
    assert_eq!(text, &json!("row 11\nrow 12\n"));
    assert_eq!(answered(chain, thread, "r3")["content"], json!("oversize"));
    let content = &answered(chain, thread, "r4")["content"];
    assert_eq!(content, &json!("binary in this range"));
    let said = answered(chain, thread, "g1");
    let row = json!({ "path": "rows.txt", "line": 11, "text": "row 11" });
    assert_eq!(said["hits"], json!([row]));
    let skipped = json!(["mixed.txt bytes 64..128: binary in this range"]);
    assert_eq!(said["skipped"], skipped);
    let first = json!({ "path": "mixed.txt", "line": 1, "text": "find me" });
    let after = json!({ "path": "mixed.txt", "line": null, "offset": 128, "text": "find me" });
    assert_eq!(answered(chain, thread, "g2")["hits"], json!([first, after]));
}

#[test]
fn a_file_is_read_by_path_and_lines_found_by_prefix_and_searched() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "read by path", Some(WINDOW));
    seeded(&world, thread);
    let read = json!({ "repo": "notes", "rev": "dev", "path": "src/lib.rs", "lines": [2, 4] });
    let whole = json!({ "repo": "notes", "path": "README.md" });
    let find = json!({ "repo": "notes", "rev": "dev", "prefix": "src/" });
    let grep = json!({ "repo": "notes", "rev": "dev", "pattern": "fn " });
    turn(
        chain,
        7,
        &[
            ("r1", "forge_read", read),
            ("r2", "forge_read", whole),
            ("r3", "forge_find", find),
            ("r4", "forge_grep", grep),
        ],
    );
    let said = answered(chain, thread, "r1");
    assert_eq!(said["text"], json!("pub mod b;\n\nfn one() {}\n"));
    assert_eq!(
        (said["lines"].clone(), said["total_lines"].clone()),
        (json!([2, 4]), json!(5))
    );
    assert_eq!(
        answered(chain, thread, "r2")["text"],
        json!("# Notes\nfirst line\n")
    );
    let paths: Vec<Value> = answered(chain, thread, "r3")["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["path"].clone())
        .collect();
    assert_eq!(paths, [json!("src/a.rs"), json!("src/lib.rs")]);
    let hits: Vec<(Value, Value)> = answered(chain, thread, "r4")["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hit| (hit["path"].clone(), hit["line"].clone()))
        .collect();
    assert_eq!(
        hits,
        [
            (json!("src/a.rs"), json!(1)),
            (json!("src/lib.rs"), json!(4)),
            (json!("src/lib.rs"), json!(5))
        ]
    );
}

#[test]
fn a_pattern_holding_a_newline_finds_a_call_split_across_lines_and_ranges() {
    let world = narrow(64);
    let chain = &world.chain;
    let thread = world.opened("m1", "split calls", Some(WINDOW));
    let rows: String = (1..=60).map(|n| format!("row {n}\n")).collect();
    let call = "fn main() {\n    run(\n        first,\n        second,\n    );\n}\n";
    let files = json!({
        "repo": "split",
        "files": [
            { "path": "call.rs", "content": call },
            { "path": "crlf.rs", "content": call.replace('\n', "\r\n") },
            { "path": "rows.txt", "content": rows },
        ],
        "message": "split calls",
    });
    turn(chain, 3, &[("c1", "forge_commit", files)]);
    answered(chain, thread, "c1");
    let split_call = json!({ "repo": "split", "pattern": "run(\n        first," });
    let split_rows = json!({ "repo": "split", "pattern": "row 10\nrow 11" });
    turn(
        chain,
        5,
        &[
            ("g1", "forge_grep", split_call),
            ("g2", "forge_grep", split_rows),
        ],
    );
    let text = "    run(\n        first,";
    let call = json!({ "path": "call.rs", "line": 2, "text": text });
    let crlf = json!({ "path": "crlf.rs", "line": 2, "text": text });
    assert_eq!(answered(chain, thread, "g1")["hits"], json!([call, crlf]));
    let row = json!({ "path": "rows.txt", "line": 10, "text": "row 10\nrow 11" });
    assert_eq!(answered(chain, thread, "g2")["hits"], json!([row]));
}
