// The program natively over `program::MockHost`: what the founding suite checks on the host, without the host.
// Who may change the programs role is `tests/access.rs`.

mod access;

use ::program::{BlobId, Env, MockHost, Origin, Program, code};
use store::PageRequest;

use crate::{CODE_KIND, Change, Entry, Genesis, Op, Programs, Query, Reply, Scheduled, View};

fn env(height: u64, origin: Origin) -> Env {
    Env {
        height,
        origin,
        // these rules read the origin alone
        sender: None,
        ..MockHost::env(crate::PROGRAM)
    }
}

const VALIDATOR: u8 = 5;

fn blobs_role(store: &MockHost) -> MockHost {
    let valset = MockHost::default();
    let genesis = valset::Genesis {
        validators: vec![valset::Member {
            key: vec![VALIDATOR; 32],
            address: None,
            halo: "validator.example.org:9100".into(),
        }],
        member_cap: 4,
    };
    valset::Valset::init(
        &valset.exec(MockHost::env(valset::PROGRAM).root()),
        &abi::encode(&genesis),
    )
    .unwrap();
    let blobs = MockHost::default();
    blobs.sibling::<valset::Valset>(MockHost::role(abi::role::VALIDATORS), &valset);
    store.sibling::<blobs::Blobs>(MockHost::role(abi::role::BLOBS), &blobs);
    blobs
}

fn put(blobs: &MockHost, kind: &str, body: &[u8]) -> (BlobId, u64) {
    let entry = blobs::Entry::of(&blob::frame(kind, body).unwrap()).unwrap();
    let id = entry.id;
    let put = blobs::Op::Put { blobs: vec![entry] };
    blobs::Blobs::execute(&blobs.exec(MockHost::env(blobs::PROGRAM)), put).unwrap();
    (id, abi::decode(&blobs.take_output()).unwrap())
}

fn receipted(blobs: &MockHost, bundle: u64) {
    let validator = MockHost::env(blobs::PROGRAM).signed(vec![VALIDATOR; 32], None);
    blobs::Blobs::execute(&blobs.exec(validator), blobs::Op::Receipt { bundle }).unwrap();
}

fn held(blobs: &MockHost, kind: &str, body: &[u8]) -> BlobId {
    let (id, bundle) = put(blobs, kind, body);
    receipted(blobs, bundle);
    id
}

fn fold_to(store: &MockHost, height: u64) {
    crate::rules::fold(&store.exec(env(height, Origin::Root)), height).unwrap();
}

fn authority(height: u64) -> Env {
    env(
        height,
        Origin::Program(MockHost::role(abi::role::VALIDATORS)),
    )
}

fn entry(program: &str, code: BlobId) -> Entry {
    Entry {
        program: program.into(),
        code,
        params: vec![],
    }
}

fn genesis(programs: Vec<Entry>, views: Vec<View>, hooks: Vec<String>) -> Genesis {
    let roles = MockHost::roles();
    let seats: std::collections::BTreeSet<String> =
        roles.iter().map(|(_, program)| program.clone()).collect();
    let seats = seats
        .into_iter()
        .map(|program| entry(&program, BlobId::Sha256([3; 32])));
    Genesis {
        programs: programs.into_iter().chain(seats).collect(),
        views,
        hooks,
        roles,
    }
}

fn seat(program: &str) -> bool {
    crate::role_bound(&MockHost::roles(), program)
}

fn founded() -> (MockHost, BlobId) {
    let (store, blobs) = founded_with_blobs();
    (store, held(&blobs, CODE_KIND, b"wasm"))
}

fn founded_with_blobs() -> (MockHost, MockHost) {
    let store = MockHost::default();
    crate::rules::init(
        &store.exec(env(0, Origin::Root)),
        genesis(
            vec![entry("boot", BlobId::Sha256([1; 32]))],
            vec![View {
                name: "lens".into(),
                view: BlobId::Sha256([2; 32]),
            }],
            vec!["boot".into()],
        ),
    );
    let blobs = blobs_role(&store);
    fold_to(&store, 1);
    (store, blobs)
}

fn programs(store: &MockHost, height: u64) -> Vec<String> {
    match Programs::query(&store.query(env(height, Origin::Root)), Query::At(height)).unwrap() {
        Reply::Programs(entries) => entries
            .into_iter()
            .map(|e| e.program)
            .filter(|program| !seat(program))
            .collect(),
        other => panic!("{other:?}"),
    }
}

fn hooks(store: &MockHost, height: u64) -> Vec<String> {
    match Programs::query(
        &store.query(env(height, Origin::Root)),
        Query::Hooks(height),
    )
    .unwrap()
    {
        Reply::Hooks(hooks) => hooks,
        other => panic!("{other:?}"),
    }
}

