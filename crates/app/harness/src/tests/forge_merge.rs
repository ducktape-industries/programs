//! `forge_commit` with `merge`: a merge commit whose second parent is a commit forge holds, its tree
//! a 3-way merge of the two that the given files and edits resolve; an unresolved conflict and a
//! merge naming anything but a commit the branch doesn't hold yet are refused, naming it.
use forge::{CommitInfo, Mergeability, Revision};

use super::forge_tools::{answered, file_at, head, refusal, seeded, turn, world};
use super::*;

fn commit_at(chain: &MockChain, oid: &str) -> CommitInfo {
    let asked = forge::Query::Log {
        repo: "notes".into(),
        from: Revision::Oid(oid.into()),
        exclude: None,
        page: PageRequest::first(1),
    };
    let forge::Reply::Log { page, .. } = chain.query(forge::PROGRAM, &asked).unwrap() else {
        panic!("a log answers Log")
    };
    page.items.into_iter().next().unwrap()
}

fn committed(chain: &MockChain, thread: u64, call: &str) -> String {
    answered(chain, thread, call)["commit"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn a_merge_takes_the_other_branch_whole_and_the_given_files_resolve_its_conflicts() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "merge main in", Some(WINDOW));
    seeded(&world, thread);
    let feature = json!({
        "repo": "notes",
        "branch": "feature",
        "from": "main",
        "files": [{ "path": "README.md", "content": "# Notes\nfeature line\n" }],
        "message": "feature",
    });
    turn(chain, 7, &[("f1", "forge_commit", feature)]);
    let feature = committed(chain, thread, "f1");
    let moved = json!({
        "repo": "notes",
        "files": [
            { "path": "README.md", "content": "# Notes\nmain line\n" },
            { "path": "docs/main.md", "content": "main's own\n" },
        ],
        "message": "main moves",
    });
    turn(chain, 9, &[("f2", "forge_commit", moved)]);
    let main = committed(chain, thread, "f2");
    let open = json!({ "repo": "notes", "from": "feature", "into": "main", "title": "feature" });
    turn(chain, 11, &[("o1", "forge_change_open", open)]);
    assert_eq!(answered(chain, thread, "o1")["number"], json!(1));
    let preview = forge::Op::Preview {
        repo: "notes".into(),
        n: 1,
    };
    let conflicted = chain
        .submit(signed(ADA), forge::PROGRAM, &preview)
        .unwrap_err();
    assert!(
        conflicted.code == code::WRONG_STATE && format!("{conflicted:?}").contains("README.md"),
        "{conflicted:?}"
    );

    let unresolved = json!({
        "repo": "notes",
        "branch": "feature",
        "merge": "main",
        "files": [{ "path": "later.md", "content": "later\n" }],
        "message": "merge main",
    });
    turn(chain, 13, &[("c1", "forge_commit", unresolved)]);
    let text = refusal(chain, thread, "c1");
    assert!(
        text.starts_with(code::WRONG_STATE) && text.contains("README.md"),
        "{text}"
    );
    assert_eq!(head(chain, "notes", "feature"), feature);

    let resolved = json!({
        "repo": "notes",
        "branch": "feature",
        "merge": "main",
        "files": [{ "path": "README.md", "edits": [{
            "replace": "<<<<<<< ours\nfeature line\n=======\nmain line\n>>>>>>> theirs\n",
            "with": "main line\nfeature line\n",
        }] }],
        "message": "merge main",
    });
    turn(chain, 15, &[("c2", "forge_commit", resolved)]);
    let said = answered(chain, thread, "c2");
    let merge = said["commit"].as_str().unwrap().to_owned();
    assert_eq!(
        (said["parent"].clone(), said["merged"].clone()),
        (json!(feature), json!(main))
    );
    assert_eq!(head(chain, "notes", "feature"), merge);
    assert_eq!(commit_at(chain, &merge).parents, [feature, main.clone()]);
    assert_eq!(
        file_at(chain, "notes", &merge, "README.md").0,
        "# Notes\nmain line\nfeature line\n"
    );
    assert_eq!(
        file_at(chain, "notes", &merge, "docs/main.md").0,
        "main's own\n"
    );

    let compared = forge::Query::Compare {
        repo: "notes".into(),
        from: Revision::Ref(b"refs/heads/feature".to_vec()),
        into: Revision::Ref(b"refs/heads/main".to_vec()),
    };
    let forge::Reply::Compare { comparison, .. } = chain.query(forge::PROGRAM, &compared).unwrap()
    else {
        panic!("a compare answers Compare")
    };
    assert_eq!(
        (
            comparison.mergeability,
            comparison.behind,
            comparison.base.clone()
        ),
        (Mergeability::FastForward, 0, Some(main))
    );
    let made = chain.submit(signed(ADA), forge::PROGRAM, &preview).unwrap();
    let forge::OpReply::Previewed { oid, .. } = abi::decode(&made).unwrap() else {
        panic!("a preview answers Previewed")
    };
    assert_eq!(commit_at(chain, &oid).tree, commit_at(chain, &merge).tree);
}

