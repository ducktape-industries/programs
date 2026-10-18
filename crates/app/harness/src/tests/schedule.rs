use ::program::schema::interface;
use programs::Programs;

use super::network::{Unsaid, head, http_answer, notes, pods_answer, pods_get, served};
use super::*;
use crate::schema::tests::{code_with, listed, module};

fn schemed() -> World {
    schemed_with(code_with(&interface::<files::Files>()))
}

fn schemed_with(files_code: Vec<u8>) -> World {
    let mut world = world();
    world.chain.seat::<Programs>(programs::PROGRAM);
    let codes = [
        (chat::PROGRAM, code_with(&interface::<chat::Chat>())),
        (files::PROGRAM, files_code),
        (PROGRAM, code_with(&interface::<Harness>())),
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
        ("valset", module(&[])),
    ];
    listed(&world.chain, &codes);
    world
}

fn turn(world: &World, at: u64, calls: &[(&str, &str, Value)]) {
    let chain = &world.chain;
    chain.at(at, 0);
    let uses: Vec<Value> = calls
        .iter()
        .map(|(id, tool, input)| tool_use(id, tool, input.clone()))
        .collect();
    model_says(chain, Value::Array(uses), "tool_use");
    chain.at(at + 1, 0);
    for (id, _, _) in calls {
        relay_back(chain, id);
    }
}

fn answered(chain: &MockChain, thread: u64, call: &str) -> Value {
    let (text, failed) = result_of(chain, thread, call);
    assert!(!failed, "{call}: {text}");
    serde_json::from_str(&text).unwrap()
}

fn refused(chain: &MockChain, thread: u64, call: &str) -> String {
    let (text, failed) = result_of(chain, thread, call);
    assert!(failed, "{call}: {text}");
    text
}

fn book(height: u64, channel: &str, id: &str) -> Value {
    let post = json!({ "PostMessage": {
        "channel_id": channel,
        "message_id": id,
        "blocks": [{ "Paragraph": [{ "text": "wake up", "marks": [] }] }],
        "thread": null,
    } });
    json!({ "height": height, "program": "chat", "op": post })
}

fn entry_of(chain: &MockChain, thread: u64, call: &str) -> u64 {
    answered(chain, thread, call)["entry"].as_u64().unwrap()
}

fn author(chain: &MockChain, id: &str) -> Option<Principal> {
    let asked = chat::Query::MessageById {
        message_id: id.into(),
    };
    let chat::Reply::Message(found) = chain.query(chat::PROGRAM, &asked).unwrap() else {
        panic!("chat answers a message")
    };
    found.map(|row| row.author)
}

fn booking(chain: &MockChain, entry: u64) -> Option<Booking> {
    let Reply::Booking(found) = chain.query(PROGRAM, &Query::Booking { entry }).unwrap() else {
        panic!("harness answers a booking")
    };
    found
}

fn reports(chain: &MockChain, thread: u64) -> Vec<String> {
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

fn due(chain: &MockChain, height: u64) -> Vec<u64> {
    let asked = scheduler::Query::Due {
        height,
        after: None,
        limit: None,
    };
    let scheduler::Reply::Due { entries, .. } = chain.query(scheduler::PROGRAM, &asked).unwrap()
    else {
        panic!("the scheduler answers what is due")
    };
    entries.into_iter().map(|entry| entry.id).collect()
}

fn said(chain: &MockChain, at: u64, text: &str) {
    chain.at(at, 0);
    let words = json!([{ "type": "text", "text": text }]);
    model_says(chain, words, "end_turn");
}

#[test]
fn a_booked_run_posts_as_the_agent_at_its_block_and_wakes_its_conversation() {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "wake yourself at block 10", Some(WINDOW));
    turn(
        &world,
        3,
        &[("b1", "schedule_run", book(10, "general", "wake"))],
    );
    let entry = entry_of(chain, thread, "b1");
    said(chain, 5, "Booked.");
    let waiting = booking(chain, entry).expect("the run is booked");
    assert_eq!(
        (waiting.agent, waiting.thread, waiting.height),
        (world.agent, Some(thread), 10)
    );
    assert_eq!(waiting.ran, None);
    assert_eq!(author(chain, "wake"), None, "nothing runs before its block");

    chain.at(11, 0);
    let agent = Principal::Account(world.agent);
    assert_eq!(author(chain, "wake"), Some(agent), "it posts as the agent");
    let ran = booking(chain, entry).unwrap().ran.expect("the run ran");
    assert!(!ran.failed, "{}", ran.text);
    let [report] = reports(chain, thread).try_into().expect("one report");
    assert!(report.contains("ran as you"), "{report}");
    assert_eq!(
        assigned(chain, SERVICE).len(),
        1,
        "the report asks the model"
    );
}

