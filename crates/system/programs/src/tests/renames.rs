// A program or a view renamed in place: its entry, roles, hook and holder move to a new owner/name
// id, and Renames answers every rename that still holds; a new id names its owner and its slug.

use ::program::AccountNumber;

use super::*;

const CAROL: AccountNumber = 7;
const EVE: AccountNumber = 9;

fn rename(from: &str, to: &str) -> Change {
    Change::Rename {
        from: from.into(),
        to: to.into(),
    }
}

fn renamed(from: &str, to: &str) -> (String, String) {
    (from.to_owned(), to.to_owned())
}

fn renames(store: &MockHost, height: u64) -> Vec<(String, String)> {
    let asked = Query::Renames(height);
    match Programs::query(&store.query(env(height, Origin::Root)), asked).unwrap() {
        Reply::Renames(renames) => renames,
        other => panic!("{other:?}"),
    }
}

fn owner(store: &MockHost, name: &str) -> Option<Holder> {
    let asked = Query::Owner(name.into());
    match Programs::query(&store.query(env(1, Origin::Root)), asked).unwrap() {
        Reply::Owner { holder, .. } => holder,
        other => panic!("{other:?}"),
    }
}

fn code_of(store: &MockHost, height: u64, program: &str) -> Option<BlobId> {
    match Programs::query(&store.query(env(height, Origin::Root)), Query::At(height)).unwrap() {
        Reply::Programs(entries) => entries
            .into_iter()
            .find(|entry| entry.program == program)
            .map(|entry| entry.code),
        other => panic!("{other:?}"),
    }
}

fn by(account: AccountNumber) -> Env {
    Env {
        height: 1,
        ..MockHost::env(crate::PROGRAM)
    }
    .signed([account as u8; 32], Some(account))
}

#[test]
fn a_rename_moves_the_entry_its_hook_and_its_holder_to_the_new_id() {
    let (store, _) = founded();
    let boot = BlobId::Sha256([1; 32]);
    schedule(&store, 5, rename("boot", "o/boot")).unwrap();
    assert_eq!(
        owner(&store, "o/boot"),
        Some(Holder::Vote),
        "claimed as it is scheduled"
    );
    assert_eq!(programs(&store, 4), ["boot"]);
    assert_eq!(programs(&store, 5), ["o/boot"]);
    assert_eq!(code_of(&store, 5, "o/boot"), Some(boot));
    assert_eq!(hooks(&store, 5), ["o/boot"]);
    assert_eq!(renames(&store, 4), Vec::<(String, String)>::new());
    assert_eq!(renames(&store, 5), [renamed("boot", "o/boot")]);
    fold_to(&store, 5);
    for height in [5, 6, 50] {
        assert_eq!(
            renames(&store, height),
            [renamed("boot", "o/boot")],
            "{height}"
        );
    }
    assert_eq!(programs(&store, 6), ["o/boot"]);
    assert_eq!(
        owner(&store, "boot"),
        Some(Holder::Vote),
        "the old id stays held"
    );
}

#[test]
fn a_renamed_id_renamed_again_answers_each_old_id_with_the_one_that_runs() {
    let (store, _) = founded();
    schedule(&store, 5, rename("boot", "o/boot")).unwrap();
    fold_to(&store, 5);
    schedule(&store, 10, rename("o/boot", "o/boot2")).unwrap();
    let both = [renamed("boot", "o/boot2"), renamed("o/boot", "o/boot2")];
    assert_eq!(renames(&store, 10), both, "the second rename still due");
    fold_to(&store, 10);
    assert_eq!(renames(&store, 10), both);
    assert_eq!(renames(&store, 7), [renamed("boot", "o/boot")]);
}

