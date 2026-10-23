//! A model call refused for the depth of the chain that woke its turn: the refusal is the chain's,
//! so harness asks again on the account its cache is on; a retry whose booking the scheduler refused
//! too is booked by the sweep, or by an answer on its service before.
use super::accounts::{
    agent_on, configured_as, harness_wired, mention_of, model_read, on, route_now, says_as,
    says_on, turn_open, turns_open,
};
use super::*;
use ::abi::role::egress::DEFAULT_WAIT;

const RELAY: &str = "relay";
const RESUMER: &str = "resumer";

const HOPS: u32 = ::program::MAX_DEPTH - 4;

struct Relay;

impl ::program::Program for Relay {
    const NAME: &'static str = RELAY;
    type Op = (u32, chat::Op);
    type Query = ();
    type Reply = ();

    fn execute(ctx: &::program::ExecCtx, (left, post): (u32, chat::Op)) -> Result<(), Error> {
        match left.checked_sub(1) {
            Some(left) => ctx.emit(RELAY, &(left, post), ::program::Reply::None),
            None => ctx.emit(chat::PROGRAM, &post, ::program::Reply::None),
        };
        Ok(())
    }

    fn query(_: &::program::QueryCtx, (): ()) -> Result<(), Error> {
        Ok(())
    }
}

struct Resumer;

impl ::program::Program for Resumer {
    const NAME: &'static str = RESUMER;
    type Op = (u32, u64);
    type Query = ();
    type Reply = ();

    fn execute(ctx: &::program::ExecCtx, (left, thread): (u32, u64)) -> Result<(), Error> {
        match left.checked_sub(1) {
            Some(left) => ctx.emit(RESUMER, &(left, thread), ::program::Reply::None),
            None => ctx.emit(PROGRAM, &Op::Resume { thread }, ::program::Reply::None),
        };
        Ok(())
    }

    fn query(_: &::program::QueryCtx, (): ()) -> Result<(), Error> {
        Ok(())
    }
}

fn seated<P: ::program::Program>(world: &mut World, name: &str) {
    world.chain.seat::<P>(name);
    let register = identity::Op::RegisterProgram {
        program: name.into(),
    };
    world
        .chain
        .submit(Origin::Root, identity::PROGRAM, &register)
        .unwrap();
}

fn relayed(world: &mut World) {
    seated::<Relay>(world, RELAY);
}

#[test]
fn a_model_call_refused_for_its_depth_is_asked_again_on_the_account_its_cache_is_on() {
    let mut world = world();
    relayed(&mut world);
    let chain = &world.chain;
    harness_wired(&world);
    let thread = world.mention("m1", "hello");
    chain.at(2, 0);
    model_read(chain);
    chain.at(3, 0);
    says_as(chain, thread, "a", "0.25");
    assert_eq!(route_now(chain, thread), on("a"));
    let post = chat::Op::PostMessage {
        channel_id: "general".into(),
        message_id: "deep".into(),
        blocks: chat::parse_message(&format!("<@{}> merged", world.agent)),
        thread: None,
    };
    chain.submit(signed(ADA), RELAY, &(HOPS, post)).unwrap();
    let told = notices(chain, thread);
    let again = "harness asks the model again at block 4 (attempt 1)";
    let refused = told.last().is_some_and(|text| {
        text.starts_with("the model was not reached: capacity") && text.ends_with(again)
    });
    assert!(refused, "{told:?}");
    chain.at(5, 0);
    let (_, pinned) = turn_open(chain, thread);
    assert_eq!(pinned.as_deref(), Some("a"), "the account its cache is on");
    assert_eq!(route_now(chain, thread), on("a"));
}

fn gave_up(world: &mut World) -> u64 {
    seated::<Resumer>(world, RESUMER);
    let chain = &world.chain;
    harness_wired(world);
    configured_as(chain, CONFIG, MODEL, SERVICE, PROVIDER);
    chain.submit(signed(ADA), PROGRAM, &Op::Sweep).unwrap();
    let thread = world.mention("m1", "hello");
    chain.at(2, 0);
    model_read(chain);
    chain.at(3, 0);
    let (request, _) = turn_open(chain, thread);
    let invalid =
        json!({ "type": "error", "error": { "type": "invalid_request_error", "message": "bad" } });
    answer(chain, request.id, Answer::Output(response(400, invalid)));
    assert!(
        turns_open(chain, thread).is_empty(),
        "a refusal ends the turn"
    );
    thread
}

