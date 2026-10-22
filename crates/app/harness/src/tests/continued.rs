//! A mention's run: once a post that mentions the agent reaches it, its author's very next posts
//! in that room reach it too, until anyone else posts there; a listened room keeps none, and a
//! continuation waits out the settle window as other general input does.
use super::listen::{CHIEF, Heard};
use super::schedule::{answered, schemed, turn};
use super::*;
use crate::state::RUNS;

const SETTLE: u64 = 10_000;
const BRIDGE: &str = "the operator is fixing the bridge";

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

fn heard_of(chain: &MockChain, thread: u64, words: &str) -> Vec<String> {
    let posts = posts(chain, thread).into_iter();
    posts.filter(|text| text.contains(words)).collect()
}

fn watches(chain: &MockChain, agent: AccountNumber) -> bool {
    let asked = chat::Query::Watching {
        account: agent,
        page: PageRequest::default(),
    };
    let chat::Reply::Watching(page) = chain.query(chat::PROGRAM, &asked).unwrap() else {
        panic!("chat answers what an account watches")
    };
    page.items.iter().any(|watched| watched == "general")
}

fn runs(chain: &MockChain, thread: u64) -> Vec<RunRow> {
    let Reply::Runs(runs) = chain.query(PROGRAM, &Query::Runs { thread }).unwrap() else {
        panic!("harness answers runs")
    };
    runs
}

fn run(world: &World, mentioned: u64, last: u64) -> RunRow {
    RunRow {
        channel: "general".into(),
        author: world.ada,
        mentioned,
        last,
    }
}

fn went_on(world: &World, seq: u64, mentioned: u64) -> String {
    format!(
        "Ada (account {}) went on in chat channel general, message {seq}, after mentioning you \
         in message {mentioned}",
        world.ada
    )
}

fn says(text: &str) -> Value {
    json!([{ "type": "text", "text": text }])
}

fn idle(world: &World, config: Config) -> u64 {
    commit(&world.chain, ADA, CONFIG, &md(&config));
    let thread = world.opened("m1", BRIDGE, Some(WINDOW));
    world.chain.at(3, 0);
    model_says(&world.chain, says("Noted."), "end_turn");
    assert!(assigned(&world.chain, SERVICE).is_empty());
    thread
}

fn threads_of(chain: &MockChain, agent: AccountNumber) -> Vec<ThreadRow> {
    let asked = Query::Threads {
        agent,
        page: PageRequest::default(),
    };
    let Reply::Threads(page) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!()
    };
    page.items
}

fn message_wait(mentions_me: bool) -> Value {
    json!({ "event": { "message": { "channel": "general", "mentions_me": mentions_me } } })
}

#[test]
fn a_burst_after_a_mention_reaches_the_agent_whole_and_names_the_mention() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", BRIDGE, Some(WINDOW));
    world.post(ADA, "general", "a2", "i have an idea, talk to me on drafts");
    world.post(ADA, "general", "a3", "also move this to drafts/narrative");
    let heard = posts(chain, thread);
    let [first, second, third] = &heard[..] else {
        panic!("{heard:?}")
    };
    assert!(first.ends_with(BRIDGE), "{first}");
    assert!(second.starts_with(&went_on(&world, 2, 1)), "{second}");
    assert!(second.ends_with("talk to me on drafts"), "{second}");
    assert!(third.starts_with(&went_on(&world, 3, 1)), "{third}");
    assert!(third.ends_with("move this to drafts/narrative"), "{third}");
    assert_eq!(runs(chain, thread), [run(&world, 1, 3)]);
    assert!(watches(chain, world.agent));
    let key = RUNS.key(&(world.agent, "general".to_owned()));
    assert_eq!(written(&key), Some(Written::Runs(world.agent)));
    assert_eq!(
        written(&Written::Runs(world.agent).key()),
        Some(Written::Runs(world.agent))
    );
}

#[test]
fn a_post_by_anyone_else_ends_the_run_and_its_watch() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", BRIDGE, Some(WINDOW));
    world.post(BO, "general", "b2", "something else");
    world.post(ADA, "general", "a3", "and one more thing");
    let heard = posts(chain, thread);
    assert_eq!(heard.len(), 1, "{heard:?}");
    assert!(runs(chain, thread).is_empty());
    assert!(!watches(chain, world.agent), "the run's watch ends with it");
}

