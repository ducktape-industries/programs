//! Files on a `MockChain` beside the access program: who owns a path, the list its owner sets
//! there, the gate deciding each commit by the nearest list, and a list moving and going with
//! its path.
use ::program::{MockChain, Origin, Ownership, Party, Writers};

use super::*;

const ADA: &[u8] = b"ada";
const BO: &[u8] = b"bo";
const CY: &[u8] = b"cy";

fn chain() -> MockChain {
    let mut chain = MockChain::default();
    chain.seat::<Files>(PROGRAM);
    chain.seat::<access::Access>("access");
    for (program, number) in [(PROGRAM, 900), ("access", 901), ("harness", 902)] {
        chain.register(program, number);
    }
    for (key, number, name) in [(ADA, 1, "ada"), (BO, 2, "bo"), (CY, 3, "cy")] {
        chain.hold(key, number);
        chain.profile(profile(number, name));
    }
    chain
}

fn text_change(chain: &MockChain, path: &str, text: &str) -> Change {
    let chunk = chain.put_blob(CHUNK, text.as_bytes().to_vec()).unwrap();
    Change::Put {
        path: path.into(),
        exec: false,
        meta: BTreeMap::new(),
        content: Content::Chunks {
            size: text.len() as u64,
            chunks: vec![chunk],
        },
    }
}

fn commit_as(chain: &MockChain, origin: Origin, changes: Vec<Change>) -> Result<(), Error> {
    let Reply::Head(head) = chain.query(PROGRAM, &Query::Head).unwrap() else {
        panic!("a head");
    };
    let op = Op::Commit {
        base: head.map(|row| row.id),
        message: "work".into(),
        changes,
    };
    chain.submit(origin, PROGRAM, &op).map(drop)
}

fn signed(key: &[u8]) -> Origin {
    Origin::Signed(key.to_vec())
}

fn write(chain: &MockChain, key: &[u8], path: &str) -> Result<(), Error> {
    let text = String::from_utf8(key.to_vec()).unwrap();
    let change = text_change(chain, path, &text);
    commit_as(chain, signed(key), vec![change])
}

fn list(chain: &MockChain, key: &[u8], path: &str, list: Option<Ownership>) -> Result<(), Error> {
    let set = access::Op::Set {
        program: PROGRAM.into(),
        key: path.into(),
        list,
    };
    chain.submit(signed(key), "access", &set).map(drop)
}

fn accounts(numbers: &[u64]) -> Ownership {
    Ownership::Listed(numbers.iter().copied().map(Party::Account).collect())
}

fn asked(chain: &MockChain, query: access::Query) -> access::Reply {
    chain.query("access", &query).unwrap()
}

fn owner(chain: &MockChain, path: &str) -> Option<Party> {
    let query = access::Query::Resource {
        program: PROGRAM.into(),
        key: path.into(),
    };
    match asked(chain, query) {
        access::Reply::Resource(resource) => resource.owner,
        other => panic!("{other:?}"),
    }
}

fn listed(chain: &MockChain, path: &str) -> Option<Ownership> {
    let query = access::Query::List {
        program: PROGRAM.into(),
        key: path.into(),
    };
    match asked(chain, query) {
        access::Reply::List(list) => list,
        other => panic!("{other:?}"),
    }
}

fn code_of(result: Result<(), Error>) -> String {
    result.expect_err("refused").code
}

fn text(chain: &MockChain, path: &str) -> String {
    let read = Query::Read {
        path: path.into(),
        at: None,
        offset: 0,
        len: u64::MAX,
    };
    let Reply::Contents(contents) = chain.query(PROGRAM, &read).unwrap() else {
        panic!("contents");
    };
    String::from_utf8(contents.bytes).unwrap()
}

#[test]
fn a_path_no_list_covers_is_anyones_and_its_maker_owns_it_and_the_folders_it_made() {
    let chain = chain();
    write(&chain, ADA, "/docs/a").unwrap();
    write(&chain, BO, "/docs/a").unwrap();
    write(&chain, BO, "/docs/b").unwrap();
    assert_eq!(owner(&chain, "/docs"), Some(Party::Account(1)));
    assert_eq!(owner(&chain, "/docs/a"), Some(Party::Account(1)));
    assert_eq!(owner(&chain, "/docs/b"), Some(Party::Account(2)));
    let writers = access::Query::Writers {
        program: PROGRAM.into(),
        key: "/docs/a".into(),
    };
    assert_eq!(
        asked(&chain, writers),
        access::Reply::Writers(Writers::Anyone)
    );
}

#[test]
fn the_nearest_list_decides_every_write_and_only_the_owner_sets_one() {
    let chain = chain();
    write(&chain, ADA, "/docs/a").unwrap();
    assert_eq!(
        code_of(list(&chain, BO, "/docs", Some(Ownership::Owner))),
        code::UNAUTHORIZED
    );
    list(&chain, ADA, "/docs", Some(accounts(&[2]))).unwrap();
    write(&chain, BO, "/docs/a").unwrap();
    write(&chain, BO, "/docs/new/deep").unwrap();
    assert_eq!(code_of(write(&chain, CY, "/docs/a")), code::UNAUTHORIZED);
    assert_eq!(
        text(&chain, "/docs/a"),
        "bo",
        "a refused commit leaves the head as it was"
    );
    list(&chain, BO, "/docs/new", Some(Ownership::Anyone)).unwrap();
    write(&chain, CY, "/docs/new/deep").unwrap();
    assert_eq!(
        code_of(write(&chain, CY, "/docs/other")),
        code::UNAUTHORIZED
    );
}