fn resumed_at_the_cap(world: &World, thread: u64) {
    let chain = &world.chain;
    let left = ::program::MAX_DEPTH - 1;
    chain.submit(signed(ADA), RESUMER, &(left, thread)).unwrap();
    let told = notices(chain, thread);
    let kept = told.last().is_some_and(|text| {
        text.starts_with("the model was not reached: capacity") && text.contains("harness's sweep")
    });
    assert!(kept, "{told:?}");
    assert!(turns_open(chain, thread).is_empty(), "nothing asked yet");
}

fn booked(chain: &MockChain, height: u64, op: &Op) -> usize {
    let asked = scheduler::Query::Due {
        height,
        after: None,
        limit: None,
    };
    let scheduler::Reply::Due { entries, .. } = chain.query(scheduler::PROGRAM, &asked).unwrap()
    else {
        panic!("the scheduler answers what is due")
    };
    entries
        .iter()
        .filter(|entry| entry.height == height && entry.receipt.is_none())
        .filter(|entry| abi::decode::<Op>(&entry.payload).is_ok_and(|due| due == *op))
        .count()
}

const SWEPT: u64 = 1 + DEFAULT_WAIT;

#[test]
fn a_retry_the_scheduler_refused_for_its_depth_is_asked_by_the_sweep_on_an_idle_service() {
    let mut world = world();
    let thread = gave_up(&mut world);
    world.chain.at(SWEPT + 3, 0);
    assert!(
        turns_open(&world.chain, thread).is_empty(),
        "nothing asks a turn given up"
    );
    resumed_at_the_cap(&world, thread);
    let chain = &world.chain;
    let next = SWEPT + DEFAULT_WAIT;
    let rearmed = booked(chain, next, &Op::Sweep);
    assert_eq!(
        rearmed, 1,
        "the sweep books the next one DEFAULT_WAIT blocks on"
    );
    chain.at(next, 0);
    assert!(turns_open(chain, thread).is_empty(), "only the sweep asks");
    chain.at(next + 1, 0);
    let kept = booked(chain, next + 1, &Op::Resume { thread });
    assert_eq!(kept, 1, "the sweep booked the kept retry");
    chain.at(next + 2, 0);
    let open = turns_open(chain, thread);
    assert_eq!(open.len(), 1, "the sweep's booking asked");
}

#[test]
fn a_sweep_while_one_is_booked_books_no_second_one() {
    let mut world = world();
    gave_up(&mut world);
    let chain = &world.chain;
    let again = 10;
    chain.at(again, 0);
    chain.submit(signed(ADA), PROGRAM, &Op::Sweep).unwrap();
    let standing = booked(chain, SWEPT, &Op::Sweep);
    assert_eq!(standing, 1, "the standing sweep stays booked");
    let second = booked(chain, again + DEFAULT_WAIT, &Op::Sweep);
    assert_eq!(second, 0, "a sweep while one is booked books no second one");
}

#[test]
fn a_retry_the_scheduler_refused_for_its_depth_is_asked_once_a_turn_on_its_service_is_answered() {
    let mut world = world();
    let thread = gave_up(&mut world);
    world.chain.at(4, 0);
    resumed_at_the_cap(&world, thread);
    let chain = &world.chain;
    let ranger = agent_on(&world, "Ranger", MODEL, SERVICE, PROVIDER);
    let ranging = mention_of(&world, ranger, "m2", "anyone there?");
    chain.at(5, 0);
    says_on(chain, SERVICE, ranging, "a", "0.1");
    chain.at(7, 0);
    let open = turns_open(chain, thread);
    assert_eq!(open.len(), 1, "the answer on its service booked it");
}
