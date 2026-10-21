//! A stale post: chat_post into a place is refused, unsent, when an input heard from that exact
//! place reached the conversation after its reply read, and lands as it does today otherwise; both
//! are properties over generated arrivals, run from a fixed seed so every gate replays them.
use proptest::collection::{SizeRange, vec};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRng, TestRunner};

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Spot {
    Foot,
    Thread,
    Dm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Learning,
    Asked,
    Running,
}

#[derive(Clone, Copy, Debug)]
struct Arrival {
    spot: Spot,
    phase: Phase,
    heard: bool,
}

#[derive(Clone, Debug)]
struct Case {
    arrivals: Vec<Arrival>,
    post: Spot,
}

const SPOTS: [Spot; 3] = [Spot::Foot, Spot::Thread, Spot::Dm];

fn spot() -> impl Strategy<Value = Spot> {
    prop_oneof![Just(Spot::Foot), Just(Spot::Thread), Just(Spot::Dm)]
}

fn phase() -> impl Strategy<Value = Phase> {
    prop_oneof![
        Just(Phase::Learning),
        Just(Phase::Asked),
        Just(Phase::Running)
    ]
}

fn cases() -> impl Strategy<Value = Case> {
    let arrival = (spot(), phase(), any::<bool>()).prop_map(|(spot, phase, heard)| Arrival {
        spot,
        phase,
        heard,
    });
    (vec(arrival, SizeRange::default()), spot())
        .prop_map(|(arrivals, post)| Case { arrivals, post })
}

fn runner() -> TestRunner {
    let config = Config {
        failure_persistence: None,
        ..Config::default()
    };
    let rng = TestRng::deterministic_rng(config.rng_algorithm);
    TestRunner::new_with_rng(config, rng)
}

fn root(spot: Spot) -> Option<u64> {
    match spot {
        Spot::Thread => Some(1),
        Spot::Foot | Spot::Dm => None,
    }
}

struct Rig {
    world: World,
    thread: u64,
    dm: String,
    sent: u64,
    stale: bool,
}

impl Rig {
    fn open() -> Rig {
        let world = world();
        let dm = chat::Op::CreateDmChannel {
            counterpart: world.agent,
            name: "scout".into(),
        };
        world.chain.submit(signed(ADA), chat::PROGRAM, &dm).unwrap();
        let dm = chat::dm_channel_id(world.ada, world.agent);
        let thread = world.mention("m0", "start a thread");
        Rig {
            world,
            thread,
            dm,
            sent: 0,
            stale: false,
        }
    }

    fn channel(&self, spot: Spot) -> String {
        match spot {
            Spot::Foot | Spot::Thread => "general".into(),
            Spot::Dm => self.dm.clone(),
        }
    }

    fn arrive(&mut self, arrival: Arrival, post: Spot) {
        self.sent += 1;
        let agent = self.world.agent;
        let (key, spot, text) = match (arrival.heard, arrival.spot) {
            (true, spot) => (ADA, spot, format!("<@{agent}> note {}", self.sent)),
            (false, Spot::Dm) => (BO, Spot::Foot, format!("chatter {}", self.sent)),
            (false, spot) => (BO, spot, format!("chatter {}", self.sent)),
        };
        let before = held(&self.world.chain, self.thread).len();
        let op = chat::Op::PostMessage {
            channel_id: self.channel(spot),
            message_id: format!("a{}", self.sent),
            blocks: chat::parse_message(&text),
            thread: root(spot),
        };
        self.world
            .chain
            .submit(signed(key), chat::PROGRAM, &op)
            .unwrap();
        let held_now = held(&self.world.chain, self.thread).len() > before;
        let after_read = arrival.phase != Phase::Learning;
        self.stale |= held_now && after_read && spot == post;
    }

    fn post(&self, spot: Spot, text: &str) -> Value {
        match root(spot) {
            Some(root) => json!({ "channel": self.channel(spot), "text": text, "reply_to": root }),
            None => json!({ "channel": self.channel(spot), "text": text }),
        }
    }

