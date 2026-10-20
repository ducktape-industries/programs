//! A required context also passes on a change's head: its reporter's status there stands while the
//! target has changed none of the paths that status depends on (its reach) since their merge base.
mod common;
use common::*;
use forge::{CheckState, OpReply, Required};

const MAIN: &str = "refs/heads/main";
const FEATURE: &str = "refs/heads/feature";
const LANE: &str = "build.sh#lane";

const BASE: &[(&str, &str)] = &[
    ("Cargo.lock", "1\n"),
    ("crates/a/lib.rs", "1\n"),
    ("crates/b/lib.rs", "1\n"),
];
const FEATURED: &[(&str, &str)] = &[
    ("Cargo.lock", "1\n"),
    ("crates/a/lib.rs", "2\n"),
    ("crates/b/lib.rs", "1\n"),
];

fn branch(name: &str) -> forge::Revision {
    forge::Revision::Ref(name.as_bytes().to_vec())
}

fn dir(store: &mut MemoryObjects, files: &[(String, Vec<u8>)]) -> Oid {
    let mut blobs = BTreeMap::new();
    let mut dirs: BTreeMap<String, Vec<(String, Vec<u8>)>> = BTreeMap::new();
    for (path, body) in files {
        match path.split_once('/') {
            Some((head, rest)) => dirs
                .entry(head.to_owned())
                .or_default()
                .push((rest.to_owned(), body.clone())),
            None => {
                blobs.insert(path.clone(), body.clone());
            }
        }
    }
    let mut entries = Vec::new();
    for (name, body) in blobs {
        let id = blob(store, &body);
        entries.push(TreeEntry {
            mode: Mode::Regular,
            name: name.into_bytes(),
            id,
        });
    }
    for (name, inner) in dirs {
        let id = dir(store, &inner);
        entries.push(TreeEntry {
            mode: Mode::Directory,
            name: name.into_bytes(),
            id,
        });
    }
    entries.sort_by_key(|entry| {
        let mut key = entry.name.clone();
        if entry.mode == Mode::Directory {
            key.push(b'/');
        }
        key
    });
    store
        .put(Kind::Tree, &Tree { entries }.serialize())
        .unwrap()
}

fn snapshot(store: &mut MemoryObjects, parents: &[Oid], time: i64, files: &[(&str, &str)]) -> Oid {
    let files: Vec<(String, Vec<u8>)> = files
        .iter()
        .map(|(path, body)| (path.to_string(), body.as_bytes().to_vec()))
        .collect();
    let root = dir(store, &files);
    commit(store, root, parents, time, "step")
}

struct World {
    sandbox: MemorySandbox,
    main: Oid,
    feature: Oid,
}

fn world(main: Option<&[(&str, &str)]>) -> World {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let root = snapshot(&mut store, &[], 1, BASE);
    let feature = snapshot(&mut store, &[root], 2, FEATURED);
    let main = match main {
        Some(files) => snapshot(&mut store, &[root], 3, files),
        None => root,
    };
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let zero = Hash::Sha1.zero();
    let refs = [(zero, main, MAIN), (zero, feature, FEATURE)];
    let pack = pack_of(&store, &all_ids(&store));
    push(&sandbox, OWNER, "project", &refs, &pack).unwrap();
    let open = Op::ChangeOpen {
        repo: "project".into(),
        from: branch(FEATURE),
        into: MAIN.as_bytes().to_vec(),
        title: "Teach a".into(),
        body: "Why a changes.".into(),
        reviewers: Vec::new(),
    };
    act(&sandbox, WRITER, &open).unwrap();
    let require = Op::RequireChecks {
        repo: "project".into(),
        branch: MAIN.as_bytes().to_vec(),
        required: Some(Required {
            reporter: person(STRANGER),
            contexts: vec![LANE.into()],
        }),
    };
    act(&sandbox, OWNER, &require).unwrap();
    World {
        sandbox,
        main,
        feature,
    }
}

impl World {
    fn gated_op(&self, state: CheckState, reach: &[&str]) -> Op {
        Op::Gated {
            repo: "project".into(),
            commit: self.feature.to_hex(),
            context: LANE.into(),
            state,
            url: Some("/shared/lane/log".into()),
            reach: reach.iter().map(|path| path.as_bytes().to_vec()).collect(),
        }
    }

    fn gated(&self, reporter: &[u8], state: CheckState, reach: &[&str]) {
        act(&self.sandbox, reporter, &self.gated_op(state, reach)).unwrap();
    }

    fn report(&self, commit: Oid, state: CheckState) {
        let op = Op::Status {
            repo: "project".into(),
            commit: commit.to_hex(),
            context: LANE.into(),
            state,
            url: None,
        };
        act(&self.sandbox, STRANGER, &op).unwrap();
    }

    fn status(&self, state: CheckState) {
        self.report(self.feature, state);
    }

    fn preview(&self) -> Oid {
        let op = Op::Preview {
            repo: "project".into(),
            n: 1,
        };
        match abi::decode(&act(&self.sandbox, STRANGER, &op).unwrap()).unwrap() {
            OpReply::Previewed { oid, .. } => Oid::from_hex(Hash::Sha1, &oid).unwrap(),
            other => panic!("{other:?}"),
        }
    }

