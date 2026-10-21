//! Waits as wakes: a wait for anything but a chat message is a wake harness arms as the agent under
//! the wait's own number; the wake's end ends the wait in the conversation that armed it, and
//! cancelling the wait cancels the wake.
use super::schedule::{answered, refused, turn};
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

fn says(text: &str) -> Value {
    json!([{ "type": "text", "text": text }])
}

fn idle(world: &World) -> u64 {
    let thread = world.opened("m1", "hello", Some(WINDOW));
    world.chain.at(3, 0);
    model_says(&world.chain, says("Noted."), "end_turn");
    thread
}

fn wait_row(chain: &MockChain, wait: u64) -> Option<WaitRow> {
    let Reply::Wait(found) = chain.query(&harness(), &Query::Wait { wait }).unwrap() else {
        panic!("harness answers a wait")
    };
    found
}

fn wakes_of_thread(chain: &MockChain, thread: u64) -> Vec<Wake> {
    let Reply::Wakes(wakes) = chain.query(&harness(), &Query::Wakes { thread }).unwrap() else {
        panic!("harness answers wakes")
    };
    wakes
}

fn legacy(world: &World, thread: u64, event: WaitFor) -> u64 {
    let chain = &world.chain;
    let host = chain.host(&harness());
    let ctx = host.exec(chain.env(harness()));
    let id = crate::state::WAIT_COUNT.get(&ctx).unwrap().unwrap_or(0) + 1;
    crate::state::WAIT_COUNT.put(&ctx, &id);
    let row = WaitRow {
        id,
        agent: world.agent,
        thread,
        made: ctx.env().height,
        event,
        ended: None,
    };
    crate::state::WAITS.put(&ctx, &id, &row);
    crate::state::WAITING.insert(&ctx, &(world.agent, id));
    crate::state::ARMED.insert(&ctx, &(thread, id));
    id
}

fn due_at(world: &World, wait: u64, height: u64) {
    let chain = &world.chain;
    let schedule = abi::role::scheduler::Op::Schedule {
        height,
        target: harness(),
        payload: abi::encode(&Op::WaitDue),
    };
    let by_harness = Origin::Program(harness());
    let made = chain
        .submit(by_harness, scheduler::PROGRAM, &schedule)
        .unwrap();
    let entry: u64 = abi::decode(&made).unwrap();
    let host = chain.host(&harness());
    let ctx = host.exec(chain.env(harness()));
    crate::state::WAIT_ENTRIES.put(&ctx, &entry, &wait);
    crate::state::SCHEDULED.put(&ctx, &wait, &entry);
}

fn scheduled(chain: &MockChain, wait: u64) -> Option<u64> {
    let host = chain.host(&harness());
    let ctx = host.exec(chain.env(harness()));
    crate::state::SCHEDULED.get(&ctx, &wait).unwrap()
}

fn move_waits(chain: &MockChain) -> Option<Vec<u8>> {
    let op = Op::MoveWaits {
        after: None,
        limit: 10,
    };
    abi::decode(&chain.submit(signed(ADA), harness(), &op).unwrap()).unwrap()
}

#[test]
fn move_waits_puts_a_wait_harness_watched_on_a_wake_under_its_own_number_and_leaves_message_waits()
{
    let world = waking();
    let chain = &world.chain;
    let thread = idle(&world);
    let height = legacy(&world, thread, WaitFor::Height { height: 20 });
    due_at(&world, height, 20);
    let message = WaitFor::Message {
        channel: "general".into(),
        from: None,
        mentions_me: false,
    };
    let message = legacy(&world, thread, message);
    let before = (wait_row(chain, height), wakes_of_thread(chain, thread));
    assert!(scheduled(chain, height).is_some());

    assert!(
        move_waits(chain).is_some(),
        "it books itself while a wait may be left"
    );
    chain.at(6, 0);
    let row = only_wake(chain, world.agent);
    assert_eq!(row.event, wake::WaitFor::Height { height: 20 });
    assert_eq!(row.ended, None);
    assert_eq!(
        (wait_row(chain, height), wakes_of_thread(chain, thread)),
        before,
        "Wait and Wakes answer as before"
    );
    assert_eq!(
        scheduled(chain, height),
        None,
        "harness's own entry is gone"
    );
    let kept = wait_row(chain, message).expect("the message wait stays");
    assert_eq!(kept.ended, None);

    chain.at(21, 0);
    let told = wakes(chain, thread);
    let [over] = &told[..] else {
        panic!("{told:?}")
    };
    let words = format!("Wait {height} is over: block 20 has come.");
    assert!(over.starts_with(&words), "{over}");
    chain.at(40, 0);
    assert_eq!(wakes(chain, thread).len(), 1, "it wakes once");
}