#[test]
fn a_merge_marks_each_line_clash_and_keeps_the_clean_rest_of_every_file() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "merge with marks", Some(WINDOW));
    seeded(&world, thread);
    let cargo = "[dependencies]\nserde = 1\n\n[workspace]\nmembers = [\n  \"a\",\n]\n";
    let start = json!({
        "repo": "notes",
        "files": [
            { "path": "Cargo.toml", "content": cargo },
            { "path": "notes.md", "content": "1\n2\n3\n4\n5\n" },
            { "path": "gone.md", "content": "old\n" },
        ],
        "message": "the shared start",
    });
    turn(chain, 7, &[("s3", "forge_commit", start)]);
    answered(chain, thread, "s3");
    let ours_cargo = cargo.replace("  \"a\",\n", "  \"a\",\n  \"c\",\n");
    let feature = json!({
        "repo": "notes",
        "branch": "feature",
        "from": "main",
        "files": [
            { "path": "Cargo.toml", "content": ours_cargo },
            { "path": "notes.md", "content": "one\n2\n3\n4\n5\n" },
        ],
        "remove": ["gone.md"],
        "message": "feature",
    });
    turn(chain, 9, &[("f1", "forge_commit", feature)]);
    let feature = committed(chain, thread, "f1");
    let theirs_cargo = cargo
        .replace("serde = 1\n", "serde = 1\ntokio = 1\n")
        .replace("  \"a\",\n", "  \"a\",\n  \"b\",\n");
    let moved = json!({
        "repo": "notes",
        "files": [
            { "path": "Cargo.toml", "content": theirs_cargo },
            { "path": "notes.md", "content": "1\n2\n3\n4\nfive\n" },
            { "path": "gone.md", "content": "changed\n" },
        ],
        "message": "main moves",
    });
    turn(chain, 11, &[("f2", "forge_commit", moved)]);
    let main = committed(chain, thread, "f2");

    let unresolved = json!({
        "repo": "notes",
        "branch": "feature",
        "merge": "main",
        "files": [],
        "message": "merge main",
    });
    turn(chain, 13, &[("c1", "forge_commit", unresolved)]);
    let text = refusal(chain, thread, "c1");
    let named = ["Cargo.toml (", "gone.md (", "notes.md ("].map(|path| text.contains(path));
    assert_eq!(named, [true, true, false], "{text}");
    assert!(text.starts_with(code::WRONG_STATE), "{text}");
    assert_eq!(head(chain, "notes", "feature"), feature);

    let still_marked = json!({
        "repo": "notes",
        "branch": "feature",
        "merge": "main",
        "files": [{ "path": "Cargo.toml", "edits": [{ "replace": "serde = 1\n", "with": "serde = 2\n" }] }],
        "remove": ["gone.md"],
        "message": "merge main",
    });
    turn(chain, 15, &[("c3", "forge_commit", still_marked)]);
    let text = refusal(chain, thread, "c3");
    assert!(
        text.starts_with(code::WRONG_STATE) && text.contains("markers in Cargo.toml"),
        "{text}"
    );
    assert_eq!(head(chain, "notes", "feature"), feature);

    let marks = "<<<<<<< ours\n  \"c\",\n=======\n  \"b\",\n>>>>>>> theirs\n";
    let resolved = json!({
        "repo": "notes",
        "branch": "feature",
        "merge": "main",
        "files": [
            { "path": "Cargo.toml", "edits": [{ "replace": marks, "with": "  \"b\",\n  \"c\",\n" }] },
            { "path": "notes.md", "edits": [{ "replace": "3\n", "with": "three\n" }] },
        ],
        "remove": ["gone.md"],
        "message": "merge main",
    });
    turn(chain, 17, &[("c2", "forge_commit", resolved)]);
    let merge = committed(chain, thread, "c2");
    assert_eq!(commit_at(chain, &merge).parents, [feature, main]);
    let merged_cargo = theirs_cargo.replace("  \"b\",\n", "  \"b\",\n  \"c\",\n");
    assert_eq!(
        file_at(chain, "notes", &merge, "Cargo.toml").0,
        merged_cargo
    );
    assert_eq!(
        file_at(chain, "notes", &merge, "notes.md").0,
        "one\n2\nthree\n4\nfive\n"
    );
    let root = forge::Query::Tree {
        repo: "notes".into(),
        at: merge.clone(),
        path: Vec::new(),
        page: PageRequest::first(64),
    };
    let forge::Reply::Tree { page, .. } = chain.query(forge::PROGRAM, &root).unwrap() else {
        panic!("a tree answers Tree")
    };
    assert!(page.items.iter().all(|entry| entry.name != b"gone.md"));

    let replaced = b"one\n2\n3\n4\nfive\n";
    let replaced = gitcore::oid_of(gitcore::Hash::Sha1, gitcore::Kind::Blob, replaced).unwrap();
    let asked = forge::Query::Blob {
        repo: "notes".into(),
        oid: replaced.to_hex(),
        range: None,
    };
    assert!(
        chain
            .query::<_, forge::Reply>(forge::PROGRAM, &asked)
            .is_err(),
        "the merged notes.md the edit replaced was pushed"
    );
}

