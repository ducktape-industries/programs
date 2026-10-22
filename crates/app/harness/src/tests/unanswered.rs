//! What a conversation was told and never answered where it was said stays the agent's: its close
//! hands it to the agent's live conversation for its place, in one input of the messages as first
//! heard, and the agent's post in that place lets it go.
use super::*;

const CARRIED: &str = "notice queue Note: 1 unanswered\n\n";

fn current(chain: &MockChain, agent: AccountNumber) -> Option<u64> {
    let Reply::Agent(Some(setup)) = chain.query(PROGRAM, &Query::Agent { agent }).unwrap() else {
        panic!()
    };
    setup.current
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

fn renewed(chain: &MockChain, op: &Op) -> u64 {
    let output = chain.submit(signed(ADA), PROGRAM, op).unwrap();
    abi::decode(&output).unwrap()
}

fn ends(chain: &MockChain, text: &str) {
    model_says(chain, json!([{ "type": "text", "text": text }]), "end_turn");
}

fn room(world: &World, id: &str) {
    let op = chat::Op::CreateChannel {
        channel_id: id.into(),
        name: id.into(),
        post_policy: chat::PostPolicy::Open,
    };
    world.chain.submit(signed(ADA), chat::PROGRAM, &op).unwrap();
}

fn posts_in(chain: &MockChain, channel: &str, call: &str) {
    let post = json!({ "channel": channel, "text": "on it" });
    model_says(
        chain,
        json!([tool_use(call, "chat_post", post)]),
        "tool_use",
    );
}

fn refused_for_good() -> Vec<u8> {
    let error = json!({ "type": "error", "error": { "type": "invalid_request_error", "message": "malformed request" } });
    response(400, error)
}

#[test]
fn a_mention_its_conversation_never_answered_reaches_the_next_one_once_it_closes() {
    let world = world();
    let chain = &world.chain;
    let first = world.opened("m1", "remember the deploy", Some(WINDOW));
    chain.at(3, 0);
    ends(chain, "Noted.");
    let heard = posts(chain, first);
    let [said] = &heard[..] else {
        panic!("{heard:?}")
    };
    let second = renewed(chain, &Op::NewThread { agent: world.agent });
    assert!(thread_row(chain, first).closed);
    assert_eq!(posts(chain, second), [format!("{CARRIED}{said}")]);
    let carried = frames(chain, second);
    assert!(
        matches!(
            &carried[..],
            [Body::Post {
                author: Principal::Root,
                ..
            }]
        ),
        "{carried:?}"
    );
}

#[test]
fn a_post_in_its_place_answers_it_and_one_elsewhere_does_not() {
    let world = world();
    let chain = &world.chain;
    room(&world, "j-a");
    let first = world.opened("m1", "answer me in general", Some(WINDOW));
    world.post(ADA, "j-a", "a1", &format!("<@{}> and in j-a", world.agent));
    chain.at(3, 0);
    ends(chain, "Reading on.");
    chain.at(4, 0);
    posts_in(chain, "j-a", "toolu_p");
    chain.at(5, 0);
    relay_back(chain, "toolu_p");
    let (posted, failed) = result_of(chain, first, "toolu_p");
    assert!(!failed, "{posted}");
    chain.at(6, 0);
    ends(chain, "Posted.");
    assert!(assigned(chain, SERVICE).is_empty());
    let general = posts(chain, first)
        .into_iter()
        .find(|text| text.ends_with("answer me in general"))
        .unwrap();
    let second = renewed(chain, &Op::NewThread { agent: world.agent });
    assert_eq!(posts(chain, second), [format!("{CARRIED}{general}")]);
}

#[test]
fn closing_a_bound_conversation_hands_what_it_never_answered_to_the_canonical_one() {
    let world = world();
    let chain = &world.chain;
    let canonical = world.opened("m0", "hello", Some(WINDOW));
    chain.at(3, 0);
    ends(chain, "Hi.");
    room(&world, "j-a");
    let place = Place::Channel {
        channel: "j-a".into(),
    };
    let bound = renewed(
        chain,
        &Op::OpenThread {
            agent: world.agent,
            place,
        },
    );
    world.post(
        ADA,
        "j-a",
        "a1",
        &format!("<@{}> fix the build", world.agent),
    );
    chain.at(4, 0);
    ends(chain, "Looking.");
    let heard = posts(chain, bound);
    let [said] = &heard[..] else {
        panic!("{heard:?}")
    };
    chain
        .submit(signed(ADA), PROGRAM, &Op::Close { thread: bound })
        .unwrap();
    assert_eq!(current(chain, world.agent), Some(canonical));
    let told = posts(chain, canonical);
    assert_eq!(told.last(), Some(&format!("{CARRIED}{said}")), "{told:?}");
    assert_eq!(told.len(), 2, "{told:?}");
}

#[test]
fn a_turn_given_up_keeps_its_input_in_its_own_conversation() {
    let world = world();
    let chain = &world.chain;
    let first = world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    let (request, _) = open_request(chain);
    answer(chain, request.id, Answer::Output(refused_for_good()));
    let after = posts(chain, first);
    let [said] = &after[..] else {
        panic!("{after:?}")
    };
    chain.at(4, 0);
    world.mention("m2", "still there?");
    let heard = posts(chain, first);
    assert_eq!(heard.len(), 2, "{heard:?}");
    assert_eq!(&heard[0], said);
    assert!(heard[1].ends_with("still there?"), "{heard:?}");
    assert_eq!(world.threads().len(), 1);
}

#[test]
fn what_the_agent_only_listens_to_is_never_carried() {
    let world = world();
    let chain = &world.chain;
    world.opened("m0", "hi", Some(WINDOW));
    chain.at(3, 0);
    posts_in(chain, "general", "toolu_h");
    chain.at(4, 0);
    relay_back(chain, "toolu_h");
    chain.at(5, 0);
    ends(chain, "Said hi.");
    let listen = chat::Op::ChooseForMany {
        accounts: vec![world.agent],
        channel_id: "general".into(),
        listening: true,
        notices: None,
    };
    chain.submit(signed(ADA), chat::PROGRAM, &listen).unwrap();
    world.post(ADA, "general", "p1", "plain words");
    chain.at(6, 0);
    ends(chain, "Heard.");
    let second = renewed(chain, &Op::NewThread { agent: world.agent });
    assert!(
        posts(chain, second).is_empty(),
        "{:?}",
        posts(chain, second)
    );
}

#[test]
fn a_reaction_on_its_message_answers_it() {
    let world = world();
    let chain = &world.chain;
    room(&world, "j-a");
    let first = world.opened("m1", "answer me in general", Some(WINDOW));
    world.post(ADA, "j-a", "a1", &format!("<@{}> and in j-a", world.agent));
    chain.at(3, 0);
    ends(chain, "Reading on.");
    chain.at(4, 0);
    let react = json!({ "channel": "j-a", "seq": 1, "emoji": "👀" });
    model_says(
        chain,
        json!([tool_use("toolu_r", "chat_react", react)]),
        "tool_use",
    );
    chain.at(5, 0);
    relay_back(chain, "toolu_r");
    let (reacted, failed) = result_of(chain, first, "toolu_r");
    assert!(!failed, "{reacted}");
    chain.at(6, 0);
    ends(chain, "Seen.");
    let general = posts(chain, first)
        .into_iter()
        .find(|text| text.ends_with("answer me in general"))
        .unwrap();
    let second = renewed(chain, &Op::NewThread { agent: world.agent });
    assert_eq!(posts(chain, second), [format!("{CARRIED}{general}")]);
}
