// A program renamed in place: the kernel moves its account to the new id, and Migrate rewrites the
// ids identity stores (scripts, whom an account answers) by the programs role's renames.

use ::program::Renamer;

use super::*;

fn register(store: &MockHost, program: &str) {
    let op = Op::RegisterProgram {
        program: program.into(),
    };
    run(store, &root(), op).unwrap();
}

fn rename(from: &str, to: &str) -> Op {
    Op::RenameProgram {
        from: from.into(),
        to: to.into(),
    }
}

fn of_program(store: &MockHost, program: &str) -> Option<u64> {
    let asked = Query::OfProgram {
        program: program.into(),
    };
    match query(store, asked) {
        Reply::Number(number) => number,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_system_moves_a_programs_account_to_its_new_id() {
    let store = memory();
    register(&store, "harness");
    let number = of_program(&store, "harness").unwrap();
    let a_key = env(Origin::Signed(ALICE.to_vec()), None, 100);
    let by_a_key = refused(&store, &a_key, rename("harness", "o/harness"));
    assert_eq!(by_a_key.code, code::UNAUTHORIZED);
    run(&store, &root(), rename("harness", "o/harness")).unwrap();
    assert_eq!(of_program(&store, "o/harness"), Some(number));
    assert_eq!(of_program(&store, "harness"), None);
    let account = get(&store, number);
    assert_eq!(
        account.control,
        Control::System {
            program: "o/harness".into()
        }
    );
    assert_eq!(account.card.name, "o/harness");
    run(&store, &root(), rename("harness", "o/harness")).unwrap();
    assert_eq!(of_program(&store, "o/harness"), Some(number), "a replay");
}

#[test]
fn a_rename_needs_an_account_to_move_and_a_free_new_id_and_keeps_a_name_set_since() {
    let store = memory();
    register(&store, "chat");
    register(&store, "files");
    let missing = refused(&store, &root(), rename("ghost", "o/ghost"));
    assert_eq!(missing.code, code::NOT_FOUND);
    let taken = refused(&store, &root(), rename("chat", "files"));
    assert_eq!(taken.code, code::ALREADY_EXISTS);
    let number = of_program(&store, "chat").unwrap();
    let named = Op::SetName {
        account: number,
        name: "Chat".into(),
    };
    run(&store, &root(), named).unwrap();
    run(&store, &root(), rename("chat", "o/chat")).unwrap();
    assert_eq!(get(&store, number).card.name, "Chat");
}

#[test]
fn migrate_rewrites_scripts_and_parties_by_the_programs_roles_renames() {
    let store = memory();
    let renamer = MockHost::default();
    store.sibling::<Renamer>(MockHost::role(abi::role::PROGRAMS), &renamer);
    let renames = vec![("harness".into(), "o/harness".into())];
    Renamer::execute(&renamer.exec(MockHost::env("programs")), renames).unwrap();
    create(&store, ALICE, "Alice");
    register(&store, "harness");
    let scout = create_agent(&store, ALICE, "Scout");
    let spare = create_agent(&store, ALICE, "Spare");
    for agent in [scout, spare] {
        run(
            &store,
            &signed(&store, ALICE),
            run_by(agent, Some("harness")),
        )
        .unwrap();
        let narrowed = Op::SetOwnership {
            account: agent,
            ownership: Ownership::Listed(vec![Party::Program("harness".into())]),
        };
        run(&store, &signed(&store, ALICE), narrowed).unwrap();
    }
    run(&store, &root(), rename("harness", "o/harness")).unwrap();
    let migrate = |first, last| Op::Migrate {
        height: 7,
        first,
        last,
    };
    let anyone = env(Origin::Signed(b"nobody".to_vec()), None, 100);
    let ahead = Op::Migrate {
        height: 8,
        first: None,
        last: None,
    };
    assert_eq!(refused(&store, &anyone, ahead).code, code::INVALID_INPUT);
    run(&store, &anyone, migrate(Some(scout), Some(scout))).unwrap();
    let calls = |agent| match get(&store, agent).control {
        Control::Program { script, .. } => script.map(|script| script.calls),
        other => panic!("{other:?}"),
    };
    let answers = |agent| match query(&store, Query::Answers { account: agent }) {
        Reply::Answers(answers) => answers,
        other => panic!("{other:?}"),
    };
    let answers_program = |agent, program: &str| {
        let party = Party::Program(program.into());
        matches!(answers(agent), Some(Ownership::Listed(parties)) if parties.contains(&party))
    };
    assert_eq!(calls(scout), Some("o/harness".into()));
    assert!(answers_program(scout, "o/harness"));
    assert!(!answers_program(scout, "harness"));
    assert_eq!(calls(spare), Some("harness".into()), "outside the range");
    run(&store, &anyone, migrate(None, None)).unwrap();
    run(&store, &anyone, migrate(None, None)).unwrap();
    assert_eq!(calls(spare), Some("o/harness".into()));
    assert!(answers_program(spare, "o/harness"));
    assert_eq!(runs(&store, "o/harness", scout).unwrap(), Some(scout));
}

#[test]
fn every_op_and_query_the_kernel_sends_meets_identitys_own_at_one_tag() {
    use abi::role::identity as role;
    let kernel_sends = |op: &role::Op| match op {
        role::Op::RegisterProgram { .. } | role::Op::RenameProgram { .. } => {}
    };
    let kernel_asks = |query: &role::Query| match query {
        role::Query::Account(_)
        | role::Query::OfProgram(_)
        | role::Query::Profile(_)
        | role::Query::Profiles { .. }
        | role::Query::Runs { .. } => {}
    };
    let ops = [
        (
            role::Op::RegisterProgram {
                program: "p".into(),
            },
            Op::RegisterProgram {
                program: "p".into(),
            },
        ),
        (
            role::Op::RenameProgram {
                from: "p".into(),
                to: "o/p".into(),
            },
            rename("p", "o/p"),
        ),
    ];
    for (sent, own) in ops {
        kernel_sends(&sent);
        assert_eq!(abi::decode::<Op>(&abi::encode(&sent)).unwrap(), own);
        assert_eq!(abi::decode::<role::Op>(&abi::encode(&own)).unwrap(), sent);
    }
    let queries = [
        (
            role::Query::Account(b"k".to_vec()),
            Query::OfKey { key: b"k".to_vec() },
        ),
        (
            role::Query::OfProgram("p".into()),
            Query::OfProgram {
                program: "p".into(),
            },
        ),
        (role::Query::Profile(7), Query::Profile { number: 7 }),
        (
            role::Query::Profiles {
                after: Some(3),
                limit: 9,
            },
            Query::Profiles {
                after: Some(3),
                limit: 9,
            },
        ),
        (
            role::Query::Runs {
                program: "p".into(),
                account: 7,
            },
            Query::Runs {
                program: "p".into(),
                account: 7,
            },
        ),
    ];
    for (asked, own) in queries {
        kernel_asks(&asked);
        assert_eq!(abi::decode::<Query>(&abi::encode(&asked)).unwrap(), own);
        assert_eq!(
            abi::decode::<role::Query>(&abi::encode(&own)).unwrap(),
            asked
        );
    }
    let replies = [
        (role::Reply::Account(Some(7)), Reply::Number(Some(7))),
        (role::Reply::Profile(None), Reply::Profile(None)),
        (
            role::Reply::Profiles {
                profiles: Vec::new(),
                next: Some(3),
            },
            Reply::Profiles {
                profiles: Vec::new(),
                next: Some(3),
            },
        ),
    ];
    for (read, own) in replies {
        assert_eq!(abi::decode::<Reply>(&abi::encode(&read)).unwrap(), own);
        assert_eq!(
            abi::decode::<role::Reply>(&abi::encode(&own)).unwrap(),
            read
        );
    }
}
