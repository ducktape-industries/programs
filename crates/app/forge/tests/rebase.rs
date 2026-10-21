mod common;
use common::*;

const MAIN: &str = "refs/heads/main";
const FEATURE: &str = "refs/heads/feature";
const OTHER: &str = "refs/heads/other";
const DEV: &str = "refs/heads/dev";

fn branch(name: &str) -> forge::Revision {
    forge::Revision::Ref(name.as_bytes().to_vec())
}

fn seeded(store: &MemoryObjects, branches: &[(&str, Oid)]) -> MemorySandbox {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    sandbox.open_to_anyone(OWNER, "project").unwrap();
    let zero = Hash::Sha1.zero();
    let refs: Vec<(Oid, Oid, &str)> = branches
        .iter()
        .map(|(name, at)| (zero, *at, *name))
        .collect();
    let pack = pack_of(store, &all_ids(store));
    push(&sandbox, OWNER, "project", &refs, &pack).unwrap();
    sandbox
}

fn diverged(store: &mut MemoryObjects) -> (Oid, Oid) {
    let root = file_commit(store, &[], 1, &[("a", b"1\n"), ("b", b"1\n")]);
    let first = file_commit(store, &[root], 2, &[("a", b"1\n"), ("b", b"2\n")]);
    let files: &[(&str, &[u8])] = &[("a", b"1\n"), ("b", b"2\n"), ("c", b"x\n")];
    let feature = file_commit(store, &[first], 3, files);
    let main = file_commit(store, &[root], 4, &[("a", b"2\n"), ("b", b"1\n")]);
    (main, feature)
}

fn opened(sandbox: &MemorySandbox, from: &str) -> u64 {
    let op = Op::ChangeOpen {
        repo: "project".into(),
        from: branch(from),
        into: MAIN.as_bytes().to_vec(),
        title: "Teach b".into(),
        body: "Why b changes.".into(),
        reviewers: Vec::new(),
    };
    match abi::decode(&act(sandbox, WRITER, &op).unwrap()).unwrap() {
        forge::OpReply::Change { n, .. } => n,
        other => panic!("{other:?}"),
    }
}

fn rebase(n: u64, onto: &str, head: Oid) -> Op {
    Op::Rebase {
        repo: "project".into(),
        n,
        onto: branch(onto),
        expected_head: head.to_hex(),
    }
}

fn repoint(n: u64, from: &str) -> Op {
    Op::ChangeRepoint {
        repo: "project".into(),
        n,
        from: branch(from),
    }
}

fn delete(name: &str, at: Oid) -> Op {
    Op::DeleteBranch {
        repo: "project".into(),
        branch: name.as_bytes().to_vec(),
        expected: at.to_hex(),
    }
}

fn protect(name: &str, protected: bool) -> Op {
    Op::Protect {
        repo: "project".into(),
        branch: name.as_bytes().to_vec(),
        protected,
    }
}

fn rebased(sandbox: &MemorySandbox, output: &[u8]) -> (Oid, Commit) {
    let forge::OpReply::Rebased { oid, .. } = abi::decode(output).unwrap() else {
        panic!("a rebase answers Rebased");
    };
    let id = Oid::from_hex(Hash::Sha1, &oid).unwrap();
    let Oid::Sha1(digest) = id else {
        panic!("a sha1 repository");
    };
    let blob = sandbox.reads(1).blob_get(BlobId::Sha1(digest)).unwrap();
    (id, Commit::parse(&blob.body, Hash::Sha1).unwrap())
}

fn tree_with(store: &mut MemoryObjects, files: &[(&str, &[u8])]) -> Oid {
    let entries: Vec<(&str, Oid)> = files
        .iter()
        .map(|(name, content)| (*name, blob(store, content)))
        .collect();
    tree(store, &entries)
}

fn change_of(sandbox: &MemorySandbox, n: u64) -> (forge::Change, Option<String>) {
    let query = Query::Change {
        repo: "project".into(),
        n,
        page: PageRequest::first(8),
    };
    let reply: Reply = abi::decode(&ask(sandbox, &query).unwrap()).unwrap();
    let Reply::Change {
        change,
        source_head,
        ..
    } = reply
    else {
        panic!("{reply:?}");
    };
    (change, source_head)
}

fn protected_of(sandbox: &MemorySandbox) -> Vec<Vec<u8>> {
    let query = Query::Protected {
        repo: "project".into(),
        page: PageRequest::first(8),
    };
    let reply: Reply = abi::decode(&ask(sandbox, &query).unwrap()).unwrap();
    let Reply::Protected { page, .. } = reply else {
        panic!("{reply:?}");
    };
    page.items
}

