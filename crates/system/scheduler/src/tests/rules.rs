use abi::Receipt;
use program::code;

use super::*;
use crate::Op;

fn by(program: &str) -> Origin {
    Origin::Program(program.into())
}

fn schedule(height: u64) -> Op {
    Op::Schedule {
        height,
        target: "mark".into(),
        payload: abi::encode(&Mark::Note("k".into())),
    }
}

fn receipt() -> Receipt {
    Receipt {
        program: "mark".into(),
        outcome: abi::Outcome::Applied { output: Vec::new() },
        events: Vec::new(),
        nested: Vec::new(),
    }
}

fn due(
    chain: &MockChain,
    height: u64,
    after: Option<EntryId>,
    limit: Option<u32>,
) -> (Vec<EntryId>, Option<EntryId>) {
    let asked = Query::Due {
        height,
        after,
        limit,
    };
    match chain.query(PROGRAM, &asked).unwrap() {
        Reply::Due { entries, next } => (entries.iter().map(|entry| entry.id).collect(), next),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_program_schedules_as_itself_and_gets_the_entry_id() {
    let chain = chain(true);
    let first = chain.submit(by("planner"), PROGRAM, &schedule(3)).unwrap();
    let second = chain.submit(by("other"), PROGRAM, &schedule(3)).unwrap();
    assert_eq!(abi::decode::<EntryId>(&first).unwrap(), 0);
    assert_eq!(abi::decode::<EntryId>(&second).unwrap(), 1);
    let kept = entry(&chain, 1).expect("the entry is kept");
    assert_eq!(
        (
            kept.scheduler.as_str(),
            kept.height,
            kept.target.as_str(),
            kept.receipt
        ),
        ("other", 3, "mark", None)
    );
}

#[test]
fn only_a_program_schedules_and_only_for_a_later_height() {
    let chain = chain(true);
    let signed = chain.submit(Origin::Signed(KEY.to_vec()), PROGRAM, &schedule(3));
    assert_eq!(signed.unwrap_err().code, code::UNAUTHORIZED);
    let root = chain.submit(Origin::Root, PROGRAM, &schedule(3));
    assert_eq!(root.unwrap_err().code, code::UNAUTHORIZED);
    for height in [0, 1] {
        let refused = chain.submit(by("planner"), PROGRAM, &schedule(height));
        assert_eq!(refused.unwrap_err().code, code::INVALID_INPUT, "{height}");
    }
    assert_eq!(entry(&chain, 0), None);
}

#[test]
fn only_the_scheduling_program_cancels_and_only_before_the_run() {
    let chain = chain(true);
    chain.submit(by("planner"), PROGRAM, &schedule(3)).unwrap();
    chain.submit(by("planner"), PROGRAM, &schedule(3)).unwrap();
    let cancel = |entry| Op::Cancel { entry };
    let theirs = chain.submit(by("other"), PROGRAM, &cancel(0));
    assert_eq!(theirs.unwrap_err().code, code::UNAUTHORIZED);
    let signed = chain.submit(Origin::Signed(KEY.to_vec()), PROGRAM, &cancel(0));
    assert_eq!(signed.unwrap_err().code, code::UNAUTHORIZED);
    chain.submit(by("planner"), PROGRAM, &cancel(0)).unwrap();
    assert_eq!(entry(&chain, 0), None);
    assert_eq!(due(&chain, 3, None, None).0, [1]);
    let gone = chain.submit(by("planner"), PROGRAM, &cancel(0));
    assert_eq!(gone.unwrap_err().code, code::NOT_FOUND);
    let ran = Op::Ran {
        entry: 1,
        receipt: receipt(),
    };
    chain.submit(Origin::Root, PROGRAM, &ran).unwrap();
    let late = chain.submit(by("planner"), PROGRAM, &cancel(1));
    assert_eq!(late.unwrap_err().code, code::WRONG_STATE);
}

#[test]
fn only_the_kernel_says_an_entry_ran_and_the_entry_keeps_the_receipt() {
    let chain = chain(true);
    chain.submit(by("planner"), PROGRAM, &schedule(3)).unwrap();
    let ran = Op::Ran {
        entry: 0,
        receipt: receipt(),
    };
    let from_planner = chain.submit(by("planner"), PROGRAM, &ran);
    assert_eq!(from_planner.unwrap_err().code, code::UNAUTHORIZED);
    chain.submit(Origin::Root, PROGRAM, &ran).unwrap();
    assert_eq!(entry(&chain, 0).unwrap().receipt, Some(receipt()));
    assert_eq!(due(&chain, 3, None, None), (Vec::new(), None));
    let again = chain.submit(Origin::Root, PROGRAM, &ran);
    assert_eq!(again.unwrap_err().code, code::WRONG_STATE);
    let missing = Op::Ran {
        entry: 9,
        receipt: receipt(),
    };
    let unknown = chain.submit(Origin::Root, PROGRAM, &missing);
    assert_eq!(unknown.unwrap_err().code, code::NOT_FOUND);
}

#[test]
fn due_pages_one_height_in_the_order_entries_were_made_and_no_limit_is_every_row() {
    let chain = chain(true);
    for height in [5, 4, 5, 5] {
        chain
            .submit(by("planner"), PROGRAM, &schedule(height))
            .unwrap();
    }
    assert_eq!(due(&chain, 5, None, None), (vec![0, 2, 3], None));
    assert_eq!(due(&chain, 4, None, None), (vec![1], None));
    assert_eq!(due(&chain, 5, None, Some(2)), (vec![0, 2], Some(2)));
    assert_eq!(due(&chain, 5, Some(2), Some(2)), (vec![3], None));
    assert_eq!(due(&chain, 6, None, None), (Vec::new(), None));
}

#[test]
fn migrate_moves_the_ids_of_entries_not_run_and_leaves_a_run_entry_as_it_ran() {
    let mut chain = chain(true);
    chain.seat::<program::Renamer>("programs");
    for height in [3, 3, 4] {
        chain
            .submit(by("planner"), PROGRAM, &schedule(height))
            .unwrap();
    }
    let ran = Op::Ran {
        entry: 0,
        receipt: receipt(),
    };
    chain.submit(Origin::Root, PROGRAM, &ran).unwrap();
    let renames = vec![
        ("planner".to_owned(), "o/planner".to_owned()),
        ("mark".to_owned(), "o/mark".to_owned()),
    ];
    chain.submit(by("planner"), "programs", &renames).unwrap();
    chain.at(2, 0);
    let migrate = |first, last| Op::Migrate {
        height: 2,
        first,
        last,
    };
    chain
        .submit(by("other"), PROGRAM, &migrate(Some((4, 2)), None))
        .unwrap();
    let ids = |id| {
        let kept = entry(&chain, id).unwrap();
        (kept.scheduler, kept.target)
    };
    assert_eq!(ids(1), ("planner".into(), "mark".into()));
    assert_eq!(ids(2), ("o/planner".into(), "o/mark".into()));
    chain
        .submit(by("other"), PROGRAM, &migrate(None, None))
        .unwrap();
    assert_eq!(ids(1), ("o/planner".into(), "o/mark".into()));
    assert_eq!(
        ids(0),
        ("planner".into(), "mark".into()),
        "it ran as it was"
    );
    assert_eq!(due(&chain, 3, None, None).0, [1]);
    let before = chain.host(PROGRAM).borrow().state.clone();
    chain
        .submit(by("other"), PROGRAM, &migrate(None, None))
        .unwrap();
    assert_eq!(
        chain.host(PROGRAM).borrow().state,
        before,
        "a second run writes nothing"
    );
    let theirs = chain.submit(by("planner"), PROGRAM, &Op::Cancel { entry: 1 });
    assert_eq!(theirs.unwrap_err().code, code::UNAUTHORIZED);
    chain
        .submit(by("o/planner"), PROGRAM, &Op::Cancel { entry: 1 })
        .unwrap();
}

#[test]
fn every_op_the_kernel_and_programs_send_meets_the_schedulers_own_at_one_tag() {
    use abi::role::scheduler as role;
    let in_the_role = |op: &role::Op| match op {
        role::Op::Schedule { .. } | role::Op::Cancel { .. } | role::Op::Ran { .. } => {}
    };
    let ops = [
        (
            role::Op::Schedule {
                height: 3,
                target: "mark".into(),
                payload: b"p".to_vec(),
            },
            Op::Schedule {
                height: 3,
                target: "mark".into(),
                payload: b"p".to_vec(),
            },
        ),
        (role::Op::Cancel { entry: 1 }, Op::Cancel { entry: 1 }),
        (
            role::Op::Ran {
                entry: 1,
                receipt: receipt(),
            },
            Op::Ran {
                entry: 1,
                receipt: receipt(),
            },
        ),
    ];
    for (sent, own) in ops {
        in_the_role(&sent);
        assert_eq!(abi::decode::<Op>(&abi::encode(&sent)).unwrap(), own);
        assert_eq!(abi::decode::<role::Op>(&abi::encode(&own)).unwrap(), sent);
    }
}
