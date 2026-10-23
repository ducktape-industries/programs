//! The `fleet_run` tool with fleet seated (beside an ownership holding no record, so any machine
//! may claim): the agent asks fleet as itself, a machine of Bo's takes the run, and its end comes
//! back as `RunEnded`, waking the conversation that asked; a forged or foreign end reaches none.
use abi::role::egress::DEFAULT_WAIT;

use super::schedule::{answered, refused, turn};
use super::*;

const MACHINE: &str = "box";
const COMMIT: &str = "c1e03779c882b9fdeeb3eff411653c54c23fdd37";

struct Unguarded;

impl ::program::Program for Unguarded {
    const NAME: &'static str = ownership::PROGRAM;
    type Op = ownership::Op;
    type Query = ownership::Query;
    type Reply = ownership::Reply;

    fn execute(_: &::program::ExecCtx, _: ownership::Op) -> Result<(), Error> {
        Ok(())
    }

    fn query(_: &::program::QueryCtx, query: ownership::Query) -> Result<ownership::Reply, Error> {
        match query {
            ownership::Query::Record { .. } => Ok(ownership::Reply::Record(None)),
            _ => Err(::program::invalid("this ownership holds no record")),
        }
    }
}

fn fleeted() -> World {
    let mut world = world();
    world.chain.seat_awaiting::<fleet::Fleet>(fleet::PROGRAM);
    let register = identity::Op::RegisterProgram {
        program: fleet::PROGRAM.into(),
    };
    world
        .chain
        .submit(Origin::Root, identity::PROGRAM, &register)
        .unwrap();
    world.chain.seat::<Unguarded>(ownership::PROGRAM);
    world
}

fn said(chain: &MockChain, at: u64, text: &str) {
    chain.at(at, 0);
    let words = json!([{ "type": "text", "text": text }]);
    model_says(chain, words, "end_turn");
}

fn fleet_op(chain: &MockChain, key: &[u8], op: &fleet::Op) -> Result<Vec<u8>, Error> {
    chain.submit(signed(key), fleet::PROGRAM, op)
}

fn ready() -> fleet::Op {
    fleet::Op::Ready {
        name: MACHINE.into(),
        labels: vec!["warm".into()],
        slots: 1,
        serves: fleet::Serves::Anyone,
    }
}

fn workload(chain: &MockChain, id: u64) -> fleet::Workload {
    let asked = fleet::Query::Workload { id };
    let fleet::Reply::Workload(Some(found)) = chain.query(fleet::PROGRAM, &asked).unwrap() else {
        panic!("fleet holds run {id}")
    };
    found
}

fn run_row(chain: &MockChain, id: u64) -> fleet::RunRow {
    let asked = fleet::Query::Run { id };
    let fleet::Reply::Run(Some(found)) = chain.query(fleet::PROGRAM, &asked).unwrap() else {
        panic!("fleet holds run {id}")
    };
    found
}

fn told(chain: &MockChain, thread: u64) -> Vec<String> {
    frames(chain, thread)
        .into_iter()
        .chain(held(chain, thread))
        .filter_map(|body| match body {
            Body::Post {
                author: Principal::Root,
                text,
            } => Some(text),
            _ => None,
        })
        .collect()
}

fn wakes(chain: &MockChain, thread: u64) -> Vec<Wake> {
    let asked = Query::Wakes { thread };
    let Reply::Wakes(wakes) = chain.query(&harness(), &asked).unwrap() else {
        panic!("harness answers what wakes a conversation")
    };
    wakes
}

fn run_of(chain: &MockChain, thread: u64, call: &str) -> u64 {
    let answer = answered(chain, thread, call);
    answer["run"].as_u64().unwrap_or_else(|| panic!("{answer}"))
}

fn waits_on_run(chain: &MockChain, thread: u64, run: u64) -> bool {
    wakes(chain, thread).contains(&Wake::Run { run })
}

