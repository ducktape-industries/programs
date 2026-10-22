//! An agent that listens to a channel: a message there (a linked network's user's too) waits out
//! its settle window, a mention ends it at once, its own posts and other channels reach nothing,
//! and `listen` seeds the room once, so chat's choice stands after it.
use ::program::{ExecCtx, Foreign, MockHost, Program, QueryCtx};
use abi::role::crosspond as role;

use super::*;

const SETTLE: u64 = 10_000;

fn says(text: &str) -> Value {
    json!([{ "type": "text", "text": text }])
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

fn settling(chain: &MockChain, thread: u64) -> Option<SettleWindow> {
    let asked = Query::Settling { thread };
    let Reply::Settling(window) = chain.query(&harness(), &asked).unwrap() else {
        panic!()
    };
    window
}

fn last_said(chain: &MockChain) -> String {
    let (request, _) = open_request(chain);
    let sent = provider_body(chain, &request);
    let last = sent["messages"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["role"], "user");
    let blocks = last["content"].as_array().unwrap();
    let texts: Vec<&str> = blocks
        .iter()
        .filter_map(|block| block["text"].as_str())
        .collect();
    texts.join("\n")
}

fn block(chain: &MockChain, height: u64) {
    chain.at(height, SETTLE + (height - 4) * 2_000);
}

fn listening(world: &World, settle: Option<u64>) -> u64 {
    let chain = &world.chain;
    let hears = Config {
        listen: vec!["general".into()],
        settle,
        ..scout()
    };
    commit(chain, ADA, CONFIG, &md(&hears));
    let thread = world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    model_says(chain, says("Hi."), "end_turn");
    assert!(assigned(chain, SERVICE).is_empty());
    thread
}

#[test]
fn a_listened_burst_waits_out_the_settle_window_and_makes_one_turn() {
    listened_burst_makes_one_turn(world());
}

#[test]
fn a_listened_burst_settles_into_one_turn_under_any_id_harness_is_seated_at() {
    listened_burst_makes_one_turn(world_as(ELSEWHERE));
}

fn listened_burst_makes_one_turn(world: World) {
    let chain = &world.chain;
    let thread = listening(&world, Some(SETTLE));
    block(chain, 4);
    world.post(ADA, "general", "g1", "first part");
    let heard = posts(chain, thread);
    let listened = format!(
        "Ada (account {}) wrote in chat channel general, which you listen to, message 2 \
         (reply_to 2 to answer in its thread):\nfirst part",
        world.ada
    );
    assert_eq!(heard.last(), Some(&listened));
    assert!(assigned(chain, SERVICE).is_empty(), "a general input waits");
    let window = settling(chain, thread).expect("the window opened");
    assert_eq!(
        (window.opened, window.opened_time, window.until),
        (4, SETTLE, 2 * SETTLE)
    );
    let early = chain.submit(signed(BO), harness(), &Op::Settle { thread });
    assert_eq!(early.unwrap_err().code, code::NOT_YET);

    block(chain, 5);
    world.post(ADA, "general", "g2", "second part");
    for height in 6..=9 {
        block(chain, height);
        assert!(assigned(chain, SERVICE).is_empty(), "block {height}");
        assert!(settling(chain, thread).is_some());
    }
    block(chain, 10);
    assert_eq!(settling(chain, thread), None);
    let turns = assigned(chain, SERVICE).len();
    assert_eq!(turns, 1, "one turn hears the burst");
    let said = last_said(chain);
    assert!(
        said.contains("first part") && said.contains("second part"),
        "{said}"
    );
}

#[test]
fn a_mention_during_the_window_starts_the_turn_at_once_with_everything_held() {
    let world = world();
    let chain = &world.chain;
    let thread = listening(&world, Some(SETTLE));
    block(chain, 4);
    world.post(ADA, "general", "g1", "some context");
    assert!(settling(chain, thread).is_some());
    block(chain, 5);
    world.mention("m2", "what do you think?");
    assert_eq!(settling(chain, thread), None, "a mention ends the window");
    assert_eq!(assigned(chain, SERVICE).len(), 1);
    let said = last_said(chain);
    assert!(
        said.contains("some context") && said.contains("what do you think?"),
        "{said}"
    );
    let mentioned = posts(chain, thread)
        .into_iter()
        .filter(|text| text.ends_with("what do you think?"))
        .count();
    assert_eq!(mentioned, 1, "heard once");
    for height in 6..=12 {
        block(chain, height);
    }
    let turns = assigned(chain, SERVICE).len();
    assert_eq!(turns, 1, "the cancelled wake-up asks nothing");
}

#[test]
fn with_settle_off_a_listened_message_starts_a_turn_at_once() {
    let world = world();
    let chain = &world.chain;
    let thread = listening(&world, None);
    block(chain, 4);
    world.post(ADA, "general", "g1", "right away");
    assert_eq!(settling(chain, thread), None);
    assert_eq!(assigned(chain, SERVICE).len(), 1);
    assert!(last_said(chain).ends_with("right away"));
}

#[test]
fn the_agents_own_posts_and_other_channels_reach_nothing_and_listen_seeds_the_room_once() {
    let world = world();
    let chain = &world.chain;
    let thread = listening(&world, Some(SETTLE));
    block(chain, 4);
    world.mention("m2", "say hello to everyone");
    block(chain, 5);
    let hello = tool_use(
        "toolu_p",
        "chat_post",
        json!({ "channel": "general", "text": "hello everyone" }),
    );
    model_says(chain, json!([hello]), "tool_use");
    block(chain, 6);
    relay_back(chain, "toolu_p");
    assert!(!result_of(chain, thread, "toolu_p").1);
    block(chain, 7);
    model_says(chain, says("Done."), "end_turn");
    let heard = posts(chain, thread).len();
    assert_eq!(heard, 2, "its own post never comes back");
    assert_eq!(settling(chain, thread), None);

    let random = chat::Op::CreateChannel {
        channel_id: "random".into(),
        name: "random".into(),
        post_policy: chat::PostPolicy::Open,
    };
    chain.submit(signed(ADA), chat::PROGRAM, &random).unwrap();
    world.post(ADA, "random", "r1", "nobody listens here");
    assert_eq!(posts(chain, thread).len(), 2, "an unlisted channel");
    assert_eq!(settling(chain, thread), None);

    let back = chat::Op::ChooseForMany {
        accounts: vec![world.agent],
        channel_id: "general".into(),
        listening: false,
        notices: None,
    };
    chain.submit(signed(ADA), chat::PROGRAM, &back).unwrap();
    world.mention("m3", "stop listening");
    block(chain, 8);
    model_says(chain, says("Stopped."), "end_turn");
    let asked = chat::Query::Listening {
        account: world.agent,
        page: PageRequest::default(),
    };
    let chat::Reply::Listening(page) = chain.query(chat::PROGRAM, &asked).unwrap() else {
        panic!()
    };
    assert!(page.items.is_empty(), "{:?}", page.items);
    world.post(BO, "general", "g9", "after it stopped");
    assert_eq!(posts(chain, thread).len(), 3);
    assert!(assigned(chain, SERVICE).is_empty());
}

fn listened_to(chain: &MockChain, agent: AccountNumber) -> Vec<String> {
    let asked = chat::Query::Listening {
        account: agent,
        page: PageRequest::default(),
    };
    let chat::Reply::Listening(page) = chain.query(chat::PROGRAM, &asked).unwrap() else {
        panic!()
    };
    page.items
}

#[test]
fn a_listen_pattern_seeds_every_open_room_it_names_as_it_appears() {
    let world = world();
    let chain = &world.chain;
    let open = |id: &str| {
        let create = chat::Op::CreateChannel {
            channel_id: id.into(),
            name: id.into(),
            post_policy: chat::PostPolicy::Open,
        };
        chain.submit(signed(ADA), chat::PROGRAM, &create).unwrap();
    };
    for id in ["j-1", "j-2", "jobs", "k-1"] {
        open(id);
    }
    let archive = chat::Op::SetChannelArchived {
        channel_id: "j-2".into(),
        archived: true,
    };
    chain.submit(signed(ADA), chat::PROGRAM, &archive).unwrap();
    let hears = Config {
        listen: vec!["j-*".into()],
        ..scout()
    };
    commit(chain, ADA, CONFIG, &md(&hears));
    world.opened("m1", "hello", Some(WINDOW));
    assert_eq!(listened_to(chain, world.agent), ["j-1"]);
    chain.at(3, 0);
    model_says(chain, says("Hi."), "end_turn");
    open("j-3");
    chain.at(4, 0);
    world.mention("m2", "a new room");
    assert_eq!(listened_to(chain, world.agent), ["j-1", "j-3"]);
}

pub(super) const CHIEF: Foreign = Foreign {
    network: [0x22; 32],
    account: 18,
};

pub(super) struct Heard;

impl Program for Heard {
    const NAME: &'static str = "crosspond";
    type Op = role::Op;
    type Query = role::Query;
    type Reply = role::Reply;

    fn execute(_: &ExecCtx, _: role::Op) -> Result<(), Error> {
        Err(::program::invalid("this crosspond sends nothing"))
    }

    fn query(_: &QueryCtx, query: role::Query) -> Result<role::Reply, Error> {
        Ok(match query {
            role::Query::Name(foreign) => {
                role::Reply::Name((foreign == CHIEF.into()).then(|| "chief".to_owned()))
            }
            role::Query::Link { network } => {
                role::Reply::Link((network == CHIEF.network).then(|| role::Link {
                    name: "there".into(),
                    epoch_length: 4,
                    trusted: 0,
                    header: None,
                    next_in: 1,
                    next_out: 1,
                    refused: None,
                    closed: false,
                }))
            }
            other => return Err(::program::invalid(format!("not asked here: {other:?}"))),
        })
    }
}

#[test]
fn a_post_from_another_network_wakes_an_agent_that_listens_there_and_names_its_author() {
    let roles = MockHost::roles_with(abi::role::CROSSPOND, "crosspond");
    let mut world = world_founded(PROGRAM, roles);
    world.chain.seat::<Heard>("crosspond");
    let chain = &world.chain;
    let thread = listening(&world, None);
    block(chain, 4);
    let link = chat::Op::Link {
        channel_id: "general".into(),
        peer: Some(chat::Peer {
            network: CHIEF.network,
            channel_id: "lobby".into(),
        }),
    };
    chain.submit(signed(ADA), chat::PROGRAM, &link).unwrap();
    let from_there = chat::Op::FromPeer {
        channel_id: "general".into(),
        write: chat::PeerWrite::Post {
            message_id: "b1".into(),
            blocks: chat::parse_message("anyone here?"),
            thread: None,
        },
    };
    chain
        .submit_foreign(CHIEF, chat::PROGRAM, &from_there)
        .unwrap();
    assert_eq!(assigned(chain, SERVICE).len(), 1, "the post woke the agent");
    let heard = "chief@there (18@there) wrote in chat channel general, which you listen to, \
                 message 2 (reply_to 2 to answer in its thread):\nanyone here?";
    assert!(last_said(chain).ends_with(heard), "{}", last_said(chain));
    let authors: Vec<Principal> = (frames(chain, thread).into_iter())
        .filter_map(|body| match body {
            Body::Post { author, .. } => Some(author),
            _ => None,
        })
        .collect();
    assert_eq!(authors.last(), Some(&Principal::Foreign(CHIEF)));
}

#[test]
fn a_post_by_a_user_of_another_network_crosspond_has_no_name_for_reads_as_their_number_there() {
    let roles = MockHost::roles_with(abi::role::CROSSPOND, "crosspond");
    let mut world = world_founded(PROGRAM, roles);
    world.chain.seat::<Heard>("crosspond");
    let chain = &world.chain;
    let thread = listening(&world, None);
    block(chain, 4);
    let link = chat::Op::Link {
        channel_id: "general".into(),
        peer: Some(chat::Peer {
            network: CHIEF.network,
            channel_id: "lobby".into(),
        }),
    };
    chain.submit(signed(ADA), chat::PROGRAM, &link).unwrap();
    let unheard = Foreign {
        network: CHIEF.network,
        account: 19,
    };
    let from_there = chat::Op::FromPeer {
        channel_id: "general".into(),
        write: chat::PeerWrite::Post {
            message_id: "b1".into(),
            blocks: chat::parse_message("hello from someone"),
            thread: None,
        },
    };
    chain
        .submit_foreign(unheard, chat::PROGRAM, &from_there)
        .unwrap();
    assert_eq!(assigned(chain, SERVICE).len(), 1, "the post woke the agent");
    let heard = "19@there wrote in chat channel general, which you listen to, \
                 message 2 (reply_to 2 to answer in its thread):\nhello from someone";
    assert!(last_said(chain).ends_with(heard), "{}", last_said(chain));
    let authors: Vec<Principal> = (frames(chain, thread).into_iter())
        .filter_map(|body| match body {
            Body::Post { author, .. } => Some(author),
            _ => None,
        })
        .collect();
    assert_eq!(authors.last(), Some(&Principal::Foreign(unheard)));
}
