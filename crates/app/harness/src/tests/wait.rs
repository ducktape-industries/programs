use super::schedule::{answered, refused, schemed, schemed_as, turn};
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

fn asking(chain: &MockChain) -> usize {
    assigned(chain, SERVICE).len()
}

fn wait_row(chain: &MockChain, wait: u64) -> Option<WaitRow> {
    let Reply::Wait(found) = chain.query(&harness(), &Query::Wait { wait }).unwrap() else {
        panic!("harness answers a wait")
    };
    found
}

fn armed(chain: &MockChain, thread: u64) -> Vec<u64> {
    let asked = Query::Armed {
        thread,
        page: PageRequest::default(),
    };
    let Reply::Waits(page) = chain.query(&harness(), &asked).unwrap() else {
        panic!("harness answers waits")
    };
    page.items.iter().map(|row| row.id).collect()
}

fn height(height: u64) -> Value {
    json!({ "event": { "height": height } })
}

fn pod(name: &str) -> Value {
    json!({ "event": { "pod": { "name": name } } })
}

#[test]
fn a_wait_for_a_height_ends_the_turn_and_wakes_its_conversation_once_at_its_block() {
    waits_for_a_height(schemed());
}

#[test]
fn a_wait_for_a_height_wakes_its_conversation_under_any_id_harness_is_seated_at() {
    waits_for_a_height(schemed_as(ELSEWHERE));
}

fn waits_for_a_height(world: World) {
    let chain = &world.chain;
    let thread = world.opened("m1", "wake me at block 10", Some(WINDOW));
    turn(&world, 3, &[("w1", "wait_for", height(10))]);
    let wait = waited(chain, thread, "w1");
    assert_eq!(asking(chain), 0, "the turn that armed a wait ends");
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);
    assert_eq!(armed(chain, thread), [wait]);
    assert!(wakes(chain, thread).is_empty());

    chain.at(9, 0);
    assert!(wakes(chain, thread).is_empty(), "nothing wakes before it");
    chain.at(11, 0);
    let [wake] = wakes(chain, thread).try_into().expect("one wake");
    assert_eq!(asking(chain), 1, "the wake asks the model");
    let ended = wait_row(chain, wait).unwrap().ended.expect("it is over");
    assert!(ended.height >= 10, "{ended:?}");
    let words = format!("Wait {wait} is over: block {} has come.", ended.height);
    assert_eq!(wake, words);
    assert!(armed(chain, thread).is_empty());

    chain.at(12, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Up." }]),
        "end_turn",
    );
    chain.at(40, 0);
    assert_eq!(wakes(chain, thread).len(), 1, "it wakes once");
    assert_eq!(asking(chain), 0);
}

#[test]
fn a_wait_whose_conversation_closed_ends_there_as_a_notice_and_wakes_no_other() {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "wake me at block 20", Some(WINDOW));
    turn(&world, 3, &[("w1", "wait_for", height(20))]);
    let wait = waited(chain, thread, "w1");
    chain
        .submit(signed(ADA), PROGRAM, &Op::Close { thread })
        .unwrap();
    chain.at(6, 0);
    let fresh = world.mention("m2", "a fresh start");
    assert_ne!(fresh, thread);
    chain.at(21, 0);
    assert!(wakes(chain, fresh).is_empty(), "{:?}", wakes(chain, fresh));
    assert!(wakes(chain, thread).is_empty());
    let noticed = frames(chain, thread).into_iter().any(|body| match body {
        Body::Notice { text } => text.starts_with(&format!(
            "Not delivered, as this conversation is closed: Wait {wait} is over: block"
        )),
        _ => false,
    });
    assert!(noticed, "{:?}", frames(chain, thread));
}

#[test]
fn a_refused_wait_does_not_end_the_turn() {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "wake me in the past", Some(WINDOW));
    turn(&world, 3, &[("w1", "wait_for", height(2))]);
    refused(chain, thread, "w1");
    assert_eq!(asking(chain), 1, "the model hears the refusal");
    assert!(armed(chain, thread).is_empty());
}