fn landed() -> (MemorySandbox, MemoryObjects, u64, Oid, Oid, Commit) {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let (main, feature) = diverged(&mut store);
    let sandbox = seeded(&store, &[(MAIN, main), (FEATURE, feature)]);
    let n = opened(&sandbox, FEATURE);
    let review = Op::ReviewSubmit {
        repo: "project".into(),
        n,
        review: forge::ReviewDraft {
            commit_oid: feature.to_hex(),
            base_oid: None,
            verdict: forge::Verdict::Comment,
            body: "Looks right.".into(),
            comments: Vec::new(),
        },
    };
    act(&sandbox, OWNER, &review).unwrap();
    let output = act(&sandbox, WRITER, &rebase(n, MAIN, feature)).unwrap();
    let (id, commit) = rebased(&sandbox, &output);
    (sandbox, store, n, main, id, commit)
}

#[test]
fn a_rebase_moves_the_change_onto_its_target_and_keeps_its_number() {
    let (sandbox, mut store, n, main, id, commit) = landed();
    let refs = refs_of(&sandbox, "project");
    assert_eq!(refs[FEATURE], id.to_hex());
    assert_eq!(refs[MAIN], main.to_hex());
    assert_eq!(commit.parents, [main]);
    let files: &[(&str, &[u8])] = &[("a", b"2\n"), ("b", b"2\n"), ("c", b"x\n")];
    assert_eq!(commit.tree, tree_with(&mut store, files));
    assert_eq!(commit.message, b"Teach b\n\nWhy b changes.\n");
    assert!(
        commit.author.email.starts_with(b"12@"),
        "{:?}",
        commit.author
    );
    assert_eq!(commit.committer.time, (TIME / 1000) as i64);
    let (change, source_head) = change_of(&sandbox, n);
    assert_eq!(change.n, n);
    assert_eq!(change.state, forge::ChangeState::Open);
    assert_eq!(change.from, branch(FEATURE));
    assert_eq!(change.review_count, 1);
    assert_eq!(source_head, Some(id.to_hex()));
}

#[test]
fn the_same_rebase_makes_the_same_commit_id() {
    let (_, _, _, _, first, _) = landed();
    let (_, _, _, _, second, _) = landed();
    assert_eq!(first, second);
}

#[test]
fn a_rebased_change_still_lands() {
    let (sandbox, mut store, n, main, id, _) = landed();
    let squash = Op::SquashMerge {
        repo: "project".into(),
        into: MAIN.as_bytes().to_vec(),
        from: branch(FEATURE),
        expected_into: main.to_hex(),
        expected_from: id.to_hex(),
        message: None,
        change: Some(n),
    };
    act(&sandbox, OWNER, &squash).unwrap();
    let (change, _) = change_of(&sandbox, n);
    assert_eq!(change.state, forge::ChangeState::Merged);
    let landed = refs_of(&sandbox, "project")[MAIN].clone();
    let landed = Oid::from_hex(Hash::Sha1, &landed).unwrap();
    let Oid::Sha1(digest) = landed else {
        panic!("a sha1 repository");
    };
    let blob = sandbox.reads(1).blob_get(BlobId::Sha1(digest)).unwrap();
    let commit = Commit::parse(&blob.body, Hash::Sha1).unwrap();
    let files: &[(&str, &[u8])] = &[("a", b"2\n"), ("b", b"2\n"), ("c", b"x\n")];
    assert_eq!(commit.tree, tree_with(&mut store, files));
}

#[test]
fn a_conflicting_rebase_is_refused_and_writes_nothing() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut store, &[], 1, &[("a", b"1\n"), ("b", b"1\n")]);
    let feature = file_commit(&mut store, &[root], 2, &[("a", b"3\n"), ("b", b"2\n")]);
    let main = file_commit(&mut store, &[root], 3, &[("a", b"2\n"), ("b", b"1\n")]);
    let sandbox = seeded(&store, &[(MAIN, main), (FEATURE, feature)]);
    let n = opened(&sandbox, FEATURE);
    let blobs = sandbox.blob_count();
    let error = refused(&sandbox, WRITER, &rebase(n, MAIN, feature));
    assert_eq!(error.code, code::WRONG_STATE);
    assert!(error.message.contains("a (both sides"), "{}", error.message);
    assert_eq!(sandbox.blob_count(), blobs);
    assert_eq!(refs_of(&sandbox, "project")[FEATURE], feature.to_hex());
}

#[test]
fn a_moved_head_is_stale() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let (main, feature) = diverged(&mut store);
    let sandbox = seeded(&store, &[(MAIN, main), (FEATURE, feature)]);
    let n = opened(&sandbox, FEATURE);
    let error = refused(&sandbox, WRITER, &rebase(n, MAIN, main));
    assert_eq!(error.code, code::STALE);
}

#[test]
fn a_change_already_on_its_target_has_nothing_to_rebase() {
    let (sandbox, _, n, _, id, _) = landed();
    let error = refused(&sandbox, WRITER, &rebase(n, MAIN, id));
    assert_eq!(error.code, code::WRONG_STATE);
}

