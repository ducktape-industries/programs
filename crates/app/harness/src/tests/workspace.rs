//! An agent's workspace in files: `forge_commit` takes a files file or a whole directory, read
//! at one snapshot; `files_edit` and `files_write` keep the mode and meta; `files_read` reads a
//! file by its lines or by a heading's section; history, list and find answer a page.
use forge::EntryKind::{Directory, Executable, File};

use super::forge_commit::{forged, log, names};
use super::*;

fn put(chain: &MockChain, path: &str, text: &str, exec: bool, meta: &[(&str, &str)]) -> BlobId {
    let base = match chain.query(files::PROGRAM, &files::Query::Head).unwrap() {
        files::Reply::Head(head) => head.map(|row| row.id),
        other => panic!("{other:?}"),
    };
    let commit = files::Op::Commit {
        base,
        message: "workspace".into(),
        changes: vec![files::Change::Put {
            path: path.into(),
            exec,
            meta: meta
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
            content: files::Content::Chunks {
                size: text.len() as u64,
                chunks: text
                    .as_bytes()
                    .chunks(files::CHUNK_SIZE as usize)
                    .map(|chunk| chain.put_blob(files::CHUNK, chunk.to_vec()).unwrap())
                    .collect(),
            },
        }],
    };
    abi::decode(&chain.submit(signed(ADA), files::PROGRAM, &commit).unwrap()).unwrap()
}

fn turn(chain: &MockChain, height: u64, calls: &[(&str, &str, Value)]) {
    chain.at(height, 0);
    let uses: Vec<Value> = calls
        .iter()
        .map(|(id, tool, input)| tool_use(id, tool, input.clone()))
        .collect();
    model_says(chain, Value::Array(uses), "tool_use");
    chain.at(height + 1, 0);
    for (id, _, _) in calls {
        relay_back(chain, id);
    }
}

fn entry_at(chain: &MockChain, path: &str) -> files::EntryInfo {
    let asked = files::Query::Stat {
        path: path.into(),
        at: None,
    };
    let files::Reply::Stat {
        entry: Some(entry), ..
    } = chain.query(files::PROGRAM, &asked).unwrap()
    else {
        panic!("nothing at {path}")
    };
    entry
}

fn executable(entry: &files::EntryInfo) -> bool {
    matches!(entry.node, files::Node::File { exec: true, .. })
}

fn meta_of(entry: &files::EntryInfo) -> Vec<(&str, &str)> {
    entry
        .meta
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect()
}

fn read_at(chain: &MockChain, path: &str) -> String {
    let asked = files::Query::Read {
        path: path.into(),
        at: None,
        offset: 0,
        len: u64::MAX,
    };
    let files::Reply::Contents(contents) = chain.query(files::PROGRAM, &asked).unwrap() else {
        panic!()
    };
    String::from_utf8(contents.bytes).unwrap()
}

fn committed(chain: &MockChain, repo: &str, at: &str, dir: &str, name: &str) -> String {
    let asked = forge::Query::Tree {
        repo: repo.into(),
        at: at.into(),
        path: dir.as_bytes().to_vec(),
        page: PageRequest::first(16),
    };
    let forge::Reply::Tree { page, .. } = chain.query(forge::PROGRAM, &asked).unwrap() else {
        panic!()
    };
    let entry = page
        .items
        .into_iter()
        .find(|entry| entry.name == name.as_bytes())
        .expect(name);
    let asked = forge::Query::Blob {
        repo: repo.into(),
        oid: entry.oid,
        range: None,
    };
    let forge::Reply::Blob { blob, .. } = chain.query(forge::PROGRAM, &asked).unwrap() else {
        panic!()
    };
    String::from_utf8(blob.bytes).unwrap()
}

fn child(chain: &MockChain, repo: &str, parent: &str) -> String {
    log(chain, repo)
        .into_iter()
        .find(|commit| commit.parents == [parent.to_owned()])
        .expect("a commit on top")
        .oid
}