#[test]
fn a_role_program_is_renamed_by_the_vote_and_its_roles_follow_it() {
    let (store, _) = founded();
    let identity = MockHost::role(abi::role::IDENTITY);
    schedule(&store, 5, rename(&identity, "o/identity")).unwrap();
    let filled = |height| roles(&store, height).get(abi::role::IDENTITY).cloned();
    assert_eq!(filled(4), Some(identity.clone()));
    assert_eq!(filled(5), Some("o/identity".to_owned()));
    fold_to(&store, 5);
    assert_eq!(filled(5), Some("o/identity".to_owned()));
    assert_eq!(renames(&store, 5), [renamed(&identity, "o/identity")]);
}

#[test]
fn an_account_renames_an_id_it_holds_and_never_a_role_program() {
    let (store, _) = founded();
    let own = Op::Own {
        program: "boot".into(),
        holder: Holder::Account(CAROL),
    };
    Programs::execute(&store.exec(authority(1)), own).unwrap();
    let lands = 2 + crate::NOTICE;
    let at = |change| {
        Op::Schedule(Scheduled {
            height: lands,
            change,
        })
    };
    let stranger = at(rename("boot", "eve/boot"));
    let refused = store.refused(|| Programs::execute(&store.exec(by(EVE)), stranger));
    assert_eq!(refused.code, code::UNAUTHORIZED, "{refused:?}");
    let identity = MockHost::role(abi::role::IDENTITY);
    let a_role = at(rename(&identity, "carol/identity"));
    let refused = store.refused(|| Programs::execute(&store.exec(by(CAROL)), a_role));
    assert_eq!(refused.code, code::UNAUTHORIZED, "{refused:?}");
    Programs::execute(&store.exec(by(CAROL)), at(rename("boot", "carol/boot"))).unwrap();
    assert_eq!(owner(&store, "carol/boot"), Some(Holder::Account(CAROL)));
    assert_eq!(programs(&store, lands), ["carol/boot"]);
}

#[test]
fn a_rename_lands_only_from_a_listed_id_onto_a_free_one_that_names_its_owner() {
    let (store, code) = founded();
    let refused = |change| schedule(&store, 5, change).unwrap_err().code;
    assert_eq!(
        refused(rename("ghost", "o/ghost")),
        code::NOT_FOUND,
        "nothing to rename"
    );
    assert_eq!(
        refused(rename("boot", "boot2")),
        code::INVALID_INPUT,
        "a new id names its owner"
    );
    assert_eq!(
        refused(rename("boot", "o/Boot")),
        code::INVALID_INPUT,
        "one spelling"
    );
    assert_eq!(
        refused(rename("boot", "lens")),
        code::ALREADY_EXISTS,
        "a listed view"
    );
    schedule(&store, 5, Change::Set(entry("o/new", code))).unwrap();
    assert_eq!(
        refused(rename("boot", "o/new")),
        code::ALREADY_EXISTS,
        "a program waiting under it"
    );
    schedule(&store, 6, rename("lens", "o/lens")).unwrap();
    assert_eq!(
        views(&store, 6),
        [("o/lens".into(), BlobId::Sha256([2; 32]))]
    );
    assert_eq!(renames(&store, 6), [renamed("lens", "o/lens")]);
}

#[test]
fn a_new_id_names_its_owner_and_an_id_with_a_holder_keeps_its_bare_name() {
    let (store, code) = founded();
    let changes = [
        Change::Set(entry("new", code)),
        Change::SetView(View {
            name: "new".into(),
            view: code,
        }),
        publish("new", code, &[]),
    ];
    for change in changes {
        let refused = schedule(&store, 5, change).unwrap_err();
        assert_eq!(refused.code, code::INVALID_INPUT, "{refused:?}");
        assert!(refused.message.contains("<owner>/new"), "{refused:?}");
    }
    schedule(&store, 5, Change::Set(entry("boot", code))).unwrap();
    schedule(&store, 6, Change::Set(entry("o/new", code))).unwrap();
}