#[test]
fn the_agents_own_reply_ends_the_run() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", BRIDGE, Some(WINDOW));
    chain.at(3, 0);
    let answer = json!({ "channel": "general", "text": "on it" });
    let posting = json!([tool_use("toolu_a", "chat_post", answer)]);
    model_says(chain, posting, "tool_use");
    chain.at(4, 0);
    relay_back(chain, "toolu_a");
    chain.at(5, 0);
    model_says(chain, says("Posted."), "end_turn");
    assert!(runs(chain, thread).is_empty(), "its answer ends the run");
    assert!(!watches(chain, world.agent));
    world.post(ADA, "general", "a3", "and one more thing");
    assert!(heard_of(chain, thread, "and one more thing").is_empty());
    assert!(runs(chain, thread).is_empty());
    assert!(!watches(chain, world.agent));
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "nothing asks the model"
    );
}

#[test]
fn a_burst_that_never_mentions_the_agent_reaches_nothing_and_watches_nothing() {
    let world = world();
    let chain = &world.chain;
    world.post(
        ADA,
        "general",
        "a1",
        &format!("<@{}> look at this", world.bo),
    );
    world.post(ADA, "general", "a2", "and this");
    world.post(ADA, "general", "a3", "and this too");
    assert!(world.threads().is_empty());
    assert!(!watches(chain, world.agent));
}

#[test]
fn a_post_harness_never_heard_ends_the_run_at_the_authors_next() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", BRIDGE, Some(WINDOW));
    let account = world.agent;
    identity_op(chain, ADA, identity::Op::Suspend { account }).unwrap();
    world.post(BO, "general", "b2", "while it was away");
    identity_op(chain, ADA, identity::Op::Resume { account }).unwrap();
    world.post(ADA, "general", "a3", "and one more thing");
    assert!(heard_of(chain, thread, "and one more thing").is_empty());
    assert!(runs(chain, thread).is_empty());
    assert!(!watches(chain, world.agent));
}

#[test]
fn in_a_room_the_agent_listens_to_a_post_after_a_mention_is_heard_once_as_it_is_today() {
    let world = world();
    let chain = &world.chain;
    let listen = chat::Op::ChooseForMany {
        accounts: vec![world.agent],
        channel_id: "general".into(),
        listening: true,
        notices: None,
    };
    chain.submit(signed(ADA), chat::PROGRAM, &listen).unwrap();
    let thread = idle(&world, scout());
    assert!(
        runs(chain, thread).is_empty(),
        "a listened room keeps no run"
    );
    assert!(!watches(chain, world.agent));
    world.post(ADA, "general", "a2", "first part");
    let heard = heard_of(chain, thread, "first part");
    let [once] = &heard[..] else {
        panic!("{heard:?}")
    };
    let listened = format!(
        "Ada (account {}) wrote in chat channel general, which you listen to, message 2 ",
        world.ada
    );
    assert!(once.starts_with(&listened), "{once}");
    assert!(runs(chain, thread).is_empty());
    assert!(!watches(chain, world.agent));
}

fn block(chain: &MockChain, height: u64) {
    chain.at(height, SETTLE + (height - 4) * 2_000);
}

fn settling(chain: &MockChain, thread: u64) -> Option<SettleWindow> {
    let asked = Query::Settling { thread };
    let Reply::Settling(window) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!()
    };
    window
}

#[test]
fn a_continuation_waits_out_the_settle_window_so_a_burst_makes_one_turn() {
    let world = world();
    let chain = &world.chain;
    let settles = Config {
        settle: Some(SETTLE),
        ..scout()
    };
    let thread = idle(&world, settles);
    block(chain, 4);
    world.post(ADA, "general", "a2", "first part");
    assert!(assigned(chain, SERVICE).is_empty(), "a continuation waits");
    assert!(settling(chain, thread).is_some());
    block(chain, 5);
    world.post(ADA, "general", "a3", "second part");
    for height in 6..=9 {
        block(chain, height);
        assert!(assigned(chain, SERVICE).is_empty(), "block {height}");
    }
    block(chain, 10);
    assert_eq!(settling(chain, thread), None);
    assert_eq!(
        assigned(chain, SERVICE).len(),
        1,
        "one turn hears the burst"
    );
    let (request, _) = open_request(chain);
    let sent = provider_body(chain, &request)["messages"].to_string();
    assert!(
        sent.contains("first part") && sent.contains("second part"),
        "{sent}"
    );
}

#[test]
fn a_message_wait_in_the_room_ends_on_a_continuation_heard_once_and_the_run_keeps_watching() {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", BRIDGE, Some(WINDOW));
    turn(&world, 3, &[("w1", "wait_for", message_wait(false))]);
    let wait = answered(chain, thread, "w1")["wait"].as_u64().unwrap();
    chain.at(5, 0);
    world.post(ADA, "general", "a2", "first part");
    let heard = heard_of(chain, thread, "first part");
    let [wake] = &heard[..] else {
        panic!("heard once, as the wake: {heard:?}")
    };
    let over = format!("Wait {wait} is over: {}", went_on(&world, 2, 1));
    assert!(wake.starts_with(&over), "{wake}");
    assert_eq!(runs(chain, thread), [run(&world, 1, 2)]);
    assert!(watches(chain, world.agent), "the run still watches");
    world.post(BO, "general", "b3", "something else");
    assert!(runs(chain, thread).is_empty());
    assert!(!watches(chain, world.agent));
}