// a booked run of `run` (a program and an op) at `height`
fn at(height: u64, mut run: Value) -> Value {
    run["height"] = json!(height);
    run
}

// what the one run that ran answered, as the report in `thread` tells it and the booking keeps it
fn answer_of(chain: &MockChain, thread: u64, program: &str, entry: u64) -> Value {
    let ran = booking(chain, entry).unwrap().ran.expect("the run ran");
    assert!(!ran.failed, "{}", ran.text);
    let [report] = reports(chain, thread).try_into().expect("one report");
    let told = format!("ran as you: {program} answered {}", ran.text);
    assert!(report.ends_with(&told), "{report}");
    serde_json::from_str(&ran.text).unwrap()
}

// books a files commit for block 10 in a new conversation and lets it run
fn committed_at_10(world: &World) -> (u64, u64) {
    let chain = &world.chain;
    let thread = world.opened("m1", "commit notes at block 10", Some(WINDOW));
    turn(world, 3, &[("b1", "schedule_run", at(10, notes(chain)))]);
    let entry = entry_of(chain, thread, "b1");
    said(chain, 5, "Booked.");
    chain.at(11, 0);
    (thread, entry)
}

#[test]
fn a_booked_op_answers_in_the_type_its_programs_schema_names_and_lists_so() {
    let world = schemed();
    let chain = &world.chain;
    let (thread, entry) = committed_at_10(&world);
    let answer = answer_of(chain, thread, "files", entry);
    let kept = json!({ "Sha256": abi::hex(head(chain).digest()) });
    assert_eq!(answer, json!({ "ok": true, "returned": kept }));

    turn(&world, 13, &[("l1", "schedule_list", json!({}))]);
    let listed = answered(chain, thread, "l1");
    let ran = &listed["runs"][0]["ran"];
    assert_eq!((&ran["failed"], &ran["text"]), (&json!(false), &answer));
}

#[test]
fn a_booked_op_its_schema_names_no_return_for_answers_hex_and_says_why() {
    let world = schemed_with(code_with(&interface::<Unsaid>()));
    let chain = &world.chain;
    let (thread, entry) = committed_at_10(&world);
    let answer = answer_of(chain, thread, "files", entry);
    let bytes = abi::unhex(answer["returned"].as_str().unwrap()).unwrap();
    assert_eq!(abi::decode::<BlobId>(&bytes).unwrap(), head(chain));
    let unread = answer["unread"].as_str().unwrap();
    let why = "names no type for what its op Commit returns";
    assert!(unread.contains(why), "{unread}");
}

