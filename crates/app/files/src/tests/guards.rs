//! Guarded paths on a `MockChain` beside access and ownership: who guards a path, the nearest
//! guard's policy deciding each write beneath it, a guarded folder moving and going with its
//! records, and the one-time adoption of today's lists and chat attachments.
use ::program::{MockChain, Origin, Ownership, code};

use super::*;
use crate::state::{GUARDED, PINS};

const ADA: &[u8] = b"ada";
const BO: &[u8] = b"bo";

fn chain() -> MockChain {
    let mut chain = bare();
    chain.seat::<ownership::Ownership>(ownership::PROGRAM);
    chain
}

fn bare() -> MockChain {
    let mut chain = MockChain::default();
    chain.seat::<Files>(PROGRAM);
    chain.seat::<access::Access>("access");
    for (program, number) in [(PROGRAM, 900), ("access", 901)] {
        chain.register(program, number);
    }
    for (key, number, name) in [(ADA, 1, "ada"), (BO, 2, "bo")] {
        chain.hold(key, number);
        chain.profile(profile(number, name));
    }
    chain
}

fn signed(key: &[u8]) -> Origin {
    Origin::Signed(key.to_vec())
}

fn commit(chain: &MockChain, key: &[u8], changes: Vec<Change>) -> Result<(), String> {
    let Reply::Head(head) = chain.query(PROGRAM, &Query::Head).unwrap() else {
        panic!("a head");
    };
    let op = Op::Commit {
        base: head.map(|row| row.id),
        message: "work".into(),
        changes,
    };
    run(chain, key, &op)
}

fn run(chain: &MockChain, key: &[u8], op: &Op) -> Result<(), String> {
    chain
        .submit(signed(key), PROGRAM, op)
        .map(drop)
        .map_err(|refusal| refusal.code)
}

fn write(chain: &MockChain, key: &[u8], path: &str) -> Result<(), String> {
    let chunk = chain.put_blob(CHUNK, key.to_vec()).unwrap();
    let put = Change::Put {
        path: path.into(),
        exec: false,
        meta: BTreeMap::new(),
        content: Content::Chunks {
            size: key.len() as u64,
            chunks: vec![chunk],
        },
    };
    commit(chain, key, vec![put])
}

fn guard(chain: &MockChain, key: &[u8], path: &str) -> Result<(), String> {
    run(chain, key, &Op::Guard { path: path.into() })
}

fn guarded(chain: &MockChain, path: &str) -> Option<String> {
    let asked = Query::Guarded { path: path.into() };
    let Reply::Guarded(nearest) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!("Guarded answers a path");
    };
    nearest
}

fn record(chain: &MockChain, path: &str) -> Option<ownership::Record> {
    let query = ownership::Query::Record {
        program: PROGRAM.into(),
        key: guard_key(path),
    };
    let ownership::Reply::Record(record) = chain.query(ownership::PROGRAM, &query).unwrap() else {
        panic!("Record answers a record");
    };
    record
}

fn policy_name(chain: &MockChain, path: &str) -> String {
    let query = ownership::Query::Effective {
        program: PROGRAM.into(),
        key: guard_key(path),
    };
    let ownership::Reply::Policy(Some(policy)) = chain.query(ownership::PROGRAM, &query).unwrap()
    else {
        panic!("{path} keeps a policy");
    };
    policy.name
}

fn only_me(chain: &MockChain, key: &[u8], path: &str) -> ownership::PolicyId {
    let strict = ownership::Op::Create {
        name: "mine".into(),
        write: ownership::Writers::Owner,
    };
    let made = chain
        .submit(signed(key), ownership::PROGRAM, &strict)
        .unwrap();
    let policy: ownership::PolicyId = abi::decode(&made).unwrap();
    let relink = ownership::Op::Relink {
        program: PROGRAM.into(),
        key: guard_key(path),
        policy,
    };
    chain
        .submit(signed(key), ownership::PROGRAM, &relink)
        .unwrap();
    policy
}

#[test]
fn files_claims_its_guards_and_pins() {
    assert_eq!(GUARDED.kind(), Some("files.path"));
    assert_eq!(PINS.kind(), Some("files.pin"));
}

#[test]
fn a_path_is_guarded_by_its_owner_alone_and_once() {
    let chain = chain();
    write(&chain, ADA, "/docs/a").unwrap();
    assert_eq!(
        guard(&chain, BO, "/docs"),
        Err(code::UNAUTHORIZED.to_string())
    );
    assert_eq!(guard(&chain, ADA, "/docs"), Ok(()));
    let owned = record(&chain, "/docs").expect("the guard recorded the path");
    assert_eq!(owned.owner, ownership::Party::Account(1));
    assert_eq!(owned.kind.as_deref(), Some("files.path"));
    assert_eq!(
        guard(&chain, ADA, "/docs"),
        Err(code::ALREADY_EXISTS.to_string())
    );
    assert_eq!(
        guard(&chain, ADA, "/nowhere"),
        Err(code::NOT_FOUND.to_string())
    );
}

