//! A wake's end, heard: the conversation whose turn armed a wake hears it is over and turns at
//! once; one closed since is told it was not delivered, and a wake harness never saw armed (a
//! booked run's) reaches the canonical conversation.
use ::program::schema::interface;
use programs::Programs;

use super::*;
use crate::schema::tests::{code_with, listed, module};

fn codes(harness_code: Vec<u8>) -> Vec<(&'static str, Vec<u8>)> {
    vec![
        (chat::PROGRAM, code_with(&interface::<chat::Chat>())),
        (files::PROGRAM, code_with(&interface::<files::Files>())),
        (PROGRAM, harness_code),
        (
            scheduler::PROGRAM,
            code_with(&interface::<scheduler::Scheduler>()),
        ),
        (egress::PROGRAM, code_with(&interface::<Egress>())),
        (
            attribution::PROGRAM,
            code_with(&interface::<attribution::Attribution>()),
        ),
        (awaits::PROGRAM, code_with(&interface::<awaits::Awaits>())),
        (wake::PROGRAM, code_with(&interface::<wake::Wake>())),
        ("valset", module(&[])),
    ]
}

pub(super) fn waking() -> World {
    let mut world = world();
    world.chain.seat::<Programs>(programs::PROGRAM);
    listed(&world.chain, &codes(code_with(&interface::<Harness>())));
    world
}

fn says(text: &str) -> Value {
    json!([{ "type": "text", "text": text }])
}

fn ends_for(chain: &MockChain, thread: u64, text: &str) {
    let deliver = egress::Delivery::Events {
        deliver: streamed_as(thread),
    };
    let row = assigned(chain, SERVICE)
        .into_iter()
        .find(|row| row.delivery == deliver)
        .expect("the conversation asks");
    answer(
        chain,
        row.id,
        Answer::Output(reply(says(text), "end_turn", 10)),
    );
}

fn canonical(world: &World) -> u64 {
    let thread = world.opened("m1", "hello", Some(WINDOW));
    world.chain.at(3, 0);
    model_says(&world.chain, says("Noted."), "end_turn");
    thread
}

fn bound(world: &World) -> u64 {
    let room = chat::Op::CreateChannel {
        channel_id: "ops".into(),
        name: "ops".into(),
        post_policy: chat::PostPolicy::Open,
    };
    world
        .chain
        .submit(signed(ADA), chat::PROGRAM, &room)
        .unwrap();
    let open = Op::OpenThread {
        agent: world.agent,
        place: Place::Channel {
            channel: "ops".into(),
        },
    };
    let made = world.chain.submit(signed(ADA), PROGRAM, &open).unwrap();
    let ask = format!("<@{}> wake yourself at block 10", world.agent);
    world.post(ADA, "ops", "o1", &ask);
    abi::decode(&made).unwrap()
}

fn wake_on(height: u64) -> Value {
    json!({ "WakeOn": { "event": { "Height": { "height": height } } } })
}

fn told(chain: &MockChain, thread: u64) -> Vec<Body> {
    frames(chain, thread)
        .into_iter()
        .chain(held(chain, thread))
        .filter(|body| body.text().contains("Wake 1 is over"))
        .collect()
}

fn armed_in_bound(world: &World) -> (u64, u64) {
    let first = canonical(world);
    let thread = bound(world);
    let call = json!({ "program": wake::PROGRAM, "op": wake_on(10) });
    super::schedule::turn(world, 4, &[("w1", "network_call", call)]);
    let armed = super::schedule::answered(&world.chain, thread, "w1");
    assert_eq!(armed, json!({ "ok": true, "returned": 1 }));
    world.chain.at(6, 0);
    model_says(&world.chain, says("Armed."), "end_turn");
    world.chain.at(7, 0);
    model_says(&world.chain, says("It is armed."), "end_turn");
    assert!(
        assigned(&world.chain, SERVICE).is_empty(),
        "both conversations are idle"
    );
    (first, thread)
}

#[test]
fn the_conversation_whose_turn_armed_a_wake_hears_it_end_and_turns_at_once() {
    let world = waking();
    let chain = &world.chain;
    let (first, thread) = armed_in_bound(&world);
    chain.at(11, 0);
    let heard = told(chain, thread);
    let [Body::Post { author, text }] = &heard[..] else {
        panic!("{heard:?}")
    };
    assert_eq!(*author, Principal::Root);
    assert!(
        text.starts_with("Wake 1 is over: block 10 has come."),
        "{text}"
    );
    assert!(
        told(chain, first).is_empty(),
        "the canonical conversation hears nothing of it"
    );
    let (request, _) = open_request(chain);
    let deliver = egress::Delivery::Events {
        deliver: streamed_as(thread),
    };
    assert_eq!(
        request.delivery, deliver,
        "the bound conversation asks at once"
    );
}