fn schedule(store: &MockHost, height: u64, change: Change) -> Result<(), ::program::Error> {
    Programs::execute(
        &store.exec(authority(1)),
        Op::Schedule(Scheduled { height, change }),
    )
}

#[test]
fn only_the_validators_vote_schedules_or_cancels() {
    let (store, code) = founded();
    let scheduled = Scheduled {
        height: 5,
        change: Change::Set(entry("new", code)),
    };
    let cancel = Op::Cancel {
        height: 5,
        program: "new".into(),
    };
    for origin in [
        Origin::Signed(vec![9]),
        Origin::Root,
        Origin::Program("probe".into()),
        Origin::Program(MockHost::role(abi::role::PROGRAMS)),
    ] {
        for op in [Op::Schedule(scheduled.clone()), cancel.clone()] {
            let refused =
                store.refused(|| Programs::execute(&store.exec(env(1, origin.clone())), op));
            assert_eq!(refused.code, code::UNAUTHORIZED);
        }
    }
    Programs::execute(&store.exec(authority(1)), Op::Schedule(scheduled)).unwrap();
    Programs::execute(&store.exec(authority(1)), cancel).unwrap();
}

#[test]
fn a_change_lands_only_held_code_in_the_future() {
    let (store, code) = founded();
    let set = Change::Set(entry("new", code));
    assert_eq!(
        schedule(&store, 1, set.clone()).unwrap_err().code,
        code::INVALID_INPUT
    );
    let unheld = Change::Set(entry("new", BlobId::Sha256([7; 32])));
    assert_eq!(
        schedule(&store, 5, unheld).unwrap_err().code,
        code::NOT_FOUND
    );
    schedule(&store, 5, set.clone()).unwrap();
    assert_eq!(
        schedule(&store, 5, set).unwrap_err().code,
        code::ALREADY_EXISTS
    );
}

