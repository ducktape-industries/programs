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

#[test]
fn every_person_and_every_place_continue_the_agents_one_conversation() {
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
            "- Conversation: conversation {thread}. It is your one conversation"
        )),
        "{manual}"
    );
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