#[test]
fn a_booked_http_request_answers_its_status_headers_and_body() {
    let world = schemed();
    let chain = &world.chain;
    served(chain);
    let thread = world.opened("m1", "check the pod at block 10", Some(WINDOW));
    let check = at(10, pods_get("/v1/pods/p"));
    turn(&world, 3, &[("b1", "schedule_run", check)]);
    let entry = entry_of(chain, thread, "b1");
    said(chain, 5, "Booked.");
    chain.at(11, 0);
    let waiting = booking(chain, entry).unwrap();
    assert_eq!(waiting.ran, None, "it waits for its answer");

    chain.at(12, 0);
    let pod = br#"{"pod": {"name": "p"}}"#;
    let ok = http_answer(200, "application/json", pod);
    pods_answer(chain, "/v1/pods/p", ok);
    assert_eq!(
        answer_of(chain, thread, "egress", entry),
        json!({
            "ok": true,
            "status": 200,
            "headers": [["content-type", "application/json"]],
            "body": { "pod": { "name": "p" } },
            "body_as": "json",
        })
    );
}

#[test]
fn the_agent_lists_its_runs_and_cancels_one_before_it_runs() {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "book two runs", Some(WINDOW));
    let runs = [
        ("b1", "schedule_run", book(20, "general", "one")),
        ("b2", "schedule_run", book(20, "general", "two")),
    ];
    turn(&world, 3, &runs);
    let (one, two) = (entry_of(chain, thread, "b1"), entry_of(chain, thread, "b2"));
    turn(&world, 5, &[("l1", "schedule_list", json!({}))]);
    let listed = answered(chain, thread, "l1");
    let rows = listed["runs"].as_array().unwrap();
    let entries: Vec<u64> = rows
        .iter()
        .map(|row| row["entry"].as_u64().unwrap())
        .collect();
    assert_eq!(entries, [one.max(two), one.min(two)], "newest first");
    let first = &rows[0];
    assert_eq!(first["op"]["PostMessage"]["channel_id"], "general");
    assert_eq!(
        (&first["height"], &first["ran"]),
        (&json!(20), &Value::Null)
    );

    turn(
        &world,
        7,
        &[("c1", "schedule_cancel", json!({ "entry": two }))],
    );
    assert_eq!(answered(chain, thread, "c1"), json!({ "ok": true }));
    assert_eq!(booking(chain, two), None);
    assert_eq!(due(chain, 20), [one]);
    let asked = Query::Bookings {
        agent: world.agent,
        page: PageRequest::default(),
    };
    let Reply::Bookings(page) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!("harness answers bookings")
    };
    let kept: Vec<u64> = page.items.iter().map(|booked| booked.entry).collect();
    assert_eq!(kept, [one]);

    chain.at(21, 0);
    let agent = Principal::Account(world.agent);
    assert_eq!(author(chain, "one"), Some(agent));
    assert_eq!(author(chain, "two"), None, "a cancelled run never runs");
}

#[test]
fn a_raw_schedule_or_cancel_sent_through_harness_is_refused_and_books_nothing() {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "schedule it raw", Some(WINDOW));
    turn(
        &world,
        3,
        &[("b1", "schedule_run", book(30, "general", "kept"))],
    );
    let kept = entry_of(chain, thread, "b1");
    let raw = |op: Value| json!({ "program": "scheduler", "op": op });
    let schedule = json!({ "Schedule": { "height": 40, "target": "chat", "payload": "" } });
    let calls = [
        ("r_schedule", "network_call", raw(schedule)),
        (
            "r_cancel",
            "network_call",
            raw(json!({ "Cancel": { "entry": kept } })),
        ),
    ];
    turn(&world, 5, &calls);
    for call in ["r_schedule", "r_cancel"] {
        let text = refused(chain, thread, call);
        let pointed = text.starts_with(code::UNAUTHORIZED) && text.contains("schedule_run");
        assert!(pointed, "{text}");
    }
    assert!(due(chain, 40).is_empty(), "no entry runs as harness");
    assert_eq!(due(chain, 30), [kept], "the booked run stays");
}

