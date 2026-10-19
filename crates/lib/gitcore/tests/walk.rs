// Walk tests on hand-built DAGs (ancestry, wants minus haves, reachable objects, path lookup)
// and on generated histories with reverts and merges, whose paged closures are checked against
// `git rev-list --objects`.

#[path = "common/mod.rs"]
mod common;

use common::{blob, commit, file_tree, git_binary, signature, temp_dir, tree};
use gitcore::walk::{
    Listing, ShallowListing, Trees, Verdict, Walk, closure, commits, history as walked_history,
    is_ancestor, shallow_history, tree_at_path,
};
use gitcore::{Commit, Error, Hash, Kind, MemoryObjects, Mode, Objects as _, Oid, Tag, Tree};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

struct Dag {
    store: MemoryObjects,
    root: Oid,
    a: Oid,
    b: Oid,
    c: Oid,
    merge: Oid,
    other: Oid,
    trees: Vec<Oid>,
}

fn dag() -> Dag {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let t0 = file_tree(&mut store, &[("f", "0\n")]);
    let t1 = file_tree(&mut store, &[("f", "1\n")]);
    let t2 = file_tree(&mut store, &[("f", "2\n")]);
    let t3 = file_tree(&mut store, &[("f", "3\n"), ("g", "g\n")]);
    let root = commit(&mut store, t0, &[], 100, "root");
    let a = commit(&mut store, t1, &[root], 200, "a");
    let b = commit(&mut store, t2, &[root], 200, "b");
    let c = commit(&mut store, t3, &[a], 300, "c");
    let merge = commit(&mut store, t3, &[c, b], 400, "merge");
    let other = commit(&mut store, t0, &[], 50, "other");
    Dag {
        store,
        root,
        a,
        b,
        c,
        merge,
        other,
        trees: vec![t0, t1, t2, t3],
    }
}

#[test]
fn ancestor_verdicts() {
    let d = dag();
    assert_eq!(
        is_ancestor(&d.store, &d.root, &d.merge, 100).unwrap(),
        Verdict::Yes
    );
    assert_eq!(
        is_ancestor(&d.store, &d.b, &d.merge, 100).unwrap(),
        Verdict::Yes
    );
    assert_eq!(
        is_ancestor(&d.store, &d.merge, &d.merge, 100).unwrap(),
        Verdict::Yes
    );
    assert_eq!(
        is_ancestor(&d.store, &d.merge, &d.root, 100).unwrap(),
        Verdict::No
    );
    assert_eq!(is_ancestor(&d.store, &d.b, &d.c, 100).unwrap(), Verdict::No);
    assert_eq!(
        is_ancestor(&d.store, &d.other, &d.merge, 100).unwrap(),
        Verdict::No
    );
    assert_eq!(
        is_ancestor(&d.store, &d.root, &d.merge, 2).unwrap(),
        Verdict::CapReached
    );
    let missing = Oid::from_hex(Hash::Sha1, "1111111111111111111111111111111111111111").unwrap();
    assert_eq!(
        is_ancestor(&d.store, &d.root, &missing, 10),
        Err(Error::MissingObject(missing))
    );
}

#[test]
fn commits_from_tips_excluding_haves() {
    let d = dag();
    let all = commits(&d.store, &[d.merge], &[], 100).unwrap();
    assert_eq!(all.len(), 5);
    assert_eq!(all[0], d.merge);
    assert_eq!(all[1], d.c);
    assert_eq!(all[4], d.root);
    let mut same_time = [d.a, d.b];
    same_time.sort();
    assert_eq!(&all[2..4], &same_time);

    let since_a = commits(&d.store, &[d.merge], &[d.a], 100).unwrap();
    assert_eq!(since_a, vec![d.merge, d.c, d.b]);

    let since_c_and_b = commits(&d.store, &[d.merge], &[d.c, d.b], 100).unwrap();
    assert_eq!(since_c_and_b, vec![d.merge]);

    let nothing = commits(&d.store, &[d.merge], &[d.merge], 100).unwrap();
    assert!(nothing.is_empty());

    let two_tips = commits(&d.store, &[d.c, d.other], &[d.root], 100).unwrap();
    assert_eq!(two_tips, vec![d.c, d.a, d.other]);

    let unknown_have =
        Oid::from_hex(Hash::Sha1, "2222222222222222222222222222222222222222").unwrap();
    assert_eq!(
        commits(&d.store, &[d.c], &[unknown_have], 100).unwrap(),
        vec![d.c, d.a, d.root]
    );
    assert_eq!(
        commits(&d.store, &[d.merge], &[], 3),
        Err(Error::CapReached)
    );
}

