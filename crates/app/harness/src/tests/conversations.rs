use super::*;
use crate::state::{CURRENT, THREADS};

fn answered(world: &World, id: &str, text: &str) -> u64 {
    let thread = world.opened(id, text, Some(WINDOW));
    world.chain.at(3, 0);
    model_says(
        &world.chain,
        json!([{ "type": "text", "text": "Noted." }]),
        "end_turn",
    );
    thread
}

fn current(chain: &MockChain, agent: AccountNumber) -> Option<u64> {
    let Reply::Agent(Some(setup)) = chain.query(PROGRAM, &Query::Agent { agent }).unwrap() else {
        panic!()
    };
    setup.current
}

fn thread_at(chain: &MockChain, agent: AccountNumber, place: &str) -> Option<u64> {
    let asked = Query::ThreadAt {
        agent,
        place: place.into(),
    };
    let Reply::Thread(row) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!()
    };
    row.map(|row| row.id)
}

fn renewed(chain: &MockChain, key: &[u8], op: &Op) -> Result<u64, Error> {
    chain
        .submit(signed(key), PROGRAM, op)
        .map(|output| abi::decode(&output).unwrap())
}

fn texts(messages: &[Value]) -> Vec<String> {
    messages
        .iter()
        .flat_map(|message| message["content"].as_array().unwrap().clone())
        .filter_map(|block| block["text"].as_str().map(str::to_owned))
        .collect()
}

fn asked_for(chain: &MockChain, thread: u64) -> RequestRow {
    let deliver = egress::Delivery::Events {
        deliver: streamed_as(thread),
    };
    assigned(chain, SERVICE)
        .into_iter()
        .find(|row| row.delivery == deliver)
        .expect("the thread's model call is open")
}

fn room(world: &World, id: &str) {
    let op = chat::Op::CreateChannel {
        channel_id: id.into(),
        name: id.into(),
        post_policy: chat::PostPolicy::Open,
    };
    world.chain.submit(signed(ADA), chat::PROGRAM, &op).unwrap();
}

fn in_room(channel: &str) -> Place {
    Place::Channel {
        channel: channel.into(),
    }
}

fn bind(world: &World, key: &[u8], place: Place) -> Result<u64, Error> {
    let open = Op::OpenThread {
        agent: world.agent,
        place,
    };
    renewed(&world.chain, key, &open)
}

fn say_in(world: &World, channel: &str, id: &str, text: &str) {
    world.post(ADA, channel, id, &format!("<@{}> {text}", world.agent));
}

fn reply_in(world: &World, channel: &str, id: &str, root: u64, text: &str) {
    let op = chat::Op::PostMessage {
        channel_id: channel.into(),
        message_id: id.into(),
        blocks: chat::parse_message(&format!("<@{}> {text}", world.agent)),
        thread: Some(root),
    };
    world.chain.submit(signed(ADA), chat::PROGRAM, &op).unwrap();
}

