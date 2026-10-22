//! Each of the kernel's frame rules, run natively. `Bot` is one program
//! seated under several names; its op is a script of steps, and every run
//! logs the env it ran in on its host, so a test reads who ran, sent by
//! whom, caused by what.

use abi::role::identity::{
    Category, Kind, Profile, Query as Asked, Reply as Answer, Script, Standing,
};
use borsh::{BorshDeserialize, BorshSerialize};

use super::*;
use crate::{BlobId, HashKind, Program, Reply};

#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
enum Step {
    Put(String, String),
    Emit {
        target: String,
        steps: Vec<Step>,
        reply: bool,
    },
    Recurse(u32),
    Peek(String, String),
    Named(AccountNumber),
    Blob,
    Stat(BlobId),
    Event,
    Output(Vec<u8>),
    Refuse,
    EmitAs {
        account: AccountNumber,
        target: String,
        steps: Vec<Step>,
    },
}

struct Bot;

impl Program for Bot {
    const NAME: &'static str = "bot";
    type Op = Vec<Step>;
    type Query = String;
    type Reply = Option<Vec<u8>>;

    fn init(ctx: &ExecCtx, params: &[u8]) -> Result<(), Error> {
        Bot::execute(ctx, crate::decoded("bot", "init", params)?)
    }

    fn execute(ctx: &ExecCtx, steps: Vec<Step>) -> Result<(), Error> {
        log(ctx)?;
        for step in steps {
            match step {
                Step::Put(key, value) => ctx.set(key, value),
                Step::Emit {
                    target,
                    steps,
                    reply,
                } => {
                    let reply = if reply { Reply::Wanted } else { Reply::None };
                    let id = ctx.emit(target, &steps, reply);
                    let mut emitted: Vec<MessageId> = ctx.record("emitted")?.unwrap_or_default();
                    emitted.push(id);
                    ctx.put("emitted", &emitted);
                }
                Step::Recurse(n) if n > 0 => {
                    let me = ctx.env().program.clone();
                    ctx.emit(me, &vec![Step::Recurse(n - 1)], Reply::None);
                }
                Step::Recurse(_) => {}
                Step::Peek(target, key) => {
                    let seen: Option<Vec<u8>> = ctx.query(target, &key)?;
                    ctx.set(format!("peek:{key}"), seen.unwrap_or_default());
                }
                Step::Named(number) => ctx.require_account(number)?,
                Step::Blob => {
                    ctx.blob_put(HashKind::Sha256, "text", b"body".to_vec())?;
                }
                Step::Stat(id) => {
                    let held = ctx.blob_stat(id).is_some();
                    ctx.set(format!("held:{}", abi::hex(id.digest())), [u8::from(held)]);
                }
                Step::Event => ctx.event(b"happened".to_vec()),
                Step::Output(bytes) => ctx.set_return_data(bytes),
                Step::Refuse => return Err(Error::new(code::WRONG_STATE, "as scripted")),
                Step::EmitAs {
                    account,
                    target,
                    steps,
                } => {
                    ctx.emit_as(account, target, &steps, Reply::None);
                }
            }
        }
        Ok(())
    }

    fn reply(ctx: &ExecCtx, id: &MessageId, outcome: &Outcome) -> Result<(), Error> {
        log(ctx)?;
        if ctx.get("absorb").is_some() {
            return Ok(());
        }
        match outcome {
            Outcome::Applied { .. } => Ok(()),
            Outcome::Rejected(refusal) => Err(refusal.clone()),
        }
        .map_err(|refusal| Error::new(refusal.code, format!("{id:?}: {}", refusal.message)))
    }

    fn query(ctx: &QueryCtx, key: String) -> Result<Option<Vec<u8>>, Error> {
        Ok(ctx.get(key))
    }
}

fn log(ctx: &ExecCtx) -> Result<(), Error> {
    let mut log: Vec<Env> = ctx.record("log")?.unwrap_or_default();
    log.push(ctx.env().as_ref().clone());
    ctx.put("log", &log);
    Ok(())
}

struct Ident;