#[test]
fn a_message_wakes_a_waiting_conversation_and_names_the_wait_which_stays() {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "wake me at block 20", Some(WINDOW));
    turn(&world, 3, &[("w1", "wait_for", height(20))]);
    let wait = waited(chain, thread, "w1");
    assert_eq!(asking(chain), 0);

    chain.at(6, 0);
    world.mention("m2", "still there?");
    assert_eq!(asking(chain), 1, "a message wakes it at once");
    let (request, _) = open_request(chain);
    let sent = body(chain, &request).to_string();
    let named = format!("(This conversation still waits: wait {wait} for block 20.)");
    assert!(sent.contains(&named), "{sent}");
    chain.at(7, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Here." }]),
        "end_turn",
    );
    assert_eq!(armed(chain, thread), [wait], "the wait stays armed");

    chain.at(21, 0);
    let [wake] = wakes(chain, thread).try_into().expect("one wake");
    assert!(wake.starts_with(&format!("Wait {wait} is over")), "{wake}");
}

#[test]
fn the_agent_lists_its_waits_and_cancels_one_which_then_never_wakes() {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "wait twice", Some(WINDOW));
    turn(&world, 3, &[("w1", "wait_for", height(20))]);
    let first = waited(chain, thread, "w1");
    chain.at(5, 0);
    world.mention("m2", "and cancel it");
    turn(
        &world,
        6,
        &[
            ("l1", "wait_list", json!({})),
            ("c1", "wait_cancel", json!({ "wait": first })),
        ],
    );
    let listed = answered(chain, thread, "l1");
    let rows = listed["waits"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["wait"], first);
    assert_eq!(rows[0]["for"], "block 20");
    assert_eq!(rows[0]["ended"], Value::Null);
    assert_eq!(answered(chain, thread, "c1"), json!({ "ok": true }));
    assert_eq!(wait_row(chain, first), None);
    assert!(armed(chain, thread).is_empty());

    chain.at(30, 0);
    assert!(
        wakes(chain, thread).is_empty(),
        "a cancelled wait never wakes"
    );
}

#[test]
fn only_the_waiting_agent_waits_in_its_conversation_or_cancels_and_only_harness_makes_one_due() {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "wait", Some(WINDOW));
    turn(&world, 3, &[("w1", "wait_for", height(20))]);
    let wait = waited(chain, thread, "w1");
    let mine = Op::Wait {
        thread,
        event: WaitFor::Height { height: 30 },
    };
    let error = chain.submit(signed(ADA), PROGRAM, &mine).unwrap_err();
    assert_eq!(error.code, code::INVALID_INPUT, "{error:?}");
    let unwait = Op::Unwait { wait };
    let error = chain.submit(signed(ADA), PROGRAM, &unwait).unwrap_err();
    assert_eq!(error.code, code::UNAUTHORIZED, "{error:?}");
    for origin in [signed(ADA), Origin::Program(PROGRAM.into())] {
        let error = chain.submit(origin, PROGRAM, &Op::WaitDue).unwrap_err();
        assert_eq!(error.code, code::UNAUTHORIZED, "{error:?}");
    }
    let event = egress::Event {
        id: 0,
        seq: 0,
        data: b"event: ended\ndata: {}".to_vec(),
    };
    let forged = Op::PodEvent { wait, event };
    let error = chain.submit(signed(ADA), PROGRAM, &forged).unwrap_err();
    assert_eq!(error.code, code::UNAUTHORIZED, "{error:?}");
    assert_eq!(armed(chain, thread), [wait]);
}

#[test]
fn an_agent_hands_harness_no_wait_event_itself() {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "forge a wake", Some(WINDOW));
    let event = json!({ "PodEvent": {
        "wait": 1,
        "event": { "id": 0, "seq": 0, "data": "" },
    } });
    let calls = [
        (
            "f1",
            "network_call",
            json!({ "program": PROGRAM, "op": "WaitDue" }),
        ),
        (
            "f2",
            "network_call",
            json!({ "program": PROGRAM, "op": event }),
        ),
    ];
    turn(&world, 3, &calls);
    for call in ["f1", "f2"] {
        let text = refused(chain, thread, call);
        let refusal = text.starts_with(code::UNAUTHORIZED) && text.contains("does not send");
        assert!(refusal, "{text}");
    }
}

