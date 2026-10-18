// Who changes the programs role: the validators' vote, or any account at
// least `NOTICE` blocks ahead on a program that fills no role; and what the
// vote's word does to the changes accounts made.

use ::program::{AccountNumber, Error, Principal};

use super::*;
use crate::rules::BY;
use crate::{NOTICE, Pending};

const CAROL: AccountNumber = 7;
const DAN: AccountNumber = 8;
const EVE: AccountNumber = 9;
const AGENT: AccountNumber = 11;
const TIP: u64 = 10;
const EARLIEST: u64 = TIP + 1 + NOTICE;

fn seated() -> (MockHost, BlobId) {
    let store = MockHost::default();
    crate::rules::init(
        &store.exec(env(0, Origin::Root)),
        genesis(
            ["chat", "forge", "activity"]
                .map(|program| entry(program, BlobId::Sha256([1; 32])))
                .into(),
            vec![View {
                name: "lens".into(),
                view: BlobId::Sha256([2; 32]),
            }],
            vec!["activity".into()],
        ),
    );
    let blobs = blobs_role(&store);
    fold_to(&store, TIP + 1);
    (store, held(&blobs, CODE_KIND, b"wasm"))
}

fn by(account: AccountNumber) -> Env {
    Env {
        height: TIP + 1,
        ..MockHost::env(crate::PROGRAM)
    }
    .signed([account as u8; 32], Some(account))
}

fn vote() -> Env {
    authority(TIP + 1)
}

fn run(store: &MockHost, env: Env, op: Op) -> Result<(), Error> {
    store.attempt(|| Programs::execute(&store.exec(env), op))
}

fn refused(store: &MockHost, env: Env, op: Op) -> String {
    store
        .refused(|| Programs::execute(&store.exec(env), op))
        .code
}

fn at(height: u64, change: Change) -> Op {
    Op::Schedule(Scheduled { height, change })
}

fn set(program: &str, code: BlobId) -> Change {
    Change::Set(entry(program, code))
}

fn cancel_at(height: u64, program: &str) -> Op {
    Op::Cancel {
        height,
        program: program.into(),
    }
}

fn waiting(store: &MockHost) -> Vec<(u64, String, Option<AccountNumber>)> {
    let asked = Query::Scheduled {
        page: PageRequest::default(),
    };
    match Programs::query(&store.query(env(TIP + 1, Origin::Root)), asked).unwrap() {
        Reply::Scheduled(page) => page
            .items
            .into_iter()
            .map(|Pending { height, change, by }| (height, change.program().to_owned(), by))
            .collect(),
        other => panic!("{other:?}"),
    }
}

fn code_at(store: &MockHost, height: u64, program: &str) -> Option<BlobId> {
    match Programs::query(&store.query(env(height, Origin::Root)), Query::At(height)).unwrap() {
        Reply::Programs(entries) => entries
            .into_iter()
            .find(|entry| entry.program == program)
            .map(|entry| entry.code),
        other => panic!("{other:?}"),
    }
}

fn views_at(store: &MockHost, height: u64) -> Vec<String> {
    match Programs::query(
        &store.query(env(height, Origin::Root)),
        Query::Views(height),
    )
    .unwrap()
    {
        Reply::Views(views) => views.into_iter().map(|view| view.name).collect(),
        other => panic!("{other:?}"),
    }
}

fn view_of(name: &str, code: BlobId) -> Change {
    Change::SetView(View {
        name: name.into(),
        view: code,
    })
}

#[test]
fn an_accounts_change_lands_no_sooner_than_notice_blocks_after_it_is_made() {
    let (store, code) = seated();
    assert_eq!(
        refused(&store, by(CAROL), at(EARLIEST - 1, set("chat", code))),
        code::INVALID_INPUT,
        "one block short of the notice"
    );
    run(&store, by(CAROL), at(EARLIEST, set("chat", code))).unwrap();
    assert_eq!(waiting(&store), [(EARLIEST, "chat".into(), Some(CAROL))]);
    assert_ne!(code_at(&store, EARLIEST - 1, "chat"), Some(code));
    assert_eq!(code_at(&store, EARLIEST, "chat"), Some(code));
    fold_to(&store, EARLIEST);
    assert_eq!(code_at(&store, EARLIEST, "chat"), Some(code));
    assert!(waiting(&store).is_empty());
    assert!(
        !BY.has(
            &store.query(env(EARLIEST, Origin::Root)),
            &(EARLIEST, "chat".into())
        ),
        "fold clears who made it"
    );
}

