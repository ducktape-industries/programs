// A change scheduled over the code it expects: what is under its name just before its height
// and once every change already waiting for that name has landed.

use ::program::AccountNumber;

use super::*;
use crate::{NOTICE, Pending};

const CAROL: AccountNumber = 7;
const LANDS_IN: u64 = 11;

fn over(
    store: &MockHost,
    height: u64,
    change: Change,
    expected: Option<BlobId>,
) -> Result<(), ::program::Error> {
    let scheduled = Scheduled { height, change };
    let op = Op::ScheduleOver {
        scheduled,
        expected,
    };
    Programs::execute(&store.exec(authority(1)), op)
}

fn runs(store: &MockHost, height: u64, program: &str) -> Option<BlobId> {
    let asked = Query::Program(program.into());
    match Programs::query(&store.query(env(height, Origin::Root)), asked).unwrap() {
        Reply::Program { entry, .. } => entry.map(|entry| entry.code),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_change_over_the_code_it_expects_lands_and_one_over_any_other_is_refused_naming_it() {
    let (store, blobs) = founded_with_blobs();
    let boot = BlobId::Sha256([1; 32]);
    let first = held(&blobs, CODE_KIND, b"first");
    let second = held(&blobs, CODE_KIND, b"second");
    let set = |code| Change::Set(entry("boot", code));
    over(&store, 10, set(first), Some(boot)).unwrap();
    let after = over(&store, 20, set(second), Some(boot)).unwrap_err();
    assert_eq!(after.code, code::WRONG_STATE, "{}", after.message);
    assert!(
        after.message.contains(&format!("{first:?}")),
        "{}",
        after.message
    );
    let under = over(&store, 5, set(second), Some(boot)).unwrap_err();
    assert_eq!(under.code, code::WRONG_STATE, "{}", under.message);
    assert!(
        under.message.contains(&format!("{first:?}")),
        "{}",
        under.message
    );
    over(&store, 20, set(second), Some(first)).unwrap();
    assert_eq!(runs(&store, 9, "boot"), Some(boot));
    assert_eq!(runs(&store, 10, "boot"), Some(first));
    assert_eq!(runs(&store, 20, "boot"), Some(second));
}

#[test]
fn a_first_set_expects_nothing_under_its_name() {
    let (store, code) = founded();
    let set = Change::Set(entry("o/new", code));
    let taken = over(&store, 5, set.clone(), Some(BlobId::Sha256([1; 32]))).unwrap_err();
    assert_eq!(taken.code, code::WRONG_STATE, "{}", taken.message);
    assert!(taken.message.contains("nothing"), "{}", taken.message);
    over(&store, 5, set, None).unwrap();
    assert_eq!(runs(&store, 5, "o/new"), Some(code));
}

fn by(account: AccountNumber, height: u64) -> Env {
    Env {
        height,
        ..MockHost::env(crate::PROGRAM)
    }
    .signed([account as u8; 32], Some(account))
}

fn over_in(blocks: u64, change: Change, expected: Option<BlobId>) -> Op {
    Op::ScheduleOverIn {
        blocks,
        change,
        expected,
    }
}

fn waiting(store: &MockHost, height: u64) -> Vec<(u64, Option<AccountNumber>)> {
    let asked = Query::Scheduled {
        page: PageRequest::default(),
    };
    match Programs::query(&store.query(env(height, Origin::Root)), asked).unwrap() {
        Reply::Scheduled(page) => page
            .items
            .into_iter()
            .map(|Pending { height, by, .. }| (height, by))
            .collect(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_height_picked_a_block_before_the_op_lands_is_refused_and_notice_blocks_in_lands_first_time() {
    let (store, code) = founded();
    fold_to(&store, LANDS_IN);
    let set = Change::Set(entry("o/new", code));
    let picked = Op::ScheduleOver {
        scheduled: Scheduled {
            height: LANDS_IN - 1 + NOTICE,
            change: set.clone(),
        },
        expected: None,
    };
    let stale = store.refused(|| Programs::execute(&store.exec(by(CAROL, LANDS_IN)), picked));
    assert_eq!(stale.code, code::INVALID_INPUT, "{}", stale.message);
    let first = (LANDS_IN + NOTICE).to_string();
    assert!(stale.message.contains(&first), "{}", stale.message);
    let landed = Programs::execute(&store.exec(by(CAROL, LANDS_IN)), over_in(NOTICE, set, None));
    assert!(landed.is_ok(), "{landed:?}");
    assert_eq!(
        waiting(&store, LANDS_IN),
        [(LANDS_IN + NOTICE, Some(CAROL))]
    );
    assert_eq!(runs(&store, LANDS_IN + NOTICE - 1, "o/new"), None);
    assert_eq!(runs(&store, LANDS_IN + NOTICE, "o/new"), Some(code));
}

#[test]
fn blocks_under_notice_are_refused_naming_the_floor_and_the_vote_takes_any_later_block() {
    let (store, code) = founded();
    fold_to(&store, LANDS_IN);
    let set = Change::Set(entry("o/new", code));
    let under = over_in(NOTICE - 1, set.clone(), None);
    let refused = store.refused(|| Programs::execute(&store.exec(by(CAROL, LANDS_IN)), under));
    assert_eq!(refused.code, code::INVALID_INPUT, "{}", refused.message);
    let floor = format!("at least {NOTICE} blocks");
    assert!(refused.message.contains(&floor), "{}", refused.message);
    let now = over_in(0, set.clone(), None);
    let refused = store.refused(|| Programs::execute(&store.exec(authority(LANDS_IN)), now));
    assert_eq!(refused.code, code::INVALID_INPUT, "{}", refused.message);
    let next = Programs::execute(&store.exec(authority(LANDS_IN)), over_in(1, set, None));
    assert!(next.is_ok(), "{next:?}");
    assert_eq!(runs(&store, LANDS_IN + 1, "o/new"), Some(code));
}

#[test]
fn blocks_in_still_land_only_over_the_code_they_expect() {
    let (store, code) = founded();
    fold_to(&store, LANDS_IN);
    let boot = BlobId::Sha256([1; 32]);
    let set = Change::Set(entry("boot", code));
    let wrong = over_in(5, set.clone(), None);
    let refused = store.refused(|| Programs::execute(&store.exec(authority(LANDS_IN)), wrong));
    assert_eq!(refused.code, code::WRONG_STATE, "{}", refused.message);
    let before = format!("just before block {}", LANDS_IN + 5);
    assert!(refused.message.contains(&before), "{}", refused.message);
    let right = Programs::execute(
        &store.exec(authority(LANDS_IN)),
        over_in(5, set, Some(boot)),
    );
    assert!(right.is_ok(), "{right:?}");
    assert_eq!(runs(&store, LANDS_IN + 4, "boot"), Some(boot));
    assert_eq!(runs(&store, LANDS_IN + 5, "boot"), Some(code));
}
