//! Each object on its path's line: a newer put lets the old version go, a remove or a move lets
//! go of every line beneath, a base is checked by the rows commits wrote, a diff folds them, and
//! `HoldHead` puts the head's objects back on their lines.
use ::program::ProgramId;

use super::*;
use crate::objects::{line, under};
use crate::paths::segments;

fn lines(host: &MockHost) -> BTreeMap<Vec<u8>, BlobId> {
    let files: ProgramId = PROGRAM.into();
    host.borrow()
        .lines
        .iter()
        .filter(|((holder, _), _)| *holder == files)
        .map(|((_, line), id)| (line.clone(), *id))
        .collect()
}

fn on(path: &str) -> Vec<u8> {
    line(&segments(path).unwrap())
}

fn beneath(path: &str) -> Vec<u8> {
    under(&segments(path).unwrap())
}

fn held(host: &MockHost, id: &BlobId) -> bool {
    host.borrow().blobs.contains_key(id)
}

fn file_of(host: &MockHost, path: &str) -> BlobId {
    let Reply::Stat {
        entry: Some(entry), ..
    } = ask(
        host,
        Query::Stat {
            path: path.into(),
            at: None,
        },
    )
    .unwrap()
    else {
        panic!("nothing at {path}")
    };
    let Node::File { file, .. } = entry.node else {
        panic!("{path} is a {}", entry.node.kind())
    };
    file
}

fn root_of(host: &MockHost) -> BlobId {
    let Reply::Head(Some(row)) = ask(host, Query::Head).unwrap() else {
        panic!("no head")
    };
    row.root
}

fn diffed(host: &MockHost, from: BlobId, to: BlobId) -> Vec<(String, DiffKind)> {
    let Reply::Diff(entries) = ask(
        host,
        Query::Diff {
            from,
            to,
            prefix: "/shared/".into(),
        },
    )
    .unwrap() else {
        panic!("not a diff")
    };
    entries
        .into_iter()
        .map(|entry| (entry.path, entry.kind))
        .collect()
}

fn moved(from: &str, to: &str) -> Change {
    Change::Move {
        from: from.into(),
        to: to.into(),
    }
}

fn removed(path: &str) -> Change {
    Change::Remove { path: path.into() }
}

#[test]
fn each_object_sits_on_its_paths_line_and_a_newer_put_lets_the_old_one_go() {
    let host = host();
    let first = commit(
        &host,
        ada(),
        None,
        vec![put(&host, "/shared/a", "1"), put(&host, "/shared/d/b", "2")],
    )
    .unwrap();
    let mut expected: Vec<Vec<u8>> = ["/", "/shared", "/shared/a", "/shared/d", "/shared/d/b"]
        .into_iter()
        .map(on)
        .collect();
    expected.push(b"s".to_vec());
    expected.sort();
    let keys = |host: &MockHost| lines(host).into_keys().collect::<Vec<_>>();
    assert_eq!(keys(&host), expected);
    let old = file_of(&host, "/shared/a");
    let old_root = root_of(&host);
    let untouched = file_of(&host, "/shared/d/b");
    assert_eq!(lines(&host)[&on("/shared/a")], old);
    commit(
        &host,
        ada(),
        Some(first),
        vec![put(&host, "/shared/a", "3")],
    )
    .unwrap();
    assert_eq!(keys(&host), expected);
    assert!(!held(&host, &old), "the old version of /shared/a is let go");
    assert!(!held(&host, &old_root), "the old root is let go");
    assert!(!held(&host, &first), "the old snapshot is let go");
    assert!(held(&host, &untouched));
    assert_eq!(text(&host, "/shared/a", None).unwrap(), "3");
    let pruned = text(&host, "/shared/a", Some(first)).unwrap_err();
    assert_eq!(pruned.code, code::NOT_FOUND);
    assert!(
        pruned.message.contains("pruned on this node"),
        "{}",
        pruned.message
    );
}