fn posts(chain: &MockChain, thread: u64) -> Vec<String> {
    frames(chain, thread)
        .into_iter()
        .chain(held(chain, thread))
        .filter_map(|body| match body {
            Body::Post { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

fn bound(chain: &MockChain, agent: AccountNumber) -> Vec<u64> {
    let asked = Query::Bound {
        agent,
        page: PageRequest::default(),
    };
    let Reply::Threads(page) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!()
    };
    page.items.into_iter().map(|row| row.id).collect()
}

fn calls(chain: &MockChain, thread: u64) -> Vec<String> {
    let asked = Query::Calls {
        thread,
        page: PageRequest::default(),
    };
    let Reply::Calls(page) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!()
    };
    page.items.into_iter().map(|row| row.id).collect()
}

fn says(text: &str) -> Value {
    json!([{ "type": "text", "text": text }])
}

#[test]
fn every_person_and_every_place_continue_the_agents_canonical_conversation() {
    let world = world();
    let chain = &world.chain;
    let thread = answered(&world, "m1", "hello from general");
    let open = chat::Op::CreateDmChannel {
        counterpart: world.agent,
        name: "scout".into(),
    };
    chain.submit(signed(ADA), chat::PROGRAM, &open).unwrap();
    let room = chat::dm_channel_id(world.ada, world.agent);
    world.post(ADA, &room, "d1", "and a word in private");
    set_list(
        chain,
        ADA,
        PROGRAM,
        crate::agent_key(world.agent),
        ::program::Ownership::Listed(vec![::program::Party::Account(world.bo)]),
    );
    world.post(BO, "general", "b1", &format!("<@{}> me too", world.agent));

    let [only] = world.threads().try_into().unwrap();
    assert_eq!(only.id, thread);
    assert_eq!(current(chain, world.agent), Some(thread));
    assert_eq!(thread_at(chain, world.agent, "chat/general"), Some(thread));
    assert_eq!(
        thread_at(chain, world.agent, &format!("chat/{room}")),
        Some(thread)
    );
    let (request, _) = open_request(chain);
    let said = texts(body(chain, &request)["messages"].as_array().unwrap());
    let ada = format!(
        "Ada (account {}) wrote to you directly in chat channel {room}",
        world.ada
    );
    let bo = format!(
        "Bo (account {}) mentioned you in chat channel general",
        world.bo
    );
    assert!(said.iter().any(|text| text.starts_with(&ada)), "{said:?}");
    let waiting: Vec<String> = held(chain, thread).iter().map(Body::text).collect();
    assert!(
        waiting.iter().any(|text| text.starts_with(&bo)),
        "{waiting:?}"
    );
    let manual = body(chain, &request)["system"][1]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        manual.contains(&format!(
            "- Conversation: conversation {thread}. It is your canonical conversation: every message to you arrives here"
        )),
        "{manual}"
    );
    assert!(bound(chain, world.agent).is_empty());
}

#[test]
fn only_the_manager_starts_a_new_conversation_and_the_old_one_stays_readable() {
    let world = world();
    let chain = &world.chain;
    let first = answered(&world, "m1", "remember the plan");
    let before = frames(chain, first);
    let new = Op::NewThread { agent: world.agent };
    let refused = renewed(chain, BO, &new).unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);

    let second = renewed(chain, ADA, &new).unwrap();
    assert_ne!(second, first);
    assert_eq!(current(chain, world.agent), Some(second));
    let old = thread_row(chain, first);
    assert!(
        old.closed,
        "an idle conversation that is no longer current closes"
    );
    assert_eq!(frames(chain, first), before);

    world.mention("m2", "what plan?");
    assert!(
        frames(chain, first) == before,
        "the old conversation hears nothing more"
    );
    let [Body::Post { text, .. }] = &frames(chain, second)[..] else {
        panic!("{:?}", frames(chain, second));
    };
    assert!(text.ends_with("what plan?"), "{text}");
    assert_eq!(thread_row(chain, second).place, "");
}

#[test]
fn whom_the_agent_answers_does_not_start_its_conversation_but_a_list_on_the_conversation_does() {
    let world = world();
    let chain = &world.chain;
    let new = Op::NewThread { agent: world.agent };
    set_list(
        chain,
        ADA,
        PROGRAM,
        crate::agent_key(world.agent),
        ::program::Ownership::Anyone,
    );
    let refused = renewed(chain, BO, &new).unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);

    set_list(
        chain,
        ADA,
        PROGRAM,
        crate::conversation_key(world.agent),
        ::program::Ownership::Listed(vec![::program::Party::Account(world.bo)]),
    );
    let started = renewed(chain, BO, &new).unwrap();
    assert_eq!(current(chain, world.agent), Some(started));
}

#[test]
fn a_conversation_for_an_account_that_is_no_agent_is_not_found() {
    let world = world();
    let refused = renewed(&world.chain, ADA, &Op::NewThread { agent: world.bo }).unwrap_err();
    assert_eq!(refused.code, code::NOT_FOUND);
}

#[test]
fn a_conversation_superseded_mid_answer_finishes_its_answer_then_closes() {
    let world = world();
    let chain = &world.chain;
    let first = world.opened("m1", "long question", Some(WINDOW));
    let second = renewed(chain, ADA, &Op::NewThread { agent: world.agent }).unwrap();
    assert!(!thread_row(chain, first).closed, "its loop is still asking");
    world.mention("m2", "a fresh start");
    assert_eq!(assigned(chain, SERVICE).len(), 2);
    chain.at(3, 0);
    let late = reply(
        json!([{ "type": "text", "text": "Long answer." }]),
        "end_turn",
        10,
    );
    answer(chain, asked_for(chain, first).id, Answer::Output(late));
    let old = thread_row(chain, first);
    assert!(old.closed);
    assert!(matches!(
        frames(chain, first).last(),
        Some(Body::Turn { .. })
    ));
    assert_eq!(current(chain, world.agent), Some(second));
    assert!(!thread_row(chain, second).closed);
}