pub(super) fn pods(world: &World) {
    let chain = &world.chain;
    let register = egress::Op::Register {
        service: crate::waits::PODS.into(),
    };
    let assign = egress::Op::Assign {
        service: crate::waits::PODS.into(),
        nodes: vec![NODE.to_vec()],
    };
    for op in [register, assign] {
        chain.submit(signed(ADA), egress::PROGRAM, &op).unwrap();
    }
    let serve = egress::Op::Serve {
        services: vec![SERVICE.into(), RELAY.into(), crate::waits::PODS.into()],
    };
    chain
        .submit(signed(&NODE), egress::PROGRAM, &serve)
        .unwrap();
}

fn pod_request(chain: &MockChain) -> (RequestRow, String) {
    let [row] = assigned(chain, crate::waits::PODS)
        .try_into()
        .expect("one request to the pods service open");
    let Request::Http { head, .. } = &row.request else {
        panic!("the pods service is asked over http")
    };
    let target = head.target.clone();
    (row, target)
}

fn pod_event(chain: &MockChain, row: &RequestRow, seq: u64, data: &str) {
    let event = egress::Event {
        id: row.id,
        seq,
        data: data.as_bytes().to_vec(),
    };
    let events = egress::Op::Events(vec![event]);
    chain
        .submit(signed(&NODE), egress::PROGRAM, &events)
        .unwrap();
}

pub(super) fn stream_ends(chain: &MockChain, row: &RequestRow, events: u64) {
    let ended = egress::Op::EndStream {
        id: row.id,
        events,
        end: egress::StreamEnd::Complete {
            status: 200,
            headers: vec![("content-type".into(), "text/event-stream".into())],
        },
    };
    chain
        .submit(signed(&NODE), egress::PROGRAM, &ended)
        .unwrap();
}

const ENDED: &str = "event: ended\ndata: {\"line\": \"pod duckpond-7 ended: exit code 0 (Completed)\", \"exit_code\": 0, \"pod\": \"duckpond-7\"}";

fn streaming(world: &World, thread: u64) -> (u64, RequestRow) {
    let chain = &world.chain;
    turn(world, 3, &[("w1", "wait_for", pod("duckpond-7"))]);
    let wait = waited(chain, thread, "w1");
    assert_eq!(asking(chain), 0, "the turn that armed a wait ends");
    assert!(assigned(chain, crate::waits::PODS).is_empty());
    chain.at(5, 1000);
    chain.at(6, 2000);
    chain.at(7, 3000);
    let (read, target) = pod_request(chain);
    assert_eq!(target, "/v1/pods/duckpond-7");
    answer(chain, read.id, Answer::Output(response(200, json!({}))));
    let (stream, target) = pod_request(chain);
    assert_eq!(target, "/v1/pods/duckpond-7/wait?every=32");
    let pod_event = egress::Delivery::Events {
        deliver: wake::pod_event_as(1),
    };
    assert_eq!(stream.delivery, pod_event, "wake follows wait {wait}'s pod");
    chain.at(8, 4000);
    (wait, stream)
}

#[test]
fn a_wait_for_a_pod_hears_its_stream_and_wakes_once_when_it_ends() {
    let world = schemed();
    pods(&world);
    let chain = &world.chain;
    let thread = world.opened("m1", "wait for my pod", Some(WINDOW));
    let (wait, stream) = streaming(&world, thread);
    pod_event(chain, &stream, 0, "event: alive\ndata: {}");
    assert!(wakes(chain, thread).is_empty(), "alive wakes nothing");
    pod_event(chain, &stream, 1, ENDED);
    let [wake] = wakes(chain, thread).try_into().expect("one wake");
    let line = "pod duckpond-7 ended: exit code 0 (Completed)";
    assert_eq!(wake, format!("Wait {wait} is over: {line}."));
    assert_eq!(asking(chain), 1, "the wake asks the model");
    stream_ends(chain, &stream, 2);
    chain.at(20, 4000);
    assert_eq!(wakes(chain, thread).len(), 1, "it wakes once");
    assert!(assigned(chain, crate::waits::PODS).is_empty());
}