#[test]
fn commits_handles_clock_skew_between_have_and_want() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let t = file_tree(&mut store, &[("f", "x\n")]);
    let old = commit(&mut store, t, &[], 500, "old but dated late");
    let mid = commit(&mut store, t, &[old], 100, "mid");
    let new = commit(&mut store, t, &[mid], 200, "new");
    assert_eq!(
        commits(&store, &[new], &[old], 100).unwrap(),
        vec![new, mid]
    );
    assert_eq!(commits(&store, &[new], &[mid], 100).unwrap(), vec![new]);
}

fn listed_in(store: &MemoryObjects, wants: &[Oid], haves: &[Oid]) -> BTreeSet<Oid> {
    paged(store, wants, haves, usize::MAX).0
}

fn paged(
    store: &MemoryObjects,
    wants: &[Oid],
    haves: &[Oid],
    most: usize,
) -> (BTreeSet<Oid>, Vec<usize>) {
    let mut listing = Listing::of(wants, haves);
    let mut pages = Vec::new();
    while let Some(walk) = listing.pending() {
        let closed = closure(store, walk, most).unwrap();
        pages.push(closed.listed.len() + closed.next.trees.len());
        listing.receive(closed);
    }
    let listed = listing
        .into_listed()
        .into_iter()
        .map(|item| item.id)
        .collect();
    (listed, pages)
}

// What a client lists from `history` pages and its own `Trees`, reading at each level only the
// trees `wanted` names.
fn split_listing(
    store: &MemoryObjects,
    wants: &[Oid],
    haves: &[Oid],
    most: usize,
) -> BTreeSet<Oid> {
    let mut listing = Listing::of(wants, haves);
    while let Some(walk) = listing.pending() {
        listing.receive(walked_history(store, walk, most).unwrap());
    }
    let listed = listing.into_listed();
    let body = |id: &Oid| store.get(id).unwrap().unwrap().body;
    let mut trees = Trees::default();
    for item in &listed {
        match item.kind {
            Kind::Commit => {
                let commit = Commit::parse(&body(&item.id), Hash::Sha1).unwrap();
                let parents = commit
                    .parents
                    .iter()
                    .map(|parent| Commit::parse(&body(parent), Hash::Sha1).unwrap().tree)
                    .collect();
                trees.commit(commit.tree, parents);
            }
            Kind::Tree => trees.tree(item.id),
            Kind::Blob | Kind::Tag => {}
        }
    }
    while !trees.is_done() {
        let mut read = MemoryObjects::new(Hash::Sha1);
        for id in trees.wanted() {
            let object = store.get(&id).unwrap().unwrap();
            read.put(object.kind, &object.body).unwrap();
        }
        trees.expand(&read).unwrap();
    }
    let trees = trees.into_listed();
    listed.iter().chain(&trees).map(|item| item.id).collect()
}

fn listed(d: &Dag, wants: &[Oid], haves: &[Oid]) -> BTreeSet<Oid> {
    listed_in(&d.store, wants, haves)
}

fn blobs(contents: &[&[u8]]) -> BTreeSet<Oid> {
    let mut scratch = MemoryObjects::new(Hash::Sha1);
    contents
        .iter()
        .map(|body| blob(&mut scratch, body))
        .collect()
}

#[test]
fn a_closure_lists_a_history_once_whatever_its_merges() {
    let d = dag();
    let mut expected: BTreeSet<Oid> = [d.root, d.a, d.b, d.c, d.merge].into_iter().collect();
    expected.extend(d.trees.iter().copied());
    expected.extend(blobs(&[b"0\n", b"1\n", b"2\n", b"3\n", b"g\n"]));
    assert_eq!(listed(&d, &[d.merge], &[]), expected);
}

#[test]
fn a_history_lists_the_commits_and_its_client_expands_the_trees_to_the_closure() {
    let d = dag();
    let closed = walked_history(&d.store, Walk::of(&[d.merge], &[]), usize::MAX).unwrap();
    assert!(closed.next.is_done());
    let commits: BTreeSet<Oid> = closed.listed.iter().map(|item| item.id).collect();
    let all = [d.root, d.a, d.b, d.c, d.merge].into_iter().collect();
    assert_eq!(commits, all);
    assert!(closed.listed.iter().all(|item| item.kind == Kind::Commit));
    for haves in [vec![], vec![d.c, d.root], vec![d.a]] {
        let whole = listed(&d, &[d.merge], &haves);
        assert_eq!(
            split_listing(&d.store, &[d.merge], &haves, usize::MAX),
            whole
        );
        assert_eq!(split_listing(&d.store, &[d.merge], &haves, 1), whole);
    }
    let named = split_listing(&d.store, &[d.trees[3]], &[], usize::MAX);
    assert_eq!(named, listed(&d, &[d.trees[3]], &[]));
}

