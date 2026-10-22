//! A foreign user, run natively: `Port` sends `Sink` the steps its op names as a foreign user,
//! from whichever program it is seated as, and `Sink` logs the env of every run.

use borsh::{BorshDeserialize, BorshSerialize};

use super::*;
use crate::{Party, Reply};

const USER: Foreign = Foreign {
    network: [7; 32],
    account: 18,
};

#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
enum Sunk {
    Log,
    Emit(String),
    AsAccount,
}

struct Sink;

impl Program for Sink {
    const NAME: &'static str = "sink";
    type Op = Vec<Sunk>;
    type Query = ();
    type Reply = ();

    fn execute(ctx: &ExecCtx, steps: Vec<Sunk>) -> Result<(), Error> {
        for step in steps {
            match step {
                Sunk::Log => {
                    let mut log: Vec<Env> = ctx.record("log")?.unwrap_or_default();
                    log.push(ctx.env().as_ref().clone());
                    ctx.put("log", &log);
                }
                Sunk::Emit(target) => {
                    ctx.emit(target, &vec![Sunk::Log], Reply::None);
                }
                Sunk::AsAccount => {
                    ctx.sender_account()?;
                }
            }
        }
        Ok(())
    }

    fn query(_: &QueryCtx, (): ()) -> Result<(), Error> {
        Ok(())
    }
}

struct Port;

impl Program for Port {
    const NAME: &'static str = "port";
    type Op = (String, Vec<Sunk>);
    type Query = ();
    type Reply = ();

    fn execute(ctx: &ExecCtx, (target, steps): (String, Vec<Sunk>)) -> Result<(), Error> {
        ctx.emit_foreign(USER, target, &steps, Reply::None);
        Ok(())
    }

    fn query(_: &QueryCtx, (): ()) -> Result<(), Error> {
        Ok(())
    }
}

fn chain() -> MockChain {
    let mut chain = MockChain::founded(MockHost::roles_with(role::CROSSPOND, "crosspond"));
    chain.seat::<Port>("crosspond");
    chain.seat::<Port>("other");
    chain.seat::<Sink>("sink");
    chain.seat::<Sink>("next");
    chain.register("crosspond", 100);
    chain.register("other", 101);
    chain.register("sink", 102);
    chain.hold(b"ada-key".to_vec(), 1);
    chain
}

fn ada() -> Origin {
    Origin::Signed(b"ada-key".to_vec())
}

fn logged(chain: &MockChain, program: &str) -> Vec<Env> {
    chain
        .host(program)
        .query(chain.env(program))
        .record("log")
        .unwrap()
        .unwrap_or_default()
}

fn sent(steps: Vec<Sunk>) -> (String, Vec<Sunk>) {
    ("sink".to_owned(), steps)
}

#[test]
fn a_foreign_sender_is_its_principal_its_party_and_no_account() {
    let env = MockHost::env("sink").from_foreign("crosspond", USER);
    assert_eq!(env.sender, Some(Principal::Foreign(USER)));
    assert_eq!(env.origin, Origin::Program("crosspond".into()));
    assert_eq!(
        env.actor(),
        [
            Party::Foreign(USER.into()),
            Party::Program("crosspond".into())
        ]
    );
    let host = MockHost::default();
    let ctx = host.exec(env);
    assert_eq!(ctx.sender().unwrap(), Principal::Foreign(USER));
    assert_eq!(ctx.sender_account().unwrap_err().code, code::UNAUTHORIZED);
}

#[test]
fn emit_foreign_hands_the_host_the_message_and_the_foreign_user_it_runs_as() {
    let host = MockHost::default();
    let ctx = host.exec(MockHost::env("crosspond"));
    ctx.emit_raw("sink", b"a".to_vec(), Reply::None);
    let id = ctx.emit_raw_foreign(USER, "sink", b"b".to_vec(), Reply::Wanted);
    assert_eq!(
        id,
        MessageId {
            program: "crosspond".into(),
            seq: 1,
        }
    );
    let emitted = host.take_emitted();
    let foreign: Vec<Option<Foreign>> = emitted.iter().map(|sent| sent.foreign).collect();
    assert_eq!(foreign, [None, Some(USER)]);
    assert_eq!(
        emitted[1].message,
        abi::Message {
            target: "sink".into(),
            payload: b"b".to_vec(),
            reply: true,
            acting: None,
        }
    );
    assert!(host.take_emitted().is_empty());
}

#[test]
fn the_program_bound_to_crosspond_sends_as_a_foreign_user_who_passes_on_to_no_one() {
    let chain = chain();
    let steps = vec![Sunk::Log, Sunk::Emit("next".into())];
    chain.submit(ada(), "crosspond", &sent(steps)).unwrap();
    let ran = &logged(&chain, "sink")[0];
    assert_eq!(
        (&ran.origin, &ran.sender),
        (
            &Origin::Program("crosspond".into()),
            &Some(Principal::Foreign(USER))
        )
    );
    let next = &logged(&chain, "next")[0];
    assert_eq!(next.sender, Some(Principal::Account(102)));
}

#[test]
fn a_foreign_message_from_any_other_program_or_acting_as_an_account_is_refused() {
    let chain = chain();
    let refused = chain
        .submit(ada(), "other", &sent(vec![Sunk::Log]))
        .unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
    assert!(logged(&chain, "sink").is_empty());
    let acting = chain
        .foreign_sender(&"crosspond".into(), Some(1), USER)
        .unwrap_err();
    assert_eq!(acting.code, code::INVALID_INPUT);
    let unbound = MockChain::default()
        .foreign_sender(&"crosspond".into(), None, USER)
        .unwrap_err();
    assert_eq!(unbound.code, code::UNAUTHORIZED);
}

#[test]
fn a_frame_submitted_as_a_foreign_user_runs_as_one_and_refuses_what_wants_an_account() {
    let chain = chain();
    chain
        .submit_foreign(USER, "sink", &vec![Sunk::Log])
        .unwrap();
    let ran = &logged(&chain, "sink")[0];
    assert_eq!(
        (&ran.origin, &ran.sender),
        (
            &Origin::Program("crosspond".into()),
            &Some(Principal::Foreign(USER))
        )
    );
    let refused = chain
        .submit_foreign(USER, "sink", &vec![Sunk::AsAccount])
        .unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
    let unbound = MockChain::default()
        .submit_foreign(USER, "sink", &vec![Sunk::Log])
        .unwrap_err();
    assert_eq!(unbound.code, code::UNSUPPORTED);
}