#[test]
fn a_commit_takes_a_files_file_a_files_directory_and_inline_content_read_at_one_snapshot() {
    let world = forged(10_000);
    let chain = &world.chain;
    put(chain, "/home/Ada/ws/README.md", "# Notes\n", false, &[]);
    put(
        chain,
        "/home/Ada/ws/src/main.rs",
        "fn main() {}\n",
        false,
        &[],
    );
    let cut = put(chain, "/home/Ada/ws/src/bin/run.sh", "echo ok\n", true, &[]);
    let thread = world.opened("m1", "commit the workspace", Some(WINDOW));
    let start = json!({
        "repo": "notes",
        "files": [
            { "path": "README.md", "source": "/home/Ada/ws/README.md" },
            { "path": "code/", "source": "/home/Ada/ws/src/" },
            { "path": "LICENSE", "content": "MIT\n" },
        ],
        "message": "From the workspace",
    });
    turn(chain, 3, &[("c1", "forge_commit", start)]);
    let (text, failed) = result_of(chain, thread, "c1");
    assert!(!failed, "{text}");
    let said: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        (
            said["ok"].clone(),
            said["pushed"].clone(),
            said["parent"].clone()
        ),
        (json!(true), json!("refs/heads/main"), Value::Null),
        "the result names the branch and the commit, never the contents"
    );
    assert!(said["commit"].is_string(), "{said}");
    let [first] = log(chain, "notes").try_into().unwrap();
    let top = [
        ("LICENSE".to_owned(), File),
        ("README.md".to_owned(), File),
        ("code".to_owned(), Directory),
    ];
    assert_eq!(names(chain, "notes", &first.oid, ""), top);
    assert_eq!(
        names(chain, "notes", &first.oid, "code"),
        [("bin".to_owned(), Directory), ("main.rs".to_owned(), File)]
    );
    assert_eq!(
        names(chain, "notes", &first.oid, "code/bin"),
        [("run.sh".to_owned(), Executable)],
        "a source keeps its files mode"
    );
    assert_eq!(
        committed(chain, "notes", &first.oid, "code", "main.rs"),
        "fn main() {}\n"
    );

    put(
        chain,
        "/home/Ada/ws/src/main.rs",
        "fn main() { run() }\n",
        false,
        &[],
    );
    let again = json!({
        "repo": "notes",
        "files": [{ "path": "code", "source": "/home/Ada/ws/src" }],
        "message": "The workspace again",
    });
    turn(chain, 5, &[("c2", "forge_commit", again)]);
    let (text, failed) = result_of(chain, thread, "c2");
    assert!(!failed, "{text}");
    let second = child(chain, "notes", &first.oid);
    assert_eq!(
        committed(chain, "notes", &second, "code", "main.rs"),
        "fn main() { run() }\n"
    );
    assert_eq!(
        names(chain, "notes", &second, ""),
        top,
        "a source directory never deletes what the workspace lacks"
    );

    let pinned = json!({
        "repo": "notes",
        "files": [{ "path": "old/main.rs", "source": "/home/Ada/ws/src/main.rs" }],
        "message": "As it was",
        "snapshot": abi::hex(cut.digest()),
    });
    turn(chain, 7, &[("c3", "forge_commit", pinned)]);
    let (text, failed) = result_of(chain, thread, "c3");
    assert!(!failed, "{text}");
    let third = child(chain, "notes", &second);
    assert_eq!(
        committed(chain, "notes", &third, "old", "main.rs"),
        "fn main() {}\n",
        "every source is read at the snapshot the call names"
    );
}

#[test]
fn a_missing_source_or_an_item_that_is_not_one_file_is_refused_and_nothing_is_committed() {
    let world = forged(10_000);
    let chain = &world.chain;
    let cut = put(chain, "/home/Ada/ws/a.md", "a\n", false, &[]);
    let thread = world.opened("m1", "commit", Some(WINDOW));
    let commit = |files: Value| json!({ "repo": "notes", "files": files, "message": "m" });
    let source = |path: &str| json!({ "path": path, "source": "/home/Ada/ws/a.md" });
    let calls = [
        (
            "c1",
            "forge_commit",
            commit(json!([{ "path": "b.md", "source": "/home/Ada/ws/b.md" }])),
        ),
        (
            "c2",
            "forge_commit",
            commit(json!([{ "path": "a.md", "source": "/home/Ada/ws/a.md", "content": "a\n" }])),
        ),
        (
            "c3",
            "forge_commit",
            commit(json!([{ "path": "a.md", "source": "/home/Ada/ws/a.md", "executable": true }])),
        ),
        (
            "c4",
            "forge_commit",
            commit(json!([source("a.md"), { "path": "a.md", "content": "b\n" }])),
        ),
        ("c5", "forge_commit", commit(json!([{ "path": "a.md" }]))),
    ];
    turn(chain, 3, &calls);
    let (missing, failed) = result_of(chain, thread, "c1");
    assert!(failed && missing.starts_with(code::NOT_FOUND), "{missing}");
    assert!(
        missing.contains("/home/Ada/ws/b.md") && missing.contains(&abi::hex(cut.digest())),
        "the refusal names the source and the snapshot it was read at: {missing}"
    );
    for (call, says) in [
        ("c2", "give only one of"),
        ("c3", "executable goes with content"),
        ("c4", "a.md is written twice"),
        ("c5", "give content, source, copy, blob or edits"),
    ] {
        let (text, failed) = result_of(chain, thread, call);
        assert!(failed && text.contains(says), "{call}: {text}");
    }
    let asked = forge::Query::Repo {
        repo: "notes".into(),
    };
    let refused = chain
        .query::<_, forge::Reply>(forge::PROGRAM, &asked)
        .unwrap_err();
    assert_eq!(
        refused.code,
        code::NOT_FOUND,
        "no call created the repository"
    );
}