#[test]
fn a_fork_begins_from_the_current_conversations_context() {
    let world = world();
    let chain = &world.chain;
    let refused = renewed(chain, ADA, &Op::ForkThread { agent: world.agent }).unwrap_err();
    assert_eq!(refused.code, code::NOT_FOUND);
    let first = answered(&world, "m1", "the code word is heron");
    let fork = renewed(chain, ADA, &Op::ForkThread { agent: world.agent }).unwrap();
    assert_eq!(frames(chain, fork), frames(chain, first));
    let row = thread_row(chain, fork);
    assert_eq!(
        row.title,
        format!("conversation {fork}, forked from conversation {first}")
    );
    assert_eq!((row.frames, row.seen), (2, 1));
    assert!(thread_row(chain, first).closed);
    assert_eq!(current(chain, world.agent), Some(fork));

    world.mention("m2", "what was the word?");
    let (request, _) = open_request(chain);
    let said = texts(body(chain, &request)["messages"].as_array().unwrap());
    assert!(said[0].ends_with("the code word is heron"), "{said:?}");
    assert_eq!(said[1], "Noted.");
    assert!(said[2].ends_with("what was the word?"), "{said:?}");
}

#[test]
fn a_fork_leaves_out_a_turn_whose_calls_have_not_all_come_back() {
    let world = world();
    let chain = &world.chain;
    let first = world.opened("m1", "list the files", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([tool_use("toolu_1", "files_list", json!({ "path": "/" }))]),
        "tool_use",
    );
    let fork = renewed(chain, ADA, &Op::ForkThread { agent: world.agent }).unwrap();
    let copied = frames(chain, fork);
    assert!(matches!(&copied[..], [Body::Post { .. }]), "{copied:?}");
    assert_eq!(thread_row(chain, fork).seen, 1);
    assert!(!thread_row(chain, first).closed, "its call is still out");
}

#[test]
fn after_a_swap_the_agents_most_recently_active_open_idle_thread_becomes_current() {
    let world = world();
    let chain = &world.chain;
    let general = answered(&world, "m1", "from general");
    let release = renewed(chain, ADA, &Op::NewThread { agent: world.agent }).unwrap();
    let dead = renewed(chain, ADA, &Op::NewThread { agent: world.agent }).unwrap();
    let host = chain.host(PROGRAM);
    let ctx = host.exec(chain.env(PROGRAM));
    let reopen = |id: u64, asking: Asking, last_height: u64| {
        let mut row = THREADS.get(&ctx, &id).unwrap().unwrap();
        row.closed = false;
        row.asking = asking;
        row.last_height = last_height;
        THREADS.put(&ctx, &id, &row);
    };
    reopen(general, Asking::Idle, 50);
    reopen(release, Asking::Idle, 40);
    reopen(dead, Asking::Asked { upto: 0 }, 60);
    CURRENT.remove(&ctx, &world.agent);

    assert_eq!(current(chain, world.agent), Some(general));
    let before = frames(chain, general).len();
    world.mention("m2", "after the swap");
    assert_eq!(current(chain, world.agent), Some(general));
    let bodies = frames(chain, general);
    assert_eq!(bodies.len(), before + 1);
    let Some(Body::Post { text, .. }) = bodies.last() else {
        panic!("{bodies:?}");
    };
    assert!(text.ends_with("after the swap"), "{text}");
    assert!(
        frames(chain, release).is_empty(),
        "the other thread stays as it was"
    );
    assert!(!thread_row(chain, release).closed);
    assert!(frames(chain, dead).is_empty());
}

fn topic(chain: &MockChain, thread: u64) -> Option<String> {
    let Reply::Topic(topic) = chain.query(PROGRAM, &Query::Topic { thread }).unwrap() else {
        panic!()
    };
    topic
}