#[test]
fn a_stranger_may_not_rebase_a_change() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let (main, feature) = diverged(&mut store);
    let sandbox = seeded(&store, &[(MAIN, main), (FEATURE, feature)]);
    let n = opened(&sandbox, FEATURE);
    let error = refused(&sandbox, STRANGER, &rebase(n, MAIN, feature));
    assert_eq!(error.code, code::UNAUTHORIZED);
}

#[test]
fn a_protected_source_is_not_rebased() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let (main, feature) = diverged(&mut store);
    let sandbox = seeded(&store, &[(MAIN, main), (DEV, feature)]);
    act(&sandbox, OWNER, &protect(DEV, true)).unwrap();
    let n = opened(&sandbox, DEV);
    let error = refused(&sandbox, WRITER, &rebase(n, MAIN, feature));
    assert_eq!(error.code, code::WRONG_STATE);
    assert!(error.message.contains("protected"), "{}", error.message);
}

#[test]
fn a_change_points_at_another_branch_and_keeps_its_number() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let (main, feature) = diverged(&mut store);
    let other = file_commit(&mut store, &[feature], 5, &[("a", b"1\n"), ("b", b"3\n")]);
    let branches = [(MAIN, main), (FEATURE, feature), (OTHER, other)];
    let sandbox = seeded(&store, &branches);
    let n = opened(&sandbox, FEATURE);
    let output = act(&sandbox, WRITER, &repoint(n, OTHER)).unwrap();
    let forge::OpReply::Change { n: answered, .. } = abi::decode(&output).unwrap() else {
        panic!("a repoint answers Change");
    };
    assert_eq!(answered, n);
    let (change, source_head) = change_of(&sandbox, n);
    assert_eq!(change.from, branch(OTHER));
    assert_eq!(source_head, Some(other.to_hex()));
    let stranger = refused(&sandbox, STRANGER, &repoint(n, FEATURE));
    assert_eq!(stranger.code, code::UNAUTHORIZED);
    let onto_target = refused(&sandbox, WRITER, &repoint(n, MAIN));
    assert_eq!(onto_target.code, code::WRONG_STATE);
}

#[test]
fn a_branch_no_open_change_names_is_deleted() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let (main, feature) = diverged(&mut store);
    let sandbox = seeded(&store, &[(MAIN, main), (FEATURE, feature)]);
    act(&sandbox, STRANGER, &delete(FEATURE, feature)).unwrap();
    assert!(!refs_of(&sandbox, "project").contains_key(FEATURE));
}

#[test]
fn a_moved_branch_is_not_deleted() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let (main, feature) = diverged(&mut store);
    let sandbox = seeded(&store, &[(MAIN, main), (FEATURE, feature)]);
    let error = refused(&sandbox, WRITER, &delete(FEATURE, main));
    assert_eq!(error.code, code::STALE);
}

#[test]
fn a_branch_an_open_change_names_stays_until_the_change_ends() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let (main, feature) = diverged(&mut store);
    let sandbox = seeded(&store, &[(MAIN, main), (FEATURE, feature)]);
    let n = opened(&sandbox, FEATURE);
    let error = refused(&sandbox, WRITER, &delete(FEATURE, feature));
    assert_eq!(error.code, code::WRONG_STATE);
    assert!(
        error.message.contains(&format!("#{n}")),
        "{}",
        error.message
    );
    let close = Op::ChangeClose {
        repo: "project".into(),
        n,
    };
    act(&sandbox, WRITER, &close).unwrap();
    act(&sandbox, WRITER, &delete(FEATURE, feature)).unwrap();
    assert!(!refs_of(&sandbox, "project").contains_key(FEATURE));
}

#[test]
fn the_head_and_protected_branches_are_not_deleted() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let (main, feature) = diverged(&mut store);
    let sandbox = seeded(&store, &[(MAIN, main), (DEV, feature)]);
    let head = refused(&sandbox, WRITER, &delete(MAIN, main));
    assert_eq!(head.code, code::WRONG_STATE);
    act(&sandbox, OWNER, &protect(DEV, true)).unwrap();
    assert_eq!(protected_of(&sandbox), [DEV.as_bytes().to_vec()]);
    let dev = refused(&sandbox, WRITER, &delete(DEV, feature));
    assert_eq!(dev.code, code::WRONG_STATE);
    let unprotect_head = refused(&sandbox, OWNER, &protect(MAIN, false));
    assert_eq!(unprotect_head.code, code::WRONG_STATE);
    act(&sandbox, OWNER, &protect(DEV, false)).unwrap();
    assert!(protected_of(&sandbox).is_empty());
    act(&sandbox, WRITER, &delete(DEV, feature)).unwrap();
    let refs = refs_of(&sandbox, "project");
    assert!(!refs.contains_key(DEV));
    assert_eq!(refs[MAIN], main.to_hex());
}
