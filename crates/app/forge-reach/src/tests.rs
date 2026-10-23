// forge-reach over forge on a MockChain: a lane's result on a change's head carries to the change's
// Preview when every path it read is the same there, and turns Pending, naming the paths, when not.
use std::collections::BTreeMap;

use ::program::{HashKind, MockChain, Origin};
use gitcore::wire::pktline;
use gitcore::{Commit, Hash, Kind, MemoryObjects, Mode, Object, Objects, Oid, Signature};
use gitcore::{Tree, TreeEntry, pack};

use crate::*;

mod enqueue;
mod train;

const OWNER: &[u8] = b"owner-key";
const OWNER_ACCOUNT: u64 = 11;
const LANE: &[u8] = b"lane-key";
const LANE_ACCOUNT: u64 = 23;
const REACH_ACCOUNT: u64 = 903;
const REPO: &str = "project";
const CONTEXT: &str = "build.sh#lane";
const REVIEWER: &[u8] = b"reviewer-key";
const REVIEWER_ACCOUNT: u64 = 56;
const REVIEW: &str = "review";

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
    chain.seat::<::scheduler::Scheduler>(::scheduler::PROGRAM);
    chain.hold(OWNER, OWNER_ACCOUNT);
    chain.hold(LANE, LANE_ACCOUNT);
    chain.hold(REVIEWER, REVIEWER_ACCOUNT);
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
        self.open_from("feature");
    }

    fn open_from(&self, branch: &str) {
        let open = forge::Op::ChangeOpen {
            repo: REPO.into(),
            from: forge::Revision::Ref(format!("refs/heads/{branch}").into_bytes()),
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

    fn say(
        &self,
        key: &[u8],
        commit: Oid,
        context: &str,
        state: CheckState,
        reach: &[&str],
    ) -> Result<Vec<u8>, ::program::Error> {
        let report = Op::Report {
            repo: REPO.into(),
            commit: commit.to_hex(),
            context: context.into(),
            state,
            url: Some("lane.log".into()),
            reach: reach.iter().map(|path| path.to_string()).collect(),
            change: None,
        };
        self.chain
            .submit(Origin::Signed(key.to_vec()), PROGRAM, &report)
    }

    fn require(&self) {
        let require = forge::Op::RequireChecks {
            repo: REPO.into(),
            branch: b"refs/heads/main".to_vec(),
            required: Some(forge::Required {
                reporter: forge::Principal::Account(REACH_ACCOUNT),
                contexts: vec![review_context(), lane_context()],
            }),
        };
        self.forge(&require).unwrap();
    }

    fn wanted(&self, reporter: u64) -> Vec<Rerun> {
        let asked = Query::Wanted {
            reporter,
            page: PageRequest::first(16),
        };
        let Reply::Wanted(page) = self.chain.query(PROGRAM, &asked).unwrap() else {
            panic!("wanted answers Wanted");
        };
        page.items
    }

    fn previewed(&self, n: u64) {
        let preview = forge::Op::Preview {
            repo: REPO.into(),
            n,
        };
        self.forge(&preview).unwrap();
    }

    fn preview_of(&self, n: u64) -> Option<String> {
        let asked = forge::Query::Preview {
            repo: REPO.into(),
            n,
        };
        let reply = self.chain.query(forge::PROGRAM, &asked).unwrap();
        let forge::Reply::Preview { preview, .. } = reply else {
            panic!("a preview answers Preview: {reply:?}");
        };
        preview.map(|preview| preview.merge)
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

fn dev_moves(world: &mut World, base: Oid, files: Files) -> Oid {
    let moved = world.commit(&[base], files);
    world.push(Some(base), moved, "main");
    moved
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

fn rebased(world: &World, feature: Oid) {
    let rebase = forge::Op::Rebase {
        repo: REPO.into(),
        n: 1,
        onto: forge::Revision::Ref(b"refs/heads/main".to_vec()),
        expected_head: feature.to_hex(),
    };
    world.forge(&rebase).unwrap();
}

#[test]
fn a_result_on_the_head_a_rebase_left_carries_when_dev_left_its_files_alone() {
    let mut world = world();
    let (base, feature) = opened(&mut world);
    world.report(feature, &["a/x.rs"], Some(1)).unwrap();
    dev_moves(
        &mut world,
        base,
        &[
            ("Cargo.lock", "lock 1\n"),
            ("a/x.rs", "1\n2\n3\n4\n5\n6\n7\n8\n"),
            ("b/y.rs", "y 2\n"),
        ],
    );
    rebased(&world, feature);
    let merge = world.preview();
    assert_eq!(
        world.statuses(merge),
        [(lane_context(), CheckState::Passed, Some("lane.log".into()))]
    );
    let verdict = world.verdict().unwrap();
    assert_ne!(verdict.head, feature.to_hex(), "the rebase moved the head");
    assert_eq!(verdict.carried[0].gated, feature.to_hex());
    assert!(verdict.carried[0].differs.is_empty());
}

#[test]
fn a_result_on_the_head_a_rebase_left_is_pending_when_dev_edited_its_file() {
    let mut world = world();
    let (base, feature) = opened(&mut world);
    world.report(feature, &["a/x.rs"], Some(1)).unwrap();
    dev_moves(
        &mut world,
        base,
        &[
            ("Cargo.lock", "lock 1\n"),
            ("a/x.rs", "1\n2\n3\n4\n5\n6\n7\neight\n"),
            ("b/y.rs", "y 1\n"),
        ],
    );
    rebased(&world, feature);
    let merge = world.preview();
    assert_eq!(
        world.statuses(merge),
        [(lane_context(), CheckState::Pending, None)]
    );
    let carried = &world.verdict().unwrap().carried[0];
    assert_eq!(carried.gated, feature.to_hex());
    assert_eq!(carried.differs, ["a/x.rs"]);
}

#[test]
fn a_lane_on_a_previewed_head_carries_across_a_rebase() {
    let mut world = world();
    let (base, feature) = opened(&mut world);
    world.preview();
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
    rebased(&world, feature);
    let merge = world.preview();
    assert_eq!(
        world.statuses(merge),
        [(lane_context(), CheckState::Passed, Some("lane.log".into()))]
    );
}

#[test]
fn a_result_on_an_earlier_head_is_pending_when_the_patch_grew() {
    let mut world = world();
    let (_, feature) = opened(&mut world);
    world.report(feature, &["a/x.rs"], Some(1)).unwrap();
    let grown = world.commit(
        &[feature],
        &[
            ("Cargo.lock", "lock 1\n"),
            ("a/x.rs", "one\n2\n3\n4\n5\n6\n7\n8\n"),
            ("b/y.rs", "y 1\n"),
            ("c.md", "new\n"),
        ],
    );
    world.push(Some(feature), grown, "feature");
    let merge = world.preview();
    assert_eq!(
        world.statuses(merge),
        [(lane_context(), CheckState::Pending, None)]
    );
    assert_eq!(world.verdict().unwrap().carried[0].differs, ["c.md"]);
}

fn review_context() -> String {
    format!("{REVIEWER_ACCOUNT}/{REVIEW}")
}

const LOCK_MOVED: Files = &[
    ("Cargo.lock", "lock 2\n"),
    ("a/x.rs", "1\n2\n3\n4\n5\n6\n7\n8\n"),
    ("b/y.rs", "y 1\n"),
];
const LANE_REACH: &[&str] = &["a", "b", "Cargo.lock"];

impl World {
    fn unrequire(&self) {
        let unrequire = forge::Op::RequireChecks {
            repo: REPO.into(),
            branch: b"refs/heads/main".to_vec(),
            required: None,
        };
        self.forge(&unrequire).unwrap();
    }
}

fn reviewed(world: &World, head: Oid) {
    let review = world.say(REVIEWER, head, REVIEW, CheckState::Passed, &["a"]);
    review.unwrap();
}

fn rerun_of(n: u64, commit: Oid) -> Rerun {
    Rerun {
        repo: REPO.into(),
        n,
        commit: commit.to_hex(),
        context: CONTEXT.into(),
    }
}

#[test]
fn a_reviewed_lane_gone_stale_on_a_moved_target_is_wanted_until_it_reports_there() {
    let mut world = world();
    let (base, feature) = opened(&mut world);
    reviewed(&world, feature);
    world.report(feature, LANE_REACH, None).unwrap();
    dev_moves(&mut world, base, LOCK_MOVED);
    world.require();
    let merge = world.preview();
    assert_eq!(world.wanted(LANE_ACCOUNT), [rerun_of(1, merge)]);
    assert!(world.wanted(REVIEWER_ACCOUNT).is_empty());
    world.report(merge, LANE_REACH, None).unwrap();
    assert!(world.wanted(LANE_ACCOUNT).is_empty());
    let lane = (lane_context(), CheckState::Passed, Some("lane.log".into()));
    assert!(world.statuses(merge).contains(&lane));
}

#[test]
fn a_stale_lane_is_not_wanted_before_every_other_required_context_passed() {
    let mut world = world();
    let (base, feature) = opened(&mut world);
    world.report(feature, LANE_REACH, None).unwrap();
    dev_moves(&mut world, base, LOCK_MOVED);
    world.require();
    world.preview();
    assert_eq!(
        world.verdict().unwrap().carried[0].state,
        CheckState::Pending
    );
    assert!(world.wanted(LANE_ACCOUNT).is_empty(), "no review yet");
}

#[test]
fn a_failed_lane_is_not_wanted_again() {
    let mut world = world();
    let (base, feature) = opened(&mut world);
    reviewed(&world, feature);
    let failed = world.say(LANE, feature, CONTEXT, CheckState::Failed, LANE_REACH);
    failed.unwrap();
    dev_moves(&mut world, base, LOCK_MOVED);
    world.require();
    world.preview();
    assert!(world.wanted(LANE_ACCOUNT).is_empty());
}

#[test]
fn a_branch_that_does_not_require_forge_reach_wants_nothing_again() {
    let mut world = world();
    let (base, feature) = opened(&mut world);
    reviewed(&world, feature);
    world.report(feature, LANE_REACH, None).unwrap();
    dev_moves(&mut world, base, LOCK_MOVED);
    let merge = world.preview();
    let lane = (lane_context(), CheckState::Pending, None);
    assert!(world.statuses(merge).contains(&lane));
    assert!(world.wanted(LANE_ACCOUNT).is_empty());
}

fn both_gone_stale(world: &mut World) -> Oid {
    let (base, feature) = opened(world);
    reviewed(world, feature);
    world.report(feature, LANE_REACH, None).unwrap();
    dev_moves(
        world,
        base,
        &[
            ("Cargo.lock", "lock 1\n"),
            ("a/x.rs", "1\n2\n3\n4\n5\n6\n7\neight\n"),
            ("b/y.rs", "y 1\n"),
        ],
    );
    world.require();
    world.preview()
}

fn review_of(n: u64, commit: Oid) -> Rerun {
    Rerun {
        repo: REPO.into(),
        n,
        commit: commit.to_hex(),
        context: REVIEW.into(),
    }
}

#[test]
fn a_review_and_a_lane_gone_stale_together_are_each_wanted_and_told_once() {
    let mut world = world();
    let merge = both_gone_stale(&mut world);
    assert_eq!(world.wanted(LANE_ACCOUNT), [rerun_of(1, merge)]);
    assert_eq!(world.wanted(REVIEWER_ACCOUNT), [review_of(1, merge)]);
    let mut concerns: Vec<Vec<u64>> = told(&world)
        .into_iter()
        .map(|notice| notice.concerns)
        .collect();
    concerns.sort();
    assert_eq!(concerns, [vec![LANE_ACCOUNT], vec![REVIEWER_ACCOUNT]]);
    world.report(merge, LANE_REACH, None).unwrap();
    assert!(world.wanted(LANE_ACCOUNT).is_empty());
    assert_eq!(world.wanted(REVIEWER_ACCOUNT), [review_of(1, merge)]);
    assert_eq!(told(&world).len(), 2, "each is told once per Preview");
}

#[test]
fn a_lane_that_fails_on_the_preview_ends_the_ask_for_a_review_gone_stale_beside_it() {
    let mut world = world();
    let merge = both_gone_stale(&mut world);
    let failed = world.say(LANE, merge, CONTEXT, CheckState::Failed, LANE_REACH);
    failed.unwrap();
    assert!(world.wanted(LANE_ACCOUNT).is_empty());
    assert!(world.wanted(REVIEWER_ACCOUNT).is_empty());
}

#[test]
fn a_newer_preview_asks_again_on_its_own_commit_only() {
    let mut world = world();
    let (base, feature) = opened(&mut world);
    reviewed(&world, feature);
    world.report(feature, LANE_REACH, None).unwrap();
    let moved = dev_moves(&mut world, base, LOCK_MOVED);
    world.require();
    let first = world.preview();
    assert_eq!(world.wanted(LANE_ACCOUNT), [rerun_of(1, first)]);
    world.unrequire();
    dev_moves(
        &mut world,
        moved,
        &[
            ("Cargo.lock", "lock 3\n"),
            ("a/x.rs", "1\n2\n3\n4\n5\n6\n7\n8\n"),
            ("b/y.rs", "y 1\n"),
        ],
    );
    world.require();
    let second = world.preview();
    assert_ne!(first, second);
    assert_eq!(world.wanted(LANE_ACCOUNT), [rerun_of(1, second)]);
}

const LOCKED_FEATURE: Files = &[
    ("Cargo.lock", "lock 2\n"),
    ("a/x.rs", "one\n2\n3\n4\n5\n6\n7\n8\n"),
    ("b/y.rs", "y 1\n"),
];
const OTHER: Files = &[
    ("Cargo.lock", "lock 1\n"),
    ("a/x.rs", "1\n2\n3\n4\n5\n6\n7\n8\n"),
    ("b/y.rs", "y other\n"),
];

fn merged_beside(world: &mut World, review_other: bool) -> String {
    let base = world.commit(&[], BASE);
    let feature = world.commit(&[base], LOCKED_FEATURE);
    let other = world.commit(&[base], OTHER);
    world.push(None, base, "main");
    world.push(None, feature, "feature");
    world.push(None, other, "other");
    world.open();
    world.open_from("other");
    world.require();
    let review = world.say(
        REVIEWER,
        feature,
        REVIEW,
        CheckState::Passed,
        &["a", "Cargo.lock"],
    );
    review.unwrap();
    if review_other {
        let review = world.say(REVIEWER, other, REVIEW, CheckState::Passed, &["b"]);
        review.unwrap();
    }
    for head in [feature, other] {
        world.report(head, LANE_REACH, None).unwrap();
    }
    world.previewed(2);
    let before = world.preview_of(2).expect("change 2 has a Preview");
    world.previewed(1);
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
    let still = world.preview_of(2);
    assert_eq!(still.as_ref(), Some(&before), "the next block previews");
    world.chain.at(3, 0);
    before
}

#[test]
fn a_merge_previews_each_other_reviewed_change_and_its_stale_lane_is_wanted() {
    let mut world = world();
    let before = merged_beside(&mut world, true);
    let after = world.preview_of(2).expect("change 2 has a Preview");
    assert_ne!(after, before, "previewed again on the moved branch");
    let after = Oid::from_hex(Hash::Sha1, &after).unwrap();
    assert_eq!(world.wanted(LANE_ACCOUNT), [rerun_of(2, after)]);
}

fn patch_reviewed(world: &World, head: Oid) -> Result<Vec<u8>, ::program::Error> {
    let review = Op::ReportPatch {
        repo: REPO.into(),
        n: 1,
        commit: head.to_hex(),
        context: REVIEW.into(),
        state: CheckState::Passed,
        url: Some("review.md".into()),
    };
    world
        .chain
        .submit(Origin::Signed(REVIEWER.to_vec()), PROGRAM, &review)
}

fn review_passed() -> (String, CheckState, Option<String>) {
    (
        review_context(),
        CheckState::Passed,
        Some("review.md".into()),
    )
}

const X_REWRITTEN: Files = &[
    ("Cargo.lock", "lock 2\n"),
    ("a/x.rs", "1\n2\n3\n4\n5\n6\n7\neight\n"),
    ("b/y.rs", "y 1\n"),
];

#[test]
fn a_review_of_the_patch_carries_while_the_head_holds_whatever_the_target_did() {
    let mut world = world();
    let (base, feature) = opened(&mut world);
    patch_reviewed(&world, feature).unwrap();
    dev_moves(&mut world, base, X_REWRITTEN);
    let merge = world.preview();
    assert_eq!(world.statuses(merge), [review_passed()]);
    let carried = &world.verdict().unwrap().carried[0];
    assert_eq!(carried.gated, feature.to_hex());
    assert!(carried.differs.is_empty());
}

#[test]
fn a_new_head_with_another_patch_resets_a_review_of_the_patch() {
    let mut world = world();
    let (_, feature) = opened(&mut world);
    patch_reviewed(&world, feature).unwrap();
    let grown = world.commit(
        &[feature],
        &[
            ("Cargo.lock", "lock 1\n"),
            ("a/x.rs", "one\n2\n3\n4\n5\n6\n7\n8\n"),
            ("b/y.rs", "y 1\n"),
            ("c.md", "new\n"),
        ],
    );
    world.push(Some(feature), grown, "feature");
    let merge = world.preview();
    assert_eq!(
        world.statuses(merge),
        [(review_context(), CheckState::Pending, None)]
    );
    assert_eq!(world.verdict().unwrap().carried[0].differs, ["c.md"]);
}

#[test]
fn a_new_head_that_edits_the_same_files_again_resets_a_review_of_the_patch() {
    let mut world = world();
    let (_, feature) = opened(&mut world);
    patch_reviewed(&world, feature).unwrap();
    let edited = world.commit(
        &[feature],
        &[
            ("Cargo.lock", "lock 1\n"),
            ("a/x.rs", "one\ntwo\n3\n4\n5\n6\n7\n8\n"),
            ("b/y.rs", "y 1\n"),
        ],
    );
    world.push(Some(feature), edited, "feature");
    let merge = world.preview();
    assert_eq!(
        world.statuses(merge),
        [(review_context(), CheckState::Pending, None)]
    );
    assert_eq!(world.verdict().unwrap().carried[0].differs, ["a/x.rs"]);
}

fn wide_commit(world: &mut World, parents: &[Oid], files: &[(String, String)]) -> Oid {
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    world.commit(parents, &files)
}

#[test]
fn a_wide_patch_is_compared_with_one_tree_read_per_folder_on_each_side() {
    let mut world = world();
    let mut files: Vec<(String, String)> = vec![("Cargo.lock".into(), "lock 1\n".into())];
    for folder in ["a", "b"] {
        for i in 0..15 {
            files.push((format!("{folder}/f{i}.rs"), "0\n".into()));
        }
    }
    let base = wide_commit(&mut world, &[], &files);
    let edited: Vec<(String, String)> = files
        .iter()
        .map(|(path, text)| match path.contains('/') {
            true => (path.clone(), "1\n".to_owned()),
            false => (path.clone(), text.clone()),
        })
        .collect();
    let feature = wide_commit(&mut world, &[base], &edited);
    world.push(None, base, "main");
    world.push(None, feature, "feature");
    world.open();
    patch_reviewed(&world, feature).unwrap();
    let mut moved = files.clone();
    moved.push(("c.md".into(), "moved\n".into()));
    let moved = wide_commit(&mut world, &[base], &moved);
    world.push(Some(base), moved, "main");
    rebased(&world, feature);
    world.preview();
    let before = crate::rules::TREE_READS.with(|reads| reads.get());
    let verdict = world.verdict().unwrap();
    let reads = crate::rules::TREE_READS.with(|reads| reads.get()) - before;
    assert!(verdict.carried[0].differs.is_empty());
    assert_eq!(
        reads, 8,
        "2 folders at each side of the 2 patches, for 30 files"
    );
}

#[test]
fn a_review_of_the_patch_carries_across_a_clean_rebase() {
    let mut world = world();
    let (base, feature) = opened(&mut world);
    patch_reviewed(&world, feature).unwrap();
    dev_moves(
        &mut world,
        base,
        &[
            ("Cargo.lock", "lock 1\n"),
            ("a/x.rs", "1\n2\n3\n4\n5\n6\n7\n8\n"),
            ("b/y.rs", "y 2\n"),
        ],
    );
    rebased(&world, feature);
    let merge = world.preview();
    assert_eq!(world.statuses(merge), [review_passed()]);
    assert_ne!(world.verdict().unwrap().head, feature.to_hex());
}

#[test]
fn a_lane_beside_a_review_of_the_patch_is_wanted_when_the_target_rewrote_a_reviewed_file() {
    let mut world = world();
    let (base, feature) = opened(&mut world);
    patch_reviewed(&world, feature).unwrap();
    world.report(feature, LANE_REACH, None).unwrap();
    dev_moves(&mut world, base, X_REWRITTEN);
    world.require();
    let merge = world.preview();
    assert!(world.statuses(merge).contains(&review_passed()));
    let lane = (lane_context(), CheckState::Pending, None);
    assert!(world.statuses(merge).contains(&lane));
    assert_eq!(world.wanted(LANE_ACCOUNT), [rerun_of(1, merge)]);
}

#[test]
fn a_review_of_the_patch_replaces_a_report_of_the_same_commit_and_context() {
    let mut world = world();
    let (_, feature) = opened(&mut world);
    reviewed(&world, feature);
    patch_reviewed(&world, feature).unwrap();
    let asked = Query::Reports {
        repo: REPO.into(),
        commit: feature.to_hex(),
        page: PageRequest::first(8),
    };
    let Reply::Reports(page) = world.chain.query(PROGRAM, &asked).unwrap() else {
        panic!("reports answer Reports");
    };
    let review: Vec<_> = page
        .items
        .iter()
        .map(|item| (item.reporter, item.context.as_str(), item.reach.len()))
        .collect();
    assert_eq!(review, [(REVIEWER_ACCOUNT, REVIEW, 0)]);
    let merge = world.preview();
    assert_eq!(world.statuses(merge), [review_passed()]);
    reviewed(&world, feature);
    let merge = world.preview();
    let review = (
        review_context(),
        CheckState::Passed,
        Some("lane.log".into()),
    );
    assert_eq!(world.statuses(merge), [review]);
    assert_eq!(world.verdict().unwrap().carried.len(), 1);
}

#[test]
fn reports_lists_a_result_of_the_patch_after_the_trees_with_an_empty_reach() {
    let mut world = world();
    let (_, feature) = opened(&mut world);
    world.report(feature, &["a"], None).unwrap();
    patch_reviewed(&world, feature).unwrap();
    let mut seen = Vec::new();
    let mut after = None;
    loop {
        let asked = Query::Reports {
            repo: REPO.into(),
            commit: feature.to_hex(),
            page: PageRequest::resume(after, 1),
        };
        let Reply::Reports(page) = world.chain.query(PROGRAM, &asked).unwrap() else {
            panic!("reports answer Reports");
        };
        seen.extend(
            page.items
                .into_iter()
                .map(|item| (item.reporter, item.context, item.reach)),
        );
        match page.next {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    let lane = (LANE_ACCOUNT, CONTEXT.to_owned(), vec!["a".to_owned()]);
    let review = (REVIEWER_ACCOUNT, REVIEW.to_owned(), Vec::new());
    assert_eq!(seen, [lane, review]);
}

#[test]
fn a_result_of_the_patch_touches_the_verdict_and_the_reports_of_its_commit() {
    use ::program::Reads;
    let row = crate::state::PATCHES.key(&(
        REPO.to_owned(),
        "0".repeat(40),
        REVIEWER_ACCOUNT,
        REVIEW.to_owned(),
    ));
    let verdict = Query::Verdict {
        repo: REPO.into(),
        n: 1,
    };
    let reports = Query::Reports {
        repo: REPO.into(),
        commit: "0".repeat(40),
        page: PageRequest::first(8),
    };
    assert!(verdict.touched_by(PROGRAM, std::slice::from_ref(&row)));
    assert!(reports.touched_by(PROGRAM, &[row]));
}

#[test]
fn a_review_of_the_patch_names_a_full_commit_and_its_context() {
    let mut world = world();
    let (_, feature) = opened(&mut world);
    for (commit, context) in [
        (feature.to_hex()[..12].to_owned(), REVIEW),
        (feature.to_hex(), " "),
    ] {
        let review = Op::ReportPatch {
            repo: REPO.into(),
            n: 1,
            commit,
            context: context.into(),
            state: CheckState::Passed,
            url: None,
        };
        let refused = world
            .chain
            .submit(Origin::Signed(REVIEWER.to_vec()), PROGRAM, &review)
            .unwrap_err();
        assert_eq!(refused.code, ::program::code::INVALID_INPUT);
    }
}

fn told(world: &World) -> Vec<chat::Notice> {
    let roots = chat::Query::Roots {
        channel_id: format!("forge:{REPO}:1"),
        viewer: Vec::new(),
        page: PageRequest::first(32),
    };
    let chat::Reply::Roots(page) = world.chain.query("chat", &roots).unwrap() else {
        panic!("Roots answers Roots");
    };
    page.items
        .iter()
        .filter_map(|row| row.notice())
        .filter(|notice| notice.kind == "check")
        .cloned()
        .collect()
}

fn asked_of(world: &World) -> Vec<(Vec<u64>, String, String)> {
    told(world)
        .into_iter()
        .map(|notice| (notice.concerns, notice.group, notice.detail))
        .collect()
}

#[test]
fn a_review_wanted_again_at_the_queue_front_tells_its_reviewer_once_in_the_changes_room() {
    let mut world = world();
    let (base, feature) = opened(&mut world);
    reviewed(&world, feature);
    dev_moves(&mut world, base, X_REWRITTEN);
    world.require();
    let merge = world.preview();
    assert!(told(&world).is_empty(), "the lane has not passed there yet");
    world.report(merge, LANE_REACH, None).unwrap();
    let review = Rerun {
        repo: REPO.into(),
        n: 1,
        commit: merge.to_hex(),
        context: REVIEW.into(),
    };
    assert_eq!(world.wanted(REVIEWER_ACCOUNT), [review]);
    let context = review_context();
    let detail = format!(
        "{context} wanted again on Preview {merge}: its result on {feature} no longer carries"
    );
    let asked = (vec![REVIEWER_ACCOUNT], format!("{context} {merge}"), detail);
    assert_eq!(asked_of(&world), std::slice::from_ref(&asked));
    let notice = &told(&world)[0];
    assert_eq!(notice.status, chat::message::Status::Running);
    assert_eq!(notice.subject, format!("{REPO} #1: Feature"));
    world.report(merge, LANE_REACH, None).unwrap();
    assert_eq!(asked_of(&world), [asked], "told once per Preview");
}

#[test]
fn each_preview_that_wants_a_result_again_tells_its_reporter_once() {
    let mut world = world();
    let (base, feature) = opened(&mut world);
    reviewed(&world, feature);
    world.report(feature, LANE_REACH, None).unwrap();
    let moved = dev_moves(&mut world, base, LOCK_MOVED);
    world.require();
    let first = world.preview();
    world.unrequire();
    dev_moves(
        &mut world,
        moved,
        &[
            ("Cargo.lock", "lock 3\n"),
            ("a/x.rs", "1\n2\n3\n4\n5\n6\n7\n8\n"),
            ("b/y.rs", "y 1\n"),
        ],
    );
    world.require();
    let second = world.preview();
    let lane = lane_context();
    let groups: Vec<(Vec<u64>, String)> = told(&world)
        .into_iter()
        .map(|notice| (notice.concerns, notice.group))
        .collect();
    assert_eq!(
        groups,
        [
            (vec![LANE_ACCOUNT], format!("{lane} {second}")),
            (vec![LANE_ACCOUNT], format!("{lane} {first}")),
        ]
    );
}

#[test]
fn a_notice_chat_refuses_leaves_the_result_wanted() {
    let mut world = world();
    let (base, feature) = opened(&mut world);
    reviewed(&world, feature);
    world.report(feature, LANE_REACH, None).unwrap();
    dev_moves(&mut world, base, LOCK_MOVED);
    world.require();
    let archive = chat::Op::SetChannelArchived {
        channel_id: format!("forge:{REPO}:1"),
        archived: true,
    };
    world.chain.submit(Origin::Root, "chat", &archive).unwrap();
    let merge = world.preview();
    assert_eq!(world.wanted(LANE_ACCOUNT), [rerun_of(1, merge)]);
    assert!(told(&world).is_empty());
}

#[test]
fn a_merge_leaves_a_change_with_a_required_context_unpassed_as_it_was() {
    let mut world = world();
    let before = merged_beside(&mut world, false);
    assert_eq!(world.preview_of(2), Some(before));
    assert!(world.wanted(LANE_ACCOUNT).is_empty());
}

const TRAIN_LENGTH: u64 = 6;

fn train_files(changed: Option<u64>) -> Vec<(String, String)> {
    (0..2 * TRAIN_LENGTH)
        .map(|i| {
            let text = match changed == Some(i) {
                true => format!("{i}"),
                false => "0".into(),
            };
            (format!("t{i}"), text)
        })
        .collect()
}

fn train_commit(world: &mut World, parents: &[Oid], changed: Option<u64>) -> Oid {
    let files = train_files(changed);
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    world.commit(parents, &files)
}

fn train_formed(world: &World) -> Vec<u64> {
    let asked = forge::Query::Train {
        repo: REPO.into(),
        branch: b"refs/heads/main".to_vec(),
    };
    let reply = world.chain.query(forge::PROGRAM, &asked).unwrap();
    let forge::Reply::Train { formed, .. } = reply else {
        panic!("a train answers Train: {reply:?}");
    };
    formed
        .map(|formed| formed.members.iter().map(|member| member.n).collect())
        .unwrap_or_default()
}

fn train_change_state(world: &World, n: u64) -> forge::ChangeState {
    let asked = forge::Query::Change {
        repo: REPO.into(),
        n,
        page: PageRequest::first(1),
    };
    let reply = world.chain.query(forge::PROGRAM, &asked).unwrap();
    let forge::Reply::Change { change, .. } = reply else {
        panic!("a change answers Change: {reply:?}");
    };
    change.state
}

#[test]
fn a_train_that_forms_inside_a_landing_takes_every_member_whatever_its_length() {
    let mut world = world();
    let base = train_commit(&mut world, &[], None);
    world.push(None, base, "main");
    let mut heads = Vec::new();
    for i in 0..2 * TRAIN_LENGTH {
        let head = train_commit(&mut world, &[base], Some(i));
        let branch = format!("t{i}");
        world.push(None, head, &branch);
        world.open_from(&branch);
        heads.push(head);
    }
    world.require();
    let train = forge::Op::SetTrain {
        repo: REPO.into(),
        branch: b"refs/heads/main".to_vec(),
        train: Some(forge::Train {
            length: TRAIN_LENGTH,
            tip: vec![lane_context()],
        }),
    };
    world.forge(&train).unwrap();
    for (i, head) in heads.iter().enumerate() {
        let reach = format!("t{i}");
        world
            .say(
                REVIEWER,
                *head,
                REVIEW,
                CheckState::Passed,
                &[reach.as_str()],
            )
            .unwrap();
    }
    for (i, head) in heads.iter().enumerate() {
        let enqueue = forge::Op::Enqueue {
            repo: REPO.into(),
            n: i as u64 + 1,
            expected_head: head.to_hex(),
        };
        world.forge(&enqueue).unwrap();
    }
    let first: Vec<u64> = (1..=TRAIN_LENGTH).collect();
    assert_eq!(train_formed(&world), first);
    let tip = world
        .preview_of(TRAIN_LENGTH)
        .expect("the tip has a Preview");
    let tip = Oid::from_hex(Hash::Sha1, &tip).unwrap();
    let everything: Vec<String> = (0..2 * TRAIN_LENGTH).map(|i| format!("t{i}")).collect();
    let reach: Vec<&str> = everything.iter().map(String::as_str).collect();
    world.report(tip, &reach, Some(TRAIN_LENGTH)).unwrap();
    for n in first {
        assert_eq!(train_change_state(&world, n), forge::ChangeState::Merged);
    }
    let second: Vec<u64> = (TRAIN_LENGTH + 1..=2 * TRAIN_LENGTH).collect();
    assert_eq!(
        train_formed(&world),
        second,
        "the next train forms whole inside the frame that landed the first"
    );
    for n in second {
        let merge = world.preview_of(n).expect("each member has a Preview");
        let merge = Oid::from_hex(Hash::Sha1, &merge).unwrap();
        let reviewed = world.statuses(merge).iter().any(|(context, state, _)| {
            *context == review_context() && *state == CheckState::Passed
        });
        assert!(reviewed, "forge-reach heard member {n}'s Preview");
    }
}

fn hearing(world: &World, deliver: Vec<u8>) {
    let watch = forge::Op::Watch { deliver };
    let from_reach = Origin::Program(PROGRAM.into());
    world
        .chain
        .submit(from_reach, forge::PROGRAM, &watch)
        .unwrap();
}

fn enqueued(world: &World, n: u64, head: Oid) {
    let enqueue = forge::Op::Enqueue {
        repo: REPO.into(),
        n,
        expected_head: head.to_hex(),
    };
    world.forge(&enqueue).unwrap();
}

fn state_of(world: &World, n: u64) -> forge::ChangeState {
    let asked = forge::Query::Change {
        repo: REPO.into(),
        n,
        page: PageRequest::first(1),
    };
    let reply = world.chain.query(forge::PROGRAM, &asked).unwrap();
    let forge::Reply::Change { change, .. } = reply else {
        panic!("a change answers Change: {reply:?}");
    };
    change.state
}

fn queued_beside(world: &mut World) -> (Oid, Oid) {
    let base = world.commit(&[], BASE);
    let feature = world.commit(&[base], LOCKED_FEATURE);
    let other = world.commit(&[base], OTHER);
    world.push(None, base, "main");
    world.push(None, feature, "feature");
    world.push(None, other, "other");
    world.open();
    world.open_from("other");
    world.require();
    let feature_reach = &["a", "Cargo.lock"];
    world
        .say(REVIEWER, feature, REVIEW, CheckState::Passed, feature_reach)
        .unwrap();
    world
        .say(LANE, feature, CONTEXT, CheckState::Passed, feature_reach)
        .unwrap();
    world
        .say(REVIEWER, other, REVIEW, CheckState::Passed, &["b"])
        .unwrap();
    world
        .say(LANE, other, CONTEXT, CheckState::Passed, &["b"])
        .unwrap();
    (feature, other)
}

const CASCADE: usize = 8;

fn cascade_files(changed: Option<usize>) -> Vec<(String, String)> {
    (0..CASCADE)
        .map(|i| {
            let text = if changed == Some(i) {
                format!("{i}")
            } else {
                "0".into()
            };
            (format!("f{i}"), text)
        })
        .collect()
}

fn committed(world: &mut World, parents: &[Oid], files: &[(String, String)]) -> Oid {
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    world.commit(parents, &files)
}

fn cascaded(world: &mut World) -> Vec<Oid> {
    let base = committed(world, &[], &cascade_files(None));
    world.push(None, base, "main");
    let mut heads = Vec::new();
    for i in 0..CASCADE {
        let head = committed(world, &[base], &cascade_files(Some(i)));
        let branch = format!("c{i}");
        world.push(None, head, &branch);
        world.open_from(&branch);
        heads.push(head);
    }
    world.require();
    for (i, head) in heads.iter().enumerate() {
        let reach = format!("f{i}");
        world
            .say(
                REVIEWER,
                *head,
                REVIEW,
                CheckState::Passed,
                &[reach.as_str()],
            )
            .unwrap();
        if i > 0 {
            world
                .say(LANE, *head, CONTEXT, CheckState::Passed, &[reach.as_str()])
                .unwrap();
        }
    }
    for (i, head) in heads.iter().enumerate() {
        enqueued(world, i as u64 + 1, *head);
    }
    heads
}

#[test]
fn a_queue_front_whose_preview_the_depth_cap_kept_from_forge_reach_lands_on_a_retell() {
    let mut world = world();
    let heads = cascaded(&mut world);
    world.report(heads[0], &["f0"], Some(1)).unwrap();
    let front = (1..=CASCADE as u64)
        .find(|n| state_of(&world, *n) == forge::ChangeState::Open)
        .expect("the depth cap ends the chain of merges before the queue empties");
    let merge = world.preview_of(front).expect("the front has a Preview");
    let merge = Oid::from_hex(Hash::Sha1, &merge).unwrap();
    assert!(
        world.statuses(merge).is_empty(),
        "forge-reach never heard of the front's Preview"
    );
    let retell = Op::Retell {
        repo: REPO.into(),
        n: front,
    };
    let anyone = Origin::Signed(OWNER.to_vec());
    world.chain.submit(anyone, PROGRAM, &retell).unwrap();
    assert_eq!(state_of(&world, front), forge::ChangeState::Merged);
}

#[test]
fn a_retell_reports_on_a_preview_forge_reach_never_heard_of() {
    let mut world = world();
    queued_beside(&mut world);
    hearing(&world, Vec::new());
    world.previewed(2);
    let merge = world.preview_of(2).expect("change 2 has a Preview");
    let merge = Oid::from_hex(Hash::Sha1, &merge).unwrap();
    assert!(world.statuses(merge).is_empty());
    let retell = Op::Retell {
        repo: REPO.into(),
        n: 2,
    };
    let anyone = Origin::Signed(OWNER.to_vec());
    world.chain.submit(anyone, PROGRAM, &retell).unwrap();
    let contexts: BTreeMap<String, CheckState> = world
        .statuses(merge)
        .into_iter()
        .map(|(context, state, _)| (context, state))
        .collect();
    let carried = BTreeMap::from([
        (lane_context(), CheckState::Passed),
        (review_context(), CheckState::Passed),
    ]);
    assert_eq!(contexts, carried);
}