#[test]
fn a_closure_stops_at_the_commits_the_client_holds() {
    let d = dag();
    let mut expected: BTreeSet<Oid> = [d.merge, d.b].into_iter().collect();
    expected.insert(d.trees[2]);
    expected.extend(blobs(&[b"2\n"]));
    assert_eq!(listed(&d, &[d.merge], &[d.c, d.root]), expected);
    let only_the_merge: BTreeSet<Oid> = [d.merge].into_iter().collect();
    assert_eq!(listed(&d, &[d.merge], &[d.c, d.b]), only_the_merge);
}

#[test]
fn a_closure_stops_at_what_the_client_holds_below_its_haves() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let first = file_tree(&mut store, &[("f", "0\n")]);
    let mut main = vec![commit(&mut store, first, &[], 100, "root")];
    for n in 1..10 {
        let tree = file_tree(&mut store, &[("f", &format!("{n}\n"))]);
        let parent = main[n - 1];
        main.push(commit(&mut store, tree, &[parent], 100 + n as i64, "main"));
    }
    let side_tree = file_tree(&mut store, &[("f", "1\n"), ("side", "s\n")]);
    let side = commit(&mut store, side_tree, &[main[1]], 500, "side");
    let mut expected: BTreeSet<Oid> = [side, side_tree].into_iter().collect();
    expected.extend(blobs(&[b"s\n"]));
    assert_eq!(listed_in(&store, &[side], &[main[9]]), expected);

    let merged = commit(&mut store, side_tree, &[main[9], side], 600, "merge");
    expected.insert(merged);
    assert_eq!(listed_in(&store, &[merged], &[main[9]]), expected);
}

#[test]
fn a_page_names_about_its_cap_and_the_pages_list_the_whole_closure() {
    let d = dag();
    let whole = listed(&d, &[d.merge], &[]);
    let (widest_tree, most) = (3, 3);
    let (listed, pages) = paged(&d.store, &[d.merge], &[], most);
    assert_eq!(listed, whole);
    assert!(pages.len() > 3, "{pages:?}");
    assert!(
        pages.iter().all(|named| *named < most + widest_tree),
        "{pages:?}"
    );
}

#[test]
fn a_tree_wider_than_a_page_is_listed_across_pages() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let names: Vec<String> = (0..10).map(|n| format!("f{n}")).collect();
    let bodies: Vec<String> = (0..10).map(|n| format!("{n}\n")).collect();
    let files: Vec<(&str, &str)> = names
        .iter()
        .zip(&bodies)
        .map(|(name, body)| (name.as_str(), body.as_str()))
        .collect();
    let wide = file_tree(&mut store, &files);
    let head = commit(&mut store, wide, &[], 1, "wide");
    let whole = listed_in(&store, &[head], &[]);
    assert_eq!(whole.len(), 12);
    let (listed, pages) = paged(&store, &[head], &[], 4);
    assert_eq!(listed, whole);
    assert!(pages.iter().all(|named| *named <= 5), "{pages:?}");
    assert!(pages.len() >= 3, "{pages:?}");
}

const PAGES: [usize; 6] = [1, 2, 3, 5, 8, usize::MAX];

fn reachable(store: &MemoryObjects, wants: &[Oid]) -> BTreeSet<Oid> {
    let mut found = BTreeSet::new();
    let mut stack = wants.to_vec();
    while let Some(id) = stack.pop() {
        if !found.insert(id) {
            continue;
        }
        let object = store.get(&id).unwrap().unwrap();
        match object.kind {
            Kind::Commit => {
                let commit = Commit::parse(&object.body, Hash::Sha1).unwrap();
                stack.push(commit.tree);
                stack.extend(commit.parents);
            }
            Kind::Tree => {
                let tree = Tree::parse(&object.body, Hash::Sha1).unwrap();
                let held = tree
                    .entries
                    .iter()
                    .filter(|entry| entry.mode != Mode::Gitlink);
                stack.extend(held.map(|entry| entry.id));
            }
            Kind::Blob | Kind::Tag => {}
        }
    }
    found
}