#[test]
fn a_pod_stream_that_ends_without_its_end_opens_again_and_an_unknown_pod_ends_the_wait() {
    let world = schemed();
    pods(&world);
    let chain = &world.chain;
    let thread = world.opened("m1", "wait for my pod", Some(WINDOW));
    let (wait, stream) = streaming(&world, thread);
    stream_ends(chain, &stream, 0);
    assert!(wakes(chain, thread).is_empty());
    assert_eq!(armed(chain, thread), [wait], "the wait stays");
    chain.at(9, 5000);
    chain.at(10, 6000);
    let (read, target) = pod_request(chain);
    assert_eq!(target, "/v1/pods/duckpond-7", "the pod is read again");
    let gone = json!({ "error": "no pod duckpond-7" });
    answer(chain, read.id, Answer::Output(response(404, gone)));
    let [wake] = wakes(chain, thread).try_into().expect("one wake");
    let words = format!("Wait {wait} is over: the pods service knows no pod duckpond-7.");
    assert_eq!(wake, words);
}

#[test]
fn the_pod_event_prefix_decodes_as_a_pod_event() {
    let event = egress::Event {
        id: 3,
        seq: 2,
        data: b"event: alive".to_vec(),
    };
    assert_eq!(
        abi::decode::<Op>(&event.payload(&pod_event_as(9))).unwrap(),
        Op::PodEvent { wait: 9, event }
    );
}

fn channel(world: &World, id: &str) {
    let create = chat::Op::CreateChannel {
        channel_id: id.into(),
        name: id.into(),
        post_policy: chat::PostPolicy::Open,
    };
    world
        .chain
        .submit(signed(ADA), chat::PROGRAM, &create)
        .unwrap();
}

fn watches(chain: &MockChain, agent: AccountNumber, channel: &str) -> bool {
    let asked = chat::Query::Watching {
        account: agent,
        page: PageRequest::default(),
    };
    let chat::Reply::Watching(page) = chain.query(chat::PROGRAM, &asked).unwrap() else {
        panic!("chat answers what an account watches")
    };
    page.items.iter().any(|watched| watched == channel)
}

fn cut(chain: &MockChain, agent: AccountNumber, channel: &str) -> Option<chat::Notices> {
    let asked = chat::Query::Chosen {
        account: agent,
        channel_id: channel.into(),
    };
    let chat::Reply::Chosen { listening, notices } = chain.query(chat::PROGRAM, &asked).unwrap()
    else {
        panic!("chat answers what an account chose")
    };
    assert!(!listening, "a wait never listens");
    notices
}

fn message(channel: &str, rest: Value) -> Value {
    let mut wanted = json!({ "channel": channel });
    if let (Some(wanted), Some(rest)) = (wanted.as_object_mut(), rest.as_object()) {
        wanted.extend(rest.clone());
    }
    json!({ "event": { "message": wanted } })
}

fn ended_turn(chain: &MockChain, at: u64) {
    chain.at(at, 0);
    let words = json!([{ "type": "text", "text": "Seen." }]);
    model_says(chain, words, "end_turn");
}

#[test]
fn a_wait_for_a_message_wakes_once_on_the_first_that_matches_whoever_writes_it() {
    let world = schemed();
    let chain = &world.chain;
    channel(&world, "random");
    let thread = world.opened("m1", "wake me when Bo writes in random", Some(WINDOW));
    let wanted = message("random", json!({ "from": world.bo }));
    turn(&world, 3, &[("w1", "wait_for", wanted)]);
    let wait = waited(chain, thread, "w1");
    assert_eq!(asking(chain), 0, "the turn that armed a wait ends");
    assert!(watches(chain, world.agent, "random"), "chat tells harness");

    chain.at(5, 0);
    world.post(BO, "general", "g1", "elsewhere");
    world.post(ADA, "random", "r1", "not Bo");
    assert!(wakes(chain, thread).is_empty(), "nothing else ends it");
    assert_eq!(
        asking(chain),
        0,
        "a channel only a wait listens to asks nothing"
    );
    world.post(BO, "random", "r2", "Bo is here");
    let [wake] = wakes(chain, thread).try_into().expect("one wake");
    let opens = format!(
        "Wait {wait} is over: Bo (account {}) wrote in chat channel random, message",
        world.bo
    );
    assert!(wake.starts_with(&opens), "{wake}");
    assert!(wake.ends_with("Bo is here"), "{wake}");
    assert_eq!(
        asking(chain),
        1,
        "a person the agent doesn't answer still ends it"
    );
    assert!(
        !watches(chain, world.agent, "random"),
        "the wait's watching ends with it"
    );

    ended_turn(chain, 6);
    chain.at(7, 0);
    world.post(BO, "random", "r3", "again");
    assert_eq!(wakes(chain, thread).len(), 1, "it wakes once");
    assert_eq!(asking(chain), 0);
}

