//! A long history: the log reads the pages its list shows, and no more.
use std::cell::Cell;
use std::rc::Rc;

use super::{answer, block, followed, ref_key, reply, review_key};
use crate::api::Ask;
use forge::{CommitInfo, PageResponse, Query, Reply, Revision};
use view::testing::TestAppContext;
use view::wire::Node;

const COMMITS: usize = 500;

const PAGE_SIZE: u64 = 64;

fn long_log(query: &Query) -> Reply {
    let Query::Log { page, .. } = query else {
        panic!("{query:?} is no log");
    };
    let Reply::Log { page: fixture, .. } = reply("log") else {
        panic!("the log fixture is a log");
    };
    let start = page.after.as_ref().map_or(0, |cursor| {
        u64::from_be_bytes(cursor.as_slice().try_into().expect("an offset")) as usize
    });
    let end = (start + page.bounded(PAGE_SIZE).limit() as usize).min(COMMITS);
    let items = (start..end)
        .map(|n| CommitInfo {
            oid: format!("{n:040x}"),
            message: format!("Commit {n}").into_bytes(),
            ..fixture.items[0].clone()
        })
        .collect();
    Reply::Log {
        height: 1,
        tip: format!("{:040x}", 0),
        page: PageResponse {
            height: 1,
            items,
            next: (end < COMMITS).then(|| (end as u64).to_be_bytes().to_vec()),
        },
    }
}

fn serve_long_log(cx: &TestAppContext) -> Rc<Cell<usize>> {
    let bytes = Rc::new(Cell::new(0));
    let counted = bytes.clone();
    cx.host().handle::<Ask>(move |query| {
        if !browsed(&query) {
            return Ok(answer(&query, "default"));
        }
        let reply = long_log(&query);
        counted.set(counted.get() + borsh::to_vec(&reply).unwrap().len());
        Ok(reply)
    });
    bytes
}

fn browsed(query: &Query) -> bool {
    matches!(
        query,
        Query::Log {
            from: Revision::Ref(_),
            ..
        }
    )
}

fn pages(cx: &TestAppContext) -> Vec<usize> {
    cx.host()
        .requests::<Ask>()
        .iter()
        .filter(|query| browsed(query))
        .map(|query| {
            query
                .page()
                .and_then(|page| page.after.as_ref())
                .map_or(0, |cursor| {
                    u64::from_be_bytes(cursor.as_slice().try_into().unwrap()) as usize
                })
        })
        .collect()
}

fn listed(cx: &TestAppContext) -> usize {
    match cx.node("forge-log") {
        Node::UniformList { count, .. } => *count,
        other => panic!("the log is no list: {other:?}"),
    }
}

#[test]
fn a_long_log_reads_the_pages_its_list_shows() {
    let (mut cx, heads) = followed("default");
    let bytes = serve_long_log(&cx);
    cx.simulate_click("forge-tab-commits");
    cx.run_until_parked();
    let opened = (pages(&cx), bytes.get(), listed(&cx));
    eprintln!(
        "AUDIT opening a log of {COMMITS} commits: {} program.query asks, {} reply bytes",
        opened.0.len(),
        opened.1
    );
    assert!(cx.has_text("Commit 0"), "{:?}", cx.texts());
    cx.simulate_range("forge-log", 20..34);
    let inside = pages(&cx).len();
    cx.simulate_range("forge-log", 50..64);
    let scrolled = (pages(&cx), listed(&cx));
    cx.simulate_range("forge-log", 50..64);
    cx.simulate_range("forge-log", 60..74);
    let held = (pages(&cx).len(), bytes.get());
    heads.forge.send(block(100, vec![ref_key()]));
    cx.run_until_parked();
    let pushed = (pages(&cx), bytes.get(), listed(&cx));
    eprintln!(
        "AUDIT a ref moved under a log of {COMMITS} commits: {} program.query asks, {} reply bytes",
        pushed.0.len() - held.0,
        pushed.1 - held.1
    );
    heads.forge.send(block(101, vec![review_key()]));
    cx.run_until_parked();
    let reviewed = pages(&cx).len();

    assert_eq!(opened.0, [0], "opening: the first page alone");
    assert_eq!(opened.2, 65, "64 commits and the row that reads on");
    assert_eq!(inside, 1, "rows inside the page held ask nothing");
    assert_eq!(scrolled.0, [0, 64], "the end of the page: the next page");
    assert_eq!(scrolled.1, 129);
    assert!(cx.has_text("Commit 64"), "{:?}", cx.texts());
    assert_eq!(held.0, 2, "rows held ask nothing");
    assert_eq!(pushed.0, [0, 64, 0, 64], "a push: the pages held, again");
    assert_eq!(pushed.2, 129);
    assert_eq!(reviewed, 4, "a review moves no history");
}