    fn squash(&self) -> Op {
        Op::SquashMerge {
            repo: "project".into(),
            into: MAIN.as_bytes().to_vec(),
            from: branch(FEATURE),
            expected_into: self.main.to_hex(),
            expected_from: self.feature.to_hex(),
            message: None,
            change: Some(1),
        }
    }

    fn refusal(&self) -> String {
        let refused = refused(&self.sandbox, OWNER, &self.squash());
        assert_eq!(refused.code, code::WRONG_STATE, "{}", refused.message);
        refused.message
    }

    fn lands(&self) {
        act(&self.sandbox, OWNER, &self.squash()).unwrap();
        assert_ne!(refs_of(&self.sandbox, "project")[MAIN], self.main.to_hex());
    }
}

#[test]
fn a_head_gated_with_a_reach_lands_with_no_preview_while_the_target_moved_only_outside_it() {
    let moved_b = &[
        ("Cargo.lock", "1\n"),
        ("crates/a/lib.rs", "1\n"),
        ("crates/b/lib.rs", "2\n"),
    ];
    let world = world(Some(moved_b));
    let silent = world.refusal();
    assert!(silent.contains("has no Preview"), "{silent}");
    assert!(
        silent.contains("no build.sh#lane from account 13"),
        "{silent}"
    );
    world.gated(WRITER, CheckState::Passed, &["crates/a", "Cargo.lock"]);
    let other = world.refusal();
    assert!(
        other.contains("no build.sh#lane from account 13"),
        "another reporter's: {other}"
    );
    world.gated(STRANGER, CheckState::Failed, &["crates/a", "Cargo.lock"]);
    let red = world.refusal();
    assert!(
        red.contains("build.sh#lane failed (/shared/lane/log)"),
        "{red}"
    );
    world.gated(STRANGER, CheckState::Passed, &["crates/a", "Cargo.lock"]);
    world.lands();
}

#[test]
fn a_target_move_inside_the_reach_refuses_the_head_naming_each_path_that_moved() {
    let moved = &[
        ("Cargo.lock", "2\n"),
        ("crates/a/extra.rs", "1\n"),
        ("crates/a/lib.rs", "1\n"),
        ("crates/b/lib.rs", "2\n"),
    ];
    let world = world(Some(moved));
    world.gated(
        STRANGER,
        CheckState::Passed,
        &["crates/a", "Cargo.lock", "docs"],
    );
    let inside = world.refusal();
    assert!(
        inside.contains("changed Cargo.lock, crates/a since"),
        "{inside}"
    );
    assert!(inside.contains("gate the head again"), "{inside}");
    assert!(!inside.contains("crates/b"), "{inside}");
    assert!(!inside.contains("docs"), "{inside}");
    world.gated(STRANGER, CheckState::Passed, &["docs", "crates/c"]);
    world.lands();
}

#[test]
fn a_reach_fails_closed() {
    let moved = &[
        ("Cargo.lock", "2\n"),
        ("crates/a/lib.rs", "1\n"),
        ("crates/b/lib.rs", "2\n"),
    ];
    let world = world(Some(moved));
    let unsound = [
        "./crates/b",
        "crates/../b",
        "/Cargo.lock",
        "crates/a/",
        "crates//a",
        "",
    ];
    for path in unsound {
        let op = world.gated_op(CheckState::Passed, &["docs", path]);
        let refusal = refused(&world.sandbox, STRANGER, &op);
        assert_eq!(
            refusal.code,
            code::INVALID_INPUT,
            "{path:?}: {}",
            refusal.message
        );
        let named = format!("reach path {path:?}");
        assert!(refusal.message.contains(&named), "{}", refusal.message);
    }
    world.gated(STRANGER, CheckState::Passed, &[]);
    let whole = world.refusal();
    assert!(whole.contains("changed the whole tree"), "{whole}");
}

#[test]
fn a_fresh_preview_red_in_the_context_refuses_whatever_the_head_says() {
    let moved_b = &[
        ("Cargo.lock", "1\n"),
        ("crates/a/lib.rs", "1\n"),
        ("crates/b/lib.rs", "2\n"),
    ];
    let world = world(Some(moved_b));
    world.gated(STRANGER, CheckState::Passed, &["crates/a"]);
    let previewed = world.preview();
    world.report(previewed, CheckState::Failed);
    let red = world.refusal();
    assert!(red.contains("build.sh#lane failed"), "{red}");
    assert!(!red.contains("nor is its head"), "{red}");
    world.report(previewed, CheckState::Pending);
    let pending = world.refusal();
    assert!(pending.contains("build.sh#lane pending"), "{pending}");
    world.report(previewed, CheckState::Passed);
    world.lands();
}

#[test]
fn a_status_with_no_reach_stands_on_the_head_only_while_the_target_sits_at_the_merge_base() {
    let still = world(None);
    still.status(CheckState::Passed);
    still.lands();

    let moved_b = &[
        ("Cargo.lock", "1\n"),
        ("crates/a/lib.rs", "1\n"),
        ("crates/b/lib.rs", "2\n"),
    ];
    let world = world(Some(moved_b));
    world.gated(STRANGER, CheckState::Passed, &["crates/a"]);
    world.status(CheckState::Passed);
    let whole = world.refusal();
    assert!(whole.contains("changed the whole tree"), "{whole}");
    world.gated(STRANGER, CheckState::Passed, &["crates/a"]);
    world.lands();
}