fn manual(chain: &MockChain) -> String {
    let (request, _) = open_request(chain);
    body(chain, &request)["system"][1]["text"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn the_agent_says_in_one_line_what_its_conversation_is_about_and_the_manual_names_its_tool() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "why is payments slow?", Some(WINDOW));
    let before = manual(chain);
    assert!(
        before.contains("- Topic: thread_topic sets one line on what this conversation is about"),
        "{before}"
    );
    assert_eq!(topic(chain, thread), None);
    chain.at(3, 0);
    let calls = json!([
        tool_use(
            "toolu_t",
            "thread_topic",
            json!({ "topic": "  Finding why payments is slow " })
        ),
        tool_use("toolu_two", "thread_topic", json!({ "topic": "one\ntwo" })),
        tool_use("toolu_blank", "thread_topic", json!({ "topic": "  " })),
        tool_use(
            "toolu_long",
            "thread_topic",
            json!({ "topic": "x".repeat(121) })
        ),
    ]);
    model_says(chain, calls, "tool_use");
    chain.at(4, 0);
    for call in ["toolu_t", "toolu_two", "toolu_blank", "toolu_long"] {
        relay_back(chain, call);
    }
    assert_eq!(
        result_of(chain, thread, "toolu_t"),
        (
            r#"{"topic":"Finding why payments is slow"}"#.to_owned(),
            false
        )
    );
    let (two, failed) = result_of(chain, thread, "toolu_two");
    assert!(failed && two.contains("a topic is one line"), "{two}");
    let (blank, failed) = result_of(chain, thread, "toolu_blank");
    assert!(
        failed && blank.contains("a topic is a line of words"),
        "{blank}"
    );
    let (long, failed) = result_of(chain, thread, "toolu_long");
    assert!(
        failed && long.contains("a topic is at most 120 characters; this one is 121"),
        "{long}"
    );
    assert_eq!(
        topic(chain, thread).as_deref(),
        Some("Finding why payments is slow"),
        "a refused topic keeps the one before"
    );
    assert_eq!(
        manual(chain),
        before,
        "the system prompt stays as it was, so its cache holds"
    );

    // a fork goes on with what it was about
    chain.at(5, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Found it." }]),
        "end_turn",
    );
    let fork = renewed(chain, ADA, &Op::ForkThread { agent: world.agent }).unwrap();
    assert_eq!(topic(chain, fork), topic(chain, thread));
}

#[test]
fn two_bound_conversations_run_at_once_and_keep_their_inputs_calls_and_context_apart() {
    let world = world();
    let chain = &world.chain;
    let canonical = answered(&world, "m1", "hello from general");
    let before = frames(chain, canonical);
    room(&world, "j-a");
    room(&world, "j-b");
    let a = bind(&world, ADA, in_room("j-a")).unwrap();
    let b = bind(&world, ADA, in_room("j-b")).unwrap();
    assert_eq!(current(chain, world.agent), Some(canonical));
    say_in(&world, "j-a", "a1", "list the root");
    say_in(&world, "j-b", "b1", "say hi");
    assert_eq!(
        assigned(chain, SERVICE).len(),
        2,
        "both conversations ask at once"
    );
    let (to_a, to_b) = (asked_for(chain, a), asked_for(chain, b));
    let manual = body(chain, &to_a)["system"][1]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        manual.contains(&format!(
            "- Conversation: conversation {a}, bound to chat channel j-a. It is one of several conversations you hold at once"
        )),
        "{manual}"
    );
    assert!(
        manual.contains("Answer there: chat_post to channel j-a"),
        "{manual}"
    );
    let to_b_said = texts(body(chain, &to_b)["messages"].as_array().unwrap());
    assert!(
        to_b_said.iter().all(|text| !text.contains("list the root")),
        "{to_b_said:?}"
    );

    chain.at(4, 0);
    let call = tool_use("toolu_a", "files_list", json!({ "path": "/" }));
    answer(
        chain,
        to_a.id,
        Answer::Output(reply(json!([call]), "tool_use", 10)),
    );
    answer(
        chain,
        to_b.id,
        Answer::Output(reply(says("Hi."), "end_turn", 20)),
    );
    assert_eq!(calls(chain, a), ["toolu_a"]);
    assert!(calls(chain, b).is_empty());
    assert_eq!(thread_row(chain, a).context, Context::Measured(18));
    assert_eq!(thread_row(chain, b).context, Context::Measured(28));
    let idle = thread_row(chain, b);
    assert_eq!((idle.asking, idle.closed), (Asking::Idle, false));

    chain.at(5, 0);
    relay_back(chain, "toolu_a");
    let again = asked_for(chain, a);
    let said = texts(body(chain, &again)["messages"].as_array().unwrap());
    assert!(said[0].ends_with("list the root"), "{said:?}");
    assert!(said.iter().all(|text| !text.contains("say hi")), "{said:?}");
    let [heard_a] = &posts(chain, a)[..] else {
        panic!("{:?}", posts(chain, a));
    };
    assert!(heard_a.ends_with("list the root"), "{heard_a}");
    let [heard_b] = &posts(chain, b)[..] else {
        panic!("{:?}", posts(chain, b));
    };
    assert!(heard_b.ends_with("say hi"), "{heard_b}");
    assert_eq!(frames(chain, canonical), before);
}

