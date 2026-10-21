// fleet on a MockChain: runs go to the machine free longest unless pinned, a machine claims,
// beats and reports, an unclaimed assignment or a silent machine moves its runs, and the asker's
// claim policy decides which machines may take their runs.
use std::cell::Cell;

use ::program::{Error, ExecCtx, MockChain, Origin, Program, QueryCtx, code};
use abi::role::egress::{Answer, DEFAULT_WAIT, Op as Egress, RequestRow, StreamEnd, http};
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
const WORKER: &str = "worker";
const WORKER_ACCOUNT: u64 = 31;
const HEARD: Item<Vec<Ended>> = Item::new("heard");
const NODE: [u8; 32] = [7; 32];
const PODS: &str = "pods";
const FLEET_ACCOUNT: u64 = 903;

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
    chain.seat_awaiting::<Fleet>(PROGRAM);
    chain.seat::<Watcher>(WATCHER);
    chain.seat::<chat::Chat>(chat::PROGRAM);
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
    for asker in [ADA, BO] {
        let anyone = ownership::Op::Create {
            name: "any machine".into(),
            write: ownership::Writers::Anyone,
        };
        let made = chain
            .submit(Origin::Signed(asker.to_vec()), ownership::PROGRAM, &anyone)
            .unwrap();
        let default = ownership::Op::SetDefault {
            scope: ownership::Scope::Kind("fleet.claim".into()),
            policy: Some(abi::decode(&made).unwrap()),
        };
        chain
            .submit(Origin::Signed(asker.to_vec()), ownership::PROGRAM, &default)
            .unwrap();
    }
    chain.register(PROGRAM, FLEET_ACCOUNT);
    chain.register(WATCHER, 904);
    World {
        chain,
        height: Cell::new(1),
    }
}

fn egressed() -> World {
    let mut world = world();
    let chain = &mut world.chain;
    chain.seat::<valset::Valset>("valset");
    chain.seat::<awaits::Awaits>(awaits::PROGRAM);
    chain.seat::<access::Access>("access");
    chain.seat_awaiting::<egress::Egress>(egress::PROGRAM);
    let node = valset::Member {
        key: NODE.to_vec(),
        address: None,
        halo: "node.example.org:9100".into(),
    };
    let genesis = valset::Genesis {
        validators: vec![node],
        member_cap: 8,
        queries: abi::role::validators::QueryPolicy::Signed,
    };
    chain.init("valset", &genesis).unwrap();
    let owner = Origin::Signed(ADA.to_vec());
    let register = Egress::Register {
        service: PODS.into(),
    };
    let assign = Egress::Assign {
        service: PODS.into(),
        nodes: vec![NODE.to_vec()],
    };
    chain
        .submit(owner.clone(), egress::PROGRAM, &register)
        .unwrap();
    chain.submit(owner, egress::PROGRAM, &assign).unwrap();
    let serve = Egress::Serve {
        services: vec![PODS.into()],
    };
    world.node(&serve);
    world
}