#[test]
fn the_row_past_the_commits_held_reads_on() {
    let (mut cx, _heads) = followed("default");
    serve_long_log(&cx);
    cx.simulate_click("forge-tab-commits");
    cx.run_until_parked();
    assert!(
        cx.find("forge-log-more").is_none(),
        "not in the first window"
    );
    cx.host().never::<Ask>();
    cx.simulate_range("forge-log", 50..64);
    assert!(cx.has_text("Reading the history…"), "{:?}", cx.texts());
    assert_eq!(cx.interactivity("forge-log-more").role, None);
    cx.simulate_theme(true);
    assert!(cx.has_text("Reading the history…"), "{:?}", cx.texts());
    assert_eq!(pages(&cx), [0, 64], "one page out");
}

#[test]
fn a_log_read_to_its_end_has_no_row_that_reads_on() {
    let (mut cx, _heads) = followed("default");
    cx.simulate_click("forge-tab-commits");
    cx.run_until_parked();
    assert_eq!(listed(&cx), 2);
    assert!(!cx.has_text("Reading the history…"), "{:?}", cx.texts());
}

#[test]
fn a_commit_past_the_first_page_is_read_by_itself_after_a_restore() {
    let (mut cx, _heads) = followed("default");
    serve_long_log(&cx);
    cx.simulate_click("forge-tab-commits");
    cx.run_until_parked();
    cx.simulate_range("forge-log", 50..64);
    let oid = format!("{:040x}", 64);
    cx.simulate_click(&format!("forge-commit-{oid}"));
    cx.run_until_parked();
    assert!(cx.has_text("Commit 64"), "{:?}", cx.texts());
    let own = |cx: &TestAppContext| {
        let asked = cx.host().requests::<Ask>();
        asked
            .iter()
            .filter(|query| matches!(query, Query::Log { from: Revision::Oid(from), .. } if *from == oid))
            .count()
    };
    assert_eq!(own(&cx), 0, "the log on screen holds the commit");
    let snapshot = cx.snapshot().unwrap();
    let mut cx = TestAppContext::new();
    super::configure(&mut cx, "default");
    cx.host().handle::<Ask>(move |query| match &query {
        Query::Log {
            from: Revision::Oid(from),
            ..
        } => {
            let Reply::Log { page: fixture, .. } = reply("log") else {
                panic!("the log fixture is a log");
            };
            let commit = CommitInfo {
                oid: from.clone(),
                message: b"Commit 64".to_vec(),
                ..fixture.items[0].clone()
            };
            Ok(Reply::Log {
                height: 1,
                tip: from.clone(),
                page: PageResponse {
                    height: 1,
                    items: vec![commit],
                    next: Some(1u64.to_be_bytes().to_vec()),
                },
            })
        }
        query if browsed(query) => Ok(long_log(query)),
        query => Ok(answer(query, "default")),
    });
    cx.restore::<crate::Forge>(&snapshot).unwrap();
    cx.run_until_parked();
    assert_eq!(pages(&cx), [0], "the log, its first page");
    assert_eq!(own(&cx), 1, "the commit's own row, once");
    assert!(cx.has_text("Commit 64"), "{:?}", cx.texts());
    let diffs: Vec<Query> = cx
        .host()
        .requests::<Ask>()
        .into_iter()
        .filter(|query| matches!(query, Query::Diff { .. }))
        .collect();
    let Reply::Log { page: fixture, .. } = reply("log") else {
        panic!("the log fixture is a log");
    };
    let parent = fixture.items[0].parents.first().cloned();
    assert!(
        matches!(diffs.as_slice(), [Query::Diff { base, head, .. }] if *base == parent && *head == oid),
        "one diff, against the commit's parent: {diffs:?}"
    );
}
