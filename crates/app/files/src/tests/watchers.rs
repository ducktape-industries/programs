//! Watchers: a program that watches hears of every commit as `deliver ++ borsh(SnapshotRow)`
//! until an empty `deliver` stops it; only a program watches, and a refused message keeps the commit.
use ::program::{MessageId, Outcome, Renamer, invalid};

use super::*;

fn harness() -> Env {
    MockHost::env(PROGRAM).from_program("harness", Some(9))
}

fn watch(host: &MockHost, deliver: Vec<u8>) {
    let op = Op::Watch { deliver };
    host.attempt(|| Files::execute(&host.exec(harness()), op))
        .unwrap();
}

fn head_row(host: &MockHost) -> SnapshotRow {
    let Reply::Head(Some(row)) = ask(host, Query::Head).unwrap() else {
        panic!("files holds a head")
    };
    row
}

#[test]
fn a_watcher_hears_each_commit_with_its_row_until_it_stops() {
    let host = host();
    watch(&host, vec![26]);
    host.take_emissions();
    let first = vec![put(&host, "/shared/a.txt", "a")];
    let id = commit(&host, ada(), None, first).unwrap();
    let [told] = host
        .take_emissions()
        .try_into()
        .expect("one message for the watcher");
    let row = head_row(&host);
    assert_eq!(row.id, id);
    let mut payload = vec![26];
    payload.extend(abi::encode(&row));
    assert_eq!(told.target, "harness");
    assert_eq!(told.payload, payload);
    assert!(
        told.reply,
        "its outcome comes back to files, which absorbs it"
    );

    watch(&host, Vec::new());
    host.take_emissions();
    let second = vec![put(&host, "/shared/b.txt", "b")];
    commit(&host, ada(), Some(id), second).unwrap();
    assert!(
        host.take_emissions().is_empty(),
        "a stopped watcher hears nothing"
    );
}

#[test]
fn only_a_program_watches_and_a_refused_message_keeps_the_commit() {
    let host = host();
    refused(&host, ada(), Op::Watch { deliver: vec![1] });
    commit(&host, ada(), None, vec![put(&host, "/shared/a.txt", "a")]).unwrap();
    assert!(host.take_emissions().is_empty(), "no account watches");
    let id = MessageId {
        program: PROGRAM.into(),
        seq: 1,
    };
    let rejected = Outcome::Rejected(invalid("the watcher refused it"));
    Files::reply(&host.exec(ada()), &id, &rejected).unwrap();
}

fn watch_as(host: &MockHost, program: &str, deliver: Vec<u8>) {
    let env = MockHost::env(PROGRAM).from_program(program, Some(10));
    let op = Op::Watch { deliver };
    host.attempt(|| Files::execute(&host.exec(env), op))
        .unwrap();
}

fn migrate(host: &MockHost, first: Option<&str>, last: Option<&str>) {
    let op = Op::Migrate {
        height: 0,
        first: first.map(str::to_owned),
        last: last.map(str::to_owned),
    };
    host.attempt(|| Files::execute(&host.exec(bo()), op))
        .unwrap();
}

#[test]
fn migrate_moves_each_renamed_watcher_and_it_hears_the_next_commit_under_its_new_id() {
    let host = host();
    watch(&host, vec![26]);
    watch_as(&host, "wake", vec![7]);
    watch_as(&host, "o/wake", vec![8]);
    let renamer = MockHost::default();
    host.sibling::<Renamer>(programs::PROGRAM, &renamer);
    let renames = vec![
        ("harness".to_owned(), "o/harness".to_owned()),
        ("wake".to_owned(), "o/wake".to_owned()),
    ];
    Renamer::execute(&renamer.exec(MockHost::env("programs")), renames).unwrap();
    migrate(&host, Some("wake"), Some("wake"));
    let watchers = |host: &MockHost| -> Vec<(String, Vec<u8>)> {
        crate::state::WATCHERS.all(&host.exec(ada())).unwrap()
    };
    assert_eq!(
        watchers(&host),
        [
            ("harness".to_owned(), vec![26]),
            ("o/wake".to_owned(), vec![8])
        ],
        "only wake is in the range, and the watch o/wake set since is kept"
    );
    migrate(&host, None, None);
    let moved = [
        ("o/harness".to_owned(), vec![26]),
        ("o/wake".to_owned(), vec![8]),
    ];
    assert_eq!(watchers(&host), moved);
    let before = host.borrow().state.clone();
    migrate(&host, None, None);
    assert_eq!(host.borrow().state, before, "a second run writes nothing");
    host.sibling::<Roster>(programs::PROGRAM, &MockHost::default());
    host.take_emissions();
    commit(&host, ada(), None, vec![put(&host, "/shared/a.txt", "a")]).unwrap();
    let told: Vec<String> = host
        .take_emissions()
        .into_iter()
        .map(|told| told.target)
        .collect();
    assert_eq!(told, ["o/harness", "o/wake"]);
}