#[test]
fn a_scheduled_change_is_seen_at_its_height_and_folded_by_the_next_op() {
    let (store, code) = founded();
    schedule(&store, 5, Change::Set(entry("new", code))).unwrap();
    schedule(&store, 6, Change::Remove("boot".into())).unwrap();
    assert_eq!(programs(&store, 4), ["boot"]);
    assert_eq!(programs(&store, 5), ["boot", "new"]);
    assert_eq!(programs(&store, 6), ["new"]);
    fold_to(&store, 6);
    assert_eq!(programs(&store, 6), ["new"]);
    let Reply::Scheduled(page) = Programs::query(
        &store.query(env(6, Origin::Root)),
        Query::Scheduled {
            page: PageRequest::default(),
        },
    )
    .unwrap() else {
        panic!()
    };
    assert!(page.items.is_empty(), "folded changes leave the schedule");
    let Reply::Program { height, entry } = Programs::query(
        &store.query(env(6, Origin::Root)),
        Query::Program("new".into()),
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!((height, entry.map(|e| e.code)), (6, Some(code)));
}

fn roster(store: &MockHost, height: u64) -> (u64, Vec<String>, Vec<String>) {
    match Programs::query(&store.query(env(height, Origin::Root)), Query::Roster).unwrap() {
        Reply::Roster {
            height,
            programs,
            views,
        } => (
            height,
            programs
                .into_iter()
                .map(|e| e.program)
                .filter(|program| !seat(program))
                .collect(),
            views.into_iter().map(|v| v.name).collect(),
        ),
        other => panic!("{other:?}"),
    }
}

/// What a client lists: no op runs here after the schedule, so nothing is
/// folded, and the changes still land at their height.
#[test]
fn the_roster_is_answered_at_the_querys_own_height() {
    let (store, code) = founded();
    schedule(&store, 5, Change::Set(entry("new", code))).unwrap();
    let explorer = View {
        name: "explorer".into(),
        view: code,
    };
    schedule(&store, 5, Change::SetView(explorer)).unwrap();
    assert_eq!(
        roster(&store, 4),
        (4, vec!["boot".into()], vec!["lens".into()])
    );
    assert_eq!(
        roster(&store, 5),
        (
            5,
            vec!["boot".into(), "new".into()],
            vec!["explorer".into(), "lens".into()]
        )
    );
}

#[test]
fn a_program_bound_to_a_role_is_never_removed() {
    let (store, code) = founded();
    for role in abi::role::REQUIRED {
        let program = MockHost::role(role);
        let removed = schedule(&store, 5, Change::Remove(program.clone())).unwrap_err();
        assert_eq!(removed.code, code::INVALID_INPUT, "{role}: {program}");
    }
    // its code still changes
    schedule(&store, 5, Change::Set(entry("identity", code))).unwrap();
}

#[test]
fn the_schedule_pages_in_height_order_and_a_cancel_removes_one_change() {
    let (store, code) = founded();
    for height in [30u64, 4, 200] {
        schedule(&store, height, Change::Set(entry("p", code))).unwrap();
    }
    let ask = |store: &MockHost, page: PageRequest| match Programs::query(
        &store.query(env(2, Origin::Root)),
        Query::Scheduled { page },
    )
    .unwrap()
    {
        Reply::Scheduled(page) => page,
        other => panic!("{other:?}"),
    };
    let first = ask(&store, PageRequest::first(2));
    assert_eq!(
        first.items.iter().map(|s| s.height).collect::<Vec<_>>(),
        [4, 30],
        "numeric order, not lexical"
    );
    assert_eq!(first.height, 2);
    let rest = ask(
        &store,
        PageRequest {
            after: first.next,
            limit: Some(2),
        },
    );
    assert_eq!(rest.items[0].height, 200);
    assert_eq!(rest.next, None);
    Programs::execute(
        &store.exec(authority(2)),
        Op::Cancel {
            height: 30,
            program: "p".into(),
        },
    )
    .unwrap();
    // the vote's cancel of a change already gone is done, not refused
    Programs::execute(
        &store.exec(authority(2)),
        Op::Cancel {
            height: 30,
            program: "p".into(),
        },
    )
    .unwrap();
    assert_eq!(ask(&store, PageRequest::default()).items.len(), 2);
}

fn views(store: &MockHost, height: u64) -> Vec<(String, BlobId)> {
    match Programs::query(
        &store.query(env(height, Origin::Root)),
        Query::Views(height),
    )
    .unwrap()
    {
        Reply::Views(views) => views.into_iter().map(|v| (v.name, v.view)).collect(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_view_is_listed_apart_from_the_programs_and_scheduled_like_one() {
    let (store, code) = founded();
    assert_eq!(views(&store, 1), [("lens".into(), BlobId::Sha256([2; 32]))]);
    assert_eq!(programs(&store, 1), ["boot"], "a view is never a program");
    let explorer = Change::SetView(View {
        name: "explorer".into(),
        view: code,
    });
    let unheld = Change::SetView(View {
        name: "explorer".into(),
        view: BlobId::Sha256([7; 32]),
    });
    assert_eq!(
        schedule(&store, 5, unheld).unwrap_err().code,
        code::NOT_FOUND
    );
    let over_a_program = Change::SetView(View {
        name: "boot".into(),
        view: code,
    });
    assert_eq!(
        schedule(&store, 5, over_a_program).unwrap_err().code,
        code::ALREADY_EXISTS
    );
    schedule(&store, 5, explorer).unwrap();
    schedule(&store, 6, Change::RemoveView("lens".into())).unwrap();
    assert_eq!(views(&store, 4).len(), 1);
    assert_eq!(
        views(&store, 5),
        [
            ("explorer".into(), code),
            ("lens".into(), BlobId::Sha256([2; 32]))
        ]
    );
    fold_to(&store, 6);
    assert_eq!(views(&store, 6), [("explorer".into(), code)]);
    assert_eq!(programs(&store, 6), ["boot"]);
}

fn cancel(store: &MockHost, height: u64, program: &str) -> Result<(), ::program::Error> {
    Programs::execute(
        &store.exec(authority(1)),
        Op::Cancel {
            height,
            program: program.into(),
        },
    )
}

#[test]
fn a_name_is_one_kind_whatever_order_the_changes_are_scheduled_in() {
    let (store, code) = founded();
    let view = |name: &str| {
        Change::SetView(View {
            name: name.into(),
            view: code,
        })
    };
    schedule(&store, 10, view("x")).unwrap();
    assert_eq!(
        schedule(&store, 5, Change::Set(entry("x", code)))
            .unwrap_err()
            .code,
        code::ALREADY_EXISTS,
        "a program landing before a pending view of its name"
    );
    schedule(&store, 5, Change::Set(entry("y", code))).unwrap();
    assert_eq!(
        schedule(&store, 3, view("y")).unwrap_err().code,
        code::ALREADY_EXISTS,
        "a view landing before a pending program of its name"
    );
    assert_eq!(
        schedule(&store, 20, Change::Set(entry("lens", code)))
            .unwrap_err()
            .code,
        code::ALREADY_EXISTS,
        "a listed view"
    );
    schedule(&store, 6, Change::Remove("boot".into())).unwrap();
    schedule(&store, 8, view("boot")).unwrap();
    assert_eq!(
        cancel(&store, 6, "boot").unwrap_err().code,
        code::ALREADY_EXISTS,
        "cancelling the removal would leave boot both kinds from 8"
    );
    assert_eq!(views(&store, 10).len(), 3);
    assert_eq!(programs(&store, 10), ["y"]);
}

/// A removal of the other kind is refused; the vote's removal of a name
/// nothing holds has its goal met: done, and nothing waits.
#[test]
fn a_removal_names_one_of_its_own_kind() {
    let (store, code) = founded();
    for change in [
        Change::Remove("ghost".into()),
        Change::RemoveView("ghost".into()),
    ] {
        schedule(&store, 5, change).unwrap();
    }
    assert_eq!(scheduled_heights(&store), Vec::<u64>::new());
    for (change, what) in [
        (Change::Remove("lens".into()), "a view is not a program"),
        (Change::RemoveView("boot".into()), "a program is not a view"),
    ] {
        assert_eq!(
            schedule(&store, 5, change).unwrap_err().code,
            code::NOT_FOUND,
            "{what}"
        );
    }
    schedule(&store, 5, Change::Set(entry("new", code))).unwrap();
    schedule(&store, 6, Change::Remove("new".into())).unwrap();
    // not yet seated at 4
    schedule(&store, 4, Change::Remove("new".into())).unwrap();
    assert_eq!(scheduled_heights(&store), [5, 6]);
}

fn scheduled_heights(store: &MockHost) -> Vec<u64> {
    match Programs::query(
        &store.query(env(1, Origin::Root)),
        Query::Scheduled {
            page: PageRequest::default(),
        },
    )
    .unwrap()
    {
        Reply::Scheduled(page) => page.items.iter().map(|change| change.height).collect(),
        other => panic!("{other:?}"),
    }
}

fn roles(store: &MockHost, height: u64) -> ::program::Roles {
    match Programs::query(&store.query(env(height, Origin::Root)), Query::Roles).unwrap() {
        Reply::Roles(roles) => roles,
        other => panic!("{other:?}"),
    }
}

fn publish(program: &str, code: BlobId, roles: &[&str]) -> Change {
    Change::Publish {
        entry: entry(program, code),
        roles: roles.iter().map(|role| (*role).to_owned()).collect(),
    }
}

#[test]
fn the_roles_are_the_table_genesis_fills_and_the_votes_publishes_change() {
    let (store, code) = founded();
    assert_eq!(roles(&store, 1), MockHost::roles());
    let egress = MockHost::role(abi::role::EGRESS);
    schedule(
        &store,
        5,
        publish("egress-b", code, &[abi::role::EGRESS, "later"]),
    )
    .unwrap();
    let moved = roles(&store, 5);
    assert_eq!(moved.get(abi::role::EGRESS), Some(&"egress-b".to_owned()));
    assert_eq!(moved.get("later"), Some(&"egress-b".to_owned()));
    assert_eq!(roles(&store, 4).get(abi::role::EGRESS), Some(&egress));
    schedule(&store, 6, publish("egress-b", code, &["later"])).unwrap();
    assert_eq!(roles(&store, 6).get(abi::role::EGRESS), None);
    schedule(&store, 7, Change::Remove("egress-b".into())).unwrap();
    fold_to(&store, 7);
    assert_eq!(roles(&store, 7).get("later"), None);
    assert_eq!(programs(&store, 7), ["boot"]);
}

#[test]
fn no_change_leaves_a_required_role_unfilled_or_naming_a_program_that_does_not_run() {
    let (store, code) = founded();
    let identity = MockHost::role(abi::role::IDENTITY);
    for (what, change) in [
        (
            "its program published with no role",
            publish(&identity, code, &[]),
        ),
        ("its program removed", Change::Remove(identity.clone())),
    ] {
        let refused = schedule(&store, 5, change).unwrap_err();
        assert_eq!(refused.code, code::INVALID_INPUT, "{what}");
    }
    for (what, roles) in [
        (
            "a role named twice",
            vec![abi::role::EGRESS, abi::role::EGRESS],
        ),
        ("no role name", vec!["Not A Role"]),
    ] {
        let refused = schedule(&store, 5, publish("new", code, &roles)).unwrap_err();
        assert_eq!(refused.code, code::INVALID_INPUT, "{what}");
    }
    schedule(
        &store,
        5,
        publish("identity-b", code, &[abi::role::IDENTITY]),
    )
    .unwrap();
    schedule(&store, 6, Change::Remove(identity)).unwrap();
    assert_eq!(
        roles(&store, 6).get(abi::role::IDENTITY),
        Some(&"identity-b".to_owned())
    );
}

#[test]
fn an_id_lands_only_in_its_one_spelling() {
    let (store, code) = founded();
    let view = |name: &str| {
        Change::SetView(View {
            name: name.into(),
            view: code,
        })
    };
    for (name, class) in [
        ("System", "uppercase"),
        ("\u{0455}ystem", "a Cyrillic look-alike"),
        ("\u{202e}metsys", "a bidi control: reads `system`"),
    ] {
        for change in [Change::Set(entry(name, code)), view(name)] {
            assert_eq!(
                schedule(&store, 5, change).unwrap_err().code,
                code::INVALID_INPUT,
                "{class}"
            );
        }
    }
    schedule(&store, 5, Change::Set(entry("system", code))).unwrap();
    schedule(&store, 5, view("a-view_2")).unwrap();
    // an id that landed before the rule can still be removed
    let store = MockHost::default();
    crate::rules::init(
        &store.exec(env(0, Origin::Root)),
        genesis(vec![entry("System", code)], vec![], vec![]),
    );
    schedule(&store, 5, Change::Remove("System".into())).unwrap();
}

#[test]
fn the_kernel_asks_its_hooks_in_the_roles_bytes() {
    use abi::role::programs;
    for height in [0, 7, u64::MAX] {
        assert_eq!(
            abi::encode(&programs::Query::Hooks(height)),
            abi::encode(&Query::Hooks(height))
        );
        assert_eq!(
            abi::encode(&programs::Query::At(height)),
            abi::encode(&Query::At(height))
        );
    }
    let hooks = vec!["activity".to_owned()];
    assert_eq!(
        abi::encode(&programs::Reply::Hooks(hooks.clone())),
        abi::encode(&Reply::Hooks(hooks))
    );
    let entries = vec![entry("boot", BlobId::Sha256([1; 32]))];
    assert_eq!(
        abi::encode(&programs::Reply::Programs(entries.clone())),
        abi::encode(&Reply::Programs(entries))
    );
}

#[test]
fn a_hook_is_set_and_removed_at_its_height_and_goes_with_its_program() {
    let (store, code) = founded();
    assert_eq!(hooks(&store, 1), ["boot"]);
    assert_eq!(
        schedule(&store, 5, Change::SetHook("ghost".into()))
            .unwrap_err()
            .code,
        code::NOT_FOUND
    );
    assert_eq!(
        schedule(&store, 5, Change::RemoveHook("lens".into()))
            .unwrap_err()
            .code,
        code::NOT_FOUND
    );
    schedule(&store, 5, Change::Set(entry("activity", code))).unwrap();
    assert_eq!(
        schedule(&store, 5, Change::SetHook("activity".into()))
            .unwrap_err()
            .code,
        code::ALREADY_EXISTS
    );
    schedule(&store, 6, Change::SetHook("activity".into())).unwrap();
    schedule(&store, 7, Change::RemoveHook("boot".into())).unwrap();
    schedule(&store, 8, Change::Remove("activity".into())).unwrap();
    assert_eq!(hooks(&store, 5), ["boot"]);
    assert_eq!(hooks(&store, 6), ["activity", "boot"]);
    assert_eq!(hooks(&store, 7), ["activity"]);
    assert_eq!(hooks(&store, 8), Vec::<String>::new());
    fold_to(&store, 8);
    assert_eq!(hooks(&store, 8), Vec::<String>::new());
    schedule(&store, 9, Change::Set(entry("activity", code))).unwrap();
    assert_eq!(
        hooks(&store, 9),
        Vec::<String>::new(),
        "a program set again is no hook again"
    );
}

#[test]
fn a_schedule_is_refused_until_the_blobs_role_holds_its_code_as_a_program() {
    let (store, blobs) = founded_with_blobs();
    let (code, bundle) = put(&blobs, CODE_KIND, b"new wasm");
    let set = Change::Set(entry("new", code));
    let refused = store.refused(|| schedule(&store, 5, set.clone()));
    assert_eq!(refused.code, code::NOT_FOUND);
    assert!(refused.message.contains("not held"), "{refused:?}");
    receipted(&blobs, bundle);
    schedule(&store, 5, set).unwrap();
    assert_eq!(programs(&store, 5), ["boot", "new"]);

    let pack = held(&blobs, "pack", b"not code");
    let refused = store.refused(|| schedule(&store, 6, Change::Set(entry("pack", pack))));
    assert_eq!(refused.code, code::INVALID_INPUT);
}