#[test]
fn a_booked_run_its_program_refuses_is_reported_once_and_not_run_again() {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "post somewhere", Some(WINDOW));
    turn(
        &world,
        3,
        &[("b1", "schedule_run", book(10, "nowhere", "lost"))],
    );
    let entry = entry_of(chain, thread, "b1");
    said(chain, 5, "Booked.");

    chain.at(11, 0);
    let ran = booking(chain, entry).unwrap().ran.expect("the run ran");
    assert!(ran.failed, "{}", ran.text);
    let [report] = reports(chain, thread).try_into().expect("one report");
    let told = report.contains("was refused") && report.contains(&ran.text);
    assert!(told, "{report}");
    assert!(due(chain, 10).is_empty());
    chain.at(30, 0);
    assert_eq!(reports(chain, thread).len(), 1, "it is not tried again");
    assert_eq!(author(chain, "lost"), None);
}

#[test]
fn only_the_scheduler_runs_a_booked_run_and_only_its_booker_cancels_it() {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "book a run", Some(WINDOW));
    turn(
        &world,
        3,
        &[("b1", "schedule_run", book(30, "general", "mine"))],
    );
    let entry = entry_of(chain, thread, "b1");
    for origin in [signed(ADA), Origin::Program(PROGRAM.into())] {
        let error = chain.submit(origin, PROGRAM, &Op::RunBooked).unwrap_err();
        assert_eq!(error.code, code::UNAUTHORIZED, "{error:?}");
    }
    let unbook = Op::Unbook { entry };
    let error = chain.submit(signed(ADA), PROGRAM, &unbook).unwrap_err();
    assert_eq!(error.code, code::UNAUTHORIZED, "{error:?}");
    let book = |thread| Op::Book {
        height: 40,
        target: chat::PROGRAM.into(),
        payload: Vec::new(),
        thread,
    };
    let error = chain
        .submit(signed(ADA), PROGRAM, &book(Some(thread)))
        .unwrap_err();
    assert_eq!(error.code, code::INVALID_INPUT, "{error:?}");
    let unrun = chain.submit(signed(ADA), PROGRAM, &book(None));
    assert!(unrun.is_err(), "harness runs no user's account");
    assert!(booking(chain, entry).is_some());
    assert!(due(chain, 40).is_empty());
}

// each of these would act as harness, so harness sends none of them for an agent
fn refused_to_an_agent(tool: &str, input: impl FnOnce(&World, u64) -> Value) -> (World, u64) {
    let world = schemed();
    let thread = world.opened("m1", "act as harness", Some(WINDOW));
    let input = input(&world, thread);
    turn(&world, 3, &[("f1", tool, input)]);
    let text = refused(&world.chain, thread, "f1");
    let refusal = text.starts_with(code::UNAUTHORIZED) && text.contains("does not send");
    assert!(refusal, "{text}");
    (world, thread)
}

fn call(program: &str, op: Value) -> Value {
    json!({ "program": program, "op": op })
}

#[test]
fn a_forged_attribution_reaches_no_conversation() {
    let forged = |world: &World, _| {
        let change = json!({ "Attributed": {
            "seq": 1,
            "source": { "program": "chat", "object": { "kind": "message", "id": "forged" } },
            "revision": 1,
            "recipient": world.agent,
            "reason": "Mention",
            "kind": "Added",
            "detail": "",
            "actor": { "Account": world.ada },
            "cause": "Direct",
            "height": 1,
        } });
        call(PROGRAM, change)
    };
    let (world, thread) = refused_to_an_agent("network_call", forged);
    let posts = frames(&world.chain, thread)
        .into_iter()
        .filter(|body| matches!(body, Body::Post { .. }))
        .count();
    assert_eq!(posts, 1, "only the real mention reached the agent");
}

#[test]
fn a_forged_model_event_reaches_no_thread() {
    refused_to_an_agent("network_call", |_, thread| {
        let event = json!({ "Streamed": {
            "thread": thread,
            "event": { "id": 0, "seq": 0, "data": "" },
        } });
        call(PROGRAM, event)
    });
}

