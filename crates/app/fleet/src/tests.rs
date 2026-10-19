// fleet on a MockChain: runs go to the machine free longest unless pinned, a machine claims,
// beats and reports, an unclaimed assignment or a silent machine moves its runs, and the asker's
// claim policy decides which machines may take their runs.
use std::cell::Cell;

use ::program::{Error, ExecCtx, MockChain, Origin, Program, QueryCtx, code};
use abi::role::egress::DEFAULT_WAIT;
use borsh::{BorshDeserialize, BorshSerialize};
use store::Item;

use crate::state::CLAIMS;
use crate::*;

const ADA: &[u8] = b"ada-key";
const ADA_ACCOUNT: u64 = 11;
const BO: &[u8] = b"bo-key";
const BO_ACCOUNT: u64 = 13;
const A: &[u8] = b"machine-a-key";
const A_ACCOUNT: u64 = 21;
const B: &[u8] = b"machine-b-key";
const B_ACCOUNT: u64 = 22;
const WATCHER: &str = "watcher";
const HEARD: Item<Vec<Ended>> = Item::new("heard");

// A program told of each run's end, as harness or actions would be.
#[derive(Debug, BorshSerialize, BorshDeserialize)]
enum Told {
    Ended(Ended),
}

struct Watcher;

impl Program for Watcher {
    const NAME: &'static str = WATCHER;
    type Op = Told;
    type Query = ();
    type Reply = Vec<Ended>;

    fn execute(ctx: &ExecCtx, op: Told) -> Result<(), Error> {
        ctx.env().sent_by(PROGRAM)?;
        let Told::Ended(ended) = op;
        let mut heard = HEARD.get(ctx)?.unwrap_or_default();
        heard.push(ended);
        HEARD.put(ctx, &heard);
        Ok(())
    }

    fn query(ctx: &QueryCtx, (): ()) -> Result<Vec<Ended>, Error> {
        Ok(HEARD.get(ctx)?.unwrap_or_default())
    }
}

struct World {
    chain: MockChain,
    height: Cell<u64>,
}

fn world() -> World {
    let mut chain = MockChain::default();
    chain.seat::<Fleet>(PROGRAM);
    chain.seat::<Watcher>(WATCHER);
    chain.seat::<::scheduler::Scheduler>(::scheduler::PROGRAM);
    chain.seat::<ownership::Ownership>(ownership::PROGRAM);
    for (key, account) in [
        (ADA, ADA_ACCOUNT),
        (BO, BO_ACCOUNT),
        (A, A_ACCOUNT),
        (B, B_ACCOUNT),
    ] {
        chain.hold(key, account);
    }
    chain.register(PROGRAM, 903);
    chain.register(WATCHER, 904);
    World {
        chain,
        height: Cell::new(1),
    }
}

fn job() -> Spec {
    Spec {
        kind: Kind::Job,
        image: Some("toolchain".into()),
        command: vec!["make".into(), "check".into()],
        env: Vec::new(),
        workdir: None,
        inputs: Vec::new(),
        asks: Asks::default(),
        on: None,
        out: None,
        on_loss: OnLoss::Retry,
        notify: Some(Notify::Program {
            program: WATCHER.into(),
            deliver: vec![0],
        }),
    }
}

fn machine(account: u64, name: &str) -> MachineId {
    MachineId {
        account,
        name: name.into(),
    }
}

impl World {
    fn act(&self, key: &[u8], op: &Op) -> Result<Vec<u8>, Error> {
        self.chain.submit(Origin::Signed(key.to_vec()), PROGRAM, op)
    }

    // The run asked for, and the block after it ended, where fleet places it.
    fn ask_as(&self, key: &[u8], spec: Spec) -> u64 {
        let made = self.act(key, &Op::Ask { spec }).unwrap();
        self.to(self.height.get() + 2);
        abi::decode(&made).unwrap()
    }

    fn ask(&self, spec: Spec) -> u64 {
        self.ask_as(ADA, spec)
    }

    fn ready(&self, key: &[u8], name: &str, labels: &[&str], slots: u64) {
        self.ready_serving(key, name, labels, slots, Serves::Anyone);
    }

    fn ready_serving(&self, key: &[u8], name: &str, labels: &[&str], slots: u64, serves: Serves) {
        let ready = Op::Ready {
            name: name.into(),
            labels: labels.iter().map(|label| label.to_string()).collect(),
            slots,
            serves,
        };
        self.act(key, &ready).unwrap();
    }

    fn claim(&self, key: &[u8], run: u64, attempt: u32) -> Result<Vec<u8>, Error> {
        self.act(key, &Op::Claim { run, attempt })
    }