#[test]
fn an_edit_replaces_inserts_and_appends_in_order_and_keeps_the_files_mode_and_meta() {
    let world = world();
    let chain = &world.chain;
    let board = "# Board\n- J-1 open\n- J-2 open\n";
    put(chain, "/shared/board.md", board, true, &[("kind", "board")]);
    let thread = world.opened("m1", "edit the board", Some(WINDOW));
    let edit = json!({
        "path": "/shared/board.md",
        "edits": [
            { "replace": "- J-1 open", "with": "- J-1 done" },
            { "after": "# Board\n", "insert": "Owner: chief\n" },
            { "before": "- J-2", "insert": "- J-1b open\n" },
            { "append": "- J-3 open\n" },
        ],
    });
    turn(chain, 3, &[("c1", "files_edit", edit)]);
    let (result, failed) = result_of(chain, thread, "c1");
    assert!(!failed, "{result}");
    assert_eq!(
        read_at(chain, "/shared/board.md"),
        "# Board\nOwner: chief\n- J-1 done\n- J-1b open\n- J-2 open\n- J-3 open\n"
    );
    let files::Reply::Head(Some(head)) = chain.query(files::PROGRAM, &files::Query::Head).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        serde_json::from_str::<Value>(&result).unwrap(),
        json!({ "snapshot": abi::hex(head.id.digest()) }),
        "the result names the snapshot, never the text"
    );
    let edited = entry_at(chain, "/shared/board.md");
    assert!(executable(&edited), "an edit keeps the mode");
    assert_eq!(
        meta_of(&edited),
        [("kind", "board")],
        "an edit keeps the meta"
    );
}

#[test]
fn an_edit_whose_span_is_missing_or_ambiguous_is_refused_and_commits_nothing() {
    let world = world();
    let chain = &world.chain;
    let board = "# Board\n- J-1 open\n- J-2 open\n";
    put(chain, "/shared/board.md", board, false, &[]);
    let thread = world.opened("m1", "edit the board", Some(WINDOW));
    let ambiguous = json!({
        "path": "/shared/board.md",
        "edits": [{ "replace": " open", "with": " done" }],
    });
    let missing = json!({
        "path": "/shared/board.md",
        "edits": [{ "append": "- J-3 open\n" }, { "replace": "- J-9", "with": "- J-10" }],
    });
    let nowhere = json!({ "path": "/shared/none.md", "edits": [{ "append": "x" }] });
    turn(
        chain,
        3,
        &[
            ("c1", "files_edit", ambiguous),
            ("c2", "files_edit", missing),
            ("c3", "files_edit", nowhere),
        ],
    );
    let (twice, failed) = result_of(chain, thread, "c1");
    assert!(
        failed && twice.contains("2 times, at lines 2, 3"),
        "{twice}"
    );
    let (absent, failed) = result_of(chain, thread, "c2");
    assert!(
        failed && absent.starts_with(code::NOT_FOUND) && absent.contains("edit 2"),
        "{absent}"
    );
    let (no_file, failed) = result_of(chain, thread, "c3");
    assert!(failed && no_file.starts_with(code::NOT_FOUND), "{no_file}");
    assert_eq!(
        read_at(chain, "/shared/board.md"),
        board,
        "a refused call commits none of its edits"
    );
}

