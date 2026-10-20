//! The host work of a push and of a log page, counted at two history lengths and two ref counts:
//! it follows what the push brings or the page lists, never the repository's history or refs.
mod common;

use common::*;
use program::Work;

const REPO: &str = "project";

struct Grown {
    sandbox: MemorySandbox,
    tip: Oid,
}

fn grown(history: i64, branches: usize) -> Grown {
    let sandbox = founded();
    create(&sandbox, REPO, HashKind::Sha1);
    let mut store = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut store, &[], 0, &[("file", b"0")]);
    let mut tip = root;
    for step in 1..history {
        let content = step.to_string();
        tip = file_commit(&mut store, &[tip], step, &[("file", content.as_bytes())]);
    }
    let zero = Hash::Sha1.zero();
    let history_pack = pack_of(&store, &all_ids(&store));
    push(
        &sandbox,
        OWNER,
        REPO,
        &[(zero, tip, "refs/heads/main")],
        &history_pack,
    )
    .unwrap();
    let names: Vec<String> = (0..branches).map(|n| format!("refs/heads/b{n}")).collect();
    let commands: Vec<(Oid, Oid, &str)> = names
        .iter()
        .map(|name| (zero, root, name.as_str()))
        .collect();
    push(&sandbox, OWNER, REPO, &commands, b"").unwrap();
    Grown { sandbox, tip }
}

fn commit_on(parent: Oid, time: i64, name: &str) -> (MemoryObjects, Oid) {
    let mut brought = MemoryObjects::new(Hash::Sha1);
    let commit = file_commit(&mut brought, &[parent], time, &[(name, name.as_bytes())]);
    (brought, commit)
}

fn worked(sandbox: &MemorySandbox, run: impl FnOnce()) -> Work {
    sandbox.forge.borrow_mut().work = Work::default();
    run();
    sandbox.forge.borrow().work
}

fn landed(grown: &Grown, commands: &[(Oid, Oid, &str)], brought: &MemoryObjects) {
    let pack = pack_of(brought, &all_ids(brought));
    let report = push(&grown.sandbox, OWNER, REPO, commands, &pack).unwrap();
    assert!(
        report.iter().all(|line| !line.starts_with("ng ")),
        "{report:?}"
    );
}

fn pushed(grown: &Grown, commands: &[(Oid, Oid, &str)], brought: &MemoryObjects) -> Work {
    worked(&grown.sandbox, || landed(grown, commands, brought))
}

fn plain(grown: &Grown) -> Work {
    let (brought, commit) = commit_on(grown.tip, 1_000, "plain");
    let zero = Hash::Sha1.zero();
    pushed(grown, &[(zero, commit, "refs/heads/plain")], &brought)
}

fn stacked(grown: &Grown) -> Work {
    let zero = Hash::Sha1.zero();
    let (mut brought, lower) = commit_on(grown.tip, 1_000, "lower");
    landed(grown, &[(zero, lower, "refs/heads/lower")], &brought);
    let upper = file_commit(&mut brought, &[lower], 1_001, &[("upper", b"upper")]);
    pushed(grown, &[(zero, upper, "refs/heads/stacked")], &brought)
}

fn fast_forward(grown: &Grown) -> Work {
    let zero = Hash::Sha1.zero();
    let (brought, first) = commit_on(grown.tip, 1_000, "first");
    landed(grown, &[(zero, first, "refs/heads/topic")], &brought);
    let (brought, second) = commit_on(first, 1_001, "second");
    pushed(grown, &[(first, second, "refs/heads/topic")], &brought)
}

fn log_page(grown: &Grown) -> Work {
    let asked = Query::Log {
        repo: REPO.into(),
        from: forge::Revision::Ref(b"refs/heads/main".to_vec()),
        exclude: None,
        page: PageRequest::first(1),
    };
    worked(&grown.sandbox, || {
        let Reply::Log { tip, page, .. } =
            abi::decode(&ask(&grown.sandbox, &asked).unwrap()).unwrap()
        else {
            panic!("a log answers Log");
        };
        assert_eq!(tip, grown.tip.to_hex());
        assert_eq!(page.items.len(), 1);
        assert!(page.next.is_some());
    })
}

#[track_caller]
fn same_at_every_size(shape: fn(&Grown) -> Work) {
    let small = shape(&grown(8, 2));
    let large = shape(&grown(64, 48));
    assert_eq!(
        small, large,
        "the work grew with the repository's history or its refs"
    );
}

#[test]
fn a_new_branch_on_the_main_tip_costs_what_it_brings() {
    same_at_every_size(plain);
}

#[test]
fn a_new_branch_stacked_on_another_branch_tip_costs_what_it_brings() {
    same_at_every_size(stacked);
}

#[test]
fn a_fast_forward_costs_what_it_brings() {
    same_at_every_size(fast_forward);
}

#[test]
fn a_log_page_reads_only_as_far_as_the_page() {
    same_at_every_size(log_page);
}