impl Program for Ident {
    const NAME: &'static str = "ident";
    type Op = ();
    type Query = Asked;
    type Reply = Answer;

    fn execute(_: &ExecCtx, (): ()) -> Result<(), Error> {
        Ok(())
    }

    fn query(ctx: &QueryCtx, asked: Asked) -> Result<Answer, Error> {
        let as_the_kernel = ctx.env().origin == Origin::Root && ctx.env().sender.is_none();
        Ok(match asked {
            _ if !as_the_kernel => Answer::Account(Some(0)),
            Asked::Account(key) => Answer::Account(key.first().map(|byte| *byte as u64)),
            Asked::OfProgram(program) if program == "bad" => {
                return Err(Error::new(code::NOT_FOUND, "bad has no account"));
            }
            Asked::OfProgram(program) => {
                Answer::Account(program.bytes().next().map(|byte| 100 + byte as u64))
            }
            Asked::Runs { program, account } if program == "a" => Answer::Account(Some(account)),
            Asked::Runs { program, account } => {
                return Err(Error::new(
                    code::UNAUTHORIZED,
                    format!("{program} does not run {account}"),
                ));
            }
            Asked::Profile(_) => Answer::Profile(None),
            Asked::Profiles { .. } => Answer::Profiles {
                profiles: Vec::new(),
                next: None,
            },
        })
    }
}

fn chain() -> MockChain {
    let mut chain = MockChain::default();
    for (name, number) in [("a", 100), ("b", 101), ("c", 102)] {
        chain.seat::<Bot>(name);
        chain.register(name, number);
    }
    chain.hold(b"ada-key".to_vec(), 1);
    chain
}

fn ada() -> Origin {
    Origin::Signed(b"ada-key".to_vec())
}

fn emit(target: &str, steps: Vec<Step>, reply: bool) -> Step {
    Step::Emit {
        target: target.into(),
        steps,
        reply,
    }
}

fn put(key: &str, value: &str) -> Step {
    Step::Put(key.into(), value.into())
}

fn logged(chain: &MockChain, program: &str) -> Vec<Env> {
    let host = chain.host(program);
    host.query(chain.env(program))
        .record("log")
        .unwrap()
        .unwrap_or_default()
}

fn id(program: &str, seq: u64) -> MessageId {
    MessageId {
        program: program.into(),
        seq,
    }
}

fn account(number: AccountNumber) -> Option<Principal> {
    Some(Principal::Account(number))
}

#[allow(clippy::type_complexity)]
fn left(
    chain: &MockChain,
    program: &str,
) -> (Vec<(Vec<u8>, Vec<u8>)>, usize, Vec<u8>, usize, usize) {
    let host = chain.host(program).borrow();
    (
        host.state.clone().into_iter().collect(),
        host.blobs.len(),
        host.output.clone(),
        host.emissions.len(),
        host.events.len(),
    )
}

#[test]
fn messages_run_after_the_emitter_in_emit_order_depth_first_as_the_emitters_account() {
    let chain = chain();
    chain.at(9, 90);
    let script = vec![
        emit(
            "b",
            vec![emit("c", vec![put("seen", "by b")], false)],
            false,
        ),
        emit("b", vec![Step::Peek("c".into(), "seen".into())], false),
        put("own", "kept"),
    ];
    chain.submit(ada(), "a", &script).unwrap();
    let (a, b, c) = (
        logged(&chain, "a"),
        logged(&chain, "b"),
        logged(&chain, "c"),
    );
    assert_eq!(a.len(), 1, "a ran once");
    assert_eq!(a[0].origin, ada());
    assert_eq!(
        (a[0].sender.clone(), a[0].cause.clone()),
        (account(1), Cause::Direct)
    );
    let message = |cause| Env {
        program: "b".into(),
        origin: Origin::Program("a".into()),
        sender: account(100),
        cause: Cause::Message(cause),
        ..chain.env("b")
    };
    assert_eq!(b, [message(id("a", 0)), message(id("a", 1))]);
    assert_eq!((b[0].height, b[0].time), (9, 90));
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].origin, Origin::Program("b".into()));
    assert_eq!(c[0].sender, account(101));
    assert_eq!(c[0].cause, Cause::Message(id("b", 2)));
    let emitted = |program: &str| -> Vec<MessageId> {
        chain
            .host(program)
            .query(chain.env(program))
            .record("emitted")
            .unwrap()
            .unwrap()
    };
    assert_eq!(emitted("a"), [id("a", 0), id("a", 1)]);
    assert_eq!(emitted("b"), [id("b", 2)]);
    assert_eq!(
        chain.host("b").borrow().state[b"peek:seen".as_slice()],
        b"by b"
    );
    assert_eq!(chain.host("a").borrow().state[b"own".as_slice()], b"kept");
    chain
        .submit(ada(), "a", &vec![emit("b", vec![], false)])
        .unwrap();
    assert_eq!(logged(&chain, "b")[2].cause, Cause::Message(id("a", 3)));
}

