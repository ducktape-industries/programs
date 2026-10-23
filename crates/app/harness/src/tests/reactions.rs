//! A reaction on the agent's own message: a light input that rides its next turn and starts none,
//! unless it ends a message wait; one taken back is never heard.
use super::schedule::{answered, schemed, turn};
use super::*;

const ASKED: &str = "shall I ship it?";

fn says(text: &str) -> Value {
    json!([{ "type": "text", "text": text }])
}

fn light(chain: &MockChain, thread: u64) -> Vec<String> {
    frames(chain, thread)
        .into_iter()
        .chain(held(chain, thread))
        .filter_map(|body| match body {
            Body::Light { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

fn react(world: &World, key: &[u8], seq: u64, on: bool) {
    let (channel_id, emoji) = ("general".to_owned(), "👍".to_owned());
    let op = match on {
        true => chat::Op::AddReaction {
            channel_id,
            seq,
            emoji,
        },
        false => chat::Op::RemoveReaction {
            channel_id,
            seq,
            emoji,
        },
    };
    world.chain.submit(signed(key), chat::PROGRAM, &op).unwrap();
}

fn own_post(world: &World) -> u64 {
    let asked = chat::Query::Roots {
        channel_id: "general".into(),
        viewer: Vec::new(),
        page: PageRequest::default(),
    };
    let chat::Reply::Roots(page) = world.chain.query(chat::PROGRAM, &asked).unwrap() else {
        panic!("chat answers roots")
    };
    let mine = page
        .items
        .iter()
        .find(|row| row.author.account() == Some(world.agent));
    mine.expect("the agent posted").seq
}

fn posting() -> Value {
    json!({ "channel": "general", "text": ASKED })
}

fn reacted(world: &World, seq: u64) -> String {
    format!(
        "Ada (account {}) reacted 👍 to your message {seq} in chat channel general:\n{ASKED}",
        world.ada
    )
}

#[test]
fn a_reaction_on_the_agents_message_rides_its_next_turn_and_starts_none() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "ask Ada", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([tool_use("toolu_a", "chat_post", posting())]),
        "tool_use",
    );
    chain.at(4, 0);
    relay_back(chain, "toolu_a");
    chain.at(5, 0);
    model_says(chain, says("Asked."), "end_turn");
    chain.at(6, 0);
    model_says(chain, says("It is posted."), "end_turn");
    let seq = own_post(&world);
    react(&world, ADA, seq, true);
    assert_eq!(light(chain, thread), [reacted(&world, seq)]);
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "a reaction starts no turn"
    );
    assert!(!thread_row(chain, thread).due());

    chain.at(7, 0);
    react(&world, ADA, seq, false);
    assert_eq!(
        light(chain, thread).len(),
        1,
        "taking it back tells nothing"
    );
    assert!(assigned(chain, SERVICE).is_empty());

    world.mention("m3", "well?");
    let (request, _) = open_request(chain);
    let sent = provider_body(chain, &request)["messages"].to_string();
    assert!(sent.contains("reacted 👍 to your message"), "{sent}");
}

#[test]
fn a_reaction_heard_while_a_turn_runs_rides_it_and_asks_nothing_after() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "ask Ada", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([tool_use("toolu_a", "chat_post", posting())]),
        "tool_use",
    );
    chain.at(4, 0);
    relay_back(chain, "toolu_a");
    assert_eq!(assigned(chain, SERVICE).len(), 1, "the turn goes on");
    react(&world, ADA, own_post(&world), true);
    assert_eq!(light(chain, thread).len(), 1, "held behind the turn");
    chain.at(5, 0);
    model_says(chain, says("Asked."), "end_turn");
    let (request, _) = open_request(chain);
    let sent = provider_body(chain, &request)["messages"].to_string();
    assert!(sent.contains("reacted 👍 to your message"), "{sent}");
    chain.at(6, 0);
    model_says(chain, says("It is posted."), "end_turn");
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "the held reaction rides the result's ask and asks nothing after"
    );
    assert!(!thread_row(chain, thread).due());
    assert_eq!(light(chain, thread).len(), 1);
}

#[test]
fn a_reaction_ends_a_wait_for_a_message_addressed_to_the_agent_and_wakes_it() {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "ask Ada", Some(WINDOW));
    let wait = json!({ "event": { "message": { "channel": "general", "mentions_me": true } } });
    turn(
        &world,
        3,
        &[("p1", "chat_post", posting()), ("w1", "wait_for", wait)],
    );
    let wait = answered(chain, thread, "w1")["wait"].as_u64().unwrap();
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "the turn that armed a wait ends"
    );
    chain.at(5, 0);
    let seq = own_post(&world);
    react(&world, ADA, seq, true);
    assert_eq!(
        assigned(chain, SERVICE).len(),
        1,
        "the reaction ends the wait"
    );
    let over = format!("Wait {wait} is over: {}", reacted(&world, seq));
    let woke = frames(chain, thread)
        .into_iter()
        .chain(held(chain, thread))
        .any(|body| matches!(body, Body::Post { text, .. } if text.starts_with(&over)));
    assert!(woke, "{:?}", frames(chain, thread));
    assert!(light(chain, thread).is_empty(), "heard once, as the wake");
}
