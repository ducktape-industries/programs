//! A change's Preview is the squash a merge would land, and a branch with required checks moves
//! only by a merge whose Preview the reporter marks green; a push to it is refused.
mod common;
use common::*;
use forge::{CheckState, OpReply, Required};

const MAIN: &str = "refs/heads/main";
const FEATURE: &str = "refs/heads/feature";
const GATES: &str = "gates.toml#gates";

fn branch(name: &str) -> forge::Revision {
    forge::Revision::Ref(name.as_bytes().to_vec())
}

// main and feature both moved on from root: feature changed b, main changed a.
fn diverged(store: &mut MemoryObjects) -> (Oid, Oid) {
    let root = file_commit(store, &[], 1, &[("a", b"1\n"), ("b", b"1\n")]);
    let feature = file_commit(store, &[root], 2, &[("a", b"1\n"), ("b", b"2\n")]);
    let main = file_commit(store, &[root], 3, &[("a", b"2\n"), ("b", b"1\n")]);
    (main, feature)
}

// The repository with main and feature pushed and the change WRITER opened from feature.
fn seeded(store: &MemoryObjects, main: Oid, feature: Oid) -> MemorySandbox {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let zero = Hash::Sha1.zero();
    let refs = [(zero, main, MAIN), (zero, feature, FEATURE)];
    let pack = pack_of(store, &all_ids(store));
    push(&sandbox, OWNER, "project", &refs, &pack).unwrap();
    let open = Op::ChangeOpen {
        repo: "project".into(),
        from: branch(FEATURE),
        into: MAIN.as_bytes().to_vec(),
        title: "Teach b".into(),
        body: "Why b changes.".into(),
        reviewers: Vec::new(),
    };
    act(&sandbox, WRITER, &open).unwrap();
    sandbox
}

fn preview(sandbox: &MemorySandbox) -> Oid {
    let op = Op::Preview {
        repo: "project".into(),
        n: 1,
    };
    match abi::decode(&act(sandbox, STRANGER, &op).unwrap()).unwrap() {
        OpReply::Previewed { oid, .. } => Oid::from_hex(Hash::Sha1, &oid).unwrap(),
        other => panic!("{other:?}"),
    }
}

fn status(sandbox: &MemorySandbox, reporter: &[u8], commit: Oid, state: CheckState) {
    let op = Op::Status {
        repo: "project".into(),
        commit: commit.to_hex(),
        context: GATES.into(),
        state,
        url: Some("/shared/actions/log".into()),
    };
    act(sandbox, reporter, &op).unwrap();
}

fn require(sandbox: &MemorySandbox, required: bool) {
    let required = required.then(|| Required {
        reporter: person(STRANGER),
        contexts: vec![GATES.into()],
    });
    let op = Op::RequireChecks {
        repo: "project".into(),
        branch: MAIN.as_bytes().to_vec(),
        required,
    };
    act(sandbox, OWNER, &op).unwrap();
}

fn squash(heads: (Oid, Oid), change: Option<u64>) -> Op {
    let (main, feature) = heads;
    Op::SquashMerge {
        repo: "project".into(),
        into: MAIN.as_bytes().to_vec(),
        from: branch(FEATURE),
        expected_into: main.to_hex(),
        expected_from: feature.to_hex(),
        message: None,
        change,
    }
}

// The tree of a commit forge stored.
fn tree_of(sandbox: &MemorySandbox, id: Oid) -> Oid {
    let Oid::Sha1(digest) = id else {
        panic!("a sha1 repository");
    };
    let blob = sandbox.reads(1).blob_get(BlobId::Sha1(digest)).unwrap();
    Commit::parse(&blob.body, Hash::Sha1).unwrap().tree
}

fn merged_oid(output: &[u8]) -> Oid {
    let OpReply::Merged { oid, .. } = abi::decode(output).unwrap() else {
        panic!("a squash answers Merged");
    };
    Oid::from_hex(Hash::Sha1, &oid).unwrap()
}

fn message(sandbox: &MemorySandbox, op: &Op) -> String {
    let refused = refused(sandbox, OWNER, op);
    assert_eq!(refused.code, code::WRONG_STATE, "{}", refused.message);
    refused.message
}