#[test]
fn the_nearest_guards_policy_decides_each_write_beneath_it() {
    let chain = chain();
    write(&chain, ADA, "/docs/a").unwrap();
    guard(&chain, ADA, "/docs").unwrap();
    assert_eq!(
        write(&chain, BO, "/docs/b"),
        Ok(()),
        "an open guard admits anyone"
    );
    only_me(&chain, ADA, "/docs");
    assert_eq!(guarded(&chain, "/docs/c/d"), Some("/docs".into()));
    assert_eq!(
        write(&chain, BO, "/docs/c/d"),
        Err(code::UNAUTHORIZED.to_string())
    );
    let removed = Change::Remove {
        path: "/docs/a".into(),
    };
    assert_eq!(
        commit(&chain, BO, vec![removed]),
        Err(code::UNAUTHORIZED.to_string())
    );
    assert_eq!(write(&chain, ADA, "/docs/c/d"), Ok(()));
    assert_eq!(write(&chain, BO, "/elsewhere"), Ok(()));
    let unguard = Op::Unguard {
        path: "/docs".into(),
    };
    assert_eq!(
        run(&chain, BO, &unguard),
        Err(code::UNAUTHORIZED.to_string())
    );
    assert_eq!(run(&chain, ADA, &unguard), Ok(()));
    assert_eq!(guarded(&chain, "/docs/c/d"), None);
    assert_eq!(write(&chain, BO, "/docs/c/d"), Ok(()));
}

#[test]
fn a_commit_that_touches_nothing_guarded_keeps_no_guard() {
    let chain = chain();
    write(&chain, BO, "/notes/today").unwrap();
    assert_eq!(guarded(&chain, "/notes/today"), None);
    assert_eq!(record(&chain, "/notes/today"), None);
    assert_eq!(record(&chain, "/notes"), None);
}

#[test]
fn a_moved_folder_keeps_the_owner_and_policy_of_every_guard_beneath_it() {
    let chain = chain();
    write(&chain, ADA, "/docs/sub/a").unwrap();
    write(&chain, ADA, "/other/z").unwrap();
    guard(&chain, ADA, "/docs/sub").unwrap();
    guard(&chain, ADA, "/other").unwrap();
    let strict = only_me(&chain, ADA, "/docs/sub");
    let other = record(&chain, "/other");
    let moved = Change::Move {
        from: "/docs".into(),
        to: "/moved".into(),
    };
    commit(&chain, ADA, vec![moved]).unwrap();
    let landed = record(&chain, "/moved/sub").expect("the record moved with its path");
    assert_eq!(landed.owner, ownership::Party::Account(1));
    assert_eq!(landed.policy, strict);
    assert_eq!(record(&chain, "/docs/sub"), None);
    assert_eq!(guarded(&chain, "/docs/sub/a"), None);
    assert_eq!(guarded(&chain, "/moved/sub/a"), Some("/moved/sub".into()));
    assert_eq!(
        write(&chain, BO, "/moved/sub/b"),
        Err(code::UNAUTHORIZED.to_string())
    );
    assert_eq!(
        record(&chain, "/other"),
        other,
        "nothing outside it changed"
    );
    assert_eq!(guarded(&chain, "/other/z"), Some("/other".into()));
}

#[test]
fn only_files_vacates_a_moved_guard() {
    let chain = chain();
    write(&chain, ADA, "/docs/a").unwrap();
    guard(&chain, ADA, "/docs").unwrap();
    let vacate = Op::Vacate {
        paths: vec!["/docs".into()],
    };
    assert_eq!(
        run(&chain, ADA, &vacate),
        Err(code::UNAUTHORIZED.to_string())
    );
    assert_eq!(guarded(&chain, "/docs/a"), Some("/docs".into()));
}

#[test]
fn a_removed_folder_takes_every_guard_beneath_it() {
    let chain = chain();
    write(&chain, ADA, "/docs/sub/a").unwrap();
    guard(&chain, ADA, "/docs").unwrap();
    guard(&chain, ADA, "/docs/sub").unwrap();
    let removed = Change::Remove {
        path: "/docs".into(),
    };
    commit(&chain, ADA, vec![removed]).unwrap();
    assert_eq!(guarded(&chain, "/docs/sub/a"), None);
    assert_eq!(record(&chain, "/docs"), None);
    assert_eq!(record(&chain, "/docs/sub"), None);
}

