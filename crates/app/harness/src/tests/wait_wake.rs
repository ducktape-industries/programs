//! Waits as wakes: a wait for anything but a chat message is a wake harness arms as the agent under
//! the wait's own number; the wake's end ends the wait in the conversation that armed it, and
//! cancelling the wait cancels the wake.
use super::schedule::{answered, asked_after_a_long_wait, refused, turn, turn_timed};
use super::woken::waking;
use super::*;

fn waited(chain: &MockChain, thread: u64, call: &str) -> u64 {
    answered(chain, thread, call)["wait"].as_u64().unwrap()
}

fn wakes(chain: &MockChain, thread: u64) -> Vec<String> {
    frames(chain, thread)
        .into_iter()
        .chain(held(chain, thread))
        .filter_map(|body| match body {
            Body::Post {
                author: Principal::Root,
                text,
            } => Some(text),
            _ => None,
        })
        .collect()
}

fn wakes_of(chain: &MockChain, account: AccountNumber) -> Vec<wake::WakeRow> {
    let asked = wake::Query::Wakes {
        account,
        page: PageRequest::default(),
    };
    let wake::Reply::Wakes(page) = chain.query(wake::PROGRAM, &asked).unwrap() else {
        panic!("wake answers an account's wakes")
    };
    page.items
}

fn only_wake(chain: &MockChain, account: AccountNumber) -> wake::WakeRow {
    let armed = wakes_of(chain, account);
    let [row] = &armed[..] else {
        panic!("one wake armed as the agent: {armed:?}")
    };
    row.clone()
}

fn height(height: u64) -> Value {
    json!({ "event": { "height": height } })
}

#[test]
fn a_wait_for_a_height_is_a_wake_armed_as_its_agent_and_ends_as_the_wait_where_it_was_armed() {
    let world = waking();
    let chain = &world.chain;
    let thread = world.opened("m1", "wake me at block 10", Some(WINDOW));
    turn(&world, 3, &[("w1", "wait_for", height(10))]);
    let wait = waited(chain, thread, "w1");
    let row = only_wake(chain, world.agent);
    assert_eq!(row.event, wake::WaitFor::Height { height: 10 });
    assert_eq!(row.ended, None);
    chain.at(9, 0);
    assert!(wakes(chain, thread).is_empty(), "nothing wakes before it");
    chain.at(11, 0);
    let words = format!("Wait {wait} is over: block 10 has come.");
    assert_eq!(wakes(chain, thread), [words]);
    let ended = only_wake(chain, world.agent)
        .ended
        .expect("the wake is over");
    assert_eq!(ended.height, 10);
}

#[test]
fn cancelling_a_wait_cancels_its_wake() {
    let world = waking();
    let chain = &world.chain;
    let thread = world.opened("m1", "wait, then cancel", Some(WINDOW));
    turn(&world, 3, &[("w1", "wait_for", height(20))]);
    let wait = waited(chain, thread, "w1");
    only_wake(chain, world.agent);
    chain.at(5, 0);
    world.mention("m2", "cancel it");
    turn(&world, 6, &[("c1", "wait_cancel", json!({ "wait": wait }))]);
    assert_eq!(answered(chain, thread, "c1"), json!({ "ok": true }));
    assert!(
        wakes_of(chain, world.agent).is_empty(),
        "a cancelled wake is gone"
    );
    chain.at(30, 0);
    assert!(wakes(chain, thread).is_empty());
}

fn wait_row(chain: &MockChain, wait: u64) -> Option<WaitRow> {
    let Reply::Wait(found) = chain.query(&harness(), &Query::Wait { wait }).unwrap() else {
        panic!("harness answers a wait")
    };
    found
}

#[test]
fn a_wake_the_agent_cancels_itself_drops_the_wait_it_backed() {
    let world = waking();
    let chain = &world.chain;
    let thread = world.opened("m1", "wait, then cancel the wake", Some(WINDOW));
    turn(&world, 3, &[("w1", "wait_for", height(20))]);
    let wait = waited(chain, thread, "w1");
    let id = only_wake(chain, world.agent).id;
    chain.at(5, 0);
    world.mention("m2", "cancel the wake itself");
    let cancel = json!({ "program": wake::PROGRAM, "op": { "Cancel": { "id": id } } });
    turn(&world, 6, &[("c1", "network_call", cancel)]);
    let (text, failed) = result_of(chain, thread, "c1");
    assert!(!failed, "{text}");
    assert_eq!(wait_row(chain, wait), None, "the wait it backed is gone");
    chain.at(30, 0);
    assert!(wakes(chain, thread).is_empty());
}

