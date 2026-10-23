//! What will wake a conversation again: its turn while it runs, nothing once the turn ended with
//! nothing armed, and the wait it armed until the wait is over.
use super::schedule::{answered, schemed, turn};
use super::*;

pub(super) fn wakes_of(chain: &MockChain, thread: u64) -> Vec<Wake> {
    let Reply::Wakes(wakes) = chain.query(PROGRAM, &Query::Wakes { thread }).unwrap() else {
        panic!("harness answers wakes")
    };
    wakes
}

#[test]
fn a_conversation_wakes_for_its_turn_and_its_wait_and_for_nothing_once_both_are_over() {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    let asking = wakes_of(chain, thread);
    assert!(asking.contains(&Wake::Turn), "{asking:?}");

    chain.at(3, 0);
    let said = json!([{ "type": "text", "text": "Hi." }]);
    model_says(chain, said.clone(), "end_turn");
    let idle = wakes_of(chain, thread);
    assert!(idle.is_empty(), "nothing will wake it: {idle:?}");

    chain.at(5, 0);
    world.mention("m2", "wake me at block 20");
    let wanted = json!({ "event": { "height": 20 } });
    turn(&world, 6, &[("w1", "wait_for", wanted)]);
    let wait = answered(chain, thread, "w1")["wait"].as_u64().unwrap();
    assert_eq!(wakes_of(chain, thread), [Wake::Wait { wait }]);

    chain.at(21, 0);
    let woken = wakes_of(chain, thread);
    assert!(woken.contains(&Wake::Turn), "{woken:?}");
    assert!(!woken.contains(&Wake::Wait { wait }), "{woken:?}");
    chain.at(22, 0);
    model_says(chain, said, "end_turn");
    let idle = wakes_of(chain, thread);
    assert!(idle.is_empty(), "{idle:?}");
}

#[test]
fn wakes_for_no_conversation_is_not_found() {
    let world = schemed();
    let error = world
        .chain
        .query::<_, Reply>(PROGRAM, &Query::Wakes { thread: 404 })
        .unwrap_err();
    assert_eq!(error.code, code::NOT_FOUND, "{error:?}");
}

#[test]
fn a_pending_booking_wakes_the_bound_heir_but_not_an_unrelated_conversation() {
    let world = schemed();
    let chain = &world.chain;
    let canonical = world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Ready." }]),
        "end_turn",
    );
    let open = Op::OpenThread {
        agent: world.agent,
        place: Place::Channel {
            channel: "general".into(),
        },
    };
    let predecessor: u64 =
        abi::decode(&chain.submit(signed(ADA), PROGRAM, &open).unwrap()).unwrap();
    chain.at(5, 0);
    world.mention("m2", "book a run");
    let post = json!({ "post_message": {
        "channel_id": "general", "message_id": "booked-post",
        "blocks": [{ "paragraph": [{ "text": "ready", "marks": [] }] }],
        "thread": null
    } });
    let book = json!({ "height": 30, "program": "chat", "op": post });
    turn(&world, 6, &[("b1", "schedule_run", book)]);
    let entry = answered(chain, predecessor, "b1")["entry"]
        .as_u64()
        .unwrap();
    let pending = Wake::Booking { entry };
    assert!(wakes_of(chain, predecessor).contains(&pending));
    assert!(!wakes_of(chain, canonical).contains(&pending));
    chain.at(8, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Booked." }]),
        "end_turn",
    );
    chain
        .submit(
            signed(ADA),
            PROGRAM,
            &Op::Close {
                thread: predecessor,
            },
        )
        .unwrap();
    assert!(!wakes_of(chain, predecessor).contains(&pending));
    let heir: u64 = abi::decode(&chain.submit(signed(ADA), PROGRAM, &open).unwrap()).unwrap();
    assert!(wakes_of(chain, heir).contains(&pending));
    assert!(!wakes_of(chain, predecessor).contains(&pending));
    assert!(!wakes_of(chain, canonical).contains(&pending));
    chain.at(31, 0);
    assert!(!wakes_of(chain, heir).contains(&pending));
}

#[test]
fn an_unnamed_booking_wakes_the_current_conversation_and_never_a_closed_one() {
    let world = schemed();
    let chain = &world.chain;
    let canonical = world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Ready." }]),
        "end_turn",
    );
    let open = Op::OpenThread {
        agent: world.agent,
        place: Place::Channel {
            channel: "general".into(),
        },
    };
    let bound: u64 = abi::decode(&chain.submit(signed(ADA), PROGRAM, &open).unwrap()).unwrap();
    chain.at(5, 0);
    world.mention("m2", "book a run for no conversation");
    let resume = abi::hex(&abi::encode(&Op::Resume { thread: canonical }));
    let book = json!({ "program": PROGRAM, "op": { "book": {
        "height": 30, "target": PROGRAM, "payload": resume, "thread": null
    } } });
    turn(&world, 6, &[("b1", "network_call", book)]);
    let returned = answered(chain, bound, "b1")["returned"]
        .as_str()
        .unwrap()
        .to_owned();
    let entry: u64 = abi::decode(&design::unhex(&returned).unwrap()).unwrap();
    let pending = Wake::Booking { entry };
    assert!(wakes_of(chain, canonical).contains(&pending));
    assert!(!wakes_of(chain, bound).contains(&pending));
    chain.at(8, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Booked." }]),
        "end_turn",
    );
    chain
        .submit(signed(ADA), PROGRAM, &Op::Close { thread: canonical })
        .unwrap();
    assert!(!wakes_of(chain, canonical).contains(&pending));
    assert!(!wakes_of(chain, bound).contains(&pending));
    let new = Op::NewThread { agent: world.agent };
    let fresh: u64 = abi::decode(&chain.submit(signed(ADA), PROGRAM, &new).unwrap()).unwrap();
    assert!(wakes_of(chain, fresh).contains(&pending));
    assert!(!wakes_of(chain, canonical).contains(&pending));
    assert!(!wakes_of(chain, bound).contains(&pending));
    chain.at(31, 0);
    assert!(!wakes_of(chain, fresh).contains(&pending));
}