#[test]
fn a_commit_that_repeats_an_earlier_tree_still_has_every_object_listed() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let first_a = blob(&mut store, b"a1\n");
    let second_a = blob(&mut store, b"a2\n");
    let b = blob(&mut store, b"b\n");
    let first_dir = tree(
        &mut store,
        &[("a", Mode::Regular, first_a), ("b", Mode::Regular, b)],
    );
    let second_dir = tree(
        &mut store,
        &[("a", Mode::Regular, second_a), ("b", Mode::Regular, b)],
    );
    let first_root = tree(&mut store, &[("dir", Mode::Directory, first_dir)]);
    let second_root = tree(&mut store, &[("dir", Mode::Directory, second_dir)]);
    let added = commit(&mut store, first_root, &[], 100, "add a and b");
    let edited = commit(&mut store, second_root, &[added], 200, "edit a");
    let reverted = commit(&mut store, first_root, &[edited], 300, "revert a");
    let every: BTreeSet<Oid> = [
        added,
        edited,
        reverted,
        first_root,
        second_root,
        first_dir,
        second_dir,
        first_a,
        second_a,
        b,
    ]
    .into_iter()
    .collect();
    assert_eq!(every.len(), 10);
    for most in PAGES {
        assert_eq!(
            paged(&store, &[reverted], &[], most).0,
            every,
            "pages of {most}"
        );
    }
    let past_the_add: BTreeSet<Oid> = [edited, reverted, second_root, second_dir, second_a]
        .into_iter()
        .collect();
    for most in PAGES {
        let listed = paged(&store, &[reverted], &[added], most).0;
        assert!(
            listed.is_superset(&past_the_add),
            "pages of {most}: {listed:?}"
        );
        assert!(
            !listed.contains(&added) && !listed.contains(&b),
            "pages of {most}"
        );
    }
}

#[test]
fn a_merge_whose_parents_share_a_subtree_has_every_object_listed() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let base_tree = file_tree(&mut store, &[("x", "1\n"), ("y", "1\n")]);
    let shared = file_tree(&mut store, &[("x", "2\n"), ("y", "1\n")]);
    let root = |store: &mut MemoryObjects, top: &str, sub: Oid| {
        let top = blob(store, top.as_bytes());
        tree(
            store,
            &[
                ("shared", Mode::Directory, sub),
                ("top", Mode::Regular, top),
            ],
        )
    };
    let base_root = root(&mut store, "base\n", base_tree);
    let left_root = root(&mut store, "left\n", shared);
    let right_root = root(&mut store, "right\n", shared);
    let merged_root = root(&mut store, "merged\n", shared);
    let base = commit(&mut store, base_root, &[], 100, "base");
    let left = commit(&mut store, left_root, &[base], 200, "left");
    let right = commit(&mut store, right_root, &[base], 210, "right");
    let merged = commit(&mut store, merged_root, &[left, right], 300, "merge");
    let back = commit(&mut store, base_root, &[merged], 400, "back to the base");
    for tip in [merged, back] {
        let every = reachable(&store, &[tip]);
        for most in PAGES {
            assert_eq!(paged(&store, &[tip], &[], most).0, every, "pages of {most}");
        }
    }
    let past_left = reachable(&store, &[back])
        .difference(&reachable(&store, &[left]))
        .copied()
        .collect::<BTreeSet<Oid>>();
    for most in PAGES {
        let listed = paged(&store, &[back], &[left], most).0;
        assert!(listed.is_superset(&past_left), "pages of {most}");
        assert!(
            listed.is_subset(&reachable(&store, &[back])),
            "pages of {most}"
        );
        assert!(
            !listed.contains(&left) && !listed.contains(&base),
            "pages of {most}"
        );
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const PATHS: [&str; 6] = ["top", "dir/a", "dir/b", "dir/sub/c", "x/y/z", "x/w"];
const BRANCHES: usize = 3;

struct History {
    import: Vec<u8>,
    commits: usize,
    heads: Vec<usize>,
}

fn history(rng: &mut Rng) -> History {
    let commits = 6 + rng.below(30);
    let mut states: Vec<BTreeMap<&str, usize>> = Vec::new();
    let mut heads: Vec<Option<usize>> = vec![None; BRANCHES];
    let mut import = Vec::new();
    for n in 0..commits {
        let branch = rng.below(BRANCHES);
        let first = match (heads[branch], states.len()) {
            (Some(head), _) => Some(head),
            (None, 0) => None,
            (None, made) => Some(rng.below(made)),
        };
        let other = heads[rng.below(BRANCHES)];
        let merges = rng.below(4) == 0;
        let second = other.filter(|other| merges && first.is_some_and(|first| first != *other));
        let mut state = first.map(|first| states[first].clone()).unwrap_or_default();
        let reverts = rng.below(4) == 0 && !states.is_empty();
        if reverts {
            state = states[rng.below(states.len())].clone();
        } else {
            for _ in 0..=rng.below(2) {
                let path = PATHS[rng.below(PATHS.len())];
                match rng.below(4) {
                    0 => state.remove(path),
                    value => state.insert(path, value),
                };
            }
        }
        if let Some(second) = second {
            for path in PATHS {
                if rng.below(2) == 0 {
                    match states[second].get(path) {
                        Some(value) => state.insert(path, *value),
                        None => state.remove(path),
                    };
                }
            }
        }
        let message = format!("commit {n}\n");
        write!(
            import,
            "commit refs/heads/b{branch}\nmark :{}\ncommitter Ada <ada@example.com> {} +0000\ndata {}\n{message}",
            n + 1,
            1_000_000 + n * 60,
            message.len()
        )
        .unwrap();
        if let Some(first) = first {
            writeln!(import, "from :{}", first + 1).unwrap();
        }
        if let Some(second) = second {
            writeln!(import, "merge :{}", second + 1).unwrap();
        }
        writeln!(import, "deleteall").unwrap();
        for (path, value) in &state {
            write!(import, "M 100644 inline {path}\ndata 2\n{value}\n").unwrap();
        }
        writeln!(import).unwrap();
        states.push(state);
        heads[branch] = Some(n);
    }
    History {
        import,
        commits,
        heads: heads.into_iter().flatten().collect(),
    }
}

fn git(git: &Path, dir: &Path, args: &[&str], input: &[u8]) -> Vec<u8> {
    let mut child = Command::new(git)
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "git {args:?}");
    output.stdout
}