#[test]
fn a_wait_wake_refuses_to_watch_ends_at_once_with_its_reason() {
    let world = waking();
    let chain = &world.chain;
    let thread = idle(&world);
    let pod = legacy(
        &world,
        thread,
        WaitFor::Pod {
            name: "Not A Pod".into(),
        },
    );
    assert_eq!(move_waits(chain), Some(crate::state::WAITS.key(&pod)));
    let told = wakes(chain, thread);
    let [over] = &told[..] else {
        panic!("{told:?}")
    };
    let words = format!("Wait {pod} is over: wake refused to watch it: ");
    assert!(over.starts_with(&words), "{over}");
    assert!(wakes_of(chain, world.agent).is_empty());
    assert!(wait_row(chain, pod).and_then(|row| row.ended).is_some());
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
fn a_moved_file_wait_ends_when_a_folder_above_its_path_is_removed() {
    let world = waking();
    let chain = &world.chain;
    commit(chain, ADA, "/shared/tree/leaf", "x");
    let thread = idle(&world);
    let path = "/shared/tree/leaf";
    let file = legacy(&world, thread, WaitFor::File { path: path.into() });
    assert!(move_waits(chain).is_some());
    let row = only_wake(chain, world.agent);
    assert_eq!(row.event, wake::WaitFor::File { path: path.into() });
    chain.at(5, 0);
    super::wait_file::remove(chain, ADA, "/shared/tree");
    chain.at(6, 0);
    let told = wakes(chain, thread);
    let [over] = &told[..] else {
        panic!("{told:?}")
    };
    let opens = format!("Wait {file} is over: {path} was removed in files snapshot");
    assert!(over.starts_with(&opens), "{over}");
}

#[test]
fn a_legacy_pod_read_answered_after_move_waits_ends_nothing() {
    let world = waking();
    super::wait::pods(&world);
    let chain = &world.chain;
    let thread = idle(&world);
    chain.at(4, 0);
    let name = "duckpond-7";
    let pod = legacy(&world, thread, WaitFor::Pod { name: name.into() });
    {
        let host = chain.host(&harness());
        let ctx = host.exec(chain.env(harness()));
        let watch = crate::state::PodWatch {
            height: 4,
            time: 0,
            entry: None,
        };
        crate::state::POD_WAITS.put(&ctx, &pod, &watch);
    }
    chain
        .submit(signed(ADA), harness(), &Op::PodWait { wait: pod })
        .unwrap();
    chain.at(5, 1_000);
    chain.at(6, 2_000);
    chain.at(7, 3_000);
    let reads = assigned(chain, crate::waits::PODS);
    let [read] = &reads[..] else {
        panic!(
            "harness's own read of the pod is in flight: {}",
            reads.len()
        )
    };
    assert!(move_waits(chain).is_some());
    chain.at(8, 4_000);
    let down = json!({ "error": "the pods service is down" });
    answer(chain, read.id, Answer::Output(response(500, down)));
    chain.at(9, 5_000);
    assert!(
        wakes(chain, thread).is_empty(),
        "{:?}",
        wakes(chain, thread)
    );
    assert_eq!(wait_row(chain, pod).and_then(|row| row.ended), None);
    assert_eq!(only_wake(chain, world.agent).ended, None);
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

fn pod_watch(chain: &MockChain, wait: u64) -> Option<crate::state::PodWatch> {
    let host = chain.host(&harness());
    let ctx = host.exec(chain.env(harness()));
    crate::state::POD_WAITS.get(&ctx, &wait).unwrap()
}

#[test]
fn a_legacy_pod_stream_cut_after_move_waits_rearms_nothing() {
    let world = waking();
    super::wait::pods(&world);
    let chain = &world.chain;
    let thread = idle(&world);
    chain.at(4, 0);
    let pod = legacy(
        &world,
        thread,
        WaitFor::Pod {
            name: "duckpond-7".into(),
        },
    );
    {
        let host = chain.host(&harness());
        let ctx = host.exec(chain.env(harness()));
        let watch = crate::state::PodWatch {
            height: 4,
            time: 0,
            entry: None,
        };
        crate::state::POD_WAITS.put(&ctx, &pod, &watch);
    }
    chain
        .submit(signed(ADA), harness(), &Op::PodWait { wait: pod })
        .unwrap();
    chain.at(5, 1_000);
    chain.at(6, 2_000);
    chain.at(7, 3_000);
    let reads = assigned(chain, crate::waits::PODS);
    let [read] = &reads[..] else {
        panic!("harness's own read of the pod: {}", reads.len())
    };
    chain.at(8, 4_000);
    answer(chain, read.id, Answer::Output(response(200, json!({}))));
    let deliver = egress::Delivery::Events {
        deliver: pod_event_as(pod),
    };
    let streams: Vec<RequestRow> = assigned(chain, crate::waits::PODS)
        .into_iter()
        .filter(|row| row.delivery == deliver)
        .collect();
    let [stream] = &streams[..] else {
        panic!("harness's own stream of the pod: {}", streams.len())
    };
    assert!(move_waits(chain).is_some());
    assert_eq!(
        pod_watch(chain, pod),
        None,
        "the move drops the legacy watch"
    );
    chain.at(9, 5_000);
    super::wait::stream_ends(chain, stream, 0);
    chain.at(10, 6_000);
    assert_eq!(
        pod_watch(chain, pod),
        None,
        "the cut stream re-arms no legacy watch"
    );
    assert!(
        wakes(chain, thread).is_empty(),
        "{:?}",
        wakes(chain, thread)
    );
    assert_eq!(only_wake(chain, world.agent).ended, None);
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
