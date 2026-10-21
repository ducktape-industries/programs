// forge-reach over forge on a MockChain: a lane's result on a change's head carries to the change's
// Preview when every path it read is the same there, and turns Pending, naming the paths, when not.
use std::collections::BTreeMap;

use ::program::{HashKind, MockChain, Origin};
use gitcore::wire::pktline;
use gitcore::{Commit, Hash, Kind, MemoryObjects, Mode, Object, Objects, Oid, Signature};
use gitcore::{Tree, TreeEntry, pack};

use crate::*;

const OWNER: &[u8] = b"owner-key";
const OWNER_ACCOUNT: u64 = 11;
const LANE: &[u8] = b"lane-key";
const LANE_ACCOUNT: u64 = 23;
const REACH_ACCOUNT: u64 = 903;
const REPO: &str = "project";
const CONTEXT: &str = "build.sh#lane";

fn bounds() -> forge::Bounds {
    forge::Bounds {
        max_objects: 10_000,
        max_delta_depth: 64,
        max_object_size: 1 << 20,
        push_walk: 1000,
        fetch_walk: 1000,
        merge_cost: 100_000,
        page_size: 128,
        log_walk: 1000,
        tree_walk: 256,
        diff_bytes: 4 << 20,
        blob_bytes: 1 << 20,
        record_bytes: 64 << 10,
    }
}

struct World {
    chain: MockChain,
    store: MemoryObjects,
    time: i64,
}

type Files<'a> = &'a [(&'a str, &'a str)];

fn world() -> World {
    let mut chain = MockChain::default();
    chain.seat::<forge::Forge>(forge::PROGRAM);
    chain.seat::<chat::Chat>("chat");
    chain.seat::<access::Access>("access");
    chain.seat::<ForgeReach>(PROGRAM);
    chain.hold(OWNER, OWNER_ACCOUNT);
    chain.hold(LANE, LANE_ACCOUNT);
    let programs = [
        (forge::PROGRAM, 900),
        ("chat", 901),
        ("access", 902),
        (PROGRAM, REACH_ACCOUNT),
    ];
    for (program, number) in programs {
        chain.register(program, number);
    }
    chain.init(forge::PROGRAM, &bounds()).unwrap();
    chain.init(PROGRAM, &()).unwrap();
    let create = forge::Op::Create {
        repo: REPO.into(),
        hash: HashKind::Sha1,
    };
    chain
        .submit(Origin::Signed(OWNER.to_vec()), forge::PROGRAM, &create)
        .unwrap();
    World {
        chain,
        store: MemoryObjects::new(Hash::Sha1),
        time: 1,
    }
}