#[test]
fn a_home_is_owned_by_the_one_account_named_so_and_its_folder_is_the_chains() {
    let chain = chain();
    write(&chain, BO, "/home/ada/from-bo").unwrap();
    assert_eq!(owner(&chain, "/home/ada"), Some(Party::Account(1)));
    assert_eq!(
        code_of(list(&chain, BO, "/home/ada", Some(Ownership::Owner))),
        code::UNAUTHORIZED
    );
    list(&chain, ADA, "/home/ada", Some(Ownership::Owner)).unwrap();
    assert_eq!(
        code_of(write(&chain, BO, "/home/ada/from-bo")),
        code::UNAUTHORIZED
    );
    write(&chain, ADA, "/home/ada/from-bo").unwrap();
    let folder = vec![Change::Mkdir {
        path: "/home/cy".into(),
    }];
    assert_eq!(
        code_of(commit_as(&chain, signed(CY), folder.clone())),
        code::UNAUTHORIZED
    );
    commit_as(&chain, Origin::Root, folder).unwrap();
    chain.profile(profile(4, "ada"));
    assert_eq!(
        owner(&chain, "/home/ada"),
        None,
        "two accounts hold the name"
    );
}

#[test]
fn a_move_carries_the_lists_at_and_below_its_path_and_a_remove_drops_them() {
    let chain = chain();
    write(&chain, ADA, "/docs/a/x").unwrap();
    list(&chain, ADA, "/docs/a", Some(Ownership::Owner)).unwrap();
    list(&chain, ADA, "/docs/a/x", Some(accounts(&[2]))).unwrap();
    let moved = vec![Change::Move {
        from: "/docs/a".into(),
        to: "/docs/b".into(),
    }];
    assert_eq!(
        code_of(commit_as(&chain, signed(BO), moved.clone())),
        code::UNAUTHORIZED
    );
    commit_as(&chain, signed(ADA), moved).unwrap();
    assert_eq!(listed(&chain, "/docs/a"), None);
    assert_eq!(listed(&chain, "/docs/b"), Some(Ownership::Owner));
    assert_eq!(listed(&chain, "/docs/b/x"), Some(accounts(&[2])));
    let removed = vec![Change::Remove {
        path: "/docs/b".into(),
    }];
    commit_as(&chain, signed(ADA), removed).unwrap();
    assert_eq!(listed(&chain, "/docs/b"), None);
    assert_eq!(listed(&chain, "/docs/b/x"), None);
}

#[test]
fn the_folders_owner_removes_what_others_made_inside_it() {
    let chain = chain();
    write(&chain, ADA, "/docs/a").unwrap();
    write(&chain, BO, "/docs/b").unwrap();
    list(&chain, BO, "/docs/b", Some(Ownership::Owner)).unwrap();
    let removed = |path: &str| vec![Change::Remove { path: path.into() }];
    assert_eq!(
        code_of(commit_as(&chain, signed(CY), removed("/docs/b"))),
        code::UNAUTHORIZED
    );
    commit_as(&chain, signed(ADA), removed("/docs/b")).unwrap();
}

#[test]
fn the_chain_writes_anywhere_whatever_a_list_says() {
    let chain = chain();
    write(&chain, ADA, "/docs/a").unwrap();
    list(&chain, ADA, "/docs", Some(Ownership::Owner)).unwrap();
    let change = text_change(&chain, "/docs/a", "chain");
    commit_as(&chain, Origin::Root, vec![change]).unwrap();
    assert_eq!(text(&chain, "/docs/a"), "chain");
}

#[test]
fn a_listed_program_writes_for_any_account_it_sends_for() {
    let chain = chain();
    write(&chain, ADA, "/docs/a").unwrap();
    let harness = Ownership::Listed(vec![Party::Program("harness".into())]);
    list(&chain, ADA, "/docs", Some(harness)).unwrap();
    assert_eq!(code_of(write(&chain, BO, "/docs/a")), code::UNAUTHORIZED);
    let change = text_change(&chain, "/docs/a", "from harness");
    commit_as(&chain, Origin::Program("harness".into()), vec![change]).unwrap();
}

#[test]
fn setting_a_list_in_a_commit_is_retired_and_a_pin_is_its_makers_to_drop() {
    let chain = chain();
    write(&chain, ADA, "/docs/a").unwrap();
    let retired = vec![Change::SetOwnership {
        path: "/docs/a".into(),
        write: Some(Ownership::Owner),
    }];
    assert_eq!(
        code_of(commit_as(&chain, signed(ADA), retired)),
        code::UNAUTHORIZED
    );
    let Reply::Head(Some(head)) = chain.query(PROGRAM, &Query::Head).unwrap() else {
        panic!("a head");
    };
    let pin = Op::Pin {
        name: "v1".into(),
        snapshot: head.id,
    };
    chain.submit(signed(ADA), PROGRAM, &pin).unwrap();
    let unpin = Op::Unpin { name: "v1".into() };
    assert_eq!(
        chain.submit(signed(BO), PROGRAM, &unpin).unwrap_err().code,
        code::UNAUTHORIZED
    );
    chain.submit(signed(ADA), PROGRAM, &unpin).unwrap();
}