#[test]
fn any_account_changes_a_program_that_fills_no_role_but_never_a_role_or_a_hook() {
    let (store, code) = seated();
    let cases: Vec<(&str, Env, Change)> = vec![
        ("a role", by(EVE), set("valset", code)),
        ("the programs role itself", by(EVE), set("programs", code)),
        (
            "a publish that fills a role",
            by(EVE),
            Change::Publish {
                entry: entry("chat", code),
                roles: vec!["scheduler".into()],
            },
        ),
        ("set a hook", by(EVE), Change::SetHook("chat".into())),
        (
            "remove a hook",
            by(EVE),
            Change::RemoveHook("activity".into()),
        ),
        (
            "remove a hooked program",
            by(EVE),
            Change::Remove("activity".into()),
        ),
        (
            "no account",
            by(EVE).signed([9; 32], None),
            set("chat", code),
        ),
        (
            "a chain frame",
            Env {
                sender: Some(Principal::Root),
                ..by(EVE)
            }
            .root(),
            set("chat", code),
        ),
    ];
    for (what, env, change) in cases {
        assert_eq!(
            refused(&store, env, at(EARLIEST, change)),
            code::UNAUTHORIZED,
            "{what}"
        );
    }
    run(&store, by(EVE), at(EARLIEST, set("activity", code))).unwrap();
    run(
        &store,
        by(EVE),
        at(EARLIEST, Change::Remove("forge".into())),
    )
    .unwrap();
    run(&store, by(CAROL), at(EARLIEST + 1, set("chat", code))).unwrap();
    run(&store, by(DAN), at(EARLIEST, set("trade", code))).unwrap();
    run(
        &store,
        by(DAN),
        at(
            EARLIEST + 1,
            Change::Publish {
                entry: entry("board", code),
                roles: Vec::new(),
            },
        ),
    )
    .unwrap();
    let through_harness = by(AGENT).from_program("harness", Some(AGENT));
    run(&store, through_harness, at(EARLIEST + 2, set("chat", code))).unwrap();
    assert_eq!(
        waiting(&store),
        [
            (EARLIEST, "activity".into(), Some(EVE)),
            (EARLIEST, "forge".into(), Some(EVE)),
            (EARLIEST, "trade".into(), Some(DAN)),
            (EARLIEST + 1, "board".into(), Some(DAN)),
            (EARLIEST + 1, "chat".into(), Some(CAROL)),
            (EARLIEST + 2, "chat".into(), Some(AGENT)),
        ]
    );
    fold_to(&store, EARLIEST + 2);
    assert_eq!(code_at(&store, EARLIEST + 2, "trade"), Some(code));
    assert_eq!(code_at(&store, EARLIEST + 2, "chat"), Some(code));
    assert_eq!(code_at(&store, EARLIEST + 2, "forge"), None);
}

#[test]
fn a_change_the_vote_left_waiting_holds_the_name_against_accounts() {
    let (store, code) = seated();
    run(&store, vote(), at(5_000, set("chat", code))).unwrap();
    assert_eq!(
        refused(&store, by(CAROL), at(EARLIEST, set("chat", code))),
        code::WRONG_STATE
    );
    run(&store, vote(), cancel_at(5_000, "chat")).unwrap();
    run(&store, by(CAROL), at(EARLIEST, set("chat", code))).unwrap();
}