fn loaded(objects: &[u8]) -> MemoryObjects {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let mut rest = objects;
    while !rest.is_empty() {
        let line_end = rest.iter().position(|byte| *byte == b'\n').unwrap();
        let header = std::str::from_utf8(&rest[..line_end]).unwrap();
        let mut fields = header.split(' ');
        let (id, kind, size) = (
            fields.next().unwrap(),
            fields.next().unwrap(),
            fields.next().unwrap().parse::<usize>().unwrap(),
        );
        let body = &rest[line_end + 1..line_end + 1 + size];
        let kind = Kind::parse(kind.as_bytes()).unwrap();
        assert_eq!(store.put(kind, body).unwrap().to_hex(), id);
        rest = &rest[line_end + 1 + size + 1..];
    }
    store
}

fn ids(listing: &[u8]) -> BTreeSet<Oid> {
    String::from_utf8_lossy(listing)
        .lines()
        .filter_map(|line| line.split(' ').next())
        .filter(|id| !id.is_empty())
        .map(|id| Oid::from_hex(Hash::Sha1, id).unwrap())
        .collect()
}

#[test]
fn paged_closures_of_generated_histories_list_what_git_lists() {
    let git_path = git_binary();
    let root = temp_dir("gitcore-closure");
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for seed in 0..120 {
        let made = history(&mut rng);
        let dir = root.join(format!("history-{seed}"));
        std::fs::create_dir_all(&dir).unwrap();
        git(&git_path, &dir, &["init", "-q", "--bare"], b"");
        let marks = dir.join("marks");
        let export = format!("--export-marks={}", marks.display());
        git(
            &git_path,
            &dir,
            &["fast-import", "--quiet", &export],
            &made.import,
        );
        let marked: BTreeMap<usize, Oid> = std::fs::read_to_string(&marks)
            .unwrap()
            .lines()
            .map(|line| {
                let (mark, id) = line.split_once(' ').unwrap();
                let mark = mark.trim_start_matches(':').parse::<usize>().unwrap();
                (mark - 1, Oid::from_hex(Hash::Sha1, id).unwrap())
            })
            .collect();
        assert_eq!(marked.len(), made.commits);
        let all = git(
            &git_path,
            &dir,
            &["cat-file", "--batch-all-objects", "--batch"],
            b"",
        );
        let store = loaded(&all);
        let tip = made.heads[rng.below(made.heads.len())];
        let mut wants = vec![marked[&tip]];
        let another = made.heads[rng.below(made.heads.len())];
        if another != tip {
            wants.push(marked[&another]);
        }
        let haves: Vec<Oid> = match rng.below(3) {
            0 => Vec::new(),
            count => (0..count)
                .map(|_| marked[&rng.below(made.commits)])
                .collect(),
        };
        let hex = |list: &[Oid]| list.iter().map(Oid::to_hex).collect::<Vec<_>>();
        let mut asked: Vec<String> = vec!["rev-list".into(), "--objects".into()];
        asked.extend(hex(&wants));
        let reached = ids(&git(&git_path, &dir, &args(&asked), b""));
        asked.push("--not".into());
        asked.extend(hex(&haves));
        let git_listed = ids(&git(&git_path, &dir, &args(&asked), b""));
        asked.remove(1);
        let git_commits = ids(&git(&git_path, &dir, &args(&asked), b""));
        assert_eq!(reachable(&store, &wants), reached, "history {seed}");
        let commits: BTreeSet<Oid> = marked.values().copied().collect();
        for most in PAGES {
            let listed = paged(&store, &wants, &haves, most).0;
            let context = format!("history {seed}, pages of {most}, haves {}", haves.len());
            let split = split_listing(&store, &wants, &haves, most);
            assert_eq!(split, listed, "{context}: history, then the client's trees");
            if haves.is_empty() {
                assert_eq!(listed, git_listed, "{context}");
                continue;
            }
            let listed_commits: BTreeSet<Oid> = listed.intersection(&commits).copied().collect();
            assert_eq!(listed_commits, git_commits, "{context}");
            assert!(listed.is_superset(&git_listed), "{context}");
            assert!(listed.is_subset(&reached), "{context}");
        }
    }
    std::fs::remove_dir_all(&root).ok();
}