#[test]
fn a_wanted_reply_brings_the_outcome_back_as_the_targets_message() {
    let chain = chain();
    let script = vec![emit("b", vec![Step::Output(b"hi".to_vec())], true)];
    chain.submit(ada(), "a", &script).unwrap();
    let a = logged(&chain, "a");
    assert_eq!(a.len(), 2);
    assert_eq!(
        a[1],
        Env {
            origin: Origin::Program("b".into()),
            sender: account(101),
            cause: Cause::Reply {
                id: id("a", 0),
                outcome: Outcome::Applied {
                    output: b"hi".to_vec()
                },
            },
            ..chain.env("a")
        }
    );
    chain
        .host("a")
        .borrow_mut()
        .state
        .insert(b"absorb".to_vec(), Vec::new());
    let script = vec![
        put("mine", "stays"),
        emit("b", vec![put("theirs", "gone"), Step::Refuse], true),
    ];
    chain.submit(ada(), "a", &script).unwrap();
    let a = logged(&chain, "a");
    assert_eq!(a.len(), 4);
    let Cause::Reply {
        id: replied,
        outcome,
    } = &a[3].cause
    else {
        panic!("{:?}", a[3].cause)
    };
    assert_eq!(*replied, id("a", 1));
    assert!(matches!(outcome, Outcome::Rejected(refusal) if refusal.code == code::WRONG_STATE));
    assert!(
        chain
            .host("a")
            .borrow()
            .state
            .contains_key(b"mine".as_slice())
    );
    assert!(
        !chain
            .host("b")
            .borrow()
            .state
            .contains_key(b"theirs".as_slice())
    );
    assert_eq!(
        logged(&chain, "b").len(),
        1,
        "b's rejected run was undone, its log too"
    );
}

#[test]
fn a_rejection_not_absorbed_undoes_the_emitter_whole_and_propagates_up() {
    let chain = chain();
    let before = left(&chain, "a");
    let script = vec![
        put("mine", "undone"),
        emit(
            "b",
            vec![
                put("theirs", "undone"),
                emit("c", vec![put("deep", "undone"), Step::Refuse], false),
            ],
            false,
        ),
    ];
    let refusal = chain.submit(ada(), "a", &script).unwrap_err();
    assert_eq!(refusal.code, code::WRONG_STATE);
    assert_eq!(left(&chain, "a"), before);
    for program in ["a", "b", "c"] {
        assert!(logged(&chain, program).is_empty(), "{program} was undone");
    }
    let script = vec![put("mine", "undone"), emit("b", vec![Step::Refuse], true)];
    let refusal = chain.submit(ada(), "a", &script).unwrap_err();
    assert_eq!(refusal.code, code::WRONG_STATE);
    assert!(refusal.message.contains("seq: 2"), "{refusal}");
    assert_eq!(left(&chain, "a"), before);
    let script = vec![emit("b", vec![], false), Step::Refuse];
    chain.submit(ada(), "a", &script).unwrap_err();
    assert!(logged(&chain, "b").is_empty());
}