#[test]
fn an_edit_reads_the_head_and_is_refused_once_the_file_moved_on_since_its_base() {
    let world = world();
    let chain = &world.chain;
    let old = put(chain, "/shared/notes.md", "one\n", false, &[]);
    let new = put(chain, "/shared/notes.md", "one\ntwo\n", false, &[]);
    let thread = world.opened("m1", "edit the notes", Some(WINDOW));
    let append = |base: &BlobId| {
        json!({
            "path": "/shared/notes.md",
            "edits": [{ "append": "three\n" }],
            "base": abi::hex(base.digest()),
        })
    };
    turn(chain, 3, &[("c1", "files_edit", append(&old))]);
    let (moved, failed) = result_of(chain, thread, "c1");
    assert!(failed && moved.starts_with(code::STALE), "{moved}");
    assert!(
        moved.contains(&abi::hex(new.digest())),
        "the refusal names the snapshot that moved the file: {moved}"
    );
    assert_eq!(read_at(chain, "/shared/notes.md"), "one\ntwo\n");
    turn(chain, 5, &[("c2", "files_edit", append(&new))]);
    let (text, failed) = result_of(chain, thread, "c2");
    assert!(!failed, "{text}");
    assert_eq!(read_at(chain, "/shared/notes.md"), "one\ntwo\nthree\n");
}

#[test]
fn history_list_and_find_answer_one_bounded_page_unless_given_a_limit() {
    let world = world();
    let chain = &world.chain;
    let listed = crate::tools::LISTED;
    for n in 0..=listed {
        put(chain, &format!("/shared/many/{n:02}"), "x", false, &[]);
    }
    let thread = world.opened("m1", "look around", Some(WINDOW));
    let tools = [
        ("files_history", "snapshots", json!({})),
        ("files_list", "entries", json!({ "path": "/shared/many" })),
        (
            "files_find",
            "entries",
            json!({ "prefix": "/shared/many/" }),
        ),
    ];
    let mut height = 3;
    let mut calls = 0;
    let mut next_call = || {
        calls += 1;
        format!("c{calls}")
    };
    for (tool, key, args) in tools {
        for limit in [None, Some(3)] {
            let mut input = args.clone();
            input["limit"] = limit.into();
            let call = next_call();
            turn(chain, height, &[(&call, tool, input.clone())]);
            height += 2;
            let first = said_by(chain, thread, &call);
            let first_items = first[key].as_array().unwrap();
            assert_eq!(first_items.len() as u64, limit.unwrap_or(listed));
            assert!(first["next"].is_string(), "{first}");
            input["cursor"] = first["next"].clone();
            let call = next_call();
            turn(chain, height, &[(&call, tool, input)]);
            height += 2;
            let second = said_by(chain, thread, &call);
            let second_items = second[key].as_array().unwrap();
            assert!(!second_items.is_empty(), "{second}");
            for item in second_items {
                assert!(!first_items.contains(item), "no repeated entry: {item}");
            }
            if limit.is_none() {
                assert!(second["next"].is_null(), "the last page: {second}");
            }
        }
        let mut input = args;
        input["limit"] = json!(0);
        let call = next_call();
        turn(chain, height, &[(&call, tool, input)]);
        height += 2;
        let (text, failed) = result_of(chain, thread, &call);
        assert!(failed && text.starts_with(code::INVALID_INPUT), "{text}");
    }
}

#[test]
fn a_rewrite_keeps_the_files_meta_and_its_mode_unless_the_call_gives_one() {
    let world = world();
    let chain = &world.chain;
    let meta = [("source.program", "chat")];
    put(chain, "/shared/run.sh", "echo one\n", true, &meta);
    let thread = world.opened("m1", "rewrite", Some(WINDOW));
    turn(
        chain,
        3,
        &[
            (
                "c1",
                "files_write",
                json!({ "path": "/shared/run.sh", "content": "echo two\n" }),
            ),
            (
                "c2",
                "files_write",
                json!({ "path": "/shared/new.md", "content": "new\n" }),
            ),
        ],
    );
    for call in ["c1", "c2"] {
        let (text, failed) = result_of(chain, thread, call);
        assert!(!failed, "{call}: {text}");
    }
    let run = entry_at(chain, "/shared/run.sh");
    assert_eq!(read_at(chain, "/shared/run.sh"), "echo two\n");
    assert!(executable(&run), "a rewrite keeps the mode");
    assert_eq!(meta_of(&run), meta, "a rewrite keeps the meta");
    let new = entry_at(chain, "/shared/new.md");
    assert!(!executable(&new) && new.meta.is_empty());

    let plain = json!({ "path": "/shared/run.sh", "content": "echo three\n", "executable": false });
    turn(chain, 5, &[("c3", "files_write", plain)]);
    let (text, failed) = result_of(chain, thread, "c3");
    assert!(!failed, "{text}");
    let run = entry_at(chain, "/shared/run.sh");
    assert!(!executable(&run), "a call that gives the mode sets it");
    assert_eq!(meta_of(&run), meta);
}