#[test]
fn a_wait_for_a_place_already_clear_ends_inside_its_arming_in_the_conversation_that_armed_it() {
    let world = waking();
    let chain = &world.chain;
    let thread = world.opened("m1", "tell me when general is clear", Some(WINDOW));
    let clear = json!({ "event": { "clear": { "channel": "general" } } });
    turn(&world, 3, &[("w1", "wait_for", clear)]);
    let wait = waited(chain, thread, "w1");
    let told = wakes(chain, thread);
    let [over] = &told[..] else {
        panic!("{told:?}")
    };
    let words = format!("Wait {wait} is over: #general is clear.");
    assert!(over.starts_with(&words), "{over}");
    assert!(wait_row(chain, wait).and_then(|row| row.ended).is_some());
    let host = chain.host(&harness());
    let ctx = host.exec(chain.env(harness()));
    assert!(
        crate::state::ARMING
            .scan(&ctx, crate::state::ARMING.prefix_of(&world.agent))
            .unwrap()
            .is_empty()
    );
    assert!(
        crate::state::UNCLAIMED
            .scan(&ctx, crate::state::UNCLAIMED.prefix_of(&world.agent))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_booked_run_that_cancels_a_waits_wake_drops_the_wait() {
    let world = waking();
    let chain = &world.chain;
    let thread = world.opened("m1", "wait, then book the wake's cancel", Some(WINDOW));
    turn(&world, 3, &[("w1", "wait_for", height(20))]);
    let wait = waited(chain, thread, "w1");
    let id = only_wake(chain, world.agent).id;
    chain.at(5, 0);
    world.mention("m2", "book it");
    let cancel = json!({ "Cancel": { "id": id } });
    let run = json!({ "height": 8, "program": wake::PROGRAM, "op": cancel });
    turn(&world, 6, &[("b1", "schedule_run", run)]);
    chain.at(9, 0);
    assert!(
        wakes_of(chain, world.agent).is_empty(),
        "the booked run cancelled the wake"
    );
    assert_eq!(wait_row(chain, wait), None, "the wait it backed is gone");
}

fn wake_kept(chain: &MockChain, id: u64, row: Option<Vec<u8>>) {
    let host = chain.host(wake::PROGRAM);
    let ctx = host.exec(chain.env(wake::PROGRAM));
    let mut key = b"wake/".to_vec();
    key.extend(id.to_be_bytes());
    match row {
        Some(bytes) => ctx.set(key, bytes),
        None => ctx.delete(key),
    }
}

fn cancelled_with(world: &World, kept: impl Fn(&wake::WakeRow) -> Option<Vec<u8>>) -> (u64, u64) {
    let chain = &world.chain;
    let thread = world.opened("m1", "wait, then cancel", Some(WINDOW));
    turn(world, 3, &[("w1", "wait_for", height(20))]);
    let wait = waited(chain, thread, "w1");
    let row = only_wake(chain, world.agent);
    wake_kept(chain, row.id, kept(&row));
    chain.at(5, 0);
    world.mention("m2", "cancel it");
    turn(world, 6, &[("c1", "wait_cancel", json!({ "wait": wait }))]);
    (thread, wait)
}

#[test]
fn wait_cancel_after_wake_dropped_its_wake_drops_the_wait() {
    let world = waking();
    let chain = &world.chain;
    let (thread, wait) = cancelled_with(&world, |_| None);
    assert_eq!(answered(chain, thread, "c1"), json!({ "ok": true }));
    assert_eq!(wait_row(chain, wait), None);
}

#[test]
fn wait_cancel_after_wake_ended_its_wake_untold_ends_the_wait_with_wakes_words() {
    let world = waking();
    let chain = &world.chain;
    let (thread, wait) = cancelled_with(&world, |row| {
        let ended = wake::WakeRow {
            ended: Some(wake::Ended {
                height: 4,
                text: "block 4 has come; not told: refused".into(),
            }),
            ..row.clone()
        };
        Some(abi::encode(&ended))
    });
    answered(chain, thread, "c1");
    let over = wait_row(chain, wait)
        .and_then(|row| row.ended)
        .expect("the wait is over, not dropped");
    assert_eq!(over.text, "block 4 has come; not told: refused");
    let words = format!("Wait {wait} is over: block 4 has come; not told: refused.");
    let told = wakes(chain, thread);
    assert!(told.iter().any(|said| said.starts_with(&words)), "{told:?}");
}

#[test]
fn wait_cancel_keeps_the_wait_when_its_wake_cannot_be_read() {
    let world = waking();
    let chain = &world.chain;
    let (thread, wait) = cancelled_with(&world, |_| Some(b"not a wake row".to_vec()));
    let text = refused(chain, thread, "c1");
    assert!(text.contains(wake::PROGRAM), "{text}");
    let kept = wait_row(chain, wait).expect("the wait stays");
    assert_eq!(kept.ended, None);
}

#[test]
fn a_wait_in_blocks_counts_them_from_the_block_its_call_runs_in_after_a_long_wait() {
    let world = waking();
    let chain = &world.chain;
    let thread = world.opened("m1", "wake me in 5 blocks", Some(WINDOW));
    asked_after_a_long_wait(chain);
    let ahead = json!({ "event": { "in": 5 } });
    turn_timed(&world, 15, 1_110_000, &[("w1", "wait_for", ahead)]);
    let wait = waited(chain, thread, "w1");
    let made = wait_row(chain, wait).expect("the wait is armed").made;
    assert!(made > 8, "the call ran after the model's wait: {made}");
    let row = only_wake(chain, world.agent);
    assert_eq!(row.event, wake::WaitFor::Height { height: made + 5 });
}
