mod common;
use common::*;

use forge::tables::{AUTHORSHIP, CUSTODY, REPOS, REVIEWS};

fn reviewed() -> (MemorySandbox, u64, Oid) {
    let mut sandbox = founded();
    sandbox
        .chain
        .seat::<ownership::Ownership>(ownership::PROGRAM);
    create(&sandbox, "project", HashKind::Sha1);
    let mut store = MemoryObjects::new(Hash::Sha1);
    let main = file_commit(&mut store, &[], 1, &[("a", b"1\n")]);
    let feature = file_commit(&mut store, &[main], 2, &[("a", b"2\n")]);
    let zero = Hash::Sha1.zero();
    let refs = [
        (zero, main, "refs/heads/main"),
        (zero, feature, "refs/heads/feature"),
    ];
    let pack = pack_of(&store, &all_ids(&store));
    push(&sandbox, OWNER, "project", &refs, &pack).unwrap();
    let open = Op::ChangeOpen {
        repo: "project".into(),
        from: forge::Revision::Ref(b"refs/heads/feature".to_vec()),
        into: b"refs/heads/main".to_vec(),
        title: "Teach a".into(),
        body: String::new(),
        reviewers: Vec::new(),
    };
    let forge::OpReply::Change { n, .. } =
        abi::decode(&act(&sandbox, WRITER, &open).unwrap()).unwrap()
    else {
        panic!("ChangeOpen answers the change");
    };
    (sandbox, n, feature)
}

fn review(n: u64, head: Oid) -> Op {
    Op::ReviewSubmit {
        repo: "project".into(),
        n,
        review: forge::ReviewDraft {
            commit_oid: head.to_hex(),
            base_oid: None,
            verdict: forge::Verdict::Comment,
            body: "Looks right.".into(),
            comments: Vec::new(),
        },
    }
}

fn record(sandbox: &MemorySandbox, n: u64, id: u64) -> Option<ownership::Record> {
    let query = ownership::Query::Record {
        program: forge::PROGRAM.into(),
        key: REVIEWS.key(&("project".to_owned(), n, id)),
    };
    let ownership::Reply::Record(record) = sandbox.chain.query(ownership::PROGRAM, &query).unwrap()
    else {
        panic!("Record answers a record");
    };
    record
}

#[test]
fn forge_claims_its_reviews_changes_and_repositories() {
    assert_eq!(REVIEWS.kind(), Some("forge.review"));
    assert_eq!(AUTHORSHIP.kind(), Some("forge.change"));
    assert_eq!(CUSTODY.kind(), Some("forge.repo"));
    assert_eq!(REPOS.kind(), None);
}

#[test]
fn a_review_is_its_authors_on_their_default_policy() {
    let (sandbox, n, head) = reviewed();
    act(&sandbox, OWNER, &review(n, head)).unwrap();
    let first = record(&sandbox, n, 1).expect("the guard recorded the review");
    assert_eq!(first.owner, ownership::Party::Account(11));
    assert_eq!(first.kind.as_deref(), Some("forge.review"));
    let strict = ownership::Op::Create {
        name: "mine".into(),
        write: ownership::Writers::Owner,
    };
    let made = sandbox
        .chain
        .submit(Origin::Signed(OWNER.to_vec()), ownership::PROGRAM, &strict)
        .unwrap();
    let policy: ownership::PolicyId = abi::decode(&made).unwrap();
    let default = ownership::Op::SetDefault {
        scope: ownership::Scope::Kind("forge.review".into()),
        policy: Some(policy),
    };
    sandbox
        .chain
        .submit(Origin::Signed(OWNER.to_vec()), ownership::PROGRAM, &default)
        .unwrap();
    act(&sandbox, OWNER, &review(n, head)).unwrap();
    assert_eq!(record(&sandbox, n, 2).unwrap().policy, policy);
    act(&sandbox, STRANGER, &review(n, head)).unwrap();
    let theirs = record(&sandbox, n, 3).unwrap();
    assert_eq!(theirs.owner, ownership::Party::Account(13));
    assert_ne!(
        theirs.policy, policy,
        "the stranger's own default, not the owner's"
    );
}

