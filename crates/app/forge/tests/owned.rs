mod common;
use common::*;

use forge::tables::REVIEWS;

// A change WRITER opened on a pushed repository, with the ownership program seated at its role.
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
fn forge_declares_its_reviews_owned_and_every_other_table_shared() {
    let owned: Vec<_> = Forge::TABLES
        .iter()
        .filter(|table| table.held != program::Held::Shared)
        .collect();
    assert_eq!(owned.len(), 1);
    assert_eq!(owned[0].prefix, REVIEWS.table().prefix);
    assert_eq!(owned[0].held, program::Held::Owned(Some("forge.review")));
}

#[test]
fn a_review_is_its_authors_on_their_default_policy() {
    let (sandbox, n, head) = reviewed();
    act(&sandbox, OWNER, &review(n, head)).unwrap();
    let first = record(&sandbox, n, 1).expect("the guard recorded the review");
    assert_eq!(first.owner, ownership::Party::Account(11));
    assert_eq!(first.kind.as_deref(), Some("forge.review"));
    // a stranger's own default narrowed: their review is still theirs to write, and the
    // change, repository and indexes others' reviews touch stay shared
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