impl World {
    fn tree(&mut self, files: &[(String, Vec<u8>)]) -> Oid {
        let mut here: BTreeMap<String, Vec<(String, Vec<u8>)>> = BTreeMap::new();
        let mut entries = Vec::new();
        for (path, bytes) in files {
            match path.split_once('/') {
                Some((folder, rest)) => here
                    .entry(folder.to_owned())
                    .or_default()
                    .push((rest.to_owned(), bytes.clone())),
                None => {
                    let blob = self.store.put(Kind::Blob, bytes).unwrap();
                    entries.push((path.clone(), Mode::Regular, blob));
                }
            }
        }
        for (folder, inner) in here {
            let id = self.tree(&inner);
            entries.push((folder, Mode::Directory, id));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let tree = Tree {
            entries: entries
                .into_iter()
                .map(|(name, mode, id)| TreeEntry {
                    mode,
                    name: name.into_bytes(),
                    id,
                })
                .collect(),
        };
        self.store.put(Kind::Tree, &tree.serialize()).unwrap()
    }

    fn commit(&mut self, parents: &[Oid], files: Files) -> Oid {
        self.time += 1;
        let files: Vec<(String, Vec<u8>)> = files
            .iter()
            .map(|(path, text)| (path.to_string(), text.as_bytes().to_vec()))
            .collect();
        let root = self.tree(&files);
        let signature = Signature {
            name: b"Ada".to_vec(),
            email: b"ada@example.com".to_vec(),
            time: self.time,
            offset_minutes: 0,
        };
        let commit = Commit {
            tree: root,
            parents: parents.to_vec(),
            author: signature.clone(),
            committer: signature,
            extra: Vec::new(),
            message: b"step\n".to_vec(),
        };
        self.store.put(Kind::Commit, &commit.serialize()).unwrap()
    }

    fn push(&self, old: Option<Oid>, new: Oid, branch: &str) {
        let objects: Vec<Object> = self
            .store
            .ids()
            .map(|id| self.store.get(id).unwrap().unwrap())
            .collect();
        let pack = pack::write(objects, Hash::Sha1).unwrap();
        let pack = self.chain.put_blob(forge::PACK_KIND, pack).unwrap();
        let old = old.unwrap_or(Hash::Sha1.zero());
        let mut request = Vec::new();
        let line = format!("{old} {new} refs/heads/{branch}\0report-status");
        pktline::push(&mut request, line.as_bytes());
        request.extend_from_slice(pktline::flush());
        let push = forge::Op::Push {
            repo: REPO.into(),
            request,
            pack: Some(pack),
        };
        self.forge(&push).unwrap();
    }

    fn forge(&self, op: &forge::Op) -> Result<Vec<u8>, ::program::Error> {
        let owner = Origin::Signed(OWNER.to_vec());
        self.chain.submit(owner, forge::PROGRAM, op)
    }

    fn open(&self) {
        let open = forge::Op::ChangeOpen {
            repo: REPO.into(),
            from: forge::Revision::Ref(b"refs/heads/feature".to_vec()),
            into: b"refs/heads/main".to_vec(),
            title: "Feature".into(),
            body: String::new(),
            reviewers: Vec::new(),
        };
        self.forge(&open).unwrap();
    }

    fn report(
        &self,
        commit: Oid,
        reach: &[&str],
        change: Option<u64>,
    ) -> Result<Vec<u8>, ::program::Error> {
        let report = Op::Report {
            repo: REPO.into(),
            commit: commit.to_hex(),
            context: CONTEXT.into(),
            state: CheckState::Passed,
            url: Some("lane.log".into()),
            reach: reach.iter().map(|path| path.to_string()).collect(),
            change,
        };
        self.chain
            .submit(Origin::Signed(LANE.to_vec()), PROGRAM, &report)
    }

    fn preview(&self) -> Oid {
        let preview = forge::Op::Preview {
            repo: REPO.into(),
            n: 1,
        };
        self.forge(&preview).unwrap();
        let verdict = self.verdict().expect("the change has a Preview");
        Oid::from_hex(Hash::Sha1, &verdict.merge).unwrap()
    }

    fn verdict(&self) -> Option<PreviewVerdict> {
        let asked = Query::Verdict {
            repo: REPO.into(),
            n: 1,
        };
        let Reply::Verdict(verdict) = self.chain.query(PROGRAM, &asked).unwrap() else {
            panic!("a verdict answers Verdict");
        };
        verdict
    }

    fn statuses(&self, commit: Oid) -> Vec<(String, CheckState, Option<String>)> {
        let asked = forge::Query::Statuses {
            repo: REPO.into(),
            commit: commit.to_hex(),
            page: PageRequest::first(16),
        };
        let reply = self.chain.query(forge::PROGRAM, &asked).unwrap();
        let forge::Reply::Statuses { page, .. } = reply else {
            panic!("statuses answer Statuses: {reply:?}");
        };
        let reach = forge::Principal::Account(REACH_ACCOUNT);
        assert!(page.items.iter().all(|status| status.reporter == reach));
        page.items
            .into_iter()
            .map(|status| (status.context, status.state, status.url))
            .collect()
    }
}

fn lane_context() -> String {
    format!("{LANE_ACCOUNT}/{CONTEXT}")
}

const BASE: Files = &[
    ("Cargo.lock", "lock 1\n"),
    ("a/x.rs", "1\n2\n3\n4\n5\n6\n7\n8\n"),
    ("b/y.rs", "y 1\n"),
];
const FEATURE: Files = &[
    ("Cargo.lock", "lock 1\n"),
    ("a/x.rs", "one\n2\n3\n4\n5\n6\n7\n8\n"),
    ("b/y.rs", "y 1\n"),
];

fn opened(world: &mut World) -> (Oid, Oid) {
    let base = world.commit(&[], BASE);
    let feature = world.commit(&[base], FEATURE);
    world.push(None, base, "main");
    world.push(None, feature, "feature");
    world.open();
    (base, feature)
}

fn dev_moves(world: &mut World, base: Oid, files: Files) {
    let moved = world.commit(&[base], files);
    world.push(Some(base), moved, "main");
}

#[test]
fn a_result_carries_to_a_preview_when_dev_moved_only_outside_its_reach() {
    let mut world = world();
    let (base, feature) = opened(&mut world);
    world.report(feature, &["a", "Cargo.lock"], None).unwrap();
    dev_moves(
        &mut world,
        base,
        &[
            ("Cargo.lock", "lock 1\n"),
            ("a/x.rs", "1\n2\n3\n4\n5\n6\n7\n8\n"),
            ("b/y.rs", "y 2\n"),
        ],
    );
    let merge = world.preview();
    let expected = (lane_context(), CheckState::Passed, Some("lane.log".into()));
    assert_eq!(world.statuses(merge), [expected]);
    let verdict = world.verdict().unwrap();
    assert_eq!(verdict.head, feature.to_hex());
    assert!(verdict.carried[0].differs.is_empty());
}

#[test]
fn a_result_whose_reach_dev_touched_is_pending_naming_the_paths() {
    let mut world = world();
    let (base, feature) = opened(&mut world);
    world.report(feature, &["a", "Cargo.lock"], None).unwrap();
    dev_moves(
        &mut world,
        base,
        &[
            ("Cargo.lock", "lock 2\n"),
            ("a/x.rs", "1\n2\n3\n4\n5\n6\n7\n8\n"),
            ("b/y.rs", "y 1\n"),
        ],
    );
    let merge = world.preview();
    assert_eq!(
        world.statuses(merge),
        [(lane_context(), CheckState::Pending, None)]
    );
    let carried = &world.verdict().unwrap().carried[0];
    assert_eq!(carried.differs, ["Cargo.lock"]);
    assert_eq!(carried.reported, CheckState::Passed);
}

#[test]
fn a_three_way_merge_that_rewrites_a_reached_file_is_pending() {
    let mut world = world();
    let (base, feature) = opened(&mut world);
    world.report(feature, &["a/x.rs"], None).unwrap();
    dev_moves(
        &mut world,
        base,
        &[
            ("Cargo.lock", "lock 1\n"),
            ("a/x.rs", "1\n2\n3\n4\n5\n6\n7\neight\n"),
            ("b/y.rs", "y 1\n"),
        ],
    );
    let merge = world.preview();
    assert_eq!(
        world.statuses(merge),
        [(lane_context(), CheckState::Pending, None)]
    );
    assert_eq!(world.verdict().unwrap().carried[0].differs, ["a/x.rs"]);
}

#[test]
fn a_report_after_the_preview_names_the_change_and_is_carried_at_once() {
    let mut world = world();
    let (_, feature) = opened(&mut world);
    let merge = world.preview();
    assert!(world.statuses(merge).is_empty());
    world.report(feature, &["a"], Some(1)).unwrap();
    assert_eq!(
        world.statuses(merge),
        [(lane_context(), CheckState::Passed, Some("lane.log".into()))]
    );
}

#[test]
fn a_report_that_names_no_change_after_the_preview_is_carried_to_it() {
    let mut world = world();
    let (_, feature) = opened(&mut world);
    let merge = world.preview();
    assert!(world.statuses(merge).is_empty());
    world.report(feature, &["a"], None).unwrap();
    assert_eq!(
        world.statuses(merge),
        [(lane_context(), CheckState::Passed, Some("lane.log".into()))]
    );
    assert_eq!(world.preview(), merge, "the same heads, the same Preview");
}

#[test]
fn a_report_on_the_preview_commit_that_names_no_change_is_carried_to_it() {
    let mut world = world();
    opened(&mut world);
    let merge = world.preview();
    world.report(merge, &[""], None).unwrap();
    assert_eq!(
        world.statuses(merge),
        [(lane_context(), CheckState::Passed, Some("lane.log".into()))]
    );
}

#[test]
fn a_report_on_a_head_the_change_moved_from_reaches_no_preview() {
    let mut world = world();
    let (_, feature) = opened(&mut world);
    world.preview();
    let pushed = world.commit(
        &[feature],
        &[("Cargo.lock", "lock 1\n"), ("a/x.rs", "two\n")],
    );
    world.push(Some(feature), pushed, "feature");
    let moved = world.preview();
    world.report(feature, &["a"], None).unwrap();
    assert!(world.statuses(moved).is_empty(), "the old head's result");
    world.report(pushed, &["a"], None).unwrap();
    assert_eq!(world.statuses(moved)[0].1, CheckState::Passed);
}

#[test]
fn a_whole_tree_reach_carries_only_while_dev_holds_still() {
    let mut world = world();
    let (base, feature) = opened(&mut world);
    world.report(feature, &[""], None).unwrap();
    let still = world.preview();
    assert_eq!(world.statuses(still)[0].1, CheckState::Passed);
    dev_moves(
        &mut world,
        base,
        &[
            ("Cargo.lock", "lock 1\n"),
            ("a/x.rs", "1\n2\n3\n4\n5\n6\n7\n8\n"),
            ("b/y.rs", "y 1\n"),
            ("c.md", "new\n"),
        ],
    );
    let moved = world.preview();
    assert_eq!(world.statuses(moved)[0].1, CheckState::Pending);
    assert_eq!(world.verdict().unwrap().carried[0].differs, [""]);
}

#[test]
fn a_branch_that_requires_forge_reach_merges_on_a_carried_result() {
    let mut world = world();
    let (base, feature) = opened(&mut world);
    let require = forge::Op::RequireChecks {
        repo: REPO.into(),
        branch: b"refs/heads/main".to_vec(),
        required: Some(forge::Required {
            reporter: forge::Principal::Account(REACH_ACCOUNT),
            contexts: vec![lane_context()],
        }),
    };
    world.forge(&require).unwrap();
    world.report(feature, &["a"], None).unwrap();
    world.preview();
    let merge = forge::Op::SquashMerge {
        repo: REPO.into(),
        into: b"refs/heads/main".to_vec(),
        from: forge::Revision::Ref(b"refs/heads/feature".to_vec()),
        expected_into: base.to_hex(),
        expected_from: feature.to_hex(),
        message: None,
        change: Some(1),
    };
    world.forge(&merge).unwrap();
}

#[test]
fn a_report_names_a_full_commit_its_context_and_a_reach() {
    let mut world = world();
    let (_, feature) = opened(&mut world);
    let short = Op::Report {
        repo: REPO.into(),
        commit: feature.to_hex()[..12].into(),
        context: CONTEXT.into(),
        state: CheckState::Passed,
        url: None,
        reach: vec!["a".into()],
        change: None,
    };
    let refused = world
        .chain
        .submit(Origin::Signed(LANE.to_vec()), PROGRAM, &short)
        .unwrap_err();
    assert_eq!(refused.code, ::program::code::INVALID_INPUT);
    assert!(world.report(feature, &[], None).is_err());
    assert!(world.report(feature, &["a/../b"], None).is_err());
    assert!(world.report(feature, &["/a"], None).is_err());
}

#[test]
fn a_report_said_again_replaces_its_reach_and_only_forge_tells_of_a_preview() {
    let mut world = world();
    let (_, feature) = opened(&mut world);
    world.report(feature, &["a", "b"], None).unwrap();
    world.report(feature, &["Cargo.lock"], None).unwrap();
    let asked = Query::Reports {
        repo: REPO.into(),
        commit: feature.to_hex(),
        page: PageRequest::first(8),
    };
    let Reply::Reports(page) = world.chain.query(PROGRAM, &asked).unwrap() else {
        panic!("reports answer Reports");
    };
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].reach, ["Cargo.lock"]);
    assert_eq!(page.items[0].reporter, LANE_ACCOUNT);
    let forged = Op::Watched(forge::Watched::Previewed {
        repo: REPO.into(),
        n: 1,
        merge: feature.to_hex(),
        by: forge::Principal::Account(LANE_ACCOUNT),
    });
    let refused = world
        .chain
        .submit(Origin::Signed(LANE.to_vec()), PROGRAM, &forged)
        .unwrap_err();
    assert_eq!(refused.code, ::program::code::UNAUTHORIZED);
}