#[test]
fn undo_covers_every_hosts_state_blobs_output_events_and_emissions() {
    let chain = chain();
    let all = || [left(&chain, "a"), left(&chain, "b"), left(&chain, "c")];
    let before = all();
    let script = vec![
        put("k", "v"),
        Step::Blob,
        Step::Event,
        Step::Output(b"out".to_vec()),
        emit(
            "b",
            vec![
                put("k", "v"),
                Step::Blob,
                Step::Event,
                emit("c", vec![Step::Event, Step::Refuse], false),
            ],
            false,
        ),
    ];
    chain.submit(ada(), "a", &script).unwrap_err();
    assert_eq!(all(), before);
    let script = vec![
        put("k", "v"),
        Step::Blob,
        Step::Event,
        Step::Output(b"out".to_vec()),
    ];
    assert_eq!(chain.submit(ada(), "a", &script).unwrap(), b"out");
    let (state, blobs, output, _, events) = left(&chain, "a");
    assert_eq!((state.len(), blobs, events), (2, 1, 1));
    assert!(
        output.is_empty(),
        "the output was handed over, not left on the host"
    );
}

#[test]
fn messages_nest_no_deeper_than_max_depth() {
    let chain = chain();
    chain
        .submit(ada(), "a", &vec![Step::Recurse(MAX_DEPTH)])
        .unwrap();
    assert_eq!(logged(&chain, "a").len() as u32, MAX_DEPTH + 1);
    let refusal = chain
        .submit(ada(), "a", &vec![Step::Recurse(MAX_DEPTH + 1)])
        .unwrap_err();
    assert_eq!(refusal.code, code::CAPACITY);
    assert_eq!(
        logged(&chain, "a").len() as u32,
        MAX_DEPTH + 1,
        "undone whole"
    );
}

#[test]
fn a_submission_acts_as_the_account_its_key_holds_or_as_no_one() {
    let chain = chain();
    chain.hold(b"agent-key".to_vec(), 5);
    chain.profile(Profile {
        number: 5,
        name: "agent".into(),
        kind: Kind::Program {
            script: None,
            manager: 1,
            category: Category::Agent,
            standing: Standing::Suspended,
        },
    });
    let nothing = abi::encode(&Vec::<Step>::new());
    chain.submit_raw(ada(), "a", &nothing).unwrap();
    chain
        .submit_raw(Origin::Signed(b"loose".to_vec()), "a", &nothing)
        .unwrap();
    let refused = chain
        .submit_raw(Origin::Signed(b"agent-key".to_vec()), "a", &nothing)
        .unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
    chain
        .submit_raw(Origin::Program("b".into()), "a", &nothing)
        .unwrap();
    chain.submit_raw(Origin::Root, "a", &nothing).unwrap();
    let senders: Vec<Option<Principal>> = logged(&chain, "a")
        .into_iter()
        .map(|env| env.sender)
        .collect();
    assert_eq!(
        senders,
        [account(1), None, account(101), Some(Principal::Root)]
    );
    let unknown = chain.submit_raw(ada(), "nobody", &nothing).unwrap_err();
    assert_eq!(unknown.code, code::UNKNOWN_PROGRAM);
}

#[test]
fn a_message_acts_as_an_agent_only_through_the_program_that_runs_it() {
    let chain = chain();
    let agent = |number, calls: Option<&str>, standing| Profile {
        number,
        name: "agent".into(),
        kind: Kind::Program {
            manager: 1,
            category: Category::Agent,
            standing,
            script: calls.map(|calls| Script {
                calls: calls.into(),
                config: String::new(),
            }),
        },
    };
    chain.profile(agent(8, Some("a"), Standing::Active));
    chain.profile(agent(9, Some("a"), Standing::Suspended));
    let as_agent = |number| Step::EmitAs {
        account: number,
        target: "c".into(),
        steps: Vec::new(),
    };
    chain.submit(ada(), "a", &vec![as_agent(8)]).unwrap();
    assert_eq!(logged(&chain, "c")[0].sender, account(8));
    for (program, number) in [("b", 8), ("a", 9), ("a", 1)] {
        let refused = chain
            .submit(ada(), program, &vec![as_agent(number)])
            .unwrap_err();
        assert_eq!(refused.code, code::UNAUTHORIZED, "{program} as {number}");
    }
    assert_eq!(logged(&chain, "c").len(), 1);
    let mut seated = chain;
    seated.seat::<Ident>(MockHost::role(abi::role::IDENTITY));
    seated
        .submit(Origin::Signed(vec![7]), "a", &vec![as_agent(40)])
        .unwrap();
    assert_eq!(logged(&seated, "c")[1].sender, account(40));
    let refused = seated
        .submit(Origin::Signed(vec![7]), "b", &vec![as_agent(40)])
        .unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
}