#[test]
fn a_merge_of_anything_but_a_commit_the_branch_lacks_is_refused_naming_it() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "merge wrongly", Some(WINDOW));
    let dev = seeded(&world, thread);
    let main = head(chain, "notes", "main");
    let blob = file_at(chain, "notes", &main, "README.md").1;
    let unknown = "0123456789abcdef0123456789abcdef01234567";
    let merging = |merge: &str| {
        json!({
            "repo": "notes",
            "merge": merge,
            "files": [{ "path": "x.md", "content": "x\n" }],
            "message": "merge",
        })
    };
    turn(
        chain,
        7,
        &[
            ("r1", "forge_commit", merging("nowhere")),
            ("r2", "forge_commit", merging(unknown)),
            ("r3", "forge_commit", merging(&blob)),
            ("r4", "forge_commit", merging("main")),
        ],
    );
    let text = refusal(chain, thread, "r1");
    assert!(
        text.starts_with(code::NOT_FOUND) && text.contains("nowhere"),
        "{text}"
    );
    let text = refusal(chain, thread, "r2");
    assert!(
        text.starts_with(code::NOT_FOUND) && text.contains(unknown),
        "{text}"
    );
    let text = refusal(chain, thread, "r3");
    assert!(
        text.starts_with(code::INVALID_INPUT) && text.contains(&blob),
        "{text}"
    );
    let text = refusal(chain, thread, "r4");
    assert!(
        text.starts_with(code::WRONG_STATE) && text.contains("main"),
        "{text}"
    );
    assert_eq!(head(chain, "notes", "main"), main);

    let clean = json!({ "repo": "notes", "merge": "dev", "files": [], "message": "merge dev" });
    turn(chain, 9, &[("c1", "forge_commit", clean)]);
    let merge = committed(chain, thread, "c1");
    assert_eq!(commit_at(chain, &merge).parents, [main, dev]);
    assert_eq!(
        file_at(chain, "notes", &merge, "src/a.rs").0,
        "pub fn a() {}\n"
    );
}