#[test]
fn a_file_beneath_a_chat_message_is_its_uploaders() {
    let chain = chain();
    write(&chain, ADA, "/chat/general/m1/pic.png").unwrap();
    let owned = record(&chain, "/chat/general/m1/pic.png").expect("the upload claimed its guard");
    assert_eq!(owned.owner, ownership::Party::Account(1));
    assert_eq!(owned.kind.as_deref(), Some("files.path"));
}

#[test]
fn adoption_guards_each_listed_path_and_chat_attachment_as_before() {
    let mut chain = bare();
    write(&chain, ADA, "/docs/a").unwrap();
    write(&chain, ADA, "/chat/general/m1/pic.png").unwrap();
    let set = access::Op::Set {
        program: PROGRAM.into(),
        key: "/docs/a".into(),
        list: Some(Ownership::Owner),
    };
    chain.submit(signed(ADA), "access", &set).unwrap();
    chain.seat::<ownership::Ownership>(ownership::PROGRAM);
    let adopt = Op::AdoptOwners {
        kind: "files.path".into(),
        after: None,
        limit: 10,
    };
    let next = chain.submit(signed(BO), PROGRAM, &adopt).unwrap();
    assert_eq!(abi::decode::<Option<Vec<u8>>>(&next).unwrap(), None);
    for path in ["/docs/a", "/chat/general/m1/pic.png"] {
        let owned = record(&chain, path).expect("adopted");
        assert_eq!(owned.owner, ownership::Party::Account(1), "{path}");
        assert_eq!(policy_name(&chain, path), ownership::AS_BEFORE, "{path}");
    }
    assert_eq!(guarded(&chain, "/docs/a"), Some("/docs/a".into()));
    let again = chain.submit(signed(BO), PROGRAM, &adopt).unwrap();
    assert_eq!(abi::decode::<Option<Vec<u8>>>(&again).unwrap(), None);
    assert_eq!(policy_name(&chain, "/docs/a"), ownership::AS_BEFORE);
}

#[test]
fn a_file_or_link_put_over_a_guarded_folder_answers_to_every_guard_beneath_it() {
    let chain = chain();
    write(&chain, ADA, "/docs/a").unwrap();
    guard(&chain, ADA, "/docs/a").unwrap();
    only_me(&chain, ADA, "/docs/a");
    assert_eq!(
        write(&chain, BO, "/docs"),
        Err(code::UNAUTHORIZED.to_string()),
        "a file over /docs would drop ada's guarded /docs/a"
    );
    let linked = Change::Symlink {
        path: "/docs".into(),
        target: "/elsewhere".into(),
    };
    assert_eq!(
        commit(&chain, BO, vec![linked]),
        Err(code::UNAUTHORIZED.to_string())
    );
    assert_eq!(guarded(&chain, "/docs/a"), Some("/docs/a".into()));
    assert_eq!(
        write(&chain, ADA, "/docs"),
        Ok(()),
        "its owner may replace it"
    );
    assert_eq!(
        guarded(&chain, "/docs/a"),
        None,
        "the guard went with what it guarded"
    );
}

#[test]
fn a_move_that_carries_guards_goes_in_a_commit_of_its_own() {
    let chain = chain();
    write(&chain, ADA, "/docs/a").unwrap();
    guard(&chain, ADA, "/docs/a").unwrap();
    only_me(&chain, ADA, "/docs/a");
    let moved = || Change::Move {
        from: "/docs".into(),
        to: "/kept".into(),
    };
    let chunk = chain.put_blob(CHUNK, ADA.to_vec()).unwrap();
    let written = Change::Put {
        path: "/kept/b".into(),
        exec: false,
        meta: BTreeMap::new(),
        content: Content::Chunks {
            size: ADA.len() as u64,
            chunks: vec![chunk],
        },
    };
    assert_eq!(
        commit(&chain, ADA, vec![moved(), written]),
        Err(code::WRONG_STATE.to_string()),
        "the guards' records move only once the commit ends"
    );
    assert_eq!(guarded(&chain, "/docs/a"), Some("/docs/a".into()));
    assert_eq!(commit(&chain, ADA, vec![moved()]), Ok(()));
    assert_eq!(guarded(&chain, "/kept/a"), Some("/kept/a".into()));
    assert!(record(&chain, "/kept/a").is_some());
}

#[test]
fn owner_names_who_alone_may_guard_a_path() {
    let chain = chain();
    write(&chain, ADA, "/shared/ada.txt").unwrap();
    let owner = |path: &str| {
        let asked = Query::Owner { path: path.into() };
        let Reply::Owner(owner) = chain.query(PROGRAM, &asked).unwrap() else {
            panic!("Owner answers a path");
        };
        owner
    };
    assert_eq!(owner("/shared/ada.txt"), Some(::program::Party::Account(1)));
    assert_eq!(owner("/home/bo"), Some(::program::Party::Account(2)));
    assert_eq!(owner("/shared/nobody.txt"), None);
}
