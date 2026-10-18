//! The reads that answer text: refs by a prefix of their names, and a diff's files with their
//! counts and no hunks.
mod common;
use common::*;
use forge::{Content, FileStatus, RefName};

// A repository with `main` on a root commit, `j-1/a` and `j-1/b` one commit on, and `j-10` at
// the root: answers the sandbox, the root and the next commit.
fn seeded() -> (MemorySandbox, Oid, Oid) {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut store, &[], 1, &[("a", b"1\n2\n3\n"), ("old", b"x\n")]);
    let files: &[(&str, &[u8])] = &[("a", b"1\ntwo\n3\nfour\n"), ("new", b"n\n")];
    let next = file_commit(&mut store, &[root], 2, files);
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let zero = Hash::Sha1.zero();
    let refs = [
        (zero, root, "refs/heads/main"),
        (zero, next, "refs/heads/j-1/a"),
        (zero, next, "refs/heads/j-1/b"),
        (zero, root, "refs/heads/j-10"),
    ];
    let pack = pack_of(&store, &all_ids(&store));
    push(&sandbox, OWNER, "project", &refs, &pack).unwrap();
    (sandbox, root, next)
}

fn ref_names(sandbox: &MemorySandbox, prefix: &str, page: PageRequest) -> (Vec<RefName>, bool) {
    let query = Query::RefNames {
        repo: "project".into(),
        prefix: prefix.into(),
        page,
    };
    let reply: Reply = abi::decode(&ask(sandbox, &query).unwrap()).unwrap();
    let Reply::RefNames { page, .. } = reply else {
        panic!("{reply:?}");
    };
    (page.items, page.next.is_some())
}

fn names(refs: &[RefName]) -> Vec<&str> {
    refs.iter().map(|named| named.name.as_str()).collect()
}

#[test]
fn ref_names_answers_the_refs_a_prefix_starts_in_name_order_and_pages() {
    let (sandbox, root, next) = seeded();
    let (all, more) = ref_names(&sandbox, "", PageRequest::first(10));
    assert!(!more);
    assert_eq!(
        names(&all),
        [
            "refs/heads/j-1/a",
            "refs/heads/j-1/b",
            "refs/heads/j-10",
            "refs/heads/main"
        ]
    );
    let (ours, _) = ref_names(&sandbox, "refs/heads/j-1/", PageRequest::first(10));
    assert_eq!(names(&ours), ["refs/heads/j-1/a", "refs/heads/j-1/b"]);
    assert_eq!(ours[0].target, next.to_hex());
    let (both, _) = ref_names(&sandbox, "refs/heads/j-1", PageRequest::first(10));
    assert_eq!(
        names(&both),
        ["refs/heads/j-1/a", "refs/heads/j-1/b", "refs/heads/j-10"]
    );
    let (main, _) = ref_names(&sandbox, "refs/heads/main", PageRequest::first(10));
    assert_eq!(
        main,
        [RefName {
            name: "refs/heads/main".into(),
            target: root.to_hex()
        }]
    );
    assert!(
        ref_names(&sandbox, "refs/tags/", PageRequest::first(10))
            .0
            .is_empty()
    );

    let query = Query::RefNames {
        repo: "project".into(),
        prefix: "refs/heads/j-1".into(),
        page: PageRequest::first(2),
    };
    let reply: Reply = abi::decode(&ask(&sandbox, &query).unwrap()).unwrap();
    let Reply::RefNames { page, .. } = reply else {
        panic!("{reply:?}");
    };
    assert_eq!(names(&page.items), ["refs/heads/j-1/a", "refs/heads/j-1/b"]);
    let rest = PageRequest::resume(page.next, 2);
    let (last, more) = ref_names(&sandbox, "refs/heads/j-1", rest);
    assert_eq!((names(&last), more), (vec!["refs/heads/j-10"], false));
}

#[test]
fn diff_stat_is_diffs_files_and_counts_without_hunks() {
    let (sandbox, root, next) = seeded();
    let diff = Query::Diff {
        repo: "project".into(),
        base: Some(root.to_hex()),
        head: next.to_hex(),
        path: None,
        page: PageRequest::first(10),
    };
    let stat = Query::DiffStat {
        repo: "project".into(),
        base: Some(root.to_hex()),
        head: next.to_hex(),
        page: PageRequest::first(10),
    };
    let Reply::Diff {
        total_files, page, ..
    } = abi::decode(&ask(&sandbox, &diff).unwrap()).unwrap()
    else {
        panic!("a diff answers Diff");
    };
    let Reply::DiffStat {
        total_files: counted,
        page: stats,
        head,
        ..
    } = abi::decode(&ask(&sandbox, &stat).unwrap()).unwrap()
    else {
        panic!("a diff stat answers DiffStat");
    };
    assert_eq!((counted, total_files, head), (3, 3, next.to_hex()));
    let read: Vec<(&str, FileStatus, u64, u64)> = stats
        .items
        .iter()
        .map(|file| {
            (
                file.path.as_str(),
                file.status,
                file.additions,
                file.deletions,
            )
        })
        .collect();
    assert_eq!(
        read,
        [
            ("a", FileStatus::Modified, 2, 1),
            ("new", FileStatus::Added, 1, 0),
            ("old", FileStatus::Deleted, 0, 1)
        ]
    );
    for (file, full) in stats.items.iter().zip(&page.items) {
        assert_eq!(file.content, Content::Text);
        assert_eq!(
            (file.additions, file.deletions, file.old_size, file.new_size),
            (full.additions, full.deletions, full.old_size, full.new_size)
        );
    }
}

#[test]
fn ref_names_agrees_with_refs() {
    let (sandbox, _, _) = seeded();
    let (all, _) = ref_names(&sandbox, "", PageRequest::first(10));
    let named: BTreeMap<String, String> = all
        .into_iter()
        .map(|named| (named.name, named.target))
        .collect();
    assert_eq!(named, refs_of(&sandbox, "project"));
}
