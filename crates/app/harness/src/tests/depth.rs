//! A model call refused for the depth of the chain that woke its turn: the refusal is the chain's,
//! not the account's, so harness asks again on the account the conversation's cache is on.
use super::accounts::{harness_wired, model_read, on, route_now, says_as, turn_open};
use super::*;

const RELAY: &str = "relay";

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

fn relayed(world: &mut World) {
    world.chain.seat::<Relay>(RELAY);
    let register = identity::Op::RegisterProgram {
        program: RELAY.into(),
    };
    world
        .chain
        .submit(Origin::Root, identity::PROGRAM, &register)
        .unwrap();
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