    fn report(&self, key: &[u8], run: u64, attempt: u32, state: State) -> Result<Vec<u8>, Error> {
        let report = Op::Report {
            run,
            attempt,
            state,
            summary: format!("{} at {attempt}", state.word()),
            out: None,
        };
        self.act(key, &report)
    }

    fn run(&self, id: u64) -> RunRow {
        let Reply::Run(Some(run)) = self.chain.query(PROGRAM, &Query::Run { id }).unwrap() else {
            panic!("no run {id}");
        };
        run
    }

    fn placed(&self, id: u64) -> (State, Option<MachineId>, u32) {
        let run = self.run(id);
        (run.state, run.machine, run.attempt)
    }

    fn machine(&self, id: MachineId) -> MachineView {
        let asked = Query::Machine { machine: id };
        let Reply::Machine(Some(view)) = self.chain.query(PROGRAM, &asked).unwrap() else {
            panic!("no such machine");
        };
        view
    }

    fn holds(&self, id: MachineId) -> Vec<u64> {
        let asked = Query::Holds {
            machine: id,
            page: PageRequest::default(),
        };
        let Reply::Runs(page) = self.chain.query(PROGRAM, &asked).unwrap() else {
            panic!("Holds answers runs");
        };
        page.items.into_iter().map(|run| run.id).collect()
    }

    fn queued(&self) -> Vec<u64> {
        let asked = Query::Queued {
            page: PageRequest::default(),
        };
        let Reply::Runs(page) = self.chain.query(PROGRAM, &asked).unwrap() else {
            panic!("Queued answers runs");
        };
        page.items.into_iter().map(|run| run.id).collect()
    }

    fn heard(&self) -> Vec<Ended> {
        self.chain.query(WATCHER, &()).unwrap()
    }

    // Every block up to `height` ends, and what fleet booked for them runs.
    fn to(&self, height: u64) {
        self.height.set(height);
        self.chain.at(height, height * 1000);
    }
}

#[test]
fn a_run_goes_to_the_machine_free_longest_and_ends_as_its_machine_reports() {
    let world = world();
    world.to(2);
    world.ready(A, "a", &[], 1);
    world.to(3);
    world.ready(B, "b", &[], 1);
    let run = world.ask(job());
    let a = machine(A_ACCOUNT, "a");
    assert_eq!(world.placed(run), (State::Assigned, Some(a.clone()), 1));
    let other = world.claim(B, run, 1).unwrap_err();
    assert_eq!(other.code, code::WRONG_STATE);
    let stale = world.claim(A, run, 2).unwrap_err();
    assert_eq!(stale.code, code::WRONG_STATE);
    world.claim(A, run, 1).unwrap();
    assert_eq!(world.run(run).state, State::Running);
    let unended = world.report(A, run, 1, State::Running).unwrap_err();
    assert_eq!(unended.code, code::INVALID_INPUT);
    world.to(6);
    let snapshot = ::program::BlobId::Sha256([3; 32]);
    let report = Op::Report {
        run,
        attempt: 1,
        state: State::Passed,
        summary: "GATES ok".into(),
        out: Some(Out {
            path: format!("/shared/fleet/{run}/"),
            snapshot,
        }),
    };
    world.act(A, &report).unwrap();
    let ended = world.run(run);
    assert_eq!((ended.state, ended.finished), (State::Passed, Some(6)));
    assert_eq!(ended.out.map(|out| out.snapshot), Some(snapshot));
    let told = Ended {
        run,
        attempt: 1,
        state: State::Passed,
    };
    assert_eq!(world.heard(), [told]);
    let freed = world.machine(a.clone()).machine;
    assert_eq!((freed.busy, freed.idle_since), (0, Some(6)));
    // b has been free since block 3, a only since 6
    let next = world.ask(job());
    assert_eq!(world.placed(next).1, Some(machine(B_ACCOUNT, "b")));
}

#[test]
fn a_pinned_run_waits_for_its_machine_and_labels_must_all_be_held() {
    let world = world();
    world.to(2);
    world.ready(A, "a", &["oci"], 1);
    world.to(3);
    world.ready(B, "b", &["oci", "warm"], 1);
    let warm = world.ask(Spec {
        on: Some(Affinity::Labels(vec!["oci".into(), "warm".into()])),
        ..job()
    });
    assert_eq!(world.placed(warm).1, Some(machine(B_ACCOUNT, "b")));
    let pinned = world.ask(Spec {
        on: Some(Affinity::Machine(machine(B_ACCOUNT, "b"))),
        ..job()
    });
    let gpu = world.ask(Spec {
        on: Some(Affinity::Labels(vec!["gpu".into()])),
        ..job()
    });
    assert_eq!(
        world.queued(),
        [pinned, gpu],
        "b is full and nothing holds gpu"
    );
    world.claim(B, warm, 1).unwrap();
    world.report(B, warm, 1, State::Failed).unwrap();
    assert_eq!(world.placed(pinned).1, Some(machine(B_ACCOUNT, "b")));
    world.ready(A, "gpu-box", &["gpu"], 1);
    assert_eq!(world.placed(gpu).1, Some(machine(A_ACCOUNT, "gpu-box")));
    assert!(world.queued().is_empty());
}

