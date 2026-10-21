//! A repository created now takes writes from its owner alone until the owner lists others,
//! and its settings from its owner alone until the owner lists others there; anyone still opens
//! and reviews a change.

mod common;
use common::*;

const MAIN: &str = "refs/heads/main";
const FEATURE: &str = "refs/heads/feature";

struct Seeded {
    sandbox: MemorySandbox,
    store: MemoryObjects,
    main: Oid,
    feature: Oid,
    n: u64,
}

fn seeded() -> Seeded {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let mut store = MemoryObjects::new(Hash::Sha1);
    let main = file_commit(&mut store, &[], 1, &[("a", b"1\n")]);
    let feature = file_commit(&mut store, &[main], 2, &[("a", b"2\n")]);
    let zero = Hash::Sha1.zero();
    let refs = [(zero, main, MAIN), (zero, feature, FEATURE)];
    let pack = pack_of(&store, &all_ids(&store));
    push(&sandbox, OWNER, "project", &refs, &pack).unwrap();
    let open = Op::ChangeOpen {
        repo: "project".into(),
        from: forge::Revision::Ref(FEATURE.as_bytes().to_vec()),
        into: MAIN.as_bytes().to_vec(),
        title: "Teach a".into(),
        body: String::new(),
        reviewers: Vec::new(),
    };
    let forge::OpReply::Change { n, .. } =
        abi::decode(&act(&sandbox, STRANGER, &open).unwrap()).unwrap()
    else {
        panic!("ChangeOpen answers the change");
    };
    Seeded {
        sandbox,
        store,
        main,
        feature,
        n,
    }
}

fn next(seeded: &mut Seeded) -> (Oid, Vec<u8>) {
    let held: BTreeSet<Oid> = all_ids(&seeded.store).into_iter().collect();
    let parent = [seeded.feature];
    let child = file_commit(&mut seeded.store, &parent, 3, &[("a", b"3\n")]);
    let new: Vec<Oid> = all_ids(&seeded.store)
        .into_iter()
        .filter(|id| !held.contains(id))
        .collect();
    (child, pack_of(&seeded.store, &new))
}

fn settings_ops() -> [Op; 4] {
    [
        Op::Configure {
            repo: "project".into(),
            settings: Settings::default(),
        },
        Op::Protect {
            repo: "project".into(),
            branch: FEATURE.as_bytes().to_vec(),
            protected: true,
        },
        Op::RequireChecks {
            repo: "project".into(),
            branch: FEATURE.as_bytes().to_vec(),
            required: None,
        },
        Op::Archive {
            repo: "project".into(),
            archived: true,
        },
    ]
}

fn squash(seeded: &Seeded) -> Op {
    Op::SquashMerge {
        repo: "project".into(),
        into: MAIN.as_bytes().to_vec(),
        from: forge::Revision::Ref(FEATURE.as_bytes().to_vec()),
        expected_into: seeded.main.to_hex(),
        expected_from: seeded.feature.to_hex(),
        message: None,
        change: Some(seeded.n),
    }
}

#[test]
fn a_new_repository_takes_writes_from_its_owner_alone() {
    let mut seeded = seeded();
    let (theirs, pack) = next(&mut seeded);
    let zero = Hash::Sha1.zero();
    let sandbox = &seeded.sandbox;
    let pushed = with_pack(sandbox, &pack, |pack| {
        let op = Op::Push {
            repo: "project".into(),
            request: push_request(&[(zero, theirs, "refs/heads/stranger")]),
            pack,
        };
        refused(sandbox, STRANGER, &op)
    });
    assert_eq!(pushed.code, code::UNAUTHORIZED, "{pushed:?}");
    let fast_forward = with_pack(sandbox, &pack, |pack| {
        let op = Op::Push {
            repo: "project".into(),
            request: push_request(&[(seeded.main, theirs, MAIN)]),
            pack,
        };
        refused(sandbox, STRANGER, &op)
    });
    assert_eq!(fast_forward.code, code::UNAUTHORIZED, "{fast_forward:?}");
    let delete = Op::DeleteBranch {
        repo: "project".into(),
        branch: FEATURE.as_bytes().to_vec(),
        expected: seeded.feature.to_hex(),
    };
    let close = Op::ChangeClose {
        repo: "project".into(),
        n: seeded.n,
    };
    let mut ops = vec![squash(&seeded), delete];
    ops.extend(settings_ops());
    for op in &ops {
        assert_eq!(
            refused(sandbox, WRITER, op).code,
            code::UNAUTHORIZED,
            "{op:?}"
        );
    }
    assert_eq!(
        refused(sandbox, WRITER, &close).code,
        code::UNAUTHORIZED,
        "another's change"
    );
    act(sandbox, STRANGER, &close).unwrap();
    for op in settings_ops().iter().take(3) {
        act(sandbox, OWNER, op).unwrap();
    }
    push(
        sandbox,
        OWNER,
        "project",
        &[(zero, theirs, "refs/heads/owners")],
        &pack,
    )
    .unwrap();
}

#[test]
fn the_owner_merges_a_change_anyone_opened() {
    let seeded = seeded();
    let sandbox = &seeded.sandbox;
    act(sandbox, OWNER, &squash(&seeded)).unwrap();
    assert_ne!(refs_of(sandbox, "project")[MAIN], seeded.main.to_hex());
}

#[test]
fn a_listed_writer_pushes_but_changes_settings_only_once_listed_there() {
    let mut seeded = seeded();
    let (theirs, pack) = next(&mut seeded);
    let zero = Hash::Sha1.zero();
    let sandbox = &seeded.sandbox;
    let writer = Principal::Account(12);
    sandbox
        .list_writers(OWNER, "project", std::slice::from_ref(&writer))
        .unwrap();
    push(
        sandbox,
        WRITER,
        "project",
        &[(zero, theirs, "refs/heads/writer")],
        &pack,
    )
    .unwrap();
    for op in &settings_ops() {
        assert_eq!(
            refused(sandbox, WRITER, op).code,
            code::UNAUTHORIZED,
            "{op:?}"
        );
    }
    sandbox
        .list_on(OWNER, &forge::settings_key("project"), &[writer])
        .unwrap();
    for op in &settings_ops() {
        act(sandbox, WRITER, op).unwrap();
    }
}

#[test]
fn a_repository_from_before_the_default_stays_open_and_its_settings_follow_its_list() {
    let mut seeded = seeded();
    predates_owner_default(&seeded.sandbox, "project");
    let (theirs, pack) = next(&mut seeded);
    let zero = Hash::Sha1.zero();
    let sandbox = &seeded.sandbox;
    push(
        sandbox,
        STRANGER,
        "project",
        &[(zero, theirs, "refs/heads/stranger")],
        &pack,
    )
    .unwrap();
    sandbox
        .list_writers(OWNER, "project", &[Principal::Account(12)])
        .unwrap();
    let [configure, ..] = settings_ops();
    act(sandbox, WRITER, &configure).unwrap();
    assert_eq!(
        refused(sandbox, STRANGER, &configure).code,
        code::UNAUTHORIZED
    );
}