#[test]
fn a_wait_for_a_message_to_the_agent_is_ended_by_a_mention_heard_once() {
    let world = schemed();
    let chain = &world.chain;
    channel(&world, "random");
    let thread = world.opened("m1", "wake me when someone asks me in random", Some(WINDOW));
    let wanted = message("random", json!({ "mentions_me": true }));
    turn(&world, 3, &[("w1", "wait_for", wanted)]);
    let wait = waited(chain, thread, "w1");

    chain.at(5, 0);
    world.post(ADA, "random", "r1", "just talk");
    assert!(wakes(chain, thread).is_empty());
    let asks = format!("<@{}> look here", world.agent);
    world.post(ADA, "random", "r2", &asks);
    let [wake] = wakes(chain, thread).try_into().expect("one wake");
    assert!(
        wake.starts_with(&format!(
            "Wait {wait} is over: Ada (account {}) mentioned you",
            world.ada
        )),
        "{wake}"
    );
    let heard = frames(chain, thread)
        .into_iter()
        .chain(held(chain, thread))
        .filter(|body| body.text().contains("look here"))
        .count();
    assert_eq!(heard, 1, "the mention is heard once, as the wake");
    assert_eq!(asking(chain), 1);
}

#[test]
fn a_cancelled_message_wait_stops_watching_and_an_unknown_channel_is_refused() {
    let world = schemed();
    let chain = &world.chain;
    channel(&world, "random");
    let thread = world.opened("m1", "wait in random", Some(WINDOW));
    let calls = [
        ("w1", "wait_for", message("random", json!({}))),
        ("w2", "wait_for", message("nowhere", json!({}))),
    ];
    turn(&world, 3, &calls);
    let wait = waited(chain, thread, "w1");
    let text = refused(chain, thread, "w2");
    assert!(text.contains("chat holds no channel nowhere"), "{text}");
    assert!(watches(chain, world.agent, "random"));

    chain.at(5, 0);
    world.mention("m2", "never mind");
    turn(&world, 6, &[("c1", "wait_cancel", json!({ "wait": wait }))]);
    assert_eq!(answered(chain, thread, "c1"), json!({ "ok": true }));
    assert!(!watches(chain, world.agent, "random"));
    ended_turn(chain, 8);
    chain.at(9, 0);
    world.post(BO, "random", "r1", "hello?");
    assert!(wakes(chain, thread).is_empty());
    assert_eq!(asking(chain), 0);
}

#[test]
fn an_agent_neither_hands_harness_a_forge_event_nor_moves_its_forge_watcher() {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "forge a wake", Some(WINDOW));
    let closed = json!({ "Forged": { "Closed": { "repo": "notes", "n": 1, "by": { "Account": world.ada } } } });
    let calls = [
        (
            "f1",
            "network_call",
            json!({ "program": PROGRAM, "op": closed }),
        ),
        (
            "f2",
            "network_call",
            json!({ "program": "forge", "op": { "Watch": { "deliver": "" } } }),
        ),
    ];
    turn(&world, 3, &calls);
    for call in ["f1", "f2"] {
        let text = refused(chain, thread, call);
        let refusal = text.starts_with(code::UNAUTHORIZED) && text.contains("does not send");
        assert!(refusal, "{text}");
    }
    let event = forge::Watched::Closed {
        repo: "notes".into(),
        n: 1,
        by: Principal::Account(world.ada),
    };
    let error = chain
        .submit(signed(ADA), PROGRAM, &Op::Forged(event))
        .unwrap_err();
    assert_eq!(error.code, code::UNAUTHORIZED, "{error:?}");
}