    fn landed(&self, call: &str) -> Option<chat::MsgRow> {
        let asked = chat::Query::MessageById {
            message_id: format!("harness-{}-{call}", self.thread),
        };
        match self.world.chain.query(chat::PROGRAM, &asked).unwrap() {
            chat::Reply::Message(row) => row,
            other => panic!("{other:?}"),
        }
    }
}

fn posted(case: &Case) -> (Rig, (String, bool)) {
    let mut rig = Rig::open();
    let arrive = |rig: &mut Rig, phase: Phase| {
        let now = case
            .arrivals
            .iter()
            .filter(|arrival| arrival.phase == phase);
        for arrival in now {
            rig.arrive(*arrival, case.post);
        }
    };
    arrive(&mut rig, Phase::Learning);
    rig.world.chain.at(2, 0);
    learn(&rig.world.chain, Some(WINDOW));
    arrive(&mut rig, Phase::Asked);
    rig.world.chain.at(3, 0);
    let first = tool_use("toolu_p", "chat_post", rig.post(case.post, "first"));
    model_says(&rig.world.chain, json!([first]), "tool_use");
    arrive(&mut rig, Phase::Running);
    rig.world.chain.at(4, 0);
    relay_back(&rig.world.chain, "toolu_p");
    let result = result_of(&rig.world.chain, rig.thread, "toolu_p");
    (rig, result)
}

#[test]
fn a_post_with_nothing_heard_from_its_place_after_its_read_lands_as_it_does_today() {
    let today: Vec<(Spot, String)> = SPOTS
        .into_iter()
        .map(|spot| {
            let alone = Case {
                arrivals: Vec::new(),
                post: spot,
            };
            let (rig, (text, failed)) = posted(&alone);
            assert!(!failed && !rig.stale, "{text}");
            (spot, text)
        })
        .collect();
    let quiet = cases().prop_map(|mut case| {
        let post = case.post;
        for arrival in &mut case.arrivals {
            if arrival.heard && arrival.spot == post {
                arrival.phase = Phase::Learning;
            }
        }
        case
    });
    let checked = runner().run(&quiet, |case| {
        let (rig, (text, failed)) = posted(&case);
        prop_assert!(
            !rig.stale,
            "nothing heard from the post's place came after its read"
        );
        let expected = today
            .iter()
            .find(|(spot, _)| *spot == case.post)
            .map(|(_, text)| text);
        prop_assert_eq!(Some(&text), expected);
        prop_assert!(!failed);
        let landed = rig
            .landed("toolu_p")
            .map(|row| (row.author, row.channel_id, row.thread, row.blocks));
        let wanted = (
            Principal::Account(rig.world.agent),
            rig.channel(case.post),
            root(case.post),
            chat::parse_message("first"),
        );
        prop_assert_eq!(landed, Some(wanted));
        Ok(())
    });
    if let Err(error) = checked {
        panic!("{error}");
    }
}

#[test]
fn a_post_is_refused_exactly_when_something_heard_from_its_place_came_after_its_read_and_lands_once_read()
 {
    let checked = runner().run(&cases(), |case| {
        let (rig, (text, failed)) = posted(&case);
        prop_assert_eq!(failed, rig.stale, "{}", text);
        if !failed {
            return Ok(());
        }
        prop_assert!(text.contains("not posted"), "{}", text);
        prop_assert!(rig.landed("toolu_p").is_none());
        let chain = &rig.world.chain;
        prop_assert!(
            held(chain, rig.thread).is_empty(),
            "the next ask reads what reached it"
        );
        chain.at(5, 0);
        let again = tool_use("toolu_q", "chat_post", rig.post(case.post, "again"));
        model_says(chain, json!([again]), "tool_use");
        chain.at(6, 0);
        relay_back(chain, "toolu_q");
        let (text, refused) = result_of(chain, rig.thread, "toolu_q");
        prop_assert!(!refused, "{}", text);
        prop_assert!(rig.landed("toolu_q").is_some());
        Ok(())
    });
    if let Err(error) = checked {
        panic!("{error}");
    }
}

