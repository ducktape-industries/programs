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
fn a_wait_whose_canonical_conversation_closed_wakes_the_fresh_one_in_its_place() {
    let world = schemed();
    let chain = &world.chain;
    let (thread, wait) = waiting_in_canonical(&world);
    chain
        .submit(signed(ADA), PROGRAM, &Op::Close { thread })
        .unwrap();
    chain.at(6, 0);
    let fresh = world.mention("m2", "a fresh start");
    assert_ne!(fresh, thread);
    assert_eq!(armed(chain, fresh), [wait]);
    assert!(armed(chain, thread).is_empty());
    chain.at(21, 0);
    assert_eq!(over(chain, fresh, wait), 1, "{:?}", wakes(chain, fresh));
    assert!(wakes(chain, thread).is_empty());
    assert!(!undelivered(chain, thread), "{:?}", frames(chain, thread));
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
    let sent = provider_body(chain, &request).to_string();
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
fn only_the_waiting_agent_waits_in_its_conversation_or_cancels() {
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
    assert_eq!(armed(chain, thread), [wait]);
}

#[test]
fn a_wait_for_a_pod_is_refused_pointing_at_the_pods_services_own_wait() {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "wait for my pod", Some(WINDOW));
    let pod = json!({ "event": { "pod": { "name": "duckpond-7" } } });
    turn(&world, 3, &[("w1", "wait_for", pod)]);
    let text = refused(chain, thread, "w1");
    let pointed = text.starts_with(code::UNSUPPORTED) && text.contains("pod_wait");
    assert!(pointed, "{text}");
    assert!(armed(chain, thread).is_empty());
}

pub(super) fn channel(world: &World, id: &str) {
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
fn an_agent_moves_no_forge_watcher() {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "move forge's watcher", Some(WINDOW));
    let watch = json!({ "program": "forge", "op": { "Watch": { "deliver": "" } } });
    turn(&world, 3, &[("f1", "network_call", watch)]);
    let text = refused(chain, thread, "f1");
    let refusal = text.starts_with(code::UNAUTHORIZED) && text.contains("does not send");
    assert!(refusal, "{text}");
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
    let inherited = waited(chain, first, "w1");
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
        [json!(inherited), json!(open)],
        "this conversation's waits not over yet, the closed one's it took over included"
    );
    assert_eq!(
        listed("l2"),
        [json!(inherited), json!(over), json!(open)],
        "every wait the agent made, in every conversation, over ones included"
    );
}

pub(super) fn opened_at(world: &World, channel: &str) -> u64 {
    let open = Op::OpenThread {
        agent: world.agent,
        place: Place::Channel {
            channel: channel.into(),
        },
    };
    abi::decode(&world.chain.submit(signed(ADA), PROGRAM, &open).unwrap()).unwrap()
}

pub(super) fn renewed(world: &World) -> u64 {
    let new = Op::NewThread { agent: world.agent };
    abi::decode(&world.chain.submit(signed(ADA), PROGRAM, &new).unwrap()).unwrap()
}

fn over(chain: &MockChain, thread: u64, wait: u64) -> usize {
    let words = format!("Wait {wait} is over");
    wakes(chain, thread)
        .iter()
        .filter(|text| text.starts_with(&words))
        .count()
}

fn undelivered(chain: &MockChain, thread: u64) -> bool {
    frames(chain, thread)
        .into_iter()
        .any(|body| body.text().contains("Not delivered"))
}

fn waiting_in_canonical(world: &World) -> (u64, u64) {
    let thread = world.opened("m1", "wake me at block 20", Some(WINDOW));
    turn(world, 3, &[("w1", "wait_for", height(20))]);
    (thread, waited(&world.chain, thread, "w1"))
}

fn closed_waiting_in_ops(world: &World) -> (u64, u64) {
    let chain = &world.chain;
    world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Hi." }]),
        "end_turn",
    );
    channel(world, "ops");
    let thread = opened_at(world, "ops");
    let ask = format!("<@{}> wake me at block 20", world.agent);
    world.post(ADA, "ops", "o1", &ask);
    turn(world, 4, &[("w1", "wait_for", height(20))]);
    let wait = waited(chain, thread, "w1");
    chain
        .submit(signed(ADA), PROGRAM, &Op::Close { thread })
        .unwrap();
    (thread, wait)
}

#[test]
fn a_wait_moves_to_the_conversation_new_thread_starts_in_its_place_and_wakes_it_there() {
    let world = schemed();
    let chain = &world.chain;
    let (thread, wait) = waiting_in_canonical(&world);
    let heir = renewed(&world);
    assert!(
        thread_row(chain, thread).closed,
        "the idle one it replaced closes"
    );
    assert_eq!(armed(chain, heir), [wait]);
    assert!(armed(chain, thread).is_empty());
    assert_eq!(wait_row(chain, wait).unwrap().thread, heir);
    chain.at(21, 0);
    assert_eq!(over(chain, heir, wait), 1, "{:?}", wakes(chain, heir));
    assert!(wakes(chain, thread).is_empty());
    assert!(!undelivered(chain, thread), "{:?}", frames(chain, thread));
}

#[test]
fn a_wait_left_on_a_closed_bound_conversation_moves_to_the_one_opened_next_at_its_place() {
    let world = schemed();
    let chain = &world.chain;
    let (thread, wait) = closed_waiting_in_ops(&world);
    let heir = opened_at(&world, "ops");
    assert_ne!(heir, thread);
    assert_eq!(armed(chain, heir), [wait]);
    assert!(armed(chain, thread).is_empty());
    chain.at(21, 0);
    assert_eq!(over(chain, heir, wait), 1, "{:?}", wakes(chain, heir));
    assert!(!undelivered(chain, thread), "{:?}", frames(chain, thread));
}

#[test]
fn a_wait_whose_bound_conversation_closed_with_none_opened_at_its_place_ends_there_as_a_notice() {
    let world = schemed();
    let chain = &world.chain;
    let (thread, wait) = closed_waiting_in_ops(&world);
    chain.at(21, 0);
    let noticed = frames(chain, thread).into_iter().any(|body| match body {
        Body::Notice { text } => text.starts_with(&format!(
            "Not delivered, as this conversation is closed: Wait {wait} is over: block"
        )),
        _ => false,
    });
    assert!(noticed, "{:?}", frames(chain, thread));
}

#[test]
fn a_wait_the_replaced_conversation_arms_while_it_finishes_its_turn_moves_to_its_heir_as_it_closes()
{
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "wake me at block 20", Some(WINDOW));
    let heir = renewed(&world);
    assert!(
        !thread_row(chain, thread).closed,
        "its turn is still asking"
    );
    turn(&world, 3, &[("w1", "wait_for", height(20))]);
    let wait = waited(chain, thread, "w1");
    assert!(
        thread_row(chain, thread).closed,
        "it closes once its turn ends"
    );
    assert_eq!(armed(chain, heir), [wait]);
    assert!(armed(chain, thread).is_empty());
    assert_eq!(wait_row(chain, wait).unwrap().thread, heir);
    chain.at(21, 0);
    assert_eq!(over(chain, heir, wait), 1, "{:?}", wakes(chain, heir));
    assert!(!undelivered(chain, thread), "{:?}", frames(chain, thread));
}