#[test]
fn a_move_holds_its_subtree_where_it_lands_and_a_remove_lets_go_of_everything_beneath() {
    let host = host();
    commit(
        &host,
        ada(),
        None,
        vec![
            put(&host, "/shared/d/x", "x"),
            put(&host, "/shared/d/e/y", "y"),
        ],
    )
    .unwrap();
    let x = file_of(&host, "/shared/d/x");
    let base = head(&host);
    commit(&host, ada(), base, vec![moved("/shared/d", "/shared/m")]).unwrap();
    let after = lines(&host);
    assert!(
        after
            .keys()
            .all(|line| !line.starts_with(&beneath("/shared/d"))),
        "nothing stays on the old lines"
    );
    for path in ["/shared/m", "/shared/m/x", "/shared/m/e", "/shared/m/e/y"] {
        assert!(after.contains_key(&on(path)), "{path}");
    }
    assert_eq!(after[&on("/shared/m/x")], x);
    assert_eq!(text(&host, "/shared/m/e/y", None).unwrap(), "y");
    let base = head(&host);
    commit(&host, ada(), base, vec![removed("/shared/m")]).unwrap();
    assert!(
        lines(&host)
            .keys()
            .all(|line| !line.starts_with(&beneath("/shared/m")))
    );
    assert!(!held(&host, &x));
}

#[test]
fn writes_into_a_folder_moved_in_the_same_commit_land_on_the_new_lines() {
    let host = host();
    commit(&host, ada(), None, vec![put(&host, "/shared/d/x", "x")]).unwrap();
    let base = head(&host);
    commit(
        &host,
        ada(),
        base,
        vec![
            moved("/shared/d", "/shared/m"),
            put(&host, "/shared/m/z", "z"),
            put(&host, "/shared/n/w", "w"),
        ],
    )
    .unwrap();
    let base = head(&host);
    commit(
        &host,
        ada(),
        base,
        vec![
            put(&host, "/shared/n/v", "v"),
            moved("/shared/n", "/shared/o"),
        ],
    )
    .unwrap();
    for (path, written) in [
        ("/shared/m/x", "x"),
        ("/shared/m/z", "z"),
        ("/shared/o/w", "w"),
        ("/shared/o/v", "v"),
    ] {
        assert_eq!(text(&host, path, None).unwrap(), written);
        assert_eq!(
            lines(&host).get(&on(path)),
            Some(&file_of(&host, path)),
            "{path}"
        );
    }
    for gone in ["/shared/d", "/shared/n"] {
        assert!(
            lines(&host)
                .keys()
                .all(|line| !line.starts_with(&beneath(gone))),
            "{gone}"
        );
    }
}

fn moved_on() -> (MockHost, BlobId) {
    let host = host();
    let first = commit(
        &host,
        ada(),
        None,
        vec![put(&host, "/shared/d/x", "1"), put(&host, "/shared/y", "1")],
    )
    .unwrap();
    commit(&host, bo(), Some(first), vec![put(&host, "/shared/y", "2")]).unwrap();
    (host, first)
}

#[test]
fn a_base_is_checked_by_the_rows_commits_wrote_without_reading_a_tree() {
    let gets = |base: fn(&MockHost, BlobId) -> Option<BlobId>| {
        let (host, first) = moved_on();
        let change = put(&host, "/shared/d/x", "2");
        let before = host.borrow().work.blob_gets;
        commit(&host, ada(), base(&host, first), vec![change]).unwrap();
        host.borrow().work.blob_gets - before
    };
    let from_an_older_base = gets(|_, first| Some(first));
    let from_the_head = gets(|host, _| head(host));
    assert_eq!(from_an_older_base, from_the_head);

    let (host, first) = moved_on();
    commit(&host, bo(), head(&host), vec![removed("/shared/d")]).unwrap();
    let under_a_removed_folder = Op::Commit {
        base: Some(first),
        message: String::new(),
        changes: vec![put(&host, "/shared/d/x", "3")],
    };
    assert_eq!(
        refused(&host, ada(), under_a_removed_folder).code,
        code::STALE
    );

    let (host, first) = moved_on();
    commit(
        &host,
        bo(),
        head(&host),
        vec![put(&host, "/shared/d/q", "q")],
    )
    .unwrap();
    let a_folder_written_beneath = Op::Commit {
        base: Some(first),
        message: String::new(),
        changes: vec![removed("/shared/d")],
    };
    assert_eq!(
        refused(&host, ada(), a_folder_written_beneath).code,
        code::STALE
    );
    commit(
        &host,
        ada(),
        Some(first),
        vec![put(&host, "/shared/d/x", "3")],
    )
    .unwrap();
    assert_eq!(text(&host, "/shared/d/q", None).unwrap(), "q");
}