#[test]
fn an_input_goes_to_the_conversation_bound_to_its_thread_then_its_channel_else_the_canonical_one() {
    let world = world();
    let chain = &world.chain;
    room(&world, "j-a");
    say_in(&world, "j-a", "a1", "start a thread");
    let canonical = current(chain, world.agent).expect("the first mention opens the canonical one");
    let whole = bind(&world, ADA, in_room("j-a")).unwrap();
    let within = Place::Thread {
        channel: "j-a".into(),
        root: 1,
    };
    let within = bind(&world, ADA, within).unwrap();
    reply_in(&world, "j-a", "a2", 1, "in the thread");
    say_in(&world, "j-a", "a3", "on top");
    world.post(
        ADA,
        "general",
        "g1",
        &format!("<@{}> elsewhere", world.agent),
    );

    let [in_thread] = &posts(chain, within)[..] else {
        panic!("{:?}", posts(chain, within));
    };
    assert!(in_thread.ends_with("in the thread"), "{in_thread}");
    let [on_top] = &posts(chain, whole)[..] else {
        panic!("{:?}", posts(chain, whole));
    };
    assert!(on_top.ends_with("on top"), "{on_top}");
    let heard = posts(chain, canonical);
    assert_eq!(heard.len(), 2, "{heard:?}");
    assert!(heard[0].ends_with("start a thread"), "{heard:?}");
    assert!(heard[1].ends_with("elsewhere"), "{heard:?}");
    assert_eq!(current(chain, world.agent), Some(canonical));
    let mut listed = bound(chain, world.agent);
    listed.sort();
    assert_eq!(listed, [whole, within]);
    assert_eq!(thread_at(chain, world.agent, "chat/j-a/1"), Some(within));
    assert_eq!(thread_at(chain, world.agent, "chat/j-a"), Some(whole));
    assert_eq!(
        thread_at(chain, world.agent, "chat/general"),
        Some(canonical)
    );
    assert_eq!(
        thread_row(chain, within).title,
        format!("conversation {within}, bound to the thread of message 1 in chat channel j-a")
    );
}

#[test]
fn closing_a_bound_conversation_sends_its_place_back_to_the_canonical_one() {
    let world = world();
    let chain = &world.chain;
    room(&world, "j-a");
    let a = bind(&world, ADA, in_room("j-a")).unwrap();
    say_in(&world, "j-a", "a1", "first");
    assert_eq!(posts(chain, a).len(), 1);
    assert_eq!(
        current(chain, world.agent),
        None,
        "a bound conversation is never the canonical one"
    );
    chain
        .submit(signed(ADA), PROGRAM, &Op::Close { thread: a })
        .unwrap();
    assert!(bound(chain, world.agent).is_empty());

    say_in(&world, "j-a", "a2", "second");
    let canonical = current(chain, world.agent).expect("the place's input opens the canonical one");
    assert_ne!(canonical, a);
    let [heard] = &posts(chain, canonical)[..] else {
        panic!("{:?}", posts(chain, canonical));
    };
    assert!(heard.ends_with("second"), "{heard}");
    assert_eq!(
        posts(chain, a).len(),
        1,
        "the closed conversation hears nothing more"
    );
    assert_eq!(thread_at(chain, world.agent, "chat/j-a"), Some(canonical));
    let again = bind(&world, ADA, in_room("j-a")).unwrap();
    assert!(again != a && again != canonical);
}

