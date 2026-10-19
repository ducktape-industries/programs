//! What will wake a conversation again: its turn while it runs, nothing once the turn ended with
//! nothing armed, and the wait it armed until the wait is over.
use super::schedule::{answered, schemed, turn};
use super::*;

fn wakes_of(chain: &MockChain, thread: u64) -> Vec<Wake> {
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
