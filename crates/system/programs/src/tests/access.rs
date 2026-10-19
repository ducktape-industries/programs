// Who changes the programs role: the validators' vote, or the account that holds a name, at
// least `NOTICE` blocks ahead on a program that fills no role; who holds a name, and what the
// vote's word does to the changes accounts made.

use ::program::{AccountNumber, Error, Principal};

use super::*;
use crate::rules::{BY, OWNERS};
use crate::{Holder, NOTICE, Pending};

const CAROL: AccountNumber = 7;
const DAN: AccountNumber = 8;
const EVE: AccountNumber = 9;
const TIP: u64 = 10;
const EARLIEST: u64 = TIP + 1 + NOTICE;

fn seated() -> (MockHost, BlobId) {
    let store = MockHost::default();
    crate::rules::init(
        &store.exec(env(0, Origin::Root)),
        genesis(
            ["chat", "forge", "activity", "files"]
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
    let handed = [
        ("chat", CAROL),
        ("forge", DAN),
        ("activity", EVE),
        ("lens", CAROL),
    ];
    for (name, account) in handed {
        run(&store, vote(), own(name, Holder::Account(account))).unwrap();
    }
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

fn refusal(store: &MockHost, env: Env, op: Op) -> Error {
    store.refused(|| Programs::execute(&store.exec(env), op))
}

fn refused(store: &MockHost, env: Env, op: Op) -> String {
    refusal(store, env, op).code
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

fn own(program: &str, holder: Holder) -> Op {
    Op::Own {
        program: program.into(),
        holder,
    }
}

fn owner(store: &MockHost, name: &str) -> Option<Holder> {
    let asked = Query::Owner(name.into());
    match Programs::query(&store.query(env(TIP + 1, Origin::Root)), asked).unwrap() {
        Reply::Owner { holder, .. } => holder,
        other => panic!("{other:?}"),
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
fn an_account_changes_a_name_it_holds_but_never_a_role_or_a_hook() {
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
        by(DAN),
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
    let through_harness = by(CAROL).from_program("harness", Some(CAROL));
    run(&store, through_harness, at(EARLIEST + 2, set("chat", code))).unwrap();
    assert_eq!(
        waiting(&store),
        [
            (EARLIEST, "activity".into(), Some(EVE)),
            (EARLIEST, "forge".into(), Some(DAN)),
            (EARLIEST, "trade".into(), Some(DAN)),
            (EARLIEST + 1, "board".into(), Some(DAN)),
            (EARLIEST + 1, "chat".into(), Some(CAROL)),
            (EARLIEST + 2, "chat".into(), Some(CAROL)),
        ]
    );
    fold_to(&store, EARLIEST + 2);
    assert_eq!(code_at(&store, EARLIEST + 2, "trade"), Some(code));
    assert_eq!(code_at(&store, EARLIEST + 2, "chat"), Some(code));
    assert_eq!(code_at(&store, EARLIEST + 2, "forge"), None);
}

#[test]
fn a_stranger_is_refused_by_name_of_the_holder_and_how_it_hands_a_name_on() {
    let (store, code) = seated();
    let changes = [
        set("chat", code),
        view_of("lens", code),
        Change::Remove("chat".into()),
    ];
    for change in changes {
        let refused = refusal(&store, by(EVE), at(EARLIEST, change.clone()));
        assert_eq!(refused.code, code::UNAUTHORIZED, "{change:?}");
        let says = &refused.message;
        assert!(says.contains("account 7") && says.contains("Own"), "{says}");
    }
    let refused = refusal(&store, by(EVE), at(EARLIEST, set("files", code)));
    assert_eq!(refused.code, code::UNAUTHORIZED);
    let says = &refused.message;
    assert!(
        says.contains("the validators' vote") && says.contains("Own"),
        "{says}"
    );
    assert!(waiting(&store).is_empty());
    run(&store, by(CAROL), at(EARLIEST, set("chat", code))).unwrap();
    run(&store, vote(), at(EARLIEST, set("files", code))).unwrap();
}

#[test]
fn the_first_account_to_set_a_free_name_holds_it_through_its_removal() {
    let (store, code) = seated();
    assert_eq!(owner(&store, "trade"), None);
    run(&store, by(DAN), at(EARLIEST, set("trade", code))).unwrap();
    assert_eq!(
        owner(&store, "trade"),
        Some(Holder::Account(DAN)),
        "held from its schedule, before it lands"
    );
    assert_eq!(
        refused(&store, by(EVE), at(EARLIEST + 1, set("trade", code))),
        code::UNAUTHORIZED,
        "a second account within the notice"
    );
    run(&store, by(EVE), at(EARLIEST, view_of("market", code))).unwrap();
    assert_eq!(owner(&store, "market"), Some(Holder::Account(EVE)));
    let then = |height: u64, account| Env {
        height,
        ..by(account)
    };
    let removed = EARLIEST + 1 + NOTICE;
    let removal = at(removed, Change::Remove("trade".into()));
    run(&store, then(EARLIEST + 1, DAN), removal).unwrap();
    fold_to(&store, removed);
    assert_eq!(code_at(&store, removed, "trade"), None);
    fold_to(&store, removed + 1);
    let again = at(removed + 1 + NOTICE, set("trade", code));
    assert_eq!(
        refused(&store, then(removed + 1, EVE), again.clone()),
        code::UNAUTHORIZED,
        "a removal keeps the holder"
    );
    assert_eq!(owner(&store, "trade"), Some(Holder::Account(DAN)));
    run(&store, then(removed + 1, DAN), again).unwrap();
}

#[test]
fn a_founded_name_is_the_votes_and_its_holder_or_the_vote_hands_it_on() {
    let (store, code) = seated();
    assert_eq!(owner(&store, "files"), Some(Holder::Vote));
    assert_eq!(
        refused(&store, by(EVE), own("files", Holder::Account(EVE))),
        code::UNAUTHORIZED,
        "a stranger takes no name"
    );
    assert_eq!(
        refused(&store, by(EVE), own("ghost", Holder::Account(EVE))),
        code::NOT_FOUND,
        "nobody holds a free name to hand on"
    );
    run(&store, vote(), own("files", Holder::Account(EVE))).unwrap();
    run(&store, by(EVE), at(EARLIEST, set("files", code))).unwrap();
    run(&store, by(EVE), own("files", Holder::Account(DAN))).unwrap();
    assert_eq!(owner(&store, "files"), Some(Holder::Account(DAN)));
    let refused_eve = refusal(&store, by(EVE), at(EARLIEST + 1, set("files", code)));
    assert_eq!(refused_eve.code, code::UNAUTHORIZED);
    assert!(refused_eve.message.contains("account 8"), "{refused_eve:?}");
    assert_eq!(
        refused(&store, by(EVE), cancel_at(EARLIEST, "files")),
        code::UNAUTHORIZED,
        "nor cancels its own change once it handed the name on"
    );
    run(&store, by(DAN), cancel_at(EARLIEST, "files")).unwrap();
    run(&store, by(DAN), own("files", Holder::Vote)).unwrap();
    assert_eq!(
        refused(&store, by(DAN), at(EARLIEST, set("files", code))),
        code::UNAUTHORIZED
    );
    run(&store, vote(), own("files", Holder::Account(CAROL))).unwrap();
    assert_eq!(owner(&store, "files"), Some(Holder::Account(CAROL)));
    run(&store, vote(), at(5_000, set("new", code))).unwrap();
    assert_eq!(
        owner(&store, "new"),
        Some(Holder::Vote),
        "the vote's first set"
    );
    assert_eq!(
        refused(&store, by(EVE), at(EARLIEST, set("new", code))),
        code::UNAUTHORIZED
    );
}

#[test]
fn a_name_with_no_record_that_runs_is_listed_or_waits_is_the_votes() {
    let (store, code) = seated();
    run(&store, by(DAN), at(EARLIEST, set("trade", code))).unwrap();
    let ctx = store.exec(env(TIP, Origin::Root));
    for name in ["chat", "lens", "trade"] {
        OWNERS.remove(&ctx, &name.to_owned());
    }
    for name in ["chat", "lens", "trade"] {
        assert_eq!(owner(&store, name), Some(Holder::Vote), "{name}");
    }
    assert_eq!(owner(&store, "ghost"), None);
    assert_eq!(
        refused(&store, by(CAROL), at(EARLIEST + 1, set("chat", code))),
        code::UNAUTHORIZED
    );
    assert_eq!(
        refused(&store, by(DAN), at(EARLIEST + 1, set("trade", code))),
        code::UNAUTHORIZED
    );
    run(&store, vote(), own("chat", Holder::Account(CAROL))).unwrap();
    run(&store, by(CAROL), at(EARLIEST + 1, set("chat", code))).unwrap();
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
fn the_votes_removal_takes_the_names_changes_along_and_the_name_stays_its_holders() {
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
    fold_to(&store, EARLIEST + 3);
    let later = |account| Env {
        height: EARLIEST + 3,
        ..by(account)
    };
    let again = at(EARLIEST + 3 + NOTICE, set("chat", code));
    assert_eq!(
        refused(&store, later(EVE), again.clone()),
        code::UNAUTHORIZED
    );
    run(&store, later(CAROL), again).unwrap();
}

#[test]
fn the_votes_change_takes_every_change_an_account_made_to_its_name_along() {
    let (store, code) = seated();
    run(&store, by(CAROL), at(EARLIEST + 5, set("chat", code))).unwrap();
    run(&store, by(CAROL), at(EARLIEST + 9, set("chat", code))).unwrap();
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
fn only_a_names_holder_cancels_an_accounts_change_and_only_the_vote_the_votes() {
    let (store, code) = seated();
    for height in EARLIEST..EARLIEST + 3 {
        run(&store, by(CAROL), at(height, set("chat", code))).unwrap();
    }
    assert_eq!(
        refused(&store, by(DAN), cancel_at(EARLIEST, "chat")),
        code::UNAUTHORIZED,
        "a stranger"
    );
    run(&store, by(CAROL), cancel_at(EARLIEST, "chat")).unwrap();
    run(&store, by(CAROL), cancel_at(EARLIEST + 1, "chat")).unwrap();
    let through_harness = by(CAROL).from_program("harness", Some(CAROL));
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
    assert_eq!(
        refused(&store, by(CAROL), at(EARLIEST + 1, view_of("chat", code))),
        code::WRONG_STATE,
        "a view of chat"
    );
    assert_eq!(
        refused(&store, by(CAROL), at(EARLIEST + 1, set("lens", code))),
        code::WRONG_STATE,
        "a program lens"
    );
    run(&store, by(CAROL), cancel_at(EARLIEST, "chat")).unwrap();
    run(&store, by(CAROL), cancel_at(EARLIEST, "lens")).unwrap();
    assert!(waiting(&store).is_empty());
    assert!(code_at(&store, EARLIEST + 1, "chat").is_some());
    assert_eq!(views_at(&store, EARLIEST + 1), ["lens"]);
}