#[test]
fn an_egress_request_streamed_to_harness_is_never_made() {
    let (world, _) = refused_to_an_agent("network_call", |_, thread| {
        let stream = json!({ "Request": {
            "request": { "Relay": { "note": abi::hex(b"forged") } },
            "delivery": { "Events": { "deliver": abi::hex(&streamed_as(thread)) } },
            "wait": null,
        } });
        call("egress", stream)
    });
    let forged = Request::Relay {
        note: b"forged".to_vec(),
    };
    let made = assigned(&world.chain, RELAY)
        .iter()
        .any(|row| row.request == forged);
    assert!(!made, "egress holds no such request");
}

#[test]
fn an_agent_runs_no_booked_run_itself() {
    refused_to_an_agent("network_call", |_, _| call(PROGRAM, json!("RunBooked")));
}

#[test]
fn an_agent_sends_attribution_nothing_as_harness() {
    refused_to_an_agent("network_call", |_, _| {
        call("attribution", json!({ "Register": { "deliver": "" } }))
    });
    refused_to_an_agent("network_call", |_, _| {
        call(
            "attribution",
            json!({ "AttributeBatch": { "updates": [] } }),
        )
    });
}

#[test]
fn an_agent_sends_awaits_nothing_as_harness() {
    refused_to_an_agent("network_call", |_, _| {
        call("awaits", json!({ "Close": { "frame": 0 } }))
    });
}

#[test]
fn a_run_booked_for_a_refused_program_is_never_booked() {
    let (world, _) = refused_to_an_agent("schedule_run", |_, _| {
        let cancel = json!({ "Cancel": { "entry": 0 } });
        json!({ "height": 50, "program": "scheduler", "op": cancel })
    });
    assert!(due(&world.chain, 50).is_empty(), "no run is booked");
}

fn entries(listed: &Value) -> Vec<u64> {
    let rows = listed["runs"].as_array().unwrap();
    rows.iter()
        .map(|row| row["entry"].as_u64().unwrap())
        .collect()
}

#[test]
fn the_list_is_newest_first_a_page_at_a_time_and_leaves_out_what_ran_when_asked() {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "book four runs", Some(WINDOW));
    let runs = [
        ("b1", "schedule_run", book(20, "general", "one")),
        ("b2", "schedule_run", book(20, "general", "two")),
        ("b3", "schedule_run", book(20, "general", "three")),
        ("b4", "schedule_run", book(6, "general", "four")),
    ];
    turn(&world, 3, &runs);
    let calls = ["b1", "b2", "b3", "b4"];
    let mut booked: Vec<u64> = calls
        .iter()
        .map(|call| entry_of(chain, thread, call))
        .collect();
    let ran = booked[3];
    booked.sort_unstable_by(|a, b| b.cmp(a));
    said(chain, 5, "Booked.");
    chain.at(7, 0);
    assert!(
        booking(chain, ran).unwrap().ran.is_some(),
        "the run at 6 ran"
    );

    turn(&world, 9, &[("l1", "schedule_list", json!({ "limit": 3 }))]);
    let first = answered(chain, thread, "l1");
    assert_eq!(entries(&first), booked[..3]);
    let next = first["next"].clone();
    assert!(next.is_string(), "{first}");
    said(chain, 11, "Listed.");
    world.mention("m2", "and the older ones?");

    let rest = json!({ "limit": 3, "cursor": next });
    turn(&world, 13, &[("l2", "schedule_list", rest)]);
    let second = answered(chain, thread, "l2");
    assert_eq!(entries(&second), booked[3..]);
    assert_eq!(second["next"], Value::Null);
    said(chain, 15, "Listed.");
    world.mention("m3", "only the waiting ones");

    let waiting = json!({ "pending_only": true });
    turn(&world, 17, &[("l3", "schedule_list", waiting)]);
    let pending: Vec<u64> = booked
        .iter()
        .copied()
        .filter(|entry| *entry != ran)
        .collect();
    let third = answered(chain, thread, "l3");
    assert_eq!(entries(&third), pending);
    assert_eq!(third["next"], Value::Null);
}