#[test]
fn a_preview_is_the_squash_a_merge_would_land_and_the_same_heads_answer_it_again() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let (main, feature) = diverged(&mut store);
    let sandbox = seeded(&store, main, feature);
    let previewed = preview(&sandbox);
    assert_eq!(
        refs_of(&sandbox, "project")["refs/changes/1/merge"],
        previewed.to_hex()
    );
    let asked = Query::Preview {
        repo: "project".into(),
        n: 1,
    };
    let reply: Reply = abi::decode(&ask(&sandbox, &asked).unwrap()).unwrap();
    let Reply::Preview {
        preview: Some(made),
        ..
    } = reply
    else {
        panic!("{reply:?}");
    };
    let heads = (made.into_head.as_str(), made.from_head.as_str());
    assert_eq!(heads, (main.to_hex().as_str(), feature.to_hex().as_str()));
    assert_eq!(
        preview(&sandbox),
        previewed,
        "the same heads, the same Preview"
    );
    let landed = merged_oid(&act(&sandbox, OWNER, &squash((main, feature), Some(1))).unwrap());
    assert_eq!(tree_of(&sandbox, landed), tree_of(&sandbox, previewed));
    let again = Op::Preview {
        repo: "project".into(),
        n: 1,
    };
    assert_eq!(refused(&sandbox, STRANGER, &again).code, code::WRONG_STATE);
}

#[test]
fn a_branch_with_required_checks_takes_only_a_merge_whose_preview_the_reporter_marks_passed() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let (main, feature) = diverged(&mut store);
    let sandbox = seeded(&store, main, feature);
    require(&sandbox, true);
    let heads = (main, feature);
    let unnamed = message(&sandbox, &squash(heads, None));
    assert!(
        unnamed.contains("names its change") && unnamed.contains("Break-glass"),
        "{unnamed}"
    );
    let unpreviewed = message(&sandbox, &squash(heads, Some(1)));
    assert!(unpreviewed.contains("has no Preview"), "{unpreviewed}");
    let previewed = preview(&sandbox);
    let silent = message(&sandbox, &squash(heads, Some(1)));
    assert!(
        silent.contains("no status from account 13 for gates.toml#gates"),
        "{silent}"
    );
    assert!(silent.contains("look at CI"), "{silent}");
    status(&sandbox, WRITER, previewed, CheckState::Passed);
    let other = message(&sandbox, &squash(heads, Some(1)));
    assert!(
        other.contains("no status from account 13"),
        "another reporter's: {other}"
    );
    status(&sandbox, STRANGER, previewed, CheckState::Failed);
    let red = message(&sandbox, &squash(heads, Some(1)));
    assert!(
        red.contains("gates.toml#gates failed (/shared/actions/log)"),
        "{red}"
    );
    status(&sandbox, STRANGER, previewed, CheckState::Errored);
    let broke = message(&sandbox, &squash(heads, Some(1)));
    assert!(broke.contains("gates.toml#gates errored"), "{broke}");
    status(&sandbox, STRANGER, previewed, CheckState::Passed);
    act(&sandbox, OWNER, &squash(heads, Some(1))).unwrap();
}

#[test]
fn a_push_to_a_branch_with_required_checks_is_refused_and_a_target_move_makes_the_preview_stale() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let (main, feature) = diverged(&mut store);
    let sandbox = seeded(&store, main, feature);
    require(&sandbox, true);
    let previewed = preview(&sandbox);
    status(&sandbox, STRANGER, previewed, CheckState::Passed);
    let files: &[(&str, &[u8])] = &[("a", b"3\n"), ("b", b"1\n")];
    let moved = file_commit(&mut store, &[main], 4, files);
    let pack = pack_of(&store, &all_ids(&store));
    let pushed = push(&sandbox, OWNER, "project", &[(main, moved, MAIN)], &pack).unwrap_err();
    assert!(
        pushed.message.contains("requires checks"),
        "{}",
        pushed.message
    );
    assert!(pushed.message.contains("Break-glass"), "{}", pushed.message);
    assert_eq!(refs_of(&sandbox, "project")[MAIN], main.to_hex());
    require(&sandbox, false);
    push(&sandbox, OWNER, "project", &[(main, moved, MAIN)], &pack).unwrap();
    require(&sandbox, true);
    let stale = message(&sandbox, &squash((moved, feature), Some(1)));
    assert!(stale.contains("Preview it again"), "{stale}");
    let again = preview(&sandbox);
    assert_ne!(again, previewed);
    status(&sandbox, STRANGER, again, CheckState::Passed);
    let landed = act(&sandbox, OWNER, &squash((moved, feature), Some(1))).unwrap();
    let landed = merged_oid(&landed);
    let merge = Op::Merge {
        repo: "project".into(),
        into: MAIN.as_bytes().to_vec(),
        from: branch(FEATURE),
        expected_into: landed.to_hex(),
        expected_from: feature.to_hex(),
        result: feature.to_hex(),
        change: None,
    };
    let unnamed = refused(&sandbox, OWNER, &merge);
    assert!(
        unnamed.message.contains("requires checks"),
        "{}",
        unnamed.message
    );
}