#[test]
fn an_unclaimed_assignment_takes_its_machine_as_gone_and_moves_the_run() {
    let world = world();
    world.to(2);
    world.ready(A, "a", &[], 1);
    world.to(3);
    world.ready(B, "b", &[], 1);
    world.to(4);
    let run = world.ask(job());
    assert_eq!(world.placed(run).1, Some(machine(A_ACCOUNT, "a")));
    // placed at block 5, the block after the Ask
    world.to(5 + DEFAULT_WAIT + 1);
    assert_eq!(
        world.placed(run),
        (State::Assigned, Some(machine(B_ACCOUNT, "b")), 2)
    );
    let a = world.machine(machine(A_ACCOUNT, "a")).machine;
    assert!(a.gone && a.busy == 0);
    let late = world.claim(A, run, 1).unwrap_err();
    assert_eq!(late.code, code::WRONG_STATE);
    // the run's own check for its second attempt finds it claimed and changes nothing
    world.claim(B, run, 2).unwrap();
    let unclaimed = Op::Unclaimed { run, attempt: 2 };
    world.act(BO, &unclaimed).unwrap();
    assert_eq!(world.run(run).state, State::Running);
    world.ready(A, "a", &[], 1);
    assert!(!world.machine(machine(A_ACCOUNT, "a")).machine.gone);
}

#[test]
fn a_silent_machine_loses_its_runs_and_a_parked_run_waits_for_it() {
    let world = world();
    world.to(2);
    world.ready(A, "a", &[], 2);
    let retry = world.ask(job());
    let parked = world.ask(Spec {
        on_loss: OnLoss::Park,
        ..job()
    });
    // both claimed at block 6, which books the lapse check for 6 + the wait
    world.claim(A, retry, 1).unwrap();
    world.claim(A, parked, 1).unwrap();
    // a beat before the check keeps the machine, and the check books the next from it
    world.to(5 + DEFAULT_WAIT);
    world.act(A, &Op::Beat { name: "a".into() }).unwrap();
    world.to(6 + DEFAULT_WAIT + 1);
    assert_eq!(world.run(retry).state, State::Running);
    let beat = 5 + DEFAULT_WAIT;
    world.to(beat + DEFAULT_WAIT + 1);
    let a = machine(A_ACCOUNT, "a");
    assert_eq!(world.placed(retry), (State::Queued, None, 1));
    assert_eq!(world.placed(parked), (State::Lost, Some(a.clone()), 1));
    let lost = Ended {
        run: parked,
        attempt: 1,
        state: State::Lost,
    };
    assert_eq!(world.heard(), [lost]);
    let gone = world.machine(a.clone()).machine;
    assert!(gone.gone && gone.busy == 0);
    world.ready(B, "b", &[], 1);
    assert_eq!(
        world.placed(retry),
        (State::Assigned, Some(machine(B_ACCOUNT, "b")), 2)
    );
    // the parked run's machine comes back and says how it ended
    world.act(A, &Op::Beat { name: "a".into() }).unwrap();
    world.report(A, parked, 1, State::Passed).unwrap();
    assert_eq!(world.run(parked).state, State::Passed);
}

#[test]
fn a_cancel_ends_a_queued_run_and_stops_a_running_one_and_requeue_runs_it_again() {
    let world = world();
    world.to(2);
    let queued = world.ask(job());
    world.act(ADA, &Op::Cancel { run: queued }).unwrap();
    assert_eq!(world.run(queued).state, State::Cancelled);
    world.ready(A, "a", &[], 1);
    let running = world.ask(job());
    world.claim(A, running, 1).unwrap();
    world.act(ADA, &Op::Cancel { run: running }).unwrap();
    assert_eq!(world.run(running).state, State::Stopping);
    world.report(A, running, 1, State::Cancelled).unwrap();
    assert_eq!(world.run(running).state, State::Cancelled);
    let again = world.act(ADA, &Op::Cancel { run: running }).unwrap_err();
    assert_eq!(again.code, code::WRONG_STATE);
    world.act(ADA, &Op::Requeue { run: running }).unwrap();
    assert_eq!(
        world.placed(running),
        (State::Assigned, Some(machine(A_ACCOUNT, "a")), 2)
    );
    let live = world.act(ADA, &Op::Requeue { run: running }).unwrap_err();
    assert_eq!(live.code, code::WRONG_STATE);
}