#[test]
fn a_wait_for_a_mention_stays_through_a_continuation_and_keeps_the_room_watched_after_the_run() {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", BRIDGE, Some(WINDOW));
    turn(&world, 3, &[("w1", "wait_for", message_wait(true))]);
    let wait = answered(chain, thread, "w1")["wait"].as_u64().unwrap();
    chain.at(5, 0);
    world.post(ADA, "general", "a2", "first part");
    let heard = heard_of(chain, thread, "first part");
    let [input] = &heard[..] else {
        panic!("{heard:?}")
    };
    assert!(input.starts_with(&went_on(&world, 2, 1)), "{input}");
    let asked = Query::Armed {
        thread,
        page: PageRequest::default(),
    };
    let Reply::Waits(armed) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!()
    };
    let armed: Vec<u64> = armed.items.iter().map(|row| row.id).collect();
    assert_eq!(armed, [wait], "a continuation does not mention the agent");
    world.post(BO, "general", "b3", "something else");
    assert!(runs(chain, thread).is_empty());
    assert!(watches(chain, world.agent), "the wait still watches");
}

#[test]
fn a_run_open_across_a_swap_still_delivers_and_still_ends() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", BRIDGE, Some(WINDOW));
    chain.cancel_parked(PROGRAM);
    world.post(ADA, "general", "a2", "first part");
    let heard = heard_of(chain, thread, "first part");
    let [input] = &heard[..] else {
        panic!("{heard:?}")
    };
    assert!(input.starts_with(&went_on(&world, 2, 1)), "{input}");
    world.post(BO, "general", "b3", "something else");
    world.post(ADA, "general", "a4", "late");
    assert!(heard_of(chain, thread, "late").is_empty());
    assert!(!watches(chain, world.agent));
}

#[test]
fn a_thread_reply_and_a_top_level_post_by_the_author_both_go_on() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", BRIDGE, Some(WINDOW));
    let reply = chat::Op::PostMessage {
        channel_id: "general".into(),
        message_id: "a2".into(),
        blocks: chat::parse_message("in the thread"),
        thread: Some(1),
    };
    chain.submit(signed(ADA), chat::PROGRAM, &reply).unwrap();
    world.post(ADA, "general", "a3", "and at the top");
    let heard = posts(chain, thread);
    let [_, reply, top] = &heard[..] else {
        panic!("{heard:?}")
    };
    let in_thread = format!(
        "{}, in the thread of message 1 (reply_to 1):",
        went_on(&world, 2, 1)
    );
    assert!(reply.starts_with(&in_thread), "{reply}");
    assert!(top.starts_with(&went_on(&world, 3, 1)), "{top}");
    assert_eq!(world.threads().len(), 1);
}

#[test]
fn an_author_who_mentions_two_agents_gives_each_its_own_run() {
    let world = world();
    let chain = &world.chain;
    let create = identity::Op::CreateAgent { name: "Pip".into() };
    let pip = number(identity_op(chain, ADA, create).unwrap());
    scripted(chain, pip, CONFIG);
    let both = format!("<@{}> <@{pip}> both of you", world.agent);
    world.post(ADA, "general", "m1", &both);
    world.post(ADA, "general", "a2", "first part");
    for agent in [world.agent, pip] {
        let rows = threads_of(chain, agent);
        let [row] = &rows[..] else {
            panic!("agent {agent}: {rows:?}")
        };
        assert_eq!(
            heard_of(chain, row.id, "first part").len(),
            1,
            "agent {agent}"
        );
        assert_eq!(runs(chain, row.id), [run(&world, 1, 2)], "agent {agent}");
        assert!(watches(chain, agent), "agent {agent}");
    }
    world.post(BO, "general", "b3", "something else");
    for agent in [world.agent, pip] {
        let row = threads_of(chain, agent)[0].id;
        assert!(runs(chain, row).is_empty(), "agent {agent}");
        assert!(!watches(chain, agent), "agent {agent}");
    }
}