#[test]
fn a_wake_that_ends_after_its_conversation_closed_is_left_there_as_not_delivered() {
    let world = waking();
    let chain = &world.chain;
    let (first, thread) = armed_in_bound(&world);
    chain
        .submit(signed(ADA), PROGRAM, &Op::Close { thread })
        .unwrap();
    chain.at(11, 0);
    let heard = told(chain, thread);
    let [Body::Notice { text }] = &heard[..] else {
        panic!("{heard:?}")
    };
    let undelivered = "Not delivered, as this conversation is closed: Wake 1 is over: block 10";
    assert!(text.starts_with(undelivered), "{text}");
    assert!(told(chain, first).is_empty());
    let its_own = ::egress::Delivery::Events {
        deliver: streamed_as(thread),
    };
    let asking = assigned(chain, SERVICE)
        .into_iter()
        .any(|row| row.delivery == its_own);
    assert!(!asking, "the closed conversation asks nothing");
}

#[test]
fn a_wake_harness_never_saw_armed_reaches_the_canonical_conversation() {
    let world = waking();
    let chain = &world.chain;
    let first = canonical(&world);
    let thread = bound(&world);
    let run = json!({ "height": 7, "program": wake::PROGRAM, "op": wake_on(10) });
    super::schedule::turn(&world, 4, &[("b1", "schedule_run", run)]);
    chain.at(6, 0);
    model_says(chain, says("Booked."), "end_turn");
    chain.at(8, 0);
    let reported = frames(chain, thread)
        .into_iter()
        .chain(held(chain, thread))
        .any(|body| body.text().contains("ran as you"));
    assert!(reported, "the booked run armed the wake as the agent");
    chain.at(9, 0);
    model_says(chain, says("Done."), "end_turn");
    chain.at(10, 0);
    ends_for(chain, thread, "Reported.");
    chain.at(11, 0);
    let heard = told(chain, first);
    let [Body::Post { text, .. }] = &heard[..] else {
        panic!("{heard:?}")
    };
    assert!(
        text.starts_with("Wake 1 is over: block 10 has come."),
        "{text}"
    );
    assert!(told(chain, thread).is_empty());
    let (request, _) = open_request(chain);
    let deliver = egress::Delivery::Events {
        deliver: streamed_as(first),
    };
    assert_eq!(
        request.delivery, deliver,
        "the canonical conversation asks at once"
    );
}

#[test]
fn a_clear_wake_on_a_free_place_ends_inside_its_arming_and_still_reaches_the_conversation_that_armed_it()
 {
    let world = waking();
    let chain = &world.chain;
    let first = canonical(&world);
    let thread = bound(&world);
    let clear =
        json!({ "WakeOn": { "event": { "Clear": { "channel_id": "ops", "thread": null } } } });
    let call = json!({ "program": wake::PROGRAM, "op": clear });
    super::schedule::turn(&world, 4, &[("w1", "network_call", call)]);
    let armed = super::schedule::answered(chain, thread, "w1");
    assert_eq!(armed, json!({ "ok": true, "returned": 1 }));
    assert!(
        told(chain, first).is_empty(),
        "the canonical conversation hears nothing of it"
    );
    let heard = told(chain, thread);
    let [Body::Post { author, text }] = &heard[..] else {
        panic!("{heard:?}")
    };
    assert_eq!(*author, Principal::Root);
    assert!(text.starts_with("Wake 1 is over: #ops is clear."), "{text}");
    chain.at(6, 0);
    model_says(chain, says("Armed."), "end_turn");
    let (request, _) = open_request(chain);
    let deliver = egress::Delivery::Events {
        deliver: streamed_as(thread),
    };
    assert_eq!(request.delivery, deliver);
    let asked = provider_body(chain, &request).to_string();
    assert!(
        asked.contains("Wake 1 is over: #ops is clear.")
            && asked.contains("Result of tool call w1"),
        "the bound conversation reads it with its call's result"
    );
    chain.at(7, 0);
    model_says(chain, says("Seen."), "end_turn");
    assert!(assigned(chain, SERVICE).is_empty());
}