#[test]
fn a_run_asks_fleet_as_the_agent_and_its_end_wakes_the_conversation_that_asked() {
    let world = fleeted();
    let chain = &world.chain;
    let thread = world.opened("m1", "run the check", Some(WINDOW));
    fleet_op(chain, BO, &ready()).unwrap();
    let run =
        json!({ "command": ["make", "check"], "on": { "labels": ["warm"] }, "env": { "A": "1" } });
    turn(&world, 3, &[("r1", "fleet_run", run)]);
    let id = run_of(chain, thread, "r1");
    said(chain, 5, "Asked.");
    said(chain, 6, "It runs.");

    let asked = workload(chain, id);
    assert_eq!(asked.by, world.agent, "it asks as the agent");
    assert_eq!(asked.spec.command, ["make", "check"]);
    assert_eq!(
        asked.spec.on,
        Some(fleet::Affinity::Labels(vec!["warm".into()]))
    );
    assert_eq!(asked.spec.env, [("A".to_owned(), "1".to_owned())]);
    let notify = fleet::Notify::Program {
        program: harness(),
        deliver: run_ended_as(thread),
    };
    assert_eq!(asked.spec.notify, Some(notify));
    assert!(
        waits_on_run(chain, thread, id),
        "{:?}",
        wakes(chain, thread)
    );

    chain.at(7, 0);
    assert_eq!(run_row(chain, id).state, fleet::State::Assigned);
    assert!(told(chain, thread).is_empty(), "nothing ended yet");
    assert!(assigned(chain, SERVICE).is_empty(), "the turn is over");
    fleet_op(
        chain,
        BO,
        &fleet::Op::Claim {
            run: id,
            attempt: 1,
        },
    )
    .unwrap();
    let report = fleet::Op::Report {
        run: id,
        attempt: 1,
        state: fleet::State::Passed,
        summary: "make check ok".into(),
        out: Some(fleet::Out {
            path: "/shared/fleet/1/".into(),
            snapshot: BlobId::Sha256([7; 32]),
        }),
    };
    fleet_op(chain, BO, &report).unwrap();

    let [message] = told(chain, thread).try_into().expect("one message");
    assert!(message.contains(&format!("run {id} passed")), "{message}");
    let detailed = message.contains("make check ok") && message.contains("/shared/fleet/1/");
    assert!(detailed, "{message}");
    assert_eq!(
        assigned(chain, SERVICE).len(),
        1,
        "the message asks the model"
    );
    assert!(
        !waits_on_run(chain, thread, id),
        "{:?}",
        wakes(chain, thread)
    );
    let summary = run_row(chain, id).summary;
    assert!(!summary.contains("not told"), "{summary}");
}

#[test]
fn a_script_runs_with_sh_and_a_run_names_one_command_or_one_script() {
    let world = fleeted();
    let chain = &world.chain;
    let thread = world.opened("m1", "run a script", Some(WINDOW));
    let snapshot = abi::hex(&[9u8; 32]);
    let scripted = json!({
        "script": "make check > $OUT/log",
        "on_loss": "park",
        "inputs": [
            { "forge": { "path": "/work", "repo": "programs", "commit": COMMIT } },
            { "files": { "path": "/in/a.md", "files": "/shared/a.md", "snapshot": snapshot } },
            { "blob": { "path": "/in/b", "blob": snapshot } },
            { "scratch": { "path": "/scratch", "memory": true, "size": "8Gi" } },
        ],
        "asks": { "cpu": "2" },
        "on": { "machine": { "account": world.bo, "name": MACHINE } },
        "out": "/shared/runs/a/",
    });
    let calls = [
        ("s1", "fleet_run", scripted),
        (
            "s2",
            "fleet_run",
            json!({ "command": ["make"], "script": "make" }),
        ),
        ("s3", "fleet_run", json!({})),
    ];
    turn(&world, 3, &calls);
    let spec = workload(chain, run_of(chain, thread, "s1")).spec;
    assert_eq!(spec.command, ["sh", "-c", "make check > $OUT/log"]);
    assert_eq!(spec.on_loss, fleet::OnLoss::Park);
    assert_eq!(spec.kind, fleet::Kind::Job);
    assert_eq!(spec.out.as_deref(), Some("/shared/runs/a/"));
    let inputs = vec![
        fleet::Input::Forge {
            path: "/work".into(),
            repo: "programs".into(),
            commit: COMMIT.into(),
        },
        fleet::Input::Files {
            path: "/in/a.md".into(),
            files: "/shared/a.md".into(),
            snapshot: Some(BlobId::Sha256([9; 32])),
        },
        fleet::Input::Blob {
            path: "/in/b".into(),
            blob: BlobId::Sha256([9; 32]),
        },
        fleet::Input::Scratch {
            path: "/scratch".into(),
            memory: true,
            size: Some("8Gi".into()),
        },
    ];
    assert_eq!(spec.inputs, inputs);
    assert_eq!(spec.asks.cpu.as_deref(), Some("2"));
    let pinned = fleet::MachineId {
        account: world.bo,
        name: MACHINE.into(),
    };
    assert_eq!(spec.on, Some(fleet::Affinity::Machine(pinned)));
    for call in ["s2", "s3"] {
        let text = refused(chain, thread, call);
        let named = text.starts_with(code::INVALID_INPUT) && text.contains("script");
        assert!(named, "{text}");
    }
}