#[test]
fn a_write_that_names_a_list_writes_the_file_then_sets_its_list() {
    let mut world = world();
    world.chain.seat::<ownership::Ownership>(ownership::PROGRAM);
    let chain = &world.chain;
    let thread = world.opened("m1", "write and list", Some(WINDOW));
    let listed = json!({ "who": "listed", "accounts": [world.bo], "programs": ["chat"] });
    let input = json!({ "path": "/shared/listed.md", "content": "it's listed\n", "write": listed });
    turn(chain, 3, &[("c1", "files_write", input)]);
    let (text, failed) = result_of(chain, thread, "c1");
    assert!(!failed, "{text}");
    let answer: Value = serde_json::from_str(&text).unwrap();
    assert!(answer["snapshot"].is_string(), "{answer}");
    assert_eq!(answer["write"], "set");
    assert_eq!(read_at(chain, "/shared/listed.md"), "it's listed\n");
    let asked = access::Query::List {
        program: files::PROGRAM.into(),
        key: "/shared/listed.md".into(),
    };
    let access::Reply::List(list) = chain.query("access", &asked).unwrap() else {
        panic!()
    };
    let parties = [
        files::Party::Account(world.bo),
        files::Party::Program("chat".into()),
    ];
    let set = files::Ownership::Listed(parties.into_iter().collect());
    assert_eq!(list, Some(set));
    let asked = files::Query::Guarded {
        path: "/shared/listed.md".into(),
    };
    let files::Reply::Guarded(nearest) = chain.query(files::PROGRAM, &asked).unwrap() else {
        panic!("Guarded answers a path");
    };
    assert_eq!(nearest.as_deref(), Some("/shared/listed.md"));

    let plain = json!({ "path": "/shared/listed.md", "content": "again\n" });
    turn(chain, 5, &[("c2", "files_write", plain)]);
    let (text, failed) = result_of(chain, thread, "c2");
    assert!(!failed, "{text}");
    let answer: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(answer["write"], Value::Null, "no list, no list set");
}

fn said_by(chain: &MockChain, thread: u64, call: &str) -> Value {
    let (text, failed) = result_of(chain, thread, call);
    assert!(!failed, "{call}: {text}");
    serde_json::from_str(&text).unwrap()
}

#[test]
fn a_read_by_lines_answers_them_whole_where_a_chunk_cuts_a_character() {
    let world = world();
    let chain = &world.chain;
    let long = "x".repeat(files::CHUNK_SIZE as usize - 4);
    let text = format!("{long}\né🦆\nend\n");
    let at = put(chain, "/shared/wide.md", &text, false, &[]);
    let thread = world.opened("m1", "read by lines", Some(WINDOW));
    let by_lines = json!({ "path": "/shared/wide.md", "lines": [2, 9] });
    let cut = json!({ "path": "/shared/wide.md", "offset": files::CHUNK_SIZE - 1, "length": 2 });
    turn(
        chain,
        3,
        &[("c1", "files_read", by_lines), ("c2", "files_read", cut)],
    );
    assert_eq!(
        said_by(chain, thread, "c1"),
        json!({
            "snapshot": abi::hex(at.digest()),
            "size": text.len(),
            "lines": [2, 3],
            "total_lines": 3,
            "eof": true,
            "text": "é🦆\nend\n",
        })
    );
    assert_eq!(
        said_by(chain, thread, "c2")["binary_bytes"],
        json!(2),
        "a byte range that cuts a character reads as bytes"
    );
}