#[test]
fn the_roster_answers_programs_as_the_identity_role_does() {
    let chain = chain();
    chain.profile(Profile {
        number: 6,
        name: "idle".into(),
        kind: Kind::Program {
            script: None,
            manager: 1,
            category: Category::Agent,
            standing: Standing::Revoked,
        },
    });
    for named in [1, 6, 100] {
        chain.submit(ada(), "a", &vec![Step::Named(named)]).unwrap();
    }
    let refused = chain
        .submit(ada(), "a", &vec![Step::Named(999)])
        .unwrap_err();
    assert_eq!(refused.code, code::INVALID_INPUT, "no account 999");
    assert_eq!(
        chain
            .roster()
            .account(&Asked::OfProgram("b".into()))
            .unwrap(),
        Some(101)
    );
    assert_eq!(
        chain
            .roster()
            .account(&Asked::OfProgram("zed".into()))
            .unwrap(),
        None
    );
}

#[test]
fn a_seated_identity_program_is_asked_as_the_kernel_asks_it() {
    let mut chain = chain();
    chain.seat::<Ident>(MockHost::role(abi::role::IDENTITY));
    chain.seat::<Bot>("bad");
    let script = vec![emit("b", vec![], false)];
    chain.submit(Origin::Signed(vec![7]), "a", &script).unwrap();
    chain.submit(Origin::Signed(vec![]), "a", &script).unwrap();
    let a = logged(&chain, "a");
    assert_eq!(
        (a[0].sender.clone(), a[1].sender.clone()),
        (account(7), None)
    );
    assert_eq!(
        logged(&chain, "b")[0].sender,
        account(100 + u64::from(b'a'))
    );
    chain.submit(ada(), "a", &script).unwrap();
    assert_eq!(logged(&chain, "a")[2].sender, account(u64::from(b'a')));
    let refused = chain.submit(Origin::Root, "bad", &script).unwrap_err();
    assert_eq!(refused.code, code::NOT_FOUND);
    assert!(logged(&chain, "b").len() == 3 && logged(&chain, "bad").is_empty());
    chain
        .host("bad")
        .borrow_mut()
        .state
        .insert(b"absorb".to_vec(), Vec::new());
    let script = vec![emit("b", vec![], true)];
    chain.submit(Origin::Root, "bad", &script).unwrap();
    let bad = logged(&chain, "bad");
    assert_eq!(bad.len(), 2);
    assert!(
        matches!(&bad[1].cause, Cause::Reply { outcome: Outcome::Rejected(refusal), .. } if refusal.code == code::NOT_FOUND),
        "{:?}",
        bad[1].cause
    );
    let mut wrong = MockChain::default();
    wrong.seat::<Bot>("a");
    wrong.seat::<Wrong>(MockHost::role(abi::role::IDENTITY));
    let refused = wrong.submit(ada(), "a", &script).unwrap_err();
    assert_eq!(refused.code, code::UNEXPECTED_REPLY);
}

#[test]
fn the_chain_asks_the_roles_it_was_founded_with() {
    let roles = MockHost::roles_with(abi::role::IDENTITY, "people");
    let mut chain = MockChain::founded(roles.clone());
    chain.seat::<Bot>("a");
    chain.seat::<Ident>("people");
    chain
        .submit(Origin::Signed(vec![7]), "a", &vec![put("k", "v")])
        .unwrap();
    let ran = &logged(&chain, "a")[0];
    assert_eq!((ran.sender.clone(), &ran.roles), (account(7), &roles));
}