#[test]
fn a_changes_policy_decides_its_authors_side_and_others_still_review_and_close() {
    let (sandbox, n, head) = reviewed();
    let at = AUTHORSHIP.key(&("project".to_owned(), n));
    let query = ownership::Query::Record {
        program: forge::PROGRAM.into(),
        key: at.clone(),
    };
    let ownership::Reply::Record(Some(owned)) =
        sandbox.chain.query(ownership::PROGRAM, &query).unwrap()
    else {
        panic!("the guard recorded the change's authorship");
    };
    assert_eq!(owned.owner, ownership::Party::Account(12));
    assert_eq!(owned.kind.as_deref(), Some("forge.change"));
    let anyone = access::Op::Set {
        program: forge::PROGRAM.into(),
        key: forge::change_key("project", n),
        list: Some(program::Ownership::Anyone),
    };
    sandbox
        .chain
        .submit(Origin::Signed(WRITER.to_vec()), "access", &anyone)
        .unwrap();
    let retitle = |title: &str| Op::ChangeEdit {
        repo: "project".into(),
        n,
        title: Some(title.into()),
        body: None,
        reviewers: None,
    };
    act(&sandbox, STRANGER, &retitle("Their title")).unwrap();
    let strict = ownership::Op::Create {
        name: "mine".into(),
        write: ownership::Writers::Owner,
    };
    let made = sandbox
        .chain
        .submit(Origin::Signed(WRITER.to_vec()), ownership::PROGRAM, &strict)
        .unwrap();
    let policy: ownership::PolicyId = abi::decode(&made).unwrap();
    let relink = ownership::Op::Relink {
        program: forge::PROGRAM.into(),
        key: at,
        policy,
    };
    sandbox
        .chain
        .submit(Origin::Signed(WRITER.to_vec()), ownership::PROGRAM, &relink)
        .unwrap();
    let repoint = Op::ChangeRepoint {
        repo: "project".into(),
        n,
        from: forge::Revision::Oid(head.to_hex()),
    };
    for op in [retitle("Theirs again"), repoint] {
        assert_eq!(
            refused(&sandbox, STRANGER, &op).code,
            code::UNAUTHORIZED,
            "{op:?}"
        );
    }
    act(&sandbox, WRITER, &retitle("Teach a, again")).unwrap();
    act(&sandbox, STRANGER, &review(n, head)).unwrap();
    let close = Op::ChangeClose {
        repo: "project".into(),
        n,
    };
    act(&sandbox, STRANGER, &close).unwrap();
}

#[test]
fn a_repositorys_policy_decides_pushes_rules_and_closing_anothers_change() {
    let (sandbox, n, head) = reviewed();
    let query = ownership::Query::Record {
        program: forge::PROGRAM.into(),
        key: CUSTODY.key(&"project".to_owned()),
    };
    let ownership::Reply::Record(Some(owned)) =
        sandbox.chain.query(ownership::PROGRAM, &query).unwrap()
    else {
        panic!("the guard recorded the repository's custody");
    };
    assert_eq!(owned.owner, ownership::Party::Account(11));
    assert_eq!(owned.kind.as_deref(), Some("forge.repo"));
    let mut store = MemoryObjects::new(Hash::Sha1);
    let theirs = file_commit(&mut store, &[head], 3, &[("a", b"3\n")]);
    let pack = pack_of(&store, &all_ids(&store));
    let zero = Hash::Sha1.zero();
    let branch = "refs/heads/stranger";
    push(
        &sandbox,
        STRANGER,
        "project",
        &[(zero, theirs, branch)],
        &pack,
    )
    .unwrap();
    let strict = ownership::Op::Create {
        name: "mine".into(),
        write: ownership::Writers::Owner,
    };
    let made = sandbox
        .chain
        .submit(Origin::Signed(OWNER.to_vec()), ownership::PROGRAM, &strict)
        .unwrap();
    let policy: ownership::PolicyId = abi::decode(&made).unwrap();
    let relink = ownership::Op::Relink {
        program: forge::PROGRAM.into(),
        key: CUSTODY.key(&"project".to_owned()),
        policy,
    };
    sandbox
        .chain
        .submit(Origin::Signed(OWNER.to_vec()), ownership::PROGRAM, &relink)
        .unwrap();
    let delete = Op::DeleteBranch {
        repo: "project".into(),
        branch: branch.as_bytes().to_vec(),
        expected: theirs.to_hex(),
    };
    let protect = Op::Protect {
        repo: "project".into(),
        branch: b"refs/heads/main".to_vec(),
        protected: true,
    };
    let close = Op::ChangeClose {
        repo: "project".into(),
        n,
    };
    for op in [&delete, &protect, &close] {
        assert_eq!(
            refused(&sandbox, STRANGER, op).code,
            code::UNAUTHORIZED,
            "{op:?}"
        );
    }
    act(&sandbox, STRANGER, &review(n, head)).unwrap();
    act(&sandbox, OWNER, &delete).unwrap();
    act(&sandbox, OWNER, &protect).unwrap();
    act(&sandbox, WRITER, &close).unwrap();
}