fn args(asked: &[String]) -> Vec<&str> {
    asked.iter().map(String::as_str).collect()
}

#[test]
fn nested_reachability_and_path_lookup() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let leaf = blob(&mut store, b"leaf\n");
    let inner = tree(&mut store, &[("leaf.txt", Mode::Regular, leaf)]);
    let gitlink = Oid::from_hex(Hash::Sha1, "3333333333333333333333333333333333333333").unwrap();
    let root = tree(
        &mut store,
        &[
            ("dir", Mode::Directory, inner),
            ("sub", Mode::Gitlink, gitlink),
        ],
    );
    let head = commit(&mut store, root, &[], 1, "head");
    assert_eq!(
        listed_in(&store, &[head], &[]),
        [head, root, inner, leaf].into_iter().collect()
    );

    let found = tree_at_path(&store, &root, b"dir/leaf.txt")
        .unwrap()
        .unwrap();
    assert_eq!(found.id, leaf);
    assert_eq!(found.mode, Mode::Regular);
    assert_eq!(
        tree_at_path(&store, &root, b"dir").unwrap().unwrap().id,
        inner
    );
    assert!(tree_at_path(&store, &root, b"dir/nope").unwrap().is_none());
    assert!(
        tree_at_path(&store, &root, b"dir/leaf.txt/deeper")
            .unwrap()
            .is_none()
    );
    assert!(tree_at_path(&store, &root, b"").unwrap().is_none());
    assert!(
        tree_at_path(&store, &root, b"dir//leaf.txt")
            .unwrap()
            .is_none()
    );
}

struct Cut {
    listed: BTreeSet<Oid>,
    shallow: BTreeSet<Oid>,
    unshallow: BTreeSet<Oid>,
}

// What a shallow client lists from `shallow_history` pages and its own `Trees`: each boundary's
// tree, a new one's or its own, against no parents, since it has none of them.
fn shallow_listing(
    store: &MemoryObjects,
    wants: &[Oid],
    haves: &[Oid],
    deepen: Option<u32>,
    client: &[Oid],
    most: usize,
) -> Cut {
    let mut listing = ShallowListing::of(wants, haves, deepen, client);
    while let Some(deep) = listing.pending() {
        listing.receive(shallow_history(store, deep, most).unwrap());
    }
    let shallow = set(listing.shallow());
    let unshallow = set(listing.unshallow());
    let cut: BTreeSet<Oid> = shallow.iter().chain(client).copied().collect();
    let listed = listing.into_listed();
    let body = |id: &Oid| store.get(id).unwrap().unwrap().body;
    let tree_of = |id: &Oid| Commit::parse(&body(id), Hash::Sha1).unwrap().tree;
    let mut trees = Trees::default();
    for item in &listed {
        match item.kind {
            Kind::Commit if cut.contains(&item.id) => trees.commit(tree_of(&item.id), Vec::new()),
            Kind::Commit => {
                let commit = Commit::parse(&body(&item.id), Hash::Sha1).unwrap();
                trees.commit(commit.tree, commit.parents.iter().map(tree_of).collect());
            }
            Kind::Tree => trees.tree(item.id),
            Kind::Blob | Kind::Tag => {}
        }
    }
    while !trees.is_done() {
        trees.expand(store).unwrap();
    }
    let trees = trees.into_listed();
    Cut {
        listed: listed.iter().chain(&trees).map(|item| item.id).collect(),
        shallow,
        unshallow,
    }
}

