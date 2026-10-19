//! What an agent hears of a room is its choice there in chat: every message, its default
//! (mentions and replies in its threads), mentions only, or off. A direct message always reaches
//! it, and nothing it chose not to hear reaches a conversation of it or the model.
use super::*;

const PLAIN: &str = "plain words";
const MENTIONED: &str = "mentioned words";
const THREADED: &str = "threaded words";

#[derive(Clone, Copy, Debug)]
enum Choice {
    Every,
    Default,
    Mentions,
    Off,
}

// its manager makes the agent's choice for `channel`
fn choose(world: &World, channel: &str, choice: Choice) {
    let account = world.agent;
    let cut = |notices| chat::Op::SetNoticesFor {
        account,
        channel_id: channel.into(),
        notices,
    };
    let op = match choice {
        Choice::Every => chat::Op::ListenFor {
            account,
            channel_id: channel.into(),
            listening: true,
        },
        Choice::Default => cut(None),
        Choice::Mentions => cut(Some(chat::Notices::Mentions)),
        Choice::Off => cut(Some(chat::Notices::Off)),
    };
    world.chain.submit(signed(ADA), chat::PROGRAM, &op).unwrap();
}

// every body any conversation of the agent holds, framed or held
fn bodies(world: &World) -> Vec<Body> {
    let chain = &world.chain;
    let threads = world.threads().into_iter();
    threads
        .flat_map(|row| frames(chain, row.id).into_iter().chain(held(chain, row.id)))
        .collect()
}

fn posts(world: &World) -> Vec<String> {
    let posts = bodies(world).into_iter();
    posts
        .filter_map(|body| match body {
            Body::Post { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

// in #general, where the agent answered in the thread of its manager's first message: a plain
// post, a mention of it, and a reply in that thread, each by its manager
fn talk(world: &World, choice: Choice) {
    let chain = &world.chain;
    world.opened("m0", "start a thread", Some(WINDOW));
    chain.at(3, 0);
    let answer = json!({ "channel": "general", "text": "on it", "reply_to": 1 });
    model_says(
        chain,
        json!([tool_use("toolu_a", "chat_post", answer)]),
        "tool_use",
    );
    chain.at(4, 0);
    relay_back(chain, "toolu_a");
    chain.at(5, 0);
    let done = json!([{ "type": "text", "text": "Posted." }]);
    model_says(chain, done, "end_turn");
    assert!(assigned(chain, SERVICE).is_empty());
    choose(world, "general", choice);
    world.post(ADA, "general", "p1", PLAIN);
    let mention = format!("<@{}> {MENTIONED}", world.agent);
    world.post(ADA, "general", "m1", &mention);
    let reply = chat::Op::PostMessage {
        channel_id: "general".into(),
        message_id: "t1".into(),
        blocks: chat::parse_message(THREADED),
        thread: Some(1),
    };
    world
        .chain
        .submit(signed(ADA), chat::PROGRAM, &reply)
        .unwrap();
}

#[test]
fn each_choice_delivers_what_it_names_and_nothing_else_reaches_the_model() {
    let table = [
        (Choice::Every, [true, true, true]),
        (Choice::Default, [false, true, true]),
        (Choice::Mentions, [false, true, false]),
        (Choice::Off, [false, false, false]),
    ];
    let words = [PLAIN, MENTIONED, THREADED];
    for (choice, delivered) in table {
        let world = world();
        let chain = &world.chain;
        talk(&world, choice);
        let heard = posts(&world);
        for (words, delivered) in words.into_iter().zip(delivered) {
            let times = heard.iter().filter(|text| text.ends_with(words)).count();
            assert_eq!(
                times,
                usize::from(delivered),
                "{choice:?} {words}: {heard:?}"
            );
        }
        if !delivered.contains(&true) {
            assert!(
                assigned(chain, SERVICE).is_empty(),
                "{choice:?} asks nothing"
            );
            continue;
        }
        let (request, _) = open_request(chain);
        let sent = body(chain, &request).to_string();
        for (words, delivered) in words.into_iter().zip(delivered) {
            let reached = sent.contains(words);
            assert!(
                delivered || !reached,
                "{choice:?}: {words} reached the model"
            );
        }
        let first = match choice {
            Choice::Every => PLAIN,
            Choice::Default | Choice::Mentions | Choice::Off => MENTIONED,
        };
        assert!(sent.contains(first), "{choice:?}: {sent}");
    }
}

// a room turned off leaves no trace of what was said there, not even a notice of a mention from
// someone the agent doesn't answer, while a direct message reaches it though its room is off too
#[test]
fn a_direct_message_reaches_the_agent_whatever_its_rooms_say() {
    let world = world();
    let chain = &world.chain;
    let dm = chat::Op::CreateDmChannel {
        counterpart: world.agent,
        name: "us".into(),
    };
    chain.submit(signed(ADA), chat::PROGRAM, &dm).unwrap();
    let room = chat::dm_channel_id(world.ada, world.agent);
    choose(&world, "general", Choice::Off);
    choose(&world, &room, Choice::Off);
    let mention = format!("<@{}> {MENTIONED}", world.agent);
    world.post(ADA, "general", "m1", &mention);
    world.post(BO, "general", "b1", &mention);
    assert!(world.threads().is_empty(), "nothing of an off room");
    world.post(ADA, &room, "d1", "a direct word");
    let heard = posts(&world);
    let [direct] = &heard[..] else {
        panic!("{heard:?}")
    };
    assert!(direct.ends_with("a direct word"), "{direct}");
    let noticed = bodies(&world).into_iter();
    let noticed: Vec<Body> = noticed
        .filter(|body| matches!(body, Body::Notice { .. }))
        .collect();
    assert!(noticed.is_empty(), "{noticed:?}");
}

// an agent with no list on its key answers anyone, a stranger's mention included; a list its
// manager sets narrows it, and what it no longer answers leaves a notice and asks nothing
#[test]
fn a_new_agent_answers_a_stranger_and_a_list_narrows_it() {
    let world = world();
    let chain = &world.chain;
    let mention = format!("<@{}> {MENTIONED}", world.agent);
    world.post(BO, "general", "b1", &mention);
    let heard = posts(&world);
    let [from_bo] = &heard[..] else {
        panic!("{heard:?}")
    };
    let bo = format!("Bo (account {}) mentioned you", world.bo);
    assert!(from_bo.starts_with(&bo), "{from_bo}");
    assert_eq!(
        assigned(chain, SERVICE).len(),
        1,
        "a stranger's mention asks the model"
    );

    manager_only(&world);
    world.post(BO, "general", "b2", &format!("<@{}> again", world.agent));
    assert_eq!(posts(&world).len(), 1, "a list narrows it");
    let noticed: Vec<Body> = bodies(&world)
        .into_iter()
        .filter(|body| matches!(body, Body::Notice { .. }))
        .collect();
    let [Body::Notice { text }] = &noticed[..] else {
        panic!("{noticed:?}")
    };
    assert!(text.starts_with("dropped a mention from account"), "{text}");
}