#[test]
fn adopt_owners_records_forges_rows_from_before_the_guard_as_access_enforces_them_today() {
    let mut sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let mut store = MemoryObjects::new(Hash::Sha1);
    let main = file_commit(&mut store, &[], 1, &[("a", b"1\n")]);
    let feature = file_commit(&mut store, &[main], 2, &[("a", b"2\n")]);
    let zero = Hash::Sha1.zero();
    let refs = [
        (zero, main, "refs/heads/main"),
        (zero, feature, "refs/heads/feature"),
    ];
    let pack = pack_of(&store, &all_ids(&store));
    push(&sandbox, OWNER, "project", &refs, &pack).unwrap();
    let open = Op::ChangeOpen {
        repo: "project".into(),
        from: forge::Revision::Ref(b"refs/heads/feature".to_vec()),
        into: b"refs/heads/main".to_vec(),
        title: "Teach a".into(),
        body: String::new(),
        reviewers: Vec::new(),
    };
    act(&sandbox, WRITER, &open).unwrap();
    act(&sandbox, OWNER, &review(1, feature)).unwrap();
    let change = AUTHORSHIP.key(&("project".to_owned(), 1u64));
    let repo = CUSTODY.key(&"project".to_owned());
    for key in [&change, &repo] {
        sandbox.forge.borrow_mut().state.remove(key);
    }
    sandbox
        .chain
        .seat::<ownership::Ownership>(ownership::PROGRAM);
    let adopt = |kind: &str, after: Option<Vec<u8>>, limit| -> Option<Vec<u8>> {
        let op = Op::AdoptOwners {
            kind: kind.into(),
            after,
            limit,
        };
        abi::decode(&act(&sandbox, STRANGER, &op).unwrap()).unwrap()
    };
    let next = adopt("forge.repo", None, 1);
    assert_eq!(next, Some(REPOS.key(&"project".to_owned())), "a full batch");
    assert_eq!(adopt("forge.repo", next, 1), None);
    for kind in ["forge.review", "forge.change"] {
        assert_eq!(adopt(kind, None, 10), None);
    }
    let policy = |key: &[u8]| {
        let query = ownership::Query::Effective {
            program: forge::PROGRAM.into(),
            key: key.to_vec(),
        };
        let ownership::Reply::Policy(Some(policy)) =
            sandbox.chain.query(ownership::PROGRAM, &query).unwrap()
        else {
            panic!("the row is recorded");
        };
        policy
    };
    let owner = |key: &[u8]| record_at(&sandbox, key).unwrap().owner;
    assert_eq!(owner(&change), ownership::Party::Account(12));
    assert_eq!(policy(&change).name, ownership::AS_BEFORE);
    assert_eq!(owner(&repo), ownership::Party::Account(11));
    assert_eq!(policy(&repo).name, ownership::OPEN_POLICY);
    let review = REVIEWS.key(&("project".to_owned(), 1u64, 1u64));
    assert_eq!(owner(&review), ownership::Party::Account(11));
    assert_eq!(policy(&review).name, ownership::OPEN_POLICY);
    let before = [&change, &repo, &review].map(|key| record_at(&sandbox, key));
    for kind in ["forge.review", "forge.change", "forge.repo"] {
        adopt(kind, None, 10);
    }
    assert_eq!(
        [&change, &repo, &review].map(|key| record_at(&sandbox, key)),
        before
    );
}

fn record_at(sandbox: &MemorySandbox, key: &[u8]) -> Option<ownership::Record> {
    let query = ownership::Query::Record {
        program: forge::PROGRAM.into(),
        key: key.to_vec(),
    };
    let ownership::Reply::Record(record) = sandbox.chain.query(ownership::PROGRAM, &query).unwrap()
    else {
        panic!("Record answers a record");
    };
    record
}
