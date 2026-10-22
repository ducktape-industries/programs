mod common;
use common::*;

const MAIN: &str = "refs/heads/main";
const FEATURE: &str = "refs/heads/feature";

fn seeded(store: &MemoryObjects, main: Oid, feature: Oid) -> MemorySandbox {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let zero = Hash::Sha1.zero();
    let refs = [(zero, main, MAIN), (zero, feature, FEATURE)];
    let pack = pack_of(store, &all_ids(store));
    push(&sandbox, OWNER, "project", &refs, &pack).unwrap();
    sandbox
}

fn squash(heads: (Oid, Oid), message: Option<&str>, change: Option<u64>) -> Op {
    let (main, feature) = heads;
    Op::SquashMerge {
        repo: "project".into(),
        into: MAIN.as_bytes().to_vec(),
        from: forge::Revision::Ref(FEATURE.as_bytes().to_vec()),
        expected_into: main.to_hex(),
        expected_from: feature.to_hex(),
        message: message.map(Into::into),
        change,
    }
}

fn opened(sandbox: &MemorySandbox) -> u64 {
    let op = Op::ChangeOpen {
        repo: "project".into(),
        from: forge::Revision::Ref(FEATURE.as_bytes().to_vec()),
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

fn squashed(sandbox: &MemorySandbox, output: &[u8]) -> (Oid, Commit) {
    let forge::OpReply::Merged { oid, .. } = abi::decode(output).unwrap() else {
        panic!("a squash answers Merged");
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

fn change_of(sandbox: &MemorySandbox, n: u64) -> forge::Change {
    let query = Query::Change {
        repo: "project".into(),
        n,
        page: PageRequest::first(8),
    };
    let reply: Reply = abi::decode(&ask(sandbox, &query).unwrap()).unwrap();
    let Reply::Change { change, .. } = reply else {
        panic!("{reply:?}");
    };
    change
}

fn diverged(store: &mut MemoryObjects) -> (Oid, Oid) {
    let root = file_commit(store, &[], 1, &[("a", b"1\n"), ("b", b"1\n")]);
    let first = file_commit(store, &[root], 2, &[("a", b"1\n"), ("b", b"2\n")]);
    let files: &[(&str, &[u8])] = &[("a", b"1\n"), ("b", b"2\n"), ("c", b"x\n")];
    let feature = file_commit(store, &[first], 3, files);
    let main = file_commit(store, &[root], 4, &[("a", b"2\n"), ("b", b"1\n")]);
    (main, feature)
}

fn landed() -> (MemorySandbox, MemoryObjects, u64, Oid, Commit) {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let (main, feature) = diverged(&mut store);
    let sandbox = seeded(&store, main, feature);
    let n = opened(&sandbox);
    let output = act(&sandbox, OWNER, &squash((main, feature), None, Some(n))).unwrap();
    let (id, commit) = squashed(&sandbox, &output);
    assert_eq!(commit.parents, [main]);
    (sandbox, store, n, id, commit)
}

#[test]
fn a_change_lands_as_one_squash_commit_on_the_target() {
    let (sandbox, mut store, n, id, commit) = landed();
    assert_eq!(refs_of(&sandbox, "project")[MAIN], id.to_hex());
    let files: &[(&str, &[u8])] = &[("a", b"2\n"), ("b", b"2\n"), ("c", b"x\n")];
    assert_eq!(commit.tree, tree_with(&mut store, files));
    let message = format!("Teach b (change {n})\n\nWhy b changes.\n");
    assert_eq!(commit.message, message.into_bytes());
    assert!(
        commit.author.email.starts_with(b"12@"),
        "{:?}",
        commit.author
    );
    assert!(
        commit.committer.email.starts_with(b"11@"),
        "{:?}",
        commit.committer
    );
    assert_eq!(commit.committer.time, (TIME / 1000) as i64);
    let change = change_of(&sandbox, n);
    assert_eq!(change.state, forge::ChangeState::Merged);
    assert_eq!(change.merge_oid, Some(id.to_hex()));
    assert_eq!(change.merged_by, Some(person(OWNER)));
}

#[test]
fn the_same_squash_makes_the_same_commit_id() {
    let (_, _, _, first, _) = landed();
    let (_, _, _, second, _) = landed();
    assert_eq!(first, second);
}

#[test]
fn a_branch_ahead_of_its_target_still_lands_as_one_commit() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut store, &[], 1, &[("a", b"1\n")]);
    let first = file_commit(&mut store, &[root], 2, &[("a", b"1\n"), ("b", b"1\n")]);
    let feature = file_commit(&mut store, &[first], 3, &[("a", b"2\n"), ("b", b"1\n")]);
    let sandbox = seeded(&store, root, feature);
    let op = squash((root, feature), Some("Land it"), None);
    let (id, commit) = squashed(&sandbox, &act(&sandbox, OWNER, &op).unwrap());
    assert_eq!(refs_of(&sandbox, "project")[MAIN], id.to_hex());
    assert_eq!(commit.parents, [root]);
    let files: &[(&str, &[u8])] = &[("a", b"2\n"), ("b", b"1\n")];
    assert_eq!(commit.tree, tree_with(&mut store, files));
    assert_eq!(commit.message, b"Land it\n");
    assert_eq!(commit.author, commit.committer);
}

#[test]
fn a_file_both_sides_changed_merges_line_by_line() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut store, &[], 1, &[("a", b"1\n2\n3\n4\n5\n")]);
    let feature = file_commit(&mut store, &[root], 2, &[("a", b"1\n2\n3\n4\nfive\n")]);
    let main = file_commit(&mut store, &[root], 3, &[("a", b"one\n2\n3\n4\n5\n")]);
    let sandbox = seeded(&store, main, feature);
    let op = squash((main, feature), Some("Five"), None);
    let (_, commit) = squashed(&sandbox, &act(&sandbox, OWNER, &op).unwrap());
    let files: &[(&str, &[u8])] = &[("a", b"one\n2\n3\n4\nfive\n")];
    assert_eq!(commit.tree, tree_with(&mut store, files));
}