#[test]
fn a_service_runs_again_after_an_end_nobody_asked_for_until_it_is_stopped() {
    let world = world();
    world.to(2);
    world.ready(A, "a", &[], 1);
    let service = world.ask(Spec {
        kind: Kind::Service,
        ..job()
    });
    world.claim(A, service, 1).unwrap();
    world.report(A, service, 1, State::Failed).unwrap();
    assert_eq!(
        world.placed(service),
        (State::Assigned, Some(machine(A_ACCOUNT, "a")), 2)
    );
    assert!(world.heard().is_empty());
    world.claim(A, service, 2).unwrap();
    world.act(ADA, &Op::Cancel { run: service }).unwrap();
    world.report(A, service, 2, State::Cancelled).unwrap();
    assert_eq!(world.run(service).state, State::Cancelled);
}

#[test]
fn a_declined_run_never_goes_back_to_that_machine() {
    let world = world();
    world.to(2);
    world.ready(A, "a", &[], 1);
    let run = world.ask(job());
    let decline = Op::Decline {
        run,
        attempt: 1,
        why: "no such image here".into(),
    };
    world.act(A, &decline).unwrap();
    assert_eq!(world.queued(), [run]);
    assert!(world.run(run).summary.contains("no such image here"));
    world.ready(B, "b", &[], 1);
    assert_eq!(
        world.placed(run),
        (State::Assigned, Some(machine(B_ACCOUNT, "b")), 2)
    );
    assert!(world.holds(machine(A_ACCOUNT, "a")).is_empty());
    assert_eq!(world.holds(machine(B_ACCOUNT, "b")), [run]);
}

#[test]
fn a_machine_takes_only_the_runs_of_whom_it_serves() {
    let world = world();
    world.to(2);
    world.ready_serving(A, "a", &[], 2, Serves::Listed(vec![BO_ACCOUNT]));
    let adas = world.ask(job());
    let bos = world.ask_as(BO, job());
    assert_eq!(world.queued(), [adas]);
    assert_eq!(world.placed(bos).1, Some(machine(A_ACCOUNT, "a")));
}

#[test]
fn the_askers_claim_policy_decides_which_machines_take_their_runs() {
    let world = world();
    let policy = ownership::Op::Create {
        name: "my machines".into(),
        write: ownership::Writers::Listed(vec![ownership::Party::Account(B_ACCOUNT)]),
    };
    let made = world
        .chain
        .submit(Origin::Signed(ADA.to_vec()), ownership::PROGRAM, &policy)
        .unwrap();
    let policy: ownership::PolicyId = abi::decode(&made).unwrap();
    let default = ownership::Op::SetDefault {
        scope: ownership::Scope::Kind("fleet.claim".into()),
        policy: Some(policy),
    };
    world
        .chain
        .submit(Origin::Signed(ADA.to_vec()), ownership::PROGRAM, &default)
        .unwrap();
    world.to(2);
    world.ready(A, "a", &[], 1);
    world.to(3);
    world.ready(B, "b", &[], 1);
    let adas = world.ask(job());
    assert_eq!(world.placed(adas).1, Some(machine(B_ACCOUNT, "b")));
    let record = ownership::Query::Record {
        program: PROGRAM.into(),
        key: CLAIMS.key(&adas),
    };
    let ownership::Reply::Record(Some(record)) =
        world.chain.query(ownership::PROGRAM, &record).unwrap()
    else {
        panic!("the guard recorded the claim row");
    };
    assert_eq!(record.kind.as_deref(), Some("fleet.claim"));
    assert_eq!(record.owner, ownership::Party::Account(ADA_ACCOUNT));
    // bo's runs follow bo's open policy, so a takes them
    let bos = world.ask_as(BO, job());
    assert_eq!(world.placed(bos).1, Some(machine(A_ACCOUNT, "a")));
    let stranger = world.act(BO, &Op::Cancel { run: adas }).unwrap_err();
    assert_eq!(stranger.code, code::UNAUTHORIZED);
    world.claim(B, adas, 1).unwrap();
    world.act(ADA, &Op::Cancel { run: adas }).unwrap();
    assert_eq!(world.run(adas).state, State::Stopping);
}