#[test]
fn no_two_ids_write_the_same_with_the_slash_as_a_dash() {
    let (store, code) = founded();
    schedule(&store, 5, Change::Set(entry("a-b/c", code))).unwrap();
    let view = Change::SetView(View {
        name: "a/b-c".into(),
        view: code,
    });
    for (height, change) in [
        (4, Change::Set(entry("a/b-c", code))),
        (4, rename("boot", "a/b-c")),
        (4, view),
        (6, Change::Set(entry("a/b-c", code))),
    ] {
        let refused = schedule(&store, height, change).unwrap_err();
        assert_eq!(refused.code, code::ALREADY_EXISTS, "{height}: {refused:?}");
        assert!(refused.message.contains("a-b/c"), "{refused:?}");
    }
    schedule(&store, 6, Change::Set(entry("a/b-d", code))).unwrap();
}

#[test]
fn a_new_owned_id_is_refused_the_slug_of_a_bare_id_that_runs() {
    let store = MockHost::default();
    let bare = entry("a-b-c", BlobId::Sha256([1; 32]));
    crate::rules::init(
        &store.exec(env(0, Origin::Root)),
        genesis(vec![bare], vec![], vec![]),
    );
    let blobs = blobs_role(&store);
    fold_to(&store, 1);
    let code = held(&blobs, CODE_KIND, b"wasm");
    for id in ["a-b/c", "a/b-c"] {
        let refused = schedule(&store, 5, Change::Set(entry(id, code))).unwrap_err();
        assert_eq!(refused.code, code::ALREADY_EXISTS, "{id}: {refused:?}");
    }
    schedule(&store, 5, rename("a-b-c", "a-b/c")).unwrap();
}

#[test]
fn a_removed_id_keeps_its_slug_while_it_is_held() {
    let (store, code) = founded();
    schedule(&store, 5, Change::Set(entry("a-b/c", code))).unwrap();
    schedule(&store, 6, Change::Remove("a-b/c".into())).unwrap();
    fold_to(&store, 6);
    let refused = schedule(&store, 9, Change::Set(entry("a/b-c", code))).unwrap_err();
    assert_eq!(refused.code, code::ALREADY_EXISTS, "{refused:?}");
}

#[test]
fn a_rename_and_a_set_of_its_new_id_at_one_height_run_the_new_code_under_the_new_id() {
    let (store, code) = founded();
    let identity = MockHost::role(abi::role::IDENTITY);
    let changes = [
        rename("boot", "a/boot"),
        Change::Set(entry("a/boot", code)),
        rename(&identity, "o/identity"),
        Change::Set(entry("o/identity", code)),
    ];
    for change in changes {
        schedule(&store, 5, change).unwrap();
    }
    let lands = |store: &MockHost| {
        for (id, gone) in [("a/boot", "boot"), ("o/identity", identity.as_str())] {
            assert_eq!(code_of(store, 5, id), Some(code), "{id}");
            assert_eq!(code_of(store, 5, gone), None, "{gone}");
        }
    };
    lands(&store);
    fold_to(&store, 5);
    lands(&store);
    assert_eq!(
        renames(&store, 5),
        [renamed("boot", "a/boot"), renamed(&identity, "o/identity")]
    );
}

#[test]
fn a_rename_over_the_code_it_expects_lands_and_a_cancelled_one_renames_nothing() {
    let (store, _) = founded();
    let over = |expected| Op::ScheduleOver {
        scheduled: Scheduled {
            height: 5,
            change: rename("boot", "o/boot"),
        },
        expected: Some(expected),
    };
    let wrong = Programs::execute(&store.exec(authority(1)), over(BlobId::Sha256([2; 32])));
    assert_eq!(wrong.unwrap_err().code, code::WRONG_STATE);
    Programs::execute(&store.exec(authority(1)), over(BlobId::Sha256([1; 32]))).unwrap();
    assert_eq!(renames(&store, 5), [renamed("boot", "o/boot")]);
    let cancel = Op::Cancel {
        height: 5,
        program: "boot".into(),
    };
    Programs::execute(&store.exec(authority(1)), cancel).unwrap();
    assert_eq!(renames(&store, 5), Vec::<(String, String)>::new());
    assert_eq!(programs(&store, 5), ["boot"]);
}