struct Wrong;

impl Program for Wrong {
    const NAME: &'static str = "wrong";
    type Op = ();
    type Query = Asked;
    type Reply = Answer;

    fn execute(_: &ExecCtx, (): ()) -> Result<(), Error> {
        Ok(())
    }

    fn query(_: &QueryCtx, _: Asked) -> Result<Answer, Error> {
        Ok(Answer::Profile(None))
    }
}

#[test]
fn init_is_a_frame_its_messages_run_after_it_and_a_refusal_undoes_it() {
    let chain = chain();
    let script = vec![
        put("init", "wrote"),
        emit("b", vec![put("from", "a")], false),
    ];
    chain.init("a", &script).unwrap();
    assert_eq!(logged(&chain, "a")[0].origin, Origin::Root);
    assert_eq!(logged(&chain, "b")[0].origin, Origin::Program("a".into()));
    let before = (left(&chain, "a"), left(&chain, "b"));
    let refused = vec![put("again", "x"), emit("b", vec![Step::Refuse], false)];
    assert_eq!(
        chain.init("a", &refused).unwrap_err().code,
        code::WRONG_STATE
    );
    assert_eq!((left(&chain, "a"), left(&chain, "b")), before);
}

#[test]
fn a_program_queries_itself() {
    let chain = chain();
    chain.submit(ada(), "a", &vec![put("k", "v")]).unwrap();
    let peek = vec![Step::Peek("a".into(), "k".into())];
    chain.submit(ada(), "a", &peek).unwrap();
    let peeked = chain
        .host("a")
        .borrow()
        .state
        .get(b"peek:k".as_slice())
        .cloned();
    assert_eq!(peeked, Some(b"v".to_vec()));
}

#[test]
fn every_program_reads_the_one_blob_store_the_network_keeps() {
    let chain = chain();
    let id = crate::blob_id(HashKind::Sha256, "text", b"body").unwrap();
    let held = |program: &str| {
        let key = format!("held:{}", abi::hex(id.digest()));
        chain
            .query::<String, Option<Vec<u8>>>(program, &key)
            .unwrap()
    };
    chain
        .submit(
            ada(),
            "a",
            &vec![Step::Blob, emit("b", vec![Step::Stat(id)], false)],
        )
        .unwrap();
    assert_eq!(held("b"), Some(vec![1]));
    chain.submit(ada(), "c", &vec![Step::Stat(id)]).unwrap();
    assert_eq!(held("c"), Some(vec![1]));
}

struct Needy;

impl Program for Needy {
    const NAME: &'static str = "needy";
    type Op = ();
    type Query = String;
    type Reply = Option<Vec<u8>>;

    fn execute(ctx: &ExecCtx, (): ()) -> Result<(), Error> {
        log(ctx)
    }

    fn access(_: &QueryCtx, (): &()) -> Result<Vec<crate::Need>, Error> {
        Ok(vec![crate::Need::write("k".to_owned())])
    }

    fn query(ctx: &QueryCtx, key: String) -> Result<Option<Vec<u8>>, Error> {
        Ok(ctx.get(key))
    }
}

#[test]
fn an_op_runs_unchecked_where_no_program_fills_the_access_role() {
    let unbound: Roles = MockHost::roles()
        .iter()
        .filter(|(role, _)| *role != abi::role::ACCESS)
        .map(|(role, program)| (role.to_owned(), program.clone()))
        .collect();
    let mut chain = MockChain::founded(unbound);
    chain.seat::<Needy>("needy");
    chain.submit(ada(), "needy", &()).unwrap();
    assert_eq!(logged(&chain, "needy").len(), 1, "the op ran");

    let mut bound = MockChain::default();
    bound.seat::<Needy>("needy");
    let refused = bound.submit(ada(), "needy", &()).unwrap_err();
    assert_eq!(
        refused.code,
        code::UNKNOWN_PROGRAM,
        "where the role is bound the chain still asks it, and nothing is seated there"
    );
    assert!(logged(&bound, "needy").is_empty());
}