#[test]
fn a_message_a_wait_heard_after_the_read_makes_a_post_into_its_place_stale() {
    let world = super::schedule::schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "wake me when Bo writes here", Some(WINDOW));
    let wanted = json!({ "event": { "message": { "channel": "general", "from": world.bo } } });
    super::schedule::turn(&world, 3, &[("w1", "wait_for", wanted)]);
    chain.at(6, 0);
    world.mention("m2", "and one more thing");
    world.post(BO, "general", "b1", "Bo is here");
    let woke = held(chain, thread).into_iter().any(|body| {
        matches!(body, Body::Post { author: Principal::Root, text } if text.ends_with("Bo is here"))
    });
    assert!(woke, "the wake reached the conversation after its read");
    chain.at(7, 0);
    let first = json!({ "channel": "general", "text": "first" });
    model_says(
        chain,
        json!([tool_use("toolu_p", "chat_post", first)]),
        "tool_use",
    );
    chain.at(8, 0);
    relay_back(chain, "toolu_p");
    let (text, failed) = result_of(chain, thread, "toolu_p");
    assert!(failed && text.contains("not posted"), "{text}");
}

fn notified(world: &World, id: &str) {
    let op = chat::Op::Notify {
        channel_id: "general".into(),
        message_id: id.into(),
        thread: None,
        notice: chat::Notice {
            kind: "pod".into(),
            group: id.into(),
            subject: format!("pod {id}"),
            link: None,
            status: chat::message::Status::Failed,
            detail: "exit code 1".into(),
            concerns: vec![world.agent],
        },
    };
    world.chain.submit(signed(BO), chat::PROGRAM, &op).unwrap();
}

fn lands(world: &World, thread: u64, at: u64) {
    let chain = &world.chain;
    chain.at(at + 1, 0);
    relay_back(chain, "toolu_p");
    let (text, failed) = result_of(chain, thread, "toolu_p");
    assert!(!failed, "{text}");
    let asked = chat::Query::MessageById {
        message_id: format!("harness-{thread}-toolu_p"),
    };
    let chat::Reply::Message(Some(row)) = chain.query(chat::PROGRAM, &asked).unwrap() else {
        panic!("the post landed")
    };
    assert_eq!(row.blocks, chat::parse_message("first"));
}

#[test]
fn a_notice_heard_from_its_place_after_the_read_leaves_a_post_to_land() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "post when you can", Some(WINDOW));
    chain.at(3, 0);
    let first = json!({ "channel": "general", "text": "first" });
    model_says(
        chain,
        json!([tool_use("toolu_p", "chat_post", first)]),
        "tool_use",
    );
    notified(&world, "n1");
    let heard = held(chain, thread)
        .iter()
        .any(|body| body.text().ends_with("pod n1 failed — exit code 1"));
    assert!(heard, "the notice reached the conversation after its read");
    lands(&world, thread, 3);
}

#[test]
fn a_notice_a_wait_heard_after_the_read_leaves_a_post_into_its_place_to_land() {
    let world = super::schedule::schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "wake me when Bo's pod ends", Some(WINDOW));
    let wanted = json!({ "event": { "message": { "channel": "general", "from": world.bo } } });
    super::schedule::turn(&world, 3, &[("w1", "wait_for", wanted)]);
    chain.at(6, 0);
    world.mention("m2", "and one more thing");
    notified(&world, "n1");
    let woke = held(chain, thread).into_iter().any(|body| {
        matches!(body, Body::Post { author: Principal::Root, text } if text.contains("pod n1 failed"))
    });
    assert!(woke, "the notice ended the wait after the read");
    chain.at(7, 0);
    let first = json!({ "channel": "general", "text": "first" });
    model_says(
        chain,
        json!([tool_use("toolu_p", "chat_post", first)]),
        "tool_use",
    );
    lands(&world, thread, 7);
}