#[test]
fn a_base_older_than_the_first_recorded_commit_is_stale_and_its_diff_answers_pruned() {
    let (host, first) = moved_on();
    let second = head(&host).unwrap();
    crate::state::RECORDED_FROM.put(&host.exec(ada()), &3);
    let op = Op::Commit {
        base: Some(first),
        message: String::new(),
        changes: vec![put(&host, "/shared/z", "z")],
    };
    assert_eq!(refused(&host, ada(), op).code, code::STALE);
    let old = ask(
        &host,
        Query::Diff {
            from: first,
            to: second,
            prefix: String::new(),
        },
    )
    .unwrap_err();
    assert_eq!(old.code, code::NOT_FOUND, "{}", old.message);
}

#[test]
fn a_diff_folds_what_each_commit_recorded_both_ways() {
    let host = host();
    let first = commit(
        &host,
        ada(),
        None,
        vec![
            put(&host, "/shared/keep", "k"),
            put(&host, "/shared/gone/x", "x"),
        ],
    )
    .unwrap();
    let second = commit(
        &host,
        ada(),
        Some(first),
        vec![
            put(&host, "/shared/brief/a", "a"),
            put(&host, "/shared/keep", "k2"),
        ],
    )
    .unwrap();
    let third = commit(
        &host,
        ada(),
        Some(second),
        vec![
            removed("/shared/brief"),
            removed("/shared/gone"),
            moved("/shared/keep", "/shared/kept"),
        ],
    )
    .unwrap();
    let named = |entries: &[(&str, DiffKind)]| -> Vec<(String, DiffKind)> {
        entries
            .iter()
            .map(|(path, kind)| ((*path).to_owned(), *kind))
            .collect()
    };
    assert_eq!(
        diffed(&host, first, second),
        named(&[
            ("/shared/brief", DiffKind::Added),
            ("/shared/brief/a", DiffKind::Added),
            ("/shared/keep", DiffKind::Modified),
        ])
    );
    assert_eq!(
        diffed(&host, first, third),
        named(&[
            ("/shared/gone", DiffKind::Removed),
            ("/shared/keep", DiffKind::Removed),
            ("/shared/kept", DiffKind::Added),
        ])
    );
    assert_eq!(
        diffed(&host, third, first),
        named(&[
            ("/shared/gone", DiffKind::Added),
            ("/shared/keep", DiffKind::Added),
            ("/shared/kept", DiffKind::Removed),
        ])
    );
}

#[test]
fn hold_head_puts_the_heads_objects_back_on_their_lines_one_folder_a_call() {
    let host = held_tree();
    let before = lines(&host);
    unheld(&host);
    assert_eq!(
        chain(&host, None),
        ["/", "/shared", "/shared/d", "/shared/d/e", "/shared/z"]
    );
    assert_eq!(lines(&host), before);
    all_held(&host);
    assert_eq!(
        chain(&host, None).len(),
        5,
        "a second chain changes nothing"
    );
    assert_eq!(lines(&host), before);
}

#[test]
fn hold_head_stops_at_a_cursor_and_resumes_past_a_commit_that_moved_the_head() {
    let host = held_tree();
    unheld(&host);
    let (held, next) = step(&host, None);
    assert_eq!(
        (held.as_deref(), next.clone()),
        (Some("/"), Some(Some("/".into())))
    );
    let (held, _) = step(&host, Some("/".into()));
    let (held_d, _) = step(&host, held);
    assert_eq!(held_d.as_deref(), Some("/shared/d"));
    let base = head(&host);
    commit(
        &host,
        ada(),
        base,
        vec![removed("/shared/d"), put(&host, "/shared/c/n", "n")],
    )
    .unwrap();
    assert_eq!(chain(&host, held_d), ["/shared/z"]);
    all_held(&host);
}