fn pods_get(service: &str, target: &str) -> Request {
    Request::Http {
        service: service.into(),
        head: http::Head {
            method: http::Method::Get,
            target: target.into(),
            headers: Vec::new(),
        },
        body: Vec::new(),
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

    fn act_as_program(&self, program: &str, op: &Op) -> Result<Vec<u8>, Error> {
        self.chain
            .submit(Origin::Program(program.into()), PROGRAM, op)
    }

    fn notice(&self, message_id: &str) -> Option<chat::Notice> {
        let asked = chat::Query::MessageById {
            message_id: message_id.into(),
        };
        let chat::Reply::Message(row) = self.chain.query(chat::PROGRAM, &asked).unwrap() else {
            panic!("MessageById answers a message");
        };
        row.and_then(|row| row.notice().cloned())
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

    fn call(
        &self,
        key: &[u8],
        run: u64,
        attempt: u32,
        call: u64,
        stream: bool,
    ) -> Result<Vec<u8>, Error> {
        self.call_to(key, run, attempt, call, stream, PODS)
    }

    fn call_to(
        &self,
        key: &[u8],
        run: u64,
        attempt: u32,
        call: u64,
        stream: bool,
        service: &str,
    ) -> Result<Vec<u8>, Error> {
        let call = Op::Call {
            run,
            attempt,
            call,
            request: pods_get(service, &format!("/v1/pods/p-{call}")),
            stream,
            wait: None,
        };
        self.act(key, &call)
    }

    fn node(&self, op: &Egress) {
        let node = Origin::Signed(NODE.to_vec());
        self.chain.submit(node, egress::PROGRAM, op).unwrap();
    }

    fn open(&self) -> Vec<RequestRow> {
        let asked = egress::Query::Assigned { key: NODE.to_vec() };
        let egress::Reply::Requests(rows) = self.chain.query(egress::PROGRAM, &asked).unwrap()
        else {
            panic!("Assigned answers requests");
        };
        rows
    }

    fn sender(&self, id: u64) -> Option<u64> {
        let asked = egress::Query::Sender { id };
        let egress::Reply::Sender(sender) = self.chain.query(egress::PROGRAM, &asked).unwrap()
        else {
            panic!("Sender answers an account");
        };
        sender
    }

    fn pieces(&self, run: u64, attempt: u32, call: u64) -> Vec<PieceRow> {
        let asked = Query::Pieces {
            run,
            attempt,
            call,
            page: PageRequest::default(),
        };
        let Reply::Pieces(page) = self.chain.query(PROGRAM, &asked).unwrap() else {
            panic!("Pieces answers pieces");
        };
        page.items
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
fn only_the_machine_running_an_attempt_calls_through_fleet_and_every_piece_lands_under_the_run() {
    let world = egressed();
    world.to(2);
    world.ready(A, "a", &[], 1);
    world.to(3);
    world.ready(B, "b", &[], 1);
    let run = world.ask(job());
    let unclaimed = world.call(A, run, 1, 1, false).unwrap_err();
    assert_eq!(unclaimed.code, code::WRONG_STATE);
    world.claim(A, run, 1).unwrap();
    let stranger = world.call(B, run, 1, 1, false).unwrap_err();
    assert_eq!(stranger.code, code::UNAUTHORIZED);
    let stale = world.call(A, run, 2, 1, false).unwrap_err();
    assert_eq!(stale.code, code::WRONG_STATE);
    let other = world.ask_as(BO, job());
    assert_eq!(world.placed(other).1, Some(machine(B_ACCOUNT, "b")));
    world.claim(B, other, 1).unwrap();
    let not_its_run = world.call(B, run, 1, 1, false).unwrap_err();
    assert_eq!(not_its_run.code, code::UNAUTHORIZED);
    let relay = Op::Call {
        run,
        attempt: 1,
        call: 1,
        request: Request::Relay { note: Vec::new() },
        stream: false,
        wait: None,
    };
    assert_eq!(world.act(A, &relay).unwrap_err().code, code::INVALID_INPUT);
    assert!(world.open().is_empty());
    let mut forged = pods_get(PODS, "/v1/pods/p-1");
    if let Request::Http { head, .. } = &mut forged {
        head.headers = vec![
            ("accept".into(), "application/json".into()),
            ("Fleet-Asker".into(), BO_ACCOUNT.to_string()),
            ("fleet-run".into(), other.to_string()),
        ];
    }
    let forging = Op::Call {
        run,
        attempt: 1,
        call: 1,
        request: forged,
        stream: false,
        wait: None,
    };
    world.act(A, &forging).unwrap();
    world.call(A, run, 1, 2, true).unwrap();
    let twice = world.call(A, run, 1, 1, false).unwrap_err();
    assert_eq!(twice.code, code::WRONG_STATE);
    let [whole, streamed] = world.open().try_into().expect("two calls sent");
    let fleet: ProgramId = PROGRAM.into();
    assert_eq!(whole.requester, fleet);
    let Request::Http { head, .. } = &whole.request else {
        panic!("fleet relays Http");
    };
    let stamped = [
        ("accept", "application/json".to_string()),
        (STAMPED[0], run.to_string()),
        (STAMPED[1], "1".to_string()),
        (STAMPED[2], ADA_ACCOUNT.to_string()),
        (STAMPED[3], A_ACCOUNT.to_string()),
    ]
    .map(|(name, value)| (name.to_string(), value));
    assert_eq!(head.headers, stamped);
    assert_eq!(world.sender(whole.id), Some(FLEET_ACCOUNT));
    assert!(world.pieces(run, 1, 1).is_empty());
    world.to(world.height.get() + 1);
    let created = http::Response {
        status: 200,
        headers: Vec::new(),
        body: br#"{"pod":{"name":"p-1"}}"#.to_vec(),
    };
    let answer = Egress::Answer {
        id: whole.id,
        answer: Answer::Output(abi::encode(&created)),
    };
    world.node(&answer);
    let event = |seq, data: &[u8]| Event {
        id: streamed.id,
        seq,
        data: data.to_vec(),
    };
    world.node(&Egress::Events(vec![event(0, b"one"), event(1, b"two")]));
    let end = Egress::EndStream {
        id: streamed.id,
        events: 2,
        end: StreamEnd::Complete {
            status: 200,
            headers: Vec::new(),
        },
    };
    world.node(&end);
    let row = |n, piece| PieceRow { n, piece };
    assert_eq!(
        world.pieces(run, 1, 1),
        [row(0, Piece::Answer(abi::encode(&created)))]
    );
    let ended = http::Response {
        status: 200,
        headers: Vec::new(),
        body: Vec::new(),
    };
    let streamed_piece = |seq, data: &[u8]| Piece::Event {
        seq,
        data: data.to_vec(),
    };
    assert_eq!(
        world.pieces(run, 1, 2),
        [
            row(0, streamed_piece(0, b"one")),
            row(1, streamed_piece(1, b"two")),
            row(2, Piece::Answer(abi::encode(&ended))),
        ]
    );
    let forged = Op::Heard {
        run,
        attempt: 1,
        call: 2,
        event: event(2, b"forged"),
    };
    assert_eq!(world.act(A, &forged).unwrap_err().code, code::UNAUTHORIZED);
    assert_eq!(world.pieces(run, 1, 2).len(), 3);
    world.call_to(A, run, 1, 3, false, "nowhere").unwrap();
    let [refused] = world.pieces(run, 1, 3).try_into().expect("one piece");
    assert!(matches!(refused.piece, Piece::Failed(_)), "{refused:?}");
    world.call(A, run, 1, 4, false).unwrap();
    world.to(world.height.get() + DEFAULT_WAIT + 1);
    let [expired] = world.pieces(run, 1, 4).try_into().expect("one piece");
    assert!(matches!(expired.piece, Piece::Failed(_)), "{expired:?}");
    let stale = world.report(A, run, 1, State::Passed).unwrap_err();
    assert_eq!(stale.code, code::WRONG_STATE);
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
fn with_no_claim_default_only_the_askers_own_machines_take_their_runs() {
    let world = world();
    world.to(2);
    world.ready(B, "b", &[], 1);
    world.to(3);
    world.ready(A, "a", &[], 1);
    let own = world.ask_as(A, job());
    assert_eq!(world.placed(own).1, Some(machine(A_ACCOUNT, "a")));
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

#[test]
fn a_run_being_stopped_whose_machine_goes_silent_ends_cancelled_and_goes_nowhere_else() {
    let world = world();
    world.to(2);
    world.ready(A, "a", &[], 2);
    let service = world.ask(Spec {
        kind: Kind::Service,
        ..job()
    });
    let retry = world.ask(job());
    // both claimed at block 6, which books the lapse check for 6 + the wait
    world.claim(A, service, 1).unwrap();
    world.claim(A, retry, 1).unwrap();
    world.act(ADA, &Op::Cancel { run: service }).unwrap();
    world.act(ADA, &Op::Cancel { run: retry }).unwrap();
    world.ready(B, "b", &[], 2);
    world.to(6 + DEFAULT_WAIT + 1);
    for run in [service, retry] {
        let ended = world.run(run);
        assert_eq!(
            (ended.state, ended.attempt, ended.finished),
            (State::Cancelled, 1, Some(6 + DEFAULT_WAIT)),
            "{ended:?}"
        );
    }
    assert!(world.holds(machine(B_ACCOUNT, "b")).is_empty());
    assert!(world.queued().is_empty());
    let told = |run| Ended {
        run,
        attempt: 1,
        state: State::Cancelled,
    };
    assert_eq!(world.heard(), [told(service), told(retry)]);
}

#[test]
fn a_machine_a_listed_program_runs_takes_the_askers_runs() {
    let world = world();
    world.chain.register(WORKER, WORKER_ACCOUNT);
    let policy = ownership::Op::Create {
        name: "my workers".into(),
        write: ownership::Writers::Listed(vec![ownership::Party::Program(WORKER.into())]),
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
    let ready = Op::Ready {
        name: "w".into(),
        labels: Vec::new(),
        slots: 1,
        serves: Serves::Anyone,
    };
    world.act_as_program(WORKER, &ready).unwrap();
    let run = world.ask(job());
    let worker = machine(WORKER_ACCOUNT, "w");
    assert_eq!(world.placed(run), (State::Assigned, Some(worker), 1));
    world
        .act_as_program(WORKER, &Op::Claim { run, attempt: 1 })
        .unwrap();
    assert_eq!(world.run(run).state, State::Running);
}

#[test]
fn a_parked_jobs_late_end_is_told_in_chat_after_its_loss() {
    let world = world();
    let channel = chat::Op::CreateChannel {
        channel_id: "builds".into(),
        name: "builds".into(),
        post_policy: chat::PostPolicy::Open,
    };
    world
        .chain
        .submit(Origin::Signed(ADA.to_vec()), chat::PROGRAM, &channel)
        .unwrap();
    world.to(2);
    world.ready(A, "a", &[], 1);
    let parked = world.ask(Spec {
        on_loss: OnLoss::Park,
        notify: Some(Notify::Chat {
            channel: "builds".into(),
        }),
        ..job()
    });
    // claimed at block 4, which books the lapse check for 4 + the wait
    world.claim(A, parked, 1).unwrap();
    world.to(4 + DEFAULT_WAIT + 1);
    assert_eq!(world.run(parked).state, State::Lost);
    world.act(A, &Op::Beat { name: "a".into() }).unwrap();
    world.report(A, parked, 1, State::Passed).unwrap();
    use chat::message::Status;
    let lost = world.notice(&format!("fleet-run-{parked}-1-1")).unwrap();
    let passed = world.notice(&format!("fleet-run-{parked}-1-2")).unwrap();
    for notice in [&lost, &passed] {
        assert_eq!(notice.kind, "run");
        assert_eq!(notice.group, format!("run {parked}"));
        assert_eq!(notice.subject, format!("run {parked}"));
        assert_eq!(notice.concerns, [ADA_ACCOUNT], "the asker alone");
    }
    assert_eq!(
        (lost.status, lost.detail.as_str()),
        (
            Status::Failed,
            "lost (attempt 1): a went silent after block 4"
        )
    );
    assert_eq!(passed.status, Status::Passed);
    assert!(
        passed.detail.starts_with("passed (attempt 1)"),
        "{passed:?}"
    );
    let ended = world.run(parked);
    assert_eq!(ended.state, State::Passed);
    assert!(!ended.summary.contains("not told"), "{}", ended.summary);
}

#[test]
fn a_channel_that_refuses_an_end_is_said_on_the_run() {
    let world = world();
    world.to(2);
    world.ready(A, "a", &[], 1);
    let run = world.ask(Spec {
        notify: Some(Notify::Chat {
            channel: "nowhere".into(),
        }),
        ..job()
    });
    world.claim(A, run, 1).unwrap();
    world.report(A, run, 1, State::Failed).unwrap();
    let ended = world.run(run);
    assert_eq!(ended.state, State::Failed);
    assert!(
        ended.summary.starts_with("failed at 1; not told: "),
        "{}",
        ended.summary
    );
}
