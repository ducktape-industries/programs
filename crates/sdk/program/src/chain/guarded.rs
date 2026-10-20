//! The kernel's ownership guard, run natively: `Keeper` writes the keys its op names, claiming
//! each `o/` key it creates as a `test.item`, and `Guardian`, seated at the ownership role, keeps
//! each guard it is asked and refuses or sends as its host is told to.

use abi::role::ownership::{Guard, Op as Guarded, Write, record_key};
use borsh::{BorshDeserialize, BorshSerialize};

use super::*;
use crate::Reply;

#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
enum Step {
    Set(String),
    Delete(String),
    Emit(String, Vec<Step>),
}

struct Keeper;

fn claim(key: &str) -> Option<&'static str> {
    key.starts_with("o/").then_some("test.item")
}

impl Program for Keeper {
    const NAME: &'static str = "keeper";
    type Op = Vec<Step>;
    type Query = ();
    type Reply = ();

    fn init(ctx: &ExecCtx, _: &[u8]) -> Result<(), Error> {
        ctx.set_with(b"o/genesis".to_vec(), b"x".to_vec(), claim("o/genesis"));
        Ok(())
    }

    fn execute(ctx: &ExecCtx, steps: Vec<Step>) -> Result<(), Error> {
        for step in steps {
            match step {
                Step::Set(key) => {
                    let kind = claim(&key);
                    ctx.set_with(key, b"v".to_vec(), kind)
                }
                Step::Delete(key) => ctx.delete(key),
                Step::Emit(target, steps) => {
                    ctx.emit(target, &steps, Reply::None);
                }
            }
        }
        Ok(())
    }

    fn query(_: &QueryCtx, (): ()) -> Result<(), Error> {
        Ok(())
    }
}

struct Guardian;

impl Program for Guardian {
    const NAME: &'static str = "ownership";
    type Op = Guarded;
    type Query = ();
    type Reply = ();

    fn execute(ctx: &ExecCtx, Guarded::Guard(guard): Guarded) -> Result<(), Error> {
        ctx.env().kernel()?;
        let mut kept: Vec<(Env, Guard)> = ctx.record("guards")?.unwrap_or_default();
        kept.push((ctx.env().as_ref().clone(), guard));
        ctx.put("guards", &kept);
        if ctx.get("deny").is_some() {
            return Err(unauthorized("denied"));
        }
        if ctx.get("chatty").is_some() {
            ctx.emit("keeper", &Vec::<Step>::new(), Reply::None);
        }
        Ok(())
    }

    fn query(_: &QueryCtx, (): ()) -> Result<(), Error> {
        Ok(())
    }
}

fn chain() -> MockChain {
    let mut chain = MockChain::default();
    chain.seat::<Keeper>("keeper");
    chain.seat::<Guardian>(MockHost::role(role::OWNERSHIP));
    chain.register("keeper", 100);
    chain.hold(b"ada-key".to_vec(), 1);
    chain
}

fn ada() -> Origin {
    Origin::Signed(b"ada-key".to_vec())
}

fn guards(chain: &MockChain) -> Vec<(Env, Guard)> {
    let host = chain.host("ownership");
    host.query(chain.env("ownership"))
        .record("guards")
        .unwrap()
        .unwrap_or_default()
}

fn record(chain: &MockChain, key: &str) {
    chain
        .host("ownership")
        .borrow_mut()
        .state
        .insert(record_key("keeper", key.as_bytes()), Vec::new());
}

fn set(key: &str) -> Step {
    Step::Set(key.into())
}

fn write(key: &str, claimed: Option<&str>, existed: bool, kept: bool) -> Write {
    Write {
        key: key.as_bytes().to_vec(),
        claimed: claimed.map(str::to_owned),
        existed,
        kept,
    }
}