#[test]
fn a_message_to_one_running_conversation_is_held_there_and_the_other_never_sees_it() {
    let world = world();
    let chain = &world.chain;
    answered(&world, "m1", "hello");
    room(&world, "j-a");
    room(&world, "j-b");
    let a = bind(&world, ADA, in_room("j-a")).unwrap();
    let b = bind(&world, ADA, in_room("j-b")).unwrap();
    say_in(&world, "j-a", "a1", "task a");
    say_in(&world, "j-b", "b1", "task b");
    let (to_a, to_b) = (asked_for(chain, a), asked_for(chain, b));
    say_in(&world, "j-a", "a2", "STEER: change course");
    let [steer] = &held(chain, a)[..] else {
        panic!("{:?}", held(chain, a));
    };
    assert!(steer.text().ends_with("STEER: change course"));
    assert!(held(chain, b).is_empty());

    chain.at(4, 0);
    answer(
        chain,
        to_b.id,
        Answer::Output(reply(says("Done with b."), "end_turn", 10)),
    );
    assert_eq!(
        assigned(chain, SERVICE).len(),
        1,
        "b has nothing more to ask"
    );
    answer(
        chain,
        to_a.id,
        Answer::Output(reply(says("On a."), "end_turn", 10)),
    );
    let next = asked_for(chain, a);
    let said = texts(body(chain, &next)["messages"].as_array().unwrap());
    assert!(
        said.last().unwrap().ends_with("STEER: change course"),
        "{said:?}"
    );
    let [only] = &posts(chain, b)[..] else {
        panic!("{:?}", posts(chain, b));
    };
    assert!(only.ends_with("task b"), "{only}");
}

#[test]
fn anyone_opens_a_conversation_once_at_a_place_that_exists() {
    let world = world();
    let chain = &world.chain;
    room(&world, "j-a");
    let a = bind(&world, BO, in_room("j-a")).unwrap();
    let row = thread_row(chain, a);
    assert_eq!((row.place.as_str(), row.manager), ("chat/j-a", world.ada));
    assert_eq!(
        row.title,
        format!("conversation {a}, bound to chat channel j-a")
    );
    assert_eq!(
        current(chain, world.agent),
        None,
        "opening one leaves the canonical conversation alone"
    );

    let twice = bind(&world, ADA, in_room("j-a")).unwrap_err();
    assert_eq!(twice.code, code::ALREADY_EXISTS);
    assert!(
        twice.message.contains(&format!("conversation {a}")),
        "{}",
        twice.message
    );
    let nowhere = bind(&world, ADA, in_room("nowhere")).unwrap_err();
    assert_eq!(nowhere.code, code::NOT_FOUND);
    let rootless = Place::Thread {
        channel: "j-a".into(),
        root: 9,
    };
    assert_eq!(
        bind(&world, ADA, rootless).unwrap_err().code,
        code::NOT_FOUND
    );
    let no_agent = Op::OpenThread {
        agent: world.bo,
        place: in_room("j-a"),
    };
    assert_eq!(
        renewed(chain, ADA, &no_agent).unwrap_err().code,
        code::NOT_FOUND
    );
    assert_eq!(bound(chain, world.agent), [a]);
}

#[test]
fn a_bound_conversation_never_becomes_the_canonical_one_after_a_swap() {
    let world = world();
    let chain = &world.chain;
    let canonical = answered(&world, "m1", "hello");
    room(&world, "j-a");
    let a = bind(&world, ADA, in_room("j-a")).unwrap();
    say_in(&world, "j-a", "a1", "work");
    chain.at(4, 0);
    let done = reply(says("Done."), "end_turn", 10);
    answer(chain, asked_for(chain, a).id, Answer::Output(done));
    let row = thread_row(chain, a);
    assert_eq!((row.asking, row.closed), (Asking::Idle, false));
    assert!(row.last_height > thread_row(chain, canonical).last_height);

    let host = chain.host(PROGRAM);
    let ctx = host.exec(chain.env(PROGRAM));
    CURRENT.remove(&ctx, &world.agent);
    assert_eq!(current(chain, world.agent), Some(canonical));
}

#[test]
fn anyone_closes_a_bound_conversation_but_only_the_manager_the_canonical_one() {
    let world = world();
    let chain = &world.chain;
    let canonical = answered(&world, "m1", "hello");
    room(&world, "j-a");
    let a = bind(&world, ADA, in_room("j-a")).unwrap();

    let refused = chain
        .submit(signed(BO), PROGRAM, &Op::Close { thread: canonical })
        .unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
    assert!(!thread_row(chain, canonical).closed);

    chain
        .submit(signed(BO), PROGRAM, &Op::Close { thread: a })
        .unwrap();
    assert!(thread_row(chain, a).closed);
    assert!(bound(chain, world.agent).is_empty());

    chain
        .submit(signed(ADA), PROGRAM, &Op::Close { thread: canonical })
        .unwrap();
    assert!(thread_row(chain, canonical).closed);
}