#[test]
fn a_new_preview_touches_the_verdict_and_it_answers_from_that_preview() {
    use ::program::Reads;
    let mut world = world();
    let (base, feature) = opened(&mut world);
    world.report(feature, &["a"], None).unwrap();
    let first = world.preview();
    let asked = Query::Verdict {
        repo: REPO.into(),
        n: 1,
    };
    let preview_row = forge::tables::PREVIEWS.key(&(REPO.to_owned(), 1u64));
    let repo_row = forge::tables::REPOS.key(&REPO.to_owned());
    let unrelated_row = forge::tables::REFS.key(&(REPO.to_owned(), b"refs/heads/main".to_vec()));
    assert!(asked.touched_by(forge::PROGRAM, &[preview_row]));
    assert!(asked.touched_by(forge::PROGRAM, &[repo_row]));
    assert!(!asked.touched_by(forge::PROGRAM, &[unrelated_row]));
    dev_moves(
        &mut world,
        base,
        &[
            ("Cargo.lock", "lock 1\n"),
            ("a/x.rs", "1\n2\n3\n4\n5\n6\n7\n8\n"),
            ("b/y.rs", "y 2\n"),
        ],
    );
    let second = world.preview();
    assert_ne!(first, second);
    let verdict = world.verdict().unwrap();
    assert_eq!(verdict.merge, second.to_hex());
    assert_eq!(world.statuses(second)[0].1, CheckState::Passed);
}