#[test]
fn a_wake_end_held_while_a_wake_on_was_in_flight_reaches_the_canonical_conversation_when_that_one_is_refused()
 {
    let world = waking();
    let chain = &world.chain;
    let first = canonical(&world);
    let thread = bound(&world);
    let run = json!({ "height": 7, "program": wake::PROGRAM, "op": wake_on(10) });
    super::schedule::turn(&world, 4, &[("b1", "schedule_run", run)]);
    chain.at(6, 0);
    model_says(chain, says("Booked."), "end_turn");
    chain.at(8, 0);
    chain.at(9, 0);
    model_says(chain, says("Done."), "end_turn");
    {
        let host = chain.host(PROGRAM);
        let ctx = host.exec(chain.env(PROGRAM));
        crate::state::ARMING.insert(&ctx, &(world.agent, thread, "w1".to_owned()));
    }
    chain.at(10, 0);
    ends_for(chain, thread, "Reported.");
    chain.at(11, 0);
    assert!(
        told(chain, first).is_empty(),
        "held while a WakeOn is in flight"
    );
    assert!(told(chain, thread).is_empty());
    assert!(assigned(chain, SERVICE).is_empty());
    world.post(
        ADA,
        "ops",
        "o2",
        &format!("<@{}> arm one more", world.agent),
    );
    let refused = json!({ "WakeOn": { "event": { "Message": {
        "channel": "ops", "from": null, "mentions_me": false,
    } } } });
    let call = json!({ "program": wake::PROGRAM, "op": refused });
    super::schedule::turn(&world, 12, &[("w1", "network_call", call)]);
    super::schedule::refused(chain, thread, "w1");
    let heard = told(chain, first);
    let [Body::Post { text, .. }] = &heard[..] else {
        panic!("{heard:?}")
    };
    assert!(
        text.starts_with("Wake 1 is over: block 10 has come."),
        "{text}"
    );
    assert!(told(chain, thread).is_empty());
    let deliver = egress::Delivery::Events {
        deliver: streamed_as(first),
    };
    let asked = assigned(chain, SERVICE)
        .into_iter()
        .any(|row| row.delivery == deliver);
    assert!(asked, "the canonical conversation asks at once");
}

fn held_for_the_reset(chain: &MockChain, thread: u64) -> bool {
    let Reply::Wakes(wakes) = chain.query(PROGRAM, &Query::Wakes { thread }).unwrap() else {
        panic!("harness answers wakes")
    };
    wakes
        .iter()
        .any(|wake| matches!(wake, Wake::RetryAfter { .. }))
}

fn limited(chain: &MockChain) {
    let (request, _) = open_request(chain);
    let limited = http::Response {
        status: 429,
        headers: vec![(
            "anthropic-ratelimit-unified-5h-reset".into(),
            "4000000000".into(),
        )],
        body: b"{}".to_vec(),
    };
    answer(chain, request.id, Answer::Output(abi::encode(&limited)));
}

fn named_account(row: &RequestRow) -> Option<String> {
    let Request::Http { head, .. } = &row.request else {
        return None;
    };
    let named = head.headers.iter().find(|(key, _)| key == "x-ccs-account");
    named.map(|(_, account)| account.clone())
}

#[test]
fn a_wake_end_given_to_a_conversation_held_for_its_reset_asks_at_once_unpinned() {
    let world = waking();
    let chain = &world.chain;
    let first = canonical(&world);
    let thread = bound(&world);
    let run = json!({ "height": 7, "program": wake::PROGRAM, "op": wake_on(11) });
    super::schedule::turn(&world, 4, &[("b1", "schedule_run", run)]);
    chain.at(6, 0);
    model_says(chain, says("Booked."), "end_turn");
    chain.at(8, 0);
    chain.at(9, 0);
    model_says(chain, says("Done."), "end_turn");
    world.mention("m2", "one more thing");
    chain.at(10, 0);
    ends_for(chain, thread, "Reported.");
    limited(chain);
    assert!(held_for_the_reset(chain, first), "held for its reset");
    assert!(assigned(chain, SERVICE).is_empty());
    {
        let host = chain.host(PROGRAM);
        let ctx = host.exec(chain.env(PROGRAM));
        crate::state::ARMING.insert(&ctx, &(world.agent, thread, "w1".to_owned()));
    }
    chain.at(12, 0);
    assert!(
        told(chain, first).is_empty(),
        "held while a WakeOn is in flight"
    );
    assert!(
        held_for_the_reset(chain, first),
        "nothing answered on its service since"
    );
    chain
        .submit(signed(ADA), PROGRAM, &Op::Close { thread })
        .unwrap();
    assert_eq!(told(chain, first).len(), 1, "the flushed end reached it");
    assert!(
        !held_for_the_reset(chain, first),
        "a new input ends the hold"
    );
    let deliver = egress::Delivery::Events {
        deliver: streamed_as(first),
    };
    let asked: Vec<RequestRow> = assigned(chain, SERVICE)
        .into_iter()
        .filter(|row| row.delivery == deliver)
        .collect();
    let [row] = &asked[..] else {
        panic!("{} asks open for the canonical conversation", asked.len())
    };
    assert_eq!(named_account(row), None, "its service picks an account");
}