#[test]
fn a_read_by_heading_answers_its_section_and_one_way_of_cutting_the_file_is_taken() {
    let world = world();
    let chain = &world.chain;
    let design = "# Design\n## Step 1\none\n```md\n## Step 2\n```\n## Step 2\ntwo\n### Detail\nthree\n## Step 3\nfour\n";
    let path = "/shared/design.md";
    put(chain, path, design, false, &[]);
    let thread = world.opened("m1", "read a section", Some(WINDOW));
    let calls = [
        (
            "c1",
            "files_read",
            json!({ "path": path, "heading": "## Step 2" }),
        ),
        (
            "c2",
            "files_read",
            json!({ "path": path, "heading": "## Step 9" }),
        ),
        (
            "c3",
            "files_read",
            json!({ "path": path, "lines": [2, 3], "heading": "## Step 2" }),
        ),
        (
            "c4",
            "files_read",
            json!({ "path": path, "lines": [2, 3], "offset": 0 }),
        ),
        ("c5", "files_read", json!({ "path": path, "lines": [3, 2] })),
    ];
    turn(chain, 3, &calls);
    let said = said_by(chain, thread, "c1");
    assert_eq!(
        said["text"],
        json!("## Step 2\ntwo\n### Detail\nthree\n"),
        "the section runs past a deeper heading, and a fenced copy heads nothing"
    );
    assert_eq!(
        (
            said["lines"].clone(),
            said["total_lines"].clone(),
            said["eof"].clone()
        ),
        (json!([7, 10]), json!(12), json!(false))
    );
    let (missing, failed) = result_of(chain, thread, "c2");
    assert!(failed && missing.starts_with(code::NOT_FOUND), "{missing}");
    for (call, says) in [
        ("c3", "lines or a heading, not both"),
        ("c4", "or a byte range, not both"),
        ("c5", "with to at least from"),
    ] {
        let (text, failed) = result_of(chain, thread, call);
        assert!(failed && text.contains(says), "{call}: {text}");
    }
}

#[test]
fn a_read_naming_no_slice_answers_a_long_file_as_its_headings_and_whole_reads_it_whole() {
    let world = world();
    let chain = &world.chain;
    let bound = crate::tools::WHOLE_UP_TO as usize;
    let filler = "word ".repeat(bound / 5 + 1);
    let text =
        format!("# Plan\n{filler}\n## Step 1\none\n```sh\n# not a heading\n```\n## Step 2\ntwo\n");
    put(chain, "/shared/long.md", &text, false, &[]);
    let script = format!(
        "#!/bin/sh\n# usage: run it\n{}",
        "echo hi\n".repeat(bound / 8 + 1)
    );
    put(chain, "/shared/long.sh", &script, false, &[]);
    let edge = "z".repeat(bound);
    let head = put(chain, "/shared/edge.md", &edge, false, &[]);
    let thread = world.opened("m1", "read whole", Some(WINDOW));
    let long = |more: Value| {
        let mut asked = json!({ "path": "/shared/long.md" });
        asked
            .as_object_mut()
            .unwrap()
            .extend(more.as_object().unwrap().clone());
        asked
    };
    turn(
        chain,
        3,
        &[
            ("c1", "files_read", long(json!({}))),
            ("c2", "files_read", long(json!({ "whole": true }))),
            ("c3", "files_read", json!({ "path": "/shared/edge.md" })),
            (
                "c4",
                "files_read",
                long(json!({ "whole": true, "lines": [1, 2] })),
            ),
            ("c5", "files_read", json!({ "path": "/shared/long.sh" })),
        ],
    );
    let outlined = said_by(chain, thread, "c1");
    let headings = json!([
        { "heading": "# Plan", "line": 1 },
        { "heading": "## Step 1", "line": 3 },
        { "heading": "## Step 2", "line": 8 },
    ]);
    assert_eq!(
        outlined["headings"], headings,
        "a fenced line is no heading"
    );
    assert_eq!(outlined["total_lines"], json!(9));
    assert_eq!(outlined["size"], json!(text.len()));
    assert_eq!(
        outlined["snapshot"],
        json!(abi::hex(head.digest())),
        "the head it read"
    );
    assert_eq!(
        outlined["text"],
        Value::Null,
        "a long file's text is left out"
    );
    assert_eq!(said_by(chain, thread, "c2")["text"], json!(text));
    assert_eq!(
        said_by(chain, thread, "c3")["text"],
        json!(edge),
        "a file at the bound reads whole"
    );
    let plain = said_by(chain, thread, "c5");
    assert_eq!(
        (plain["headings"].clone(), plain["text"].clone()),
        (Value::Null, Value::Null),
        "a long file that is no markdown answers no headings, its comments included"
    );
    assert_eq!(
        plain["total_lines"],
        json!(script.split_inclusive('\n').count())
    );
    let (refused, failed) = result_of(chain, thread, "c4");
    assert!(
        failed && refused.contains("whole reads the file whole"),
        "{refused}"
    );
}
