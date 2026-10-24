use super::schedule::{answered, schemed, turn};
use super::*;
use crate::promises::{ASKING, ENTRIES, PROMISES, PROMISING, PromiseRow};

fn seed(chain: &MockChain, id: u64, agent: AccountNumber, asker: AccountNumber, thread: u64) {
    let row = PromiseRow {
        id,
        owed_by: agent,
        asked_by: asker,
        thread,
        channel: "general".into(),
        seq: 1,
        text: "legacy unfinished work".into(),
        made: 1,
        due: 40,
        next: 40,
        closed: None,
    };
    let host = chain.host(PROGRAM);
    let mut host = host.borrow_mut();
    host.state.insert(PROMISES.key(&id), abi::encode(&row));
    host.state.insert(PROMISING.key(&(agent, id)), Vec::new());
    host.state.insert(ASKING.key(&(asker, id)), Vec::new());
}

fn retire(chain: &MockChain, after: Option<u64>, limit: u64) -> Option<u64> {
    abi::decode(
        &chain
            .submit(signed(ADA), PROGRAM, &Op::RetirePromises { after, limit })
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn retirement_pages_only_legacy_ids_and_never_marks_work_completed() {
    let world = world();
    let chain = &world.chain;
    for id in [1, 2, 3] {
        seed(chain, id, world.agent, world.ada, 1);
    }
    let host = chain.host(PROGRAM);
    let untouched = host
        .borrow()
        .state
        .get(&PROMISES.key(&3u64))
        .cloned()
        .unwrap();
    assert_eq!(retire(chain, None, 1), Some(1));
    assert!(!host.borrow().state.contains_key(&PROMISES.key(&1u64)));
    assert!(
        !host
            .borrow()
            .state
            .contains_key(&PROMISING.key(&(world.agent, 1u64)))
    );
    assert!(
        !host
            .borrow()
            .state
            .contains_key(&ASKING.key(&(world.ada, 1u64)))
    );
    assert_eq!(
        host.borrow().state.get(&PROMISES.key(&3u64)),
        Some(&untouched)
    );
    assert_eq!(retire(chain, Some(1), 1), Some(2));
    assert_eq!(retire(chain, Some(2), 1), None);
    assert_eq!(retire(chain, None, 1), None);
    let error = chain
        .submit(
            signed(ADA),
            PROGRAM,
            &Op::RetirePromises {
                after: None,
                limit: 0,
            },
        )
        .unwrap_err();
    assert_eq!(error.code, code::INVALID_INPUT);
}

#[test]
fn old_promise_ops_and_queries_are_vacant_while_surviving_tags_stay_put() {
    for tag in [35, 36, 37] {
        let mut bytes = vec![tag];
        bytes.extend([0; 100]);
        assert!(abi::decode::<Op>(&bytes).is_err());
    }
    for tag in [39, 40, 41] {
        let mut bytes = vec![tag];
        bytes.extend([0; 100]);
        assert!(abi::decode::<Query>(&bytes).is_err());
    }
    assert_eq!(abi::encode(&Op::Sweep), [38]);
    assert_eq!(abi::encode(&Op::Renew { thread: 0 })[0], 39);
    assert_eq!(abi::encode(&Query::ThreadCard { thread: 0 })[0], 42);
    for tool in ["promise", "promise_keep", "promise_list"] {
        assert!(crate::tools::find(tool).is_none());
    }
}

#[test]
fn retirement_cancels_indexed_callbacks_but_preserves_bookings_and_event_waits() {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "ordinary wake", Some(WINDOW));
    let post = json!({ "post_message": {
        "channel_id": "general", "message_id": "ordinary-scheduled",
        "blocks": [{ "paragraph": [{ "text": "ordinary wake", "marks": [] }] }], "thread": null
    }});
    turn(
        &world,
        3,
        &[
            (
                "book",
                "schedule_run",
                json!({ "height": 40, "program": "chat", "op": post }),
            ),
            ("wait", "wait_for", json!({ "event": { "height": 50 } })),
        ],
    );
    let booking = answered(chain, thread, "book")["entry"].as_u64().unwrap();
    let wait = answered(chain, thread, "wait")["wait"].as_u64().unwrap();
    seed(chain, 1, world.agent, world.ada, thread);
    let mut callback = vec![37];
    callback.extend(1u64.to_le_bytes());
    let schedule = scheduler::Op::Schedule {
        height: 30,
        target: PROGRAM.into(),
        payload: callback.clone(),
    };
    let entry: u64 = abi::decode(
        &chain
            .submit(
                Origin::Program(PROGRAM.into()),
                scheduler::PROGRAM,
                &schedule,
            )
            .unwrap(),
    )
    .unwrap();
    chain
        .host(PROGRAM)
        .borrow_mut()
        .state
        .insert(ENTRIES.key(&1u64), abi::encode(&entry));
    assert_eq!(retire(chain, None, 1), None);
    let scheduler::Reply::Entry(found) = chain
        .query(scheduler::PROGRAM, &scheduler::Query::Entry { id: entry })
        .unwrap()
    else {
        panic!("scheduler entry reply")
    };
    assert!(found.is_none());
    assert!(
        !chain
            .host(PROGRAM)
            .borrow()
            .state
            .contains_key(&ENTRIES.key(&1u64))
    );
    let Reply::Booking(Some(row)) = chain
        .query(PROGRAM, &Query::Booking { entry: booking })
        .unwrap()
    else {
        panic!("ordinary booking remains")
    };
    assert_eq!(row.ran, None);
    let Reply::Wait(Some(row)) = chain.query(PROGRAM, &Query::Wait { wait }).unwrap() else {
        panic!("ordinary wait remains")
    };
    assert_eq!(row.ended, None);
    chain.at(41, 0);
    let Reply::Booking(Some(row)) = chain
        .query(PROGRAM, &Query::Booking { entry: booking })
        .unwrap()
    else {
        panic!("ordinary booking runs")
    };
    assert!(row.ran.is_some());
    chain.at(51, 0);
    let Reply::Wait(Some(row)) = chain.query(PROGRAM, &Query::Wait { wait }).unwrap() else {
        panic!("ordinary wait fires")
    };
    assert!(row.ended.is_some());
    chain.at(4000, 0);
    assert!(
        !chain
            .host(PROGRAM)
            .borrow()
            .state
            .keys()
            .any(|key| key.starts_with(b"promise-entry/"))
    );
    assert!(abi::decode::<Op>(&callback).is_err());
}

#[test]
fn old_callback_cannot_rebook_and_retirement_handles_consumed_and_orphan_entries() {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "unfinished work stays unfinished", Some(WINDOW));
    seed(chain, 1, world.agent, world.ada, thread);
    let mut callback = vec![37];
    callback.extend(1u64.to_le_bytes());
    let schedule = scheduler::Op::Schedule {
        height: 30,
        target: PROGRAM.into(),
        payload: callback,
    };
    let scheduled = || -> u64 {
        abi::decode(
            &chain
                .submit(
                    Origin::Program(PROGRAM.into()),
                    scheduler::PROGRAM,
                    &schedule,
                )
                .unwrap(),
        )
        .unwrap()
    };
    let consumed = scheduled();
    let cancelled = scheduled();
    chain
        .submit(
            Origin::Program(PROGRAM.into()),
            scheduler::PROGRAM,
            &scheduler::Op::Cancel { entry: cancelled },
        )
        .unwrap();
    let pending = scheduled();
    let host = chain.host(PROGRAM);
    for (id, entry) in [(1u64, consumed), (2, cancelled), (3, pending)] {
        host.borrow_mut()
            .state
            .insert(ENTRIES.key(&id), abi::encode(&entry));
    }
    chain.at(31, 0);
    assert_eq!(
        host.borrow().state.get(&ENTRIES.key(&1u64)),
        Some(&abi::encode(&consumed))
    );
    chain.at(4000, 0);
    assert_eq!(
        host.borrow().state.get(&ENTRIES.key(&1u64)),
        Some(&abi::encode(&consumed))
    );
    let scheduler::Reply::Entry(Some(row)) = chain
        .query(
            scheduler::PROGRAM,
            &scheduler::Query::Entry { id: consumed },
        )
        .unwrap()
    else {
        panic!("old callback receipt")
    };
    assert!(row.receipt.is_some());
    let row: PromiseRow =
        abi::decode(host.borrow().state.get(&PROMISES.key(&1u64)).unwrap()).unwrap();
    assert!(row.closed.is_none());
    assert_eq!(retire(chain, None, 1), Some(1));
    assert_eq!(retire(chain, Some(1), 1), Some(2));
    assert_eq!(retire(chain, Some(2), 1), None);
    assert!(
        !host
            .borrow()
            .state
            .keys()
            .any(|key| key.starts_with(b"promise-entry/") || key.starts_with(b"promise/"))
    );
}

#[test]
fn legacy_rows_are_not_reinjected_after_compaction() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "remember the plan", Some(2_000));
    seed(chain, 1, world.agent, world.ada, thread);
    chain.at(3, 0);
    model_says_after(
        chain,
        json!([{ "type": "text", "text": "Noted." }]),
        "end_turn",
        1000,
    );
    world.mention("m2", "and now?");
    chain.at(4, 0);
    summarized(chain, "Ada set out the plan.");
    let sent = json!(sent_messages(chain)).to_string();
    assert!(!sent.contains("Your open promises:"));
    assert!(!sent.contains("legacy unfinished work"));
    assert!(
        chain
            .host(PROGRAM)
            .borrow()
            .state
            .contains_key(&PROMISES.key(&1u64))
    );
}