#[test]
fn the_forge_prefix_decodes_as_forged() {
    let event = forge::Watched::Closed {
        repo: "notes".into(),
        n: 4,
        by: Principal::Account(9),
    };
    let mut delivered = forged_as();
    delivered.extend(abi::encode(&event));
    assert_eq!(abi::decode::<Op>(&delivered).unwrap(), Op::Forged(event));
}

#[test]
fn a_wait_in_a_room_turned_off_ends_with_its_message_and_nothing_else_arrives() {
    let world = schemed();
    let chain = &world.chain;
    channel(&world, "random");
    let off = chat::Op::ChooseForMany {
        accounts: vec![world.agent],
        channel_id: "random".into(),
        listening: false,
        notices: Some(chat::Notices::Off),
    };
    chain.submit(signed(ADA), chat::PROGRAM, &off).unwrap();
    let thread = world.opened("m1", "wake me when Bo writes in random", Some(WINDOW));
    let wanted = message("random", json!({ "from": world.bo }));
    turn(&world, 3, &[("w1", "wait_for", wanted)]);
    let wait = waited(chain, thread, "w1");
    assert!(watches(chain, world.agent, "random"));
    assert_eq!(cut(chain, world.agent, "random"), Some(chat::Notices::Off));

    chain.at(5, 0);
    let early = format!("<@{}> are you there?", world.agent);
    world.post(ADA, "random", "r1", &early);
    assert!(wakes(chain, thread).is_empty());
    assert_eq!(asking(chain), 0, "an off room asks nothing");
    world.post(BO, "random", "r2", "Bo is here");
    let [wake] = wakes(chain, thread).try_into().expect("one wake");
    assert!(wake.starts_with(&format!("Wait {wait} is over")), "{wake}");
    assert!(wake.ends_with("Bo is here"), "{wake}");
    assert!(!watches(chain, world.agent, "random"));
    assert_eq!(cut(chain, world.agent, "random"), Some(chat::Notices::Off));

    ended_turn(chain, 6);
    chain.at(7, 0);
    let late = format!("<@{}> and now?", world.agent);
    world.post(ADA, "random", "r3", &late);
    let leaked = frames(chain, thread)
        .into_iter()
        .chain(held(chain, thread))
        .filter(|body| body.text().contains("are you there") || body.text().contains("and now"))
        .count();
    assert_eq!(leaked, 0, "nothing else of the room arrives");
    assert_eq!(world.threads().len(), 1);
    assert_eq!(asking(chain), 0);
}

#[test]
fn wait_list_names_this_conversations_open_waits_and_every_wait_with_all() {
    let world = schemed();
    let chain = &world.chain;
    let first = world.opened("m1", "wait here", Some(WINDOW));
    turn(&world, 3, &[("w1", "wait_for", height(40))]);
    let elsewhere = waited(chain, first, "w1");
    chain
        .submit(signed(ADA), PROGRAM, &Op::Close { thread: first })
        .unwrap();
    chain.at(6, 0);
    let here = world.mention("m2", "a fresh start");
    assert_ne!(here, first);
    turn(&world, 7, &[("w2", "wait_for", height(12))]);
    let over = waited(chain, here, "w2");
    chain.at(13, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Up." }]),
        "end_turn",
    );
    chain.at(14, 0);
    world.mention("m3", "what do you wait for?");
    turn(&world, 15, &[("w3", "wait_for", height(50))]);
    let open = waited(chain, here, "w3");
    chain.at(17, 0);
    world.mention("m4", "list them");
    turn(
        &world,
        18,
        &[
            ("l1", "wait_list", json!({})),
            ("l2", "wait_list", json!({ "all": true })),
        ],
    );
    let listed = |id: &str| -> Vec<Value> {
        answered(chain, here, id)["waits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["wait"].clone())
            .collect()
    };
    assert_eq!(
        listed("l1"),
        [json!(open)],
        "this conversation's waits not over yet"
    );
    assert_eq!(
        listed("l2"),
        [json!(elsewhere), json!(over), json!(open)],
        "every wait the agent made, in every conversation, over ones included"
    );
}