fn set(ids: &[Oid]) -> BTreeSet<Oid> {
    ids.iter().copied().collect()
}

// What a repository holding `commits`, and none of their parents besides, holds.
fn held(store: &MemoryObjects, commits: &[Oid]) -> BTreeSet<Oid> {
    let mut held = set(commits);
    for id in commits {
        let body = store.get(id).unwrap().unwrap().body;
        let tree = Commit::parse(&body, Hash::Sha1).unwrap().tree;
        held.extend(reachable(store, &[tree]));
    }
    held
}

#[test]
fn a_shallow_walk_lists_the_commits_within_its_depth_and_names_the_boundary() {
    let d = dag();
    for most in PAGES {
        let one = shallow_listing(&d.store, &[d.merge], &[], Some(1), &[], most);
        assert_eq!(one.listed, held(&d.store, &[d.merge]), "pages of {most}");
        assert_eq!(one.shallow, set(&[d.merge]));
        let two = shallow_listing(&d.store, &[d.merge], &[], Some(2), &[], most);
        let within = held(&d.store, &[d.merge, d.c, d.b]);
        assert_eq!(two.listed, within, "pages of {most}");
        assert_eq!(two.shallow, set(&[d.c, d.b]));
        let three = shallow_listing(&d.store, &[d.merge], &[], Some(3), &[], most);
        assert_eq!(three.listed, listed(&d, &[d.merge], &[]), "pages of {most}");
        assert_eq!(three.shallow, set(&[d.a, d.root]));
        let four = shallow_listing(&d.store, &[d.merge], &[], Some(4), &[], most);
        assert_eq!(four.listed, three.listed);
        assert!(four.shallow.is_empty() && four.unshallow.is_empty());
    }
}

#[test]
fn a_wanted_tag_puts_its_commit_one_deep_whatever_reaches_it_further_down() {
    let mut d = dag();
    let tag = Tag {
        object: d.a,
        kind: Kind::Commit,
        name: b"v1".to_vec(),
        tagger: Some(signature(250)),
        message: b"v1\n".to_vec(),
    };
    let tag = d.store.put(Kind::Tag, &tag.serialize()).unwrap();
    for wants in [[d.merge, tag], [tag, d.merge]] {
        let two = shallow_listing(&d.store, &wants, &[], Some(2), &[], usize::MAX);
        assert_eq!(two.shallow, set(&[d.c, d.b, d.root]), "{wants:?}");
        let mut expected = held(&d.store, &[d.merge, d.c, d.b, d.a, d.root]);
        expected.insert(tag);
        assert_eq!(two.listed, expected, "{wants:?}");
    }
}

#[test]
fn a_client_deepening_its_boundary_hears_it_lifted_and_gets_what_lies_below() {
    let d = dag();
    let had = held(&d.store, &[d.merge]);
    let now = held(&d.store, &[d.merge, d.c, d.b]);
    for most in PAGES {
        let deeper = shallow_listing(&d.store, &[d.merge], &[d.merge], Some(2), &[d.merge], most);
        assert_eq!(deeper.unshallow, set(&[d.merge]), "pages of {most}");
        assert_eq!(deeper.shallow, set(&[d.c, d.b]), "pages of {most}");
        assert!(!deeper.listed.contains(&d.merge), "pages of {most}");
        assert!(deeper.listed.is_subset(&now), "pages of {most}");
        let holds: BTreeSet<Oid> = had.union(&deeper.listed).copied().collect();
        assert_eq!(holds, now, "pages of {most}");
    }
}