#[test]
fn a_wake_end_held_for_a_conversation_that_closes_mid_arming_reaches_the_canonical_one() {
    let world = waking();
    let chain = &world.chain;
    let first = canonical(&world);
    let thread = bound(&world);
    let run = json!({ "height": 7, "program": wake::PROGRAM, "op": wake_on(10) });
    super::schedule::turn(&world, 4, &[("b1", "schedule_run", run)]);
    chain.at(6, 0);
    model_says(chain, says("Booked."), "end_turn");
    chain.at(8, 0);
    chain.at(9, 0);
    model_says(chain, says("Done."), "end_turn");
    {
        let host = chain.host(PROGRAM);
        let ctx = host.exec(chain.env(PROGRAM));
        crate::state::ARMING.insert(&ctx, &(world.agent, thread, "w1".to_owned()));
    }
    chain.at(10, 0);
    ends_for(chain, thread, "Reported.");
    chain.at(11, 0);
    assert!(
        told(chain, first).is_empty(),
        "held while a WakeOn is in flight"
    );
    chain
        .submit(signed(ADA), PROGRAM, &Op::Close { thread })
        .unwrap();
    let heard = told(chain, first);
    let [Body::Post { text, .. }] = &heard[..] else {
        panic!("{heard:?}")
    };
    assert!(
        text.starts_with("Wake 1 is over: block 10 has come."),
        "{text}"
    );
    let (request, _) = open_request(chain);
    let deliver = egress::Delivery::Events {
        deliver: streamed_as(first),
    };
    assert_eq!(
        request.delivery, deliver,
        "the canonical conversation asks at once"
    );
}

#[test]
fn a_wake_end_held_behind_an_arming_an_update_cut_off_reaches_the_canonical_conversation() {
    let world = waking();
    let chain = &world.chain;
    super::network::served(chain);
    let first = canonical(&world);
    let thread = bound(&world);
    let run = json!({ "height": 7, "program": wake::PROGRAM, "op": wake_on(20) });
    super::schedule::turn(&world, 4, &[("b1", "schedule_run", run)]);
    chain.at(6, 0);
    model_says(chain, says("Booked."), "end_turn");
    chain.at(8, 0);
    chain.at(9, 0);
    model_says(chain, says("Done."), "end_turn");
    world.post(
        ADA,
        "ops",
        "o2",
        &format!("<@{}> list the pods", world.agent),
    );
    let pods = super::network::pods_get("/v1/pods");
    super::schedule::turn(&world, 10, &[("n1", "network_call", pods)]);
    {
        let host = chain.host(PROGRAM);
        let ctx = host.exec(chain.env(PROGRAM));
        crate::state::ARMING.insert(&ctx, &(world.agent, thread, "n1".to_owned()));
    }
    chain.at(21, 0);
    assert!(
        told(chain, first).is_empty(),
        "held while the arming is open"
    );
    listed(chain, &codes(module(&[("ducktape.build", b"2")])));
    chain.cancel_parked(PROGRAM);
    chain
        .submit(signed(ADA), PROGRAM, &Op::Resume { thread })
        .unwrap();
    let (text, failed) = result_of(chain, thread, "n1");
    assert!(failed && text.starts_with("harness was updated"), "{text}");
    let heard = told(chain, first);
    let [Body::Post { text, .. }] = &heard[..] else {
        panic!("{heard:?}")
    };
    assert!(
        text.starts_with("Wake 1 is over: block 20 has come."),
        "{text}"
    );
    let deliver = egress::Delivery::Events {
        deliver: streamed_as(first),
    };
    let asked = assigned(chain, SERVICE)
        .into_iter()
        .any(|row| row.delivery == deliver);
    assert!(asked, "the canonical conversation asks at once");
}