#[test]
fn a_conversation_bound_to_the_room_hears_the_whole_burst() {
    let world = world();
    let chain = &world.chain;
    let open = Op::OpenThread {
        agent: world.agent,
        place: Place::Channel {
            channel: "general".into(),
        },
    };
    let bound: u64 = abi::decode(&chain.submit(signed(ADA), PROGRAM, &open).unwrap()).unwrap();
    world.post(
        ADA,
        "general",
        "m1",
        &format!("<@{}> {BRIDGE}", world.agent),
    );
    world.post(ADA, "general", "a2", "first part");
    let heard = posts(chain, bound);
    let [_, second] = &heard[..] else {
        panic!("{heard:?}")
    };
    assert!(second.starts_with(&went_on(&world, 2, 1)), "{second}");
    assert_eq!(runs(chain, bound), [run(&world, 1, 2)]);
    assert_eq!(world.threads().len(), 1, "no other conversation hears it");
}

#[test]
fn cut_to_mentions_a_continuation_still_arrives_and_turned_off_no_run_opens() {
    for (notices, heard) in [(chat::Notices::Mentions, 1), (chat::Notices::Off, 0)] {
        let world = world();
        let chain = &world.chain;
        let cut = chat::Op::ChooseForMany {
            accounts: vec![world.agent],
            channel_id: "general".into(),
            listening: false,
            notices: Some(notices),
        };
        chain.submit(signed(ADA), chat::PROGRAM, &cut).unwrap();
        world.post(
            ADA,
            "general",
            "m1",
            &format!("<@{}> {BRIDGE}", world.agent),
        );
        world.post(ADA, "general", "a2", "first part");
        let continued: usize = world
            .threads()
            .iter()
            .map(|row| heard_of(chain, row.id, "first part").len())
            .sum();
        assert_eq!(continued, heard, "{notices:?}");
        assert_eq!(watches(chain, world.agent), heard == 1, "{notices:?}");
    }
}

#[test]
fn an_answer_in_another_room_ends_the_run_and_its_watch() {
    let world = world();
    let chain = &world.chain;
    let random = chat::Op::CreateChannel {
        channel_id: "random".into(),
        name: "random".into(),
        post_policy: chat::PostPolicy::Open,
    };
    chain.submit(signed(ADA), chat::PROGRAM, &random).unwrap();
    let thread = world.opened("m1", BRIDGE, Some(WINDOW));
    chain.at(3, 0);
    let answer = json!({ "channel": "random", "text": "answered here" });
    let posting = json!([tool_use("toolu_a", "chat_post", answer)]);
    model_says(chain, posting, "tool_use");
    assert_eq!(
        runs(chain, thread),
        [run(&world, 1, 1)],
        "open until it posts"
    );
    chain.at(4, 0);
    relay_back(chain, "toolu_a");
    assert!(runs(chain, thread).is_empty(), "the answer ends the run");
    assert!(!watches(chain, world.agent));
    chain.at(5, 0);
    model_says(chain, says("Posted."), "end_turn");
    world.post(ADA, "general", "a2", "days later, another subject");
    assert!(heard_of(chain, thread, "another subject").is_empty());
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "nothing asks the model"
    );
}

#[test]
fn a_post_from_another_network_ends_the_run_and_a_mention_from_there_opens_none() {
    let roles = ::program::MockHost::roles_with(abi::role::CROSSPOND, "crosspond");
    let mut world = world_founded(PROGRAM, roles);
    world.chain.seat::<Heard>("crosspond");
    let chain = &world.chain;
    let thread = world.opened("m1", BRIDGE, Some(WINDOW));
    assert_eq!(runs(chain, thread), [run(&world, 1, 1)]);
    let link = chat::Op::Link {
        channel_id: "general".into(),
        peer: Some(chat::Peer {
            network: CHIEF.network,
            channel_id: "lobby".into(),
        }),
    };
    chain.submit(signed(ADA), chat::PROGRAM, &link).unwrap();
    let from_there = |id: &str, text: &str| chat::Op::FromPeer {
        channel_id: "general".into(),
        write: chat::PeerWrite::Post {
            message_id: id.into(),
            blocks: chat::parse_message(text),
            thread: None,
        },
    };
    let elsewhere = from_there("c2", "the bridge is fine");
    chain
        .submit_foreign(CHIEF, chat::PROGRAM, &elsewhere)
        .unwrap();
    assert!(
        runs(chain, thread).is_empty(),
        "the post from there ends the run"
    );
    assert!(!watches(chain, world.agent), "and its watch");
    let mention = from_there("c3", &format!("<@{}> are you there", world.agent));
    chain
        .submit_foreign(CHIEF, chat::PROGRAM, &mention)
        .unwrap();
    let reached: Vec<u64> = (world.threads().into_iter())
        .map(|row| row.id)
        .filter(|id| !heard_of(chain, *id, "are you there").is_empty())
        .collect();
    let [reached] = reached[..] else {
        panic!("the mention from there reaches one conversation: {reached:?}")
    };
    assert!(runs(chain, reached).is_empty(), "and opens no run");
    assert!(!watches(chain, world.agent));
}