#[test]
fn a_shallow_clients_fetch_walks_from_its_boundary_and_never_past_it() {
    let d = dag();
    let fetched = shallow_listing(&d.store, &[d.merge], &[d.c], None, &[d.c], usize::MAX);
    let commits = set(&[d.merge, d.c, d.b, d.a, d.root]);
    let listed_commits: BTreeSet<Oid> = fetched.listed.intersection(&commits).copied().collect();
    assert_eq!(listed_commits, set(&[d.merge, d.b, d.root]));
    assert!(fetched.shallow.is_empty() && fetched.unshallow.is_empty());
    assert!(
        !listed(&d, &[d.merge], &[d.c]).contains(&d.root),
        "a client that is not shallow holds the root"
    );
}

#[test]
fn shallow_walks_of_generated_histories_hold_what_git_clones_and_deepens() {
    let git_path = git_binary();
    let root = temp_dir("gitcore-shallow");
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    for seed in 0..40 {
        let made = history(&mut rng);
        let source = root.join(format!("source-{seed}"));
        std::fs::create_dir_all(&source).unwrap();
        git(&git_path, &source, &["init", "-q", "--bare"], b"");
        git(
            &git_path,
            &source,
            &["fast-import", "--quiet"],
            &made.import,
        );
        let branches = git(
            &git_path,
            &source,
            &["for-each-ref", "--format=%(refname)", "refs/heads/"],
            b"",
        );
        let first_branch = String::from_utf8(branches).unwrap();
        let first_branch = first_branch.lines().next().unwrap();
        git(
            &git_path,
            &source,
            &["symbolic-ref", "HEAD", first_branch],
            b"",
        );
        let all = git(
            &git_path,
            &source,
            &["cat-file", "--batch-all-objects", "--batch"],
            b"",
        );
        let store = loaded(&all);
        let tips = git(
            &git_path,
            &source,
            &["for-each-ref", "--format=%(objectname)", "refs/heads/"],
            b"",
        );
        let wants: Vec<Oid> = ids(&tips).into_iter().collect();
        let depth = 1 + rng.below(3) as u32;
        let deeper = depth + rng.below(3) as u32;
        let context = format!("history {seed}, depth {depth} then {deeper}");
        let clone = root.join(format!("clone-{seed}"));
        let url = format!("file://{}", source.display());
        let clone_path = clone.display().to_string();
        let depth_arg = format!("--depth={depth}");
        let cloning: [&str; 7] = [
            "clone",
            "-q",
            "--bare",
            "--no-single-branch",
            &depth_arg,
            &url,
            &clone_path,
        ];
        git(&git_path, &root, &cloning, b"");
        let cloned = objects_in(&git_path, &clone);
        let cloned_cut = shallow_file(&clone);
        let ours = shallow_listing(&store, &wants, &[], Some(depth), &[], usize::MAX);
        assert_eq!(ours.listed, cloned, "{context}: the clone");
        assert_eq!(ours.shallow, cloned_cut, "{context}: the clone's boundary");
        assert!(ours.unshallow.is_empty(), "{context}");
        let deeper_arg = format!("--depth={deeper}");
        let fetching: [&str; 5] = [
            "fetch",
            "-q",
            &deeper_arg,
            "origin",
            "+refs/heads/*:refs/heads/*",
        ];
        git(&git_path, &clone, &fetching, b"");
        let fetched = objects_in(&git_path, &clone);
        let fetched_cut = shallow_file(&clone);
        let client: Vec<Oid> = cloned_cut.iter().copied().collect();
        for most in PAGES {
            let again = shallow_listing(&store, &wants, &wants, Some(deeper), &client, most);
            let holds: BTreeSet<Oid> = cloned.union(&again.listed).copied().collect();
            assert_eq!(holds, fetched, "{context}, pages of {most}: the fetch");
            let cut: BTreeSet<Oid> = cloned_cut
                .difference(&again.unshallow)
                .chain(&again.shallow)
                .copied()
                .collect();
            assert_eq!(cut, fetched_cut, "{context}, pages of {most}: its boundary");
        }
    }
    std::fs::remove_dir_all(&root).ok();
}

fn objects_in(git_path: &Path, dir: &Path) -> BTreeSet<Oid> {
    let listed = git(
        git_path,
        dir,
        &[
            "cat-file",
            "--batch-all-objects",
            "--batch-check=%(objectname)",
        ],
        b"",
    );
    ids(&listed)
}

fn shallow_file(dir: &Path) -> BTreeSet<Oid> {
    ids(&std::fs::read(dir.join("shallow")).unwrap_or_default())
}