#[test]
fn the_votes_removal_takes_the_names_changes_along_and_frees_it_once_it_lands() {
    let (store, code) = seated();
    run(&store, by(CAROL), at(EARLIEST, set("chat", code))).unwrap();
    run(&store, by(DAN), at(EARLIEST, set("forge", code))).unwrap();
    run(
        &store,
        vote(),
        at(EARLIEST + 2, Change::Remove("chat".into())),
    )
    .unwrap();
    assert_eq!(
        waiting(&store),
        [
            (EARLIEST, "forge".into(), Some(DAN)),
            (EARLIEST + 2, "chat".into(), None),
        ]
    );
    fold_to(&store, EARLIEST + 2);
    assert_eq!(code_at(&store, EARLIEST + 2, "chat"), None);
    let later = Env {
        height: EARLIEST + 3,
        ..by(EVE)
    };
    run(&store, later, at(EARLIEST + 3 + NOTICE, set("chat", code))).unwrap();
}

#[test]
fn the_votes_change_takes_every_change_an_account_made_to_its_name_along() {
    let (store, code) = seated();
    run(&store, by(CAROL), at(EARLIEST + 5, set("chat", code))).unwrap();
    run(&store, by(DAN), at(EARLIEST + 9, set("chat", code))).unwrap();
    run(&store, by(DAN), at(EARLIEST + 5, set("forge", code))).unwrap();
    run(&store, by(EVE), at(EARLIEST, view_of("trade", code))).unwrap();
    run(&store, by(EVE), at(EARLIEST, set("board", code))).unwrap();
    run(&store, by(CAROL), at(EARLIEST, view_of("lens", code))).unwrap();
    run(&store, vote(), at(EARLIEST + 5, set("chat", code))).unwrap();
    run(
        &store,
        vote(),
        at(EARLIEST + 5, Change::Remove("forge".into())),
    )
    .unwrap();
    run(&store, vote(), at(EARLIEST + 1, set("trade", code))).unwrap();
    run(&store, vote(), at(EARLIEST + 1, view_of("board", code))).unwrap();
    run(
        &store,
        vote(),
        at(EARLIEST + 1, Change::RemoveView("lens".into())),
    )
    .unwrap();
    assert_eq!(
        waiting(&store),
        [
            (EARLIEST + 1, "board".into(), None),
            (EARLIEST + 1, "lens".into(), None),
            (EARLIEST + 1, "trade".into(), None),
            (EARLIEST + 5, "chat".into(), None),
            (EARLIEST + 5, "forge".into(), None),
        ]
    );
}

#[test]
fn the_votes_change_over_an_accounts_removal_meets_what_runs() {
    let (store, code) = seated();
    run(
        &store,
        by(CAROL),
        at(EARLIEST, Change::Remove("chat".into())),
    )
    .unwrap();
    assert_eq!(
        refused(&store, vote(), at(EARLIEST + 1, view_of("chat", code))),
        code::ALREADY_EXISTS,
        "chat runs till the vote removes it"
    );
    assert_eq!(waiting(&store), [(EARLIEST, "chat".into(), Some(CAROL))]);
    run(
        &store,
        vote(),
        at(EARLIEST + 10, Change::Remove("chat".into())),
    )
    .unwrap();
    assert_eq!(waiting(&store), [(EARLIEST + 10, "chat".into(), None)]);
}

#[test]
fn the_votes_removal_of_what_only_an_account_would_add_leaves_nothing_waiting() {
    let (store, code) = seated();
    run(&store, by(CAROL), at(EARLIEST, set("trade", code))).unwrap();
    run(&store, by(DAN), at(EARLIEST, view_of("board", code))).unwrap();
    run(
        &store,
        vote(),
        at(EARLIEST + 10, Change::Remove("trade".into())),
    )
    .unwrap();
    run(
        &store,
        vote(),
        at(EARLIEST + 10, Change::RemoveView("board".into())),
    )
    .unwrap();
    assert!(waiting(&store).is_empty());
    assert_eq!(
        refused(
            &store,
            by(EVE),
            at(EARLIEST, Change::Remove("ghost".into()))
        ),
        code::NOT_FOUND
    );
}

