// A change scheduled over the code it expects: what is under its name just before its height
// and once every change already waiting for that name has landed.

use super::*;

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