#[test]
fn a_parked_run_whose_machine_goes_silent_is_told_lost_and_may_still_end() {
    let world = fleeted();
    let chain = &world.chain;
    let thread = world.opened("m1", "apply once", Some(WINDOW));
    fleet_op(chain, BO, &ready()).unwrap();
    let run = json!({ "command": ["apply"], "on_loss": "park" });
    turn(&world, 3, &[("r1", "fleet_run", run)]);
    let id = run_of(chain, thread, "r1");
    said(chain, 5, "Asked.");
    chain.at(7, 0);
    fleet_op(
        chain,
        BO,
        &fleet::Op::Claim {
            run: id,
            attempt: 1,
        },
    )
    .unwrap();

    chain.at(7 + DEFAULT_WAIT + 2, 0);
    assert_eq!(run_row(chain, id).state, fleet::State::Lost);
    let [message] = told(chain, thread).try_into().expect("one message");
    assert!(message.contains(&format!("run {id} was lost")), "{message}");
    assert!(
        waits_on_run(chain, thread, id),
        "{:?}",
        wakes(chain, thread)
    );
}

#[test]
fn only_fleet_tells_a_run_end_and_only_of_a_run_the_conversations_agent_asked_for() {
    let world = fleeted();
    let chain = &world.chain;
    let thread = world.opened("m1", "wait for runs", Some(WINDOW));
    said(chain, 3, "Waiting.");
    let forged = Op::RunEnded {
        thread,
        ended: fleet::RunEnd {
            run: 1,
            attempt: 1,
            state: fleet::State::Passed,
        },
    };
    let refusal = chain
        .submit(signed(ADA), harness().as_str(), &forged)
        .unwrap_err();
    assert_eq!(refusal.code, code::UNAUTHORIZED, "{refusal}");

    let spec = fleet::Spec {
        kind: fleet::Kind::Job,
        image: None,
        command: vec!["true".into()],
        env: Vec::new(),
        workdir: None,
        inputs: Vec::new(),
        asks: fleet::Asks::default(),
        on: None,
        out: None,
        on_loss: fleet::OnLoss::Retry,
        notify: Some(fleet::Notify::Program {
            program: harness(),
            deliver: run_ended_as(thread),
        }),
    };
    let made = fleet_op(chain, ADA, &fleet::Op::Ask { spec }).unwrap();
    let run: u64 = abi::decode(&made).unwrap();
    fleet_op(chain, ADA, &fleet::Op::Cancel { run }).unwrap();
    let row = run_row(chain, run);
    assert_eq!(row.state, fleet::State::Cancelled);
    assert!(row.summary.contains("not told"), "{}", row.summary);
    assert!(
        told(chain, thread).is_empty(),
        "Ada's run reaches no conversation of the agent's"
    );
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "nothing asks the model"
    );
}