#[test]
fn a_run_tells_the_guardian_each_claimed_create_and_recorded_key_once() {
    let chain = chain();
    chain.init("keeper", &()).unwrap();
    assert!(guards(&chain).is_empty(), "init is not guarded");
    record(&chain, "loose");
    let steps = vec![
        set("s/a"),
        set("o/b"),
        set("o/b"),
        set("loose"),
        Step::Delete("o/genesis".into()),
        set("o/brief"),
        Step::Delete("o/brief".into()),
    ];
    chain.submit(ada(), "keeper", &steps).unwrap();
    let asked = guards(&chain);
    assert_eq!(asked.len(), 1, "one call for the whole run");
    let (env, guard) = &asked[0];
    assert_eq!(
        (&env.origin, &env.sender, &env.cause),
        (&Origin::Root, &Some(Principal::Root), &Cause::Direct)
    );
    assert_eq!(guard.program, "keeper");
    assert_eq!(guard.origin, abi::Origin::External(b"ada-key".to_vec()));
    assert_eq!(guard.sender, Some(abi::Principal::Account(1)));
    assert_eq!(
        guard.writes,
        [
            write("loose", None, false, true),
            write("o/b", Some("test.item"), false, true),
        ],
        "an unrecorded key from before, deleted, and a claim that went again ask nothing"
    );
    chain.submit(ada(), "keeper", &vec![set("s/only")]).unwrap();
    chain.submit(ada(), "keeper", &vec![set("o/b")]).unwrap();
    assert_eq!(guards(&chain).len(), 1, "shared writes ask nothing");
}

#[test]
fn a_refusal_undoes_the_run_and_nothing_it_emitted_runs() {
    let chain = chain();
    chain
        .host("ownership")
        .borrow_mut()
        .state
        .insert(b"deny".to_vec(), Vec::new());
    let before = chain.host("keeper").borrow().state.clone();
    let steps = vec![
        set("s/a"),
        set("o/b"),
        Step::Emit("keeper".into(), vec![set("s/c")]),
    ];
    let refused = chain.submit(ada(), "keeper", &steps).unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
    assert_eq!(refused.message, "denied");
    assert_eq!(chain.host("keeper").borrow().state, before);
    assert!(
        guards(&chain).is_empty(),
        "the guardian's own writes went too"
    );
}

#[test]
fn a_guardian_that_sends_is_refused() {
    let chain = chain();
    chain
        .host("ownership")
        .borrow_mut()
        .state
        .insert(b"chatty".to_vec(), Vec::new());
    let refused = chain
        .submit(ada(), "keeper", &vec![set("o/b")])
        .unwrap_err();
    assert_eq!(refused.code, code::UNSUPPORTED);
    assert!(
        !chain
            .host("keeper")
            .borrow()
            .state
            .contains_key(b"o/b".as_slice())
    );
}

#[test]
fn the_chain_the_vote_and_an_unseated_role_are_not_guarded() {
    let mut chain = chain();
    chain
        .submit(Origin::Root, "keeper", &vec![set("o/a")])
        .unwrap();
    let validators = MockHost::role(role::VALIDATORS);
    chain
        .submit(Origin::Program(validators), "keeper", &vec![set("o/b")])
        .unwrap();
    assert!(guards(&chain).is_empty());
    chain.seat::<Keeper>("other");
    chain.register("other", 101);
    let emit = vec![Step::Emit("keeper".into(), vec![set("o/c")])];
    chain.submit(ada(), "other", &emit).unwrap();
    let asked = guards(&chain);
    assert_eq!(asked.len(), 1, "keeper's run, as other's message");
    assert_eq!(asked[0].1.origin, abi::Origin::Program("other".into()));
    assert_eq!(asked[0].1.sender, Some(abi::Principal::Account(101)));
    let mut bare = MockChain::default();
    bare.seat::<Keeper>("keeper");
    bare.hold(b"ada-key".to_vec(), 1);
    bare.submit(ada(), "keeper", &vec![set("o/a")]).unwrap();
}
