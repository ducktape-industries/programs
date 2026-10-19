//! `Query::Compares`: every branch against one head in one read, each counted on its own bound,
//! and `Compare` walking only where the two histories part.

mod common;
use common::*;
use forge::{Compared, Revision, Tally};

// main: c1 ← … ← c5; `near` one commit past c5; `far` one commit past c1, four behind main.
fn branches(bounds: Bounds) -> MemorySandbox {
    let sandbox = MemorySandbox::default();
    Forge::init(&sandbox.exec(1), &abi::encode(&bounds)).unwrap();
    create(&sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let mut main = vec![file_commit(&mut source, &[], 1, &[("f", b"1")])];
    for n in 2..=5u8 {
        let parent = *main.last().unwrap();
        let body = [b'0' + n];
        main.push(file_commit(
            &mut source,
            &[parent],
            i64::from(n),
            &[("f", &body[..])],
        ));
    }
    let near = file_commit(&mut source, &[main[4]], 6, &[("near", b"n")]);
    let far = file_commit(&mut source, &[main[0]], 7, &[("far", b"f")]);
    let zero = Hash::Sha1.zero();
    let moved = push(
        &sandbox,
        OWNER,
        "project",
        &[
            (zero, main[4], "refs/heads/main"),
            (zero, near, "refs/heads/near"),
            (zero, far, "refs/heads/far"),
        ],
        &pack_of(&source, &all_ids(&source)),
    )
    .unwrap();
    assert_eq!(moved.len(), 4, "{moved:?}");
    sandbox
}

fn branch(name: &str) -> Revision {
    Revision::Ref(format!("refs/heads/{name}").into_bytes())
}

fn compares(sandbox: &MemorySandbox, from: &[&str]) -> Vec<Compared> {
    let query = Query::Compares {
        repo: "project".into(),
        into: branch("main"),
        from: from.iter().map(|name| branch(name)).collect(),
    };
    let Reply::Compares { compared, .. } = abi::decode(&ask(sandbox, &query).unwrap()).unwrap()
    else {
        panic!("a compares answers Compares");
    };
    compared
}

fn compare(sandbox: &MemorySandbox, from: &str) -> Result<forge::Comparison, program::Error> {
    let query = Query::Compare {
        repo: "project".into(),
        from: branch(from),
        into: branch("main"),
    };
    let Reply::Compare { comparison, .. } = abi::decode(&ask(sandbox, &query)?).unwrap() else {
        panic!("a compare answers Compare");
    };
    Ok(comparison)
}

#[test]
fn compares_answers_each_branch_in_order_as_compare_does() {
    let sandbox = branches(bounds());
    let compared = compares(&sandbox, &["near", "far", "main"]);
    let names: Vec<Revision> = compared.iter().map(|item| item.from.clone()).collect();
    assert_eq!(names, [branch("near"), branch("far"), branch("main")]);
    for (item, name) in compared.iter().zip(["near", "far", "main"]) {
        assert_eq!(
            item.tally,
            Tally::Counted(compare(&sandbox, name).unwrap()),
            "{name}"
        );
    }
    let Tally::Counted(far) = &compared[1].tally else {
        panic!("far is counted");
    };
    assert_eq!((far.ahead, far.behind), (1, 4));
    assert_eq!(far.mergeability, forge::Mergeability::Diverged);
}

#[test]
fn a_branch_that_cannot_be_counted_is_refused_alone() {
    let sandbox = branches(Bounds {
        log_walk: 3,
        ..bounds()
    });
    let compared = compares(&sandbox, &["near", "far", "gone"]);
    let Tally::Counted(near) = &compared[0].tally else {
        panic!("near is counted: {:?}", compared[0]);
    };
    assert_eq!((near.ahead, near.behind), (1, 0));
    assert_eq!(near.mergeability, forge::Mergeability::FastForward);
    assert!(
        matches!(compared[1].tally, Tally::Refused(_)),
        "far is four behind, past a walk of three: {:?}",
        compared[1]
    );
    assert!(
        matches!(compared[2].tally, Tally::Refused(_)),
        "no ref is named gone: {:?}",
        compared[2]
    );
}

#[test]
fn compare_walks_only_where_the_histories_part() {
    let narrow = branches(Bounds {
        log_walk: 3,
        ..bounds()
    });
    let near = compare(&narrow, "near").unwrap();
    assert_eq!((near.ahead, near.behind), (1, 0));
    assert!(compare(&narrow, "far").is_err());
}