#[test]
fn two_hold_head_chains_at_once_each_end_and_leave_every_object_held() {
    let host = held_tree();
    let before = lines(&host);
    unheld(&host);
    let (mut one, mut two) = (None, None);
    let (mut ones, mut twos) = (Vec::new(), Vec::new());
    loop {
        let (held_one, _) = step(&host, one.clone());
        let (held_two, _) = step(&host, two.clone());
        let both_ended = held_one.is_none() && held_two.is_none();
        if both_ended {
            break;
        }
        ones.extend(held_one.clone());
        twos.extend(held_two.clone());
        (one, two) = (held_one.or(one), held_two.or(two));
    }
    assert_eq!(ones, twos);
    assert_eq!(ones.len(), 5);
    assert_eq!(lines(&host), before);
    all_held(&host);
}

#[test]
fn hold_head_on_an_empty_filesystem_holds_nothing_and_books_nothing() {
    let host = host();
    assert_eq!(step(&host, None), (None, None));
    assert!(lines(&host).is_empty());
}

fn held_tree() -> MockHost {
    let host = host();
    commit(
        &host,
        ada(),
        None,
        vec![
            put(&host, "/shared/a", "1"),
            put(&host, "/shared/d/b", "2"),
            put(&host, "/shared/d/e/c", "3"),
            put(&host, "/shared/z/f", "4"),
        ],
    )
    .unwrap();
    host
}

fn unheld(host: &MockHost) {
    let exec = host.exec(ada());
    for id in lines(host).values() {
        let (kind, body) = {
            let state = host.borrow();
            let blob = &state.blobs[id];
            (blob.kind.clone(), blob.body.clone())
        };
        exec.blob_put(HashKind::Sha256, kind, body).unwrap();
    }
    exec.blob_release_lines(Vec::new());
    assert!(lines(host).is_empty());
}

fn booked(emitted: &[abi::Message]) -> Option<Option<String>> {
    let [message] = emitted else {
        assert!(emitted.is_empty(), "{emitted:?}");
        return None;
    };
    assert_eq!(message.target, MockHost::role(abi::role::SCHEDULER));
    let abi::role::scheduler::Op::Schedule {
        height, payload, ..
    } = abi::decode(&message.payload).unwrap()
    else {
        panic!("not a Schedule")
    };
    assert_eq!(height, ada().height + 1);
    let Op::HoldHead { after } = abi::decode(&payload).unwrap() else {
        panic!("not a HoldHead")
    };
    Some(after)
}

fn step(host: &MockHost, after: Option<String>) -> (Option<String>, Option<Option<String>>) {
    host.take_emissions();
    let op = Op::HoldHead { after };
    host.attempt(|| Files::execute(&host.exec(ada()), op))
        .unwrap();
    let held: Option<String> = abi::decode(&host.take_output()).unwrap();
    let next = booked(&host.take_emissions());
    (held, next)
}

fn chain(host: &MockHost, from: Option<String>) -> Vec<String> {
    let mut held = Vec::new();
    let mut after = from;
    loop {
        let (folder, next) = step(host, after);
        let Some(folder) = folder else {
            assert_eq!(next, None, "the last call books nothing");
            return held;
        };
        assert_eq!(next, Some(Some(folder.clone())), "each call books the next");
        held.push(folder.clone());
        after = Some(folder);
    }
}

fn all_held(host: &MockHost) {
    let held = lines(host);
    let Reply::Head(Some(row)) = ask(host, Query::Head).unwrap() else {
        panic!("no head")
    };
    assert_eq!(held.get(b"s".as_slice()), Some(&row.id));
    assert_eq!(held.get(&on("/")), Some(&row.root));
    let Reply::Listing { entries, .. } = ask(
        host,
        Query::Find {
            prefix: "/".into(),
            at: None,
            page: PageRequest::default(),
        },
    )
    .unwrap() else {
        panic!("not a listing")
    };
    for entry in entries.items {
        let id = match entry.node {
            Node::File { file, .. } => file,
            Node::Dir { tree, .. } => tree,
            Node::Symlink { .. } => continue,
        };
        assert_eq!(held.get(&on(&entry.path)), Some(&id), "{}", entry.path);
    }
}