#[test]
fn the_votes_cancel_of_a_change_already_gone_is_done() {
    let (store, code) = seated();
    run(&store, by(CAROL), at(EARLIEST, set("chat", code))).unwrap();
    run(&store, by(CAROL), cancel_at(EARLIEST, "chat")).unwrap();
    run(&store, vote(), cancel_at(EARLIEST, "chat")).unwrap();
    assert_eq!(
        refused(&store, by(DAN), cancel_at(EARLIEST, "chat")),
        code::NOT_FOUND,
        "an account's is refused"
    );
}

#[test]
fn any_account_cancels_an_accounts_change_and_only_the_vote_cancels_the_votes() {
    let (store, code) = seated();
    run(&store, by(CAROL), at(EARLIEST, set("chat", code))).unwrap();
    run(&store, by(DAN), at(EARLIEST + 1, set("chat", code))).unwrap();
    run(&store, by(DAN), at(EARLIEST + 2, set("chat", code))).unwrap();
    run(&store, by(CAROL), cancel_at(EARLIEST, "chat")).unwrap();
    run(&store, by(CAROL), cancel_at(EARLIEST + 1, "chat")).unwrap();
    let through_harness = by(AGENT).from_program("harness", Some(AGENT));
    run(&store, through_harness, cancel_at(EARLIEST + 2, "chat")).unwrap();
    let read = store.query(env(TIP + 1, Origin::Root));
    for height in EARLIEST..EARLIEST + 3 {
        assert!(
            !BY.has(&read, &(height, "chat".into())),
            "cancel clears {height}"
        );
    }
    run(&store, vote(), at(EARLIEST, set("chat", code))).unwrap();
    assert_eq!(
        refused(&store, by(CAROL), cancel_at(EARLIEST, "chat")),
        code::UNAUTHORIZED,
        "the vote's change"
    );
    run(&store, vote(), at(EARLIEST, set("valset", code))).unwrap();
    assert_eq!(
        refused(&store, by(CAROL), cancel_at(EARLIEST, "valset")),
        code::UNAUTHORIZED,
        "a role's program"
    );
    run(&store, vote(), cancel_at(EARLIEST, "chat")).unwrap();
    run(&store, vote(), cancel_at(EARLIEST, "valset")).unwrap();
    assert!(waiting(&store).is_empty());
}

#[test]
fn the_votes_change_clears_a_stale_maker() {
    let (store, code) = seated();
    let stale = |height: u64| {
        let ctx = store.exec(env(TIP, Origin::Root));
        BY.put(&ctx, &(height, "chat".into()), &CAROL);
    };
    stale(EARLIEST);
    run(&store, vote(), at(EARLIEST, set("chat", code))).unwrap();
    assert_eq!(waiting(&store), [(EARLIEST, "chat".into(), None)]);
    stale(EARLIEST + 1);
    run(
        &store,
        vote(),
        at(EARLIEST + 1, Change::Remove("chat".into())),
    )
    .unwrap();
    assert_eq!(
        waiting(&store),
        [
            (EARLIEST, "chat".into(), None),
            (EARLIEST + 1, "chat".into(), None)
        ]
    );
}

#[test]
fn a_name_an_accounts_removal_frees_is_held_until_it_lands() {
    let (store, code) = seated();
    run(
        &store,
        by(CAROL),
        at(EARLIEST, Change::Remove("chat".into())),
    )
    .unwrap();
    run(
        &store,
        by(CAROL),
        at(EARLIEST, Change::RemoveView("lens".into())),
    )
    .unwrap();
    for who in [CAROL, DAN] {
        assert_eq!(
            refused(&store, by(who), at(EARLIEST + 1, view_of("chat", code))),
            code::WRONG_STATE,
            "a view of chat by {who}"
        );
        assert_eq!(
            refused(&store, by(who), at(EARLIEST + 1, set("lens", code))),
            code::WRONG_STATE,
            "a program lens by {who}"
        );
    }
    run(&store, by(DAN), cancel_at(EARLIEST, "chat")).unwrap();
    run(&store, by(CAROL), cancel_at(EARLIEST, "lens")).unwrap();
    assert!(waiting(&store).is_empty());
    assert!(code_at(&store, EARLIEST + 1, "chat").is_some());
    assert_eq!(views_at(&store, EARLIEST + 1), ["lens"]);
}