#[test]
fn a_conflict_is_refused_and_writes_nothing() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut store, &[], 1, &[("a", b"1\n"), ("b", b"1\n")]);
    let feature = file_commit(&mut store, &[root], 2, &[("a", b"3\n"), ("b", b"2\n")]);
    let main = file_commit(&mut store, &[root], 3, &[("a", b"2\n"), ("b", b"1\n")]);
    let sandbox = seeded(&store, main, feature);
    let n = opened(&sandbox);
    let blobs = sandbox.blob_count();
    let error = refused(&sandbox, OWNER, &squash((main, feature), None, Some(n)));
    assert_eq!(error.code, code::WRONG_STATE);
    assert!(error.message.contains("a (both sides"), "{}", error.message);
    assert_eq!(sandbox.blob_count(), blobs);
    assert_eq!(refs_of(&sandbox, "project")[MAIN], main.to_hex());
    assert_eq!(change_of(&sandbox, n).state, forge::ChangeState::Open);
}

#[test]
fn a_moved_head_is_stale() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let (main, feature) = diverged(&mut store);
    let sandbox = seeded(&store, main, feature);
    let error = refused(
        &sandbox,
        OWNER,
        &squash((feature, feature), Some("x"), None),
    );
    assert_eq!(error.code, code::STALE);
}

#[test]
fn a_squash_without_a_change_needs_a_message() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let (main, feature) = diverged(&mut store);
    let sandbox = seeded(&store, main, feature);
    let error = refused(&sandbox, OWNER, &squash((main, feature), None, None));
    assert_eq!(error.code, code::INVALID_INPUT);
}

#[test]
fn unrelated_histories_are_refused() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let main = file_commit(&mut store, &[], 1, &[("a", b"1\n")]);
    let feature = file_commit(&mut store, &[], 2, &[("b", b"1\n")]);
    let sandbox = seeded(&store, main, feature);
    let error = refused(&sandbox, OWNER, &squash((main, feature), Some("x"), None));
    assert_eq!(error.code, code::WRONG_STATE);
}

fn stacked(store: &mut MemoryObjects, main_a: &[u8]) -> (Oid, Oid) {
    let root = file_commit(store, &[], 1, &[("a", b"1\n2\n3\n")]);
    let lower = file_commit(store, &[root], 2, &[("a", b"1\nlow\n3\n")]);
    let upper = file_commit(store, &[lower], 3, &[("a", b"1\nlow up\n3\n")]);
    let squashed = file_commit(store, &[root], 4, &[("a", b"1\nlow\n3\n")]);
    let main = file_commit(store, &[squashed], 5, &[("a", main_a), ("b", b"1\n")]);
    (main, upper)
}

#[test]
fn a_change_stacked_on_a_squashed_change_lands_with_only_its_own_edits() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let (main, upper) = stacked(&mut store, b"1\nlow\n3\n");
    let sandbox = seeded(&store, main, upper);
    let n = opened(&sandbox);
    let preview = Op::Preview {
        repo: "project".into(),
        n,
    };
    act(&sandbox, OWNER, &preview).unwrap();
    let output = act(&sandbox, OWNER, &squash((main, upper), None, Some(n))).unwrap();
    let (_, commit) = squashed(&sandbox, &output);
    assert_eq!(commit.parents, [main]);
    let files: &[(&str, &[u8])] = &[("a", b"1\nlow up\n3\n"), ("b", b"1\n")];
    assert_eq!(commit.tree, tree_with(&mut store, files));
}

#[test]
fn a_stacked_change_still_conflicts_with_a_divergent_edit_on_the_target() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let (main, upper) = stacked(&mut store, b"1\nlow main\n3\n");
    let sandbox = seeded(&store, main, upper);
    let error = refused(&sandbox, OWNER, &squash((main, upper), Some("x"), None));
    assert_eq!(error.code, code::WRONG_STATE);
    assert!(error.message.contains("a (both sides"), "{}", error.message);
}
