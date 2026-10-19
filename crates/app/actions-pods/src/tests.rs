// actions-pods on a MockChain with egress, the scheduler and awaits roles and a stand-in for
// actions, the test playing egress's node: an offer claimed and run the block after, the pod made
// and waited on as a stream, `/out` copied, the check reported and the pod deleted.
use ::program::{
    Error, ExecCtx, MockChain, MockHost, Origin, Program, QueryCtx, Scheme, code, invalid,
    wrong_state,
};
use abi::role::egress::{
    Answer, DEFAULT_WAIT, Event, Op as Egress, Request, RequestRow, StreamEnd, http,
};
use actions::{CheckRow, Input, Job, Offer, State};
use serde_json::{Value, json};
use store::Item;
use valset::{Genesis, Member, Valset};

use crate::*;

const ADA: [u8; 32] = [1; 32];
const BO: [u8; 32] = [2; 32];
const OWNER: &[u8] = b"owner-key";
const COMMIT: &str = "1111111111111111111111111111111111111111";

type Report = (u64, State, String, Option<String>);

const CLAIMED: Item<Vec<u64>> = Item::new("claimed");
const REPORTS: Item<Vec<Report>> = Item::new("reports");

// actions as a runner meets it: a check to read, a claim taken once, reports kept.
struct Stand;

impl Program for Stand {
    const NAME: &'static str = actions::PROGRAM;
    type Op = actions::Op;
    type Query = actions::Query;
    type Reply = actions::Reply;

    fn execute(ctx: &ExecCtx, op: actions::Op) -> Result<(), Error> {
        match op {
            actions::Op::Register { .. } => Ok(()),
            actions::Op::Claim { check } => {
                let mut claimed = CLAIMED.get(ctx)?.unwrap_or_default();
                if claimed.contains(&check) {
                    return Err(wrong_state(format!("check {check} is running")));
                }
                claimed.push(check);
                CLAIMED.put(ctx, &claimed);
                Ok(())
            }
            actions::Op::Report {
                check,
                state,
                summary,
                log,
            } => {
                let mut reports = REPORTS.get(ctx)?.unwrap_or_default();
                reports.push((check, state, summary, log));
                REPORTS.put(ctx, &reports);
                Ok(())
            }
            _ => Err(invalid("the stand-in takes registers, claims and reports")),
        }
    }

    fn query(_ctx: &QueryCtx, query: actions::Query) -> Result<actions::Reply, Error> {
        match query {
            actions::Query::Check { id } => Ok(actions::Reply::Check(Some(check_row(id)))),
            _ => Err(invalid("the stand-in answers checks")),
        }
    }
}

fn check_row(id: u64) -> CheckRow {
    CheckRow {
        id,
        run: 1,
        repo: "programs".into(),
        commit: COMMIT.into(),
        workflow: "gates.toml".into(),
        attempt: 1,
        job: Job {
            id: "gates".into(),
            runs_on: vec!["pods".into()],
            image: "toolchain".into(),
            steps: vec!["sh /tools/gates.sh /work/programs".into()],
            env: vec![("FEATURE_SETS".into(), "forge:program".into())],
            inputs: vec![Input {
                path: "/tools/gates.sh".into(),
                files: "/shared/tools/gates.sh".into(),
            }],
        },
        state: State::Running,
        claimant: None,
        reporter: None,
        queued: 1,
        started: Some(1),
        finished: None,
        summary: String::new(),
        log: None,
    }
}

// The chain, and the block the node answers in next.
struct Node {
    chain: MockChain,
    height: u64,
    time: u64,
}

fn node() -> Node {
    let mut chain = MockChain::founded(MockHost::roles());
    chain.seat::<identity::Identity>(identity::PROGRAM);
    chain.seat::<Valset>("valset");
    chain.seat::<awaits::Awaits>(awaits::PROGRAM);
    chain.seat::<::scheduler::Scheduler>(::scheduler::PROGRAM);
    chain.seat::<access::Access>("access");
    chain.seat_awaiting::<egress::Egress>(egress::PROGRAM);
    chain.seat::<Stand>(actions::PROGRAM);
    chain.seat_awaiting::<ActionsPods>(PROGRAM);
    let members = [ADA, BO].map(|key| Member {
        key: key.to_vec(),
        address: None,
        halo: "node.example.org:9100".into(),
    });
    let genesis = Genesis {
        validators: members.to_vec(),
        member_cap: 8,
    };
    chain.init("valset", &genesis).unwrap();
    let create = identity::Op::Create {
        name: "Owner".into(),
        scheme: Scheme::Ed25519,
    };
    let owner = Origin::Signed(OWNER.to_vec());
    chain
        .submit(owner.clone(), identity::PROGRAM, &create)
        .unwrap();
    let register = Egress::Register {
        service: SERVICE.into(),
    };
    let assign = Egress::Assign {
        service: SERVICE.into(),
        nodes: vec![ADA.to_vec()],
    };
    let serve = Egress::Serve {
        services: vec![SERVICE.into()],
    };
    chain
        .submit(owner.clone(), egress::PROGRAM, &register)
        .unwrap();
    chain.submit(owner, egress::PROGRAM, &assign).unwrap();
    chain
        .submit(Origin::Signed(ADA.to_vec()), egress::PROGRAM, &serve)
        .unwrap();
    let config = Config {
        labels: vec!["pods".into()],
    };
    chain.init(PROGRAM, &config).unwrap();
    chain.at(2, 1_000);
    Node {
        chain,
        height: 2,
        time: 1_000,
    }
}

impl Node {
    // The next block, `ms` after this one.
    fn after(&mut self, ms: u64) {
        self.height += 1;
        self.time += ms;
        self.chain.at(self.height, self.time);
    }

    fn offer(&self, check: u64) -> Result<Vec<u8>, Error> {
        let from_actions = Origin::Program(actions::PROGRAM.into());
        self.chain
            .submit(from_actions, PROGRAM, &Op::Offer(Offer { check }))
    }

    // The one request egress assigned the node and it has not answered.
    fn pending(&self) -> RequestRow {
        let asked = egress::Query::Assigned { key: ADA.to_vec() };
        let egress::Reply::Requests(rows) = self.chain.query(egress::PROGRAM, &asked).unwrap()
        else {
            panic!("assigned answers Requests");
        };
        let [row] = rows.try_into().expect("one request in flight");
        row
    }

    fn idle(&self) -> bool {
        let asked = egress::Query::Assigned { key: ADA.to_vec() };
        let egress::Reply::Requests(rows) = self.chain.query(egress::PROGRAM, &asked).unwrap()
        else {
            panic!("assigned answers Requests");
        };
        rows.is_empty()
    }

    fn node(&self, op: &Egress) {
        let ada = Origin::Signed(ADA.to_vec());
        self.chain.submit(ada, egress::PROGRAM, op).unwrap();
    }

    // The pending request answered whole in the next block: its method and target.
    fn answer(&mut self, ms: u64, status: u16, body: Value) -> (String, String) {
        self.after(ms);
        let row = self.pending();
        let response = http::Response {
            status,
            headers: vec![("content-type".into(), "application/json".into())],
            body: serde_json::to_vec(&body).unwrap(),
        };
        let answer = Egress::Answer {
            id: row.id,
            answer: Answer::Output(abi::encode(&response)),
        };
        self.node(&answer);
        aimed(&row)
    }

    fn body(&self, row: &RequestRow) -> Value {
        let Request::Http { body, .. } = &row.request else {
            panic!("an http request");
        };
        let blob = self.chain.host(PROGRAM).borrow().blobs[&body[0]].clone();
        serde_json::from_slice(&blob.body).unwrap()
    }

    fn reports(&self) -> Vec<Report> {
        let stand = self.chain.host(actions::PROGRAM);
        let read = stand.query(MockHost::env(actions::PROGRAM));
        REPORTS.get(&read).unwrap().unwrap_or_default()
    }

    fn run(&self, check: u64) -> Option<RunRow> {
        let Reply::Run(row) = self.chain.query(PROGRAM, &Query::Run { check }).unwrap();
        row
    }
}

fn aimed(row: &RequestRow) -> (String, String) {
    let Request::Http { head, .. } = &row.request else {
        panic!("an http request");
    };
    (head.method.as_str().to_owned(), head.target.clone())
}

fn sent(method: &str, target: &str) -> (String, String) {
    (method.to_owned(), target.to_owned())
}

// An offer at 1000 ms, its run the block after at 3000 ms: a block of 2000 ms. The pod's creation
// lands 500 ms later, so a wait request lasts 64 × 2000 ms and its alive events come every
// (128000 / 2 − 500) / 1000 = 63 s.
fn ran_to_its_end(exit_code: Value) -> Node {
    let mut node = node();
    node.offer(1).unwrap();
    node.after(2_000);
    node.after(0);
    let made = node.pending();
    assert_eq!(aimed(&made), sent("POST", "/v1/pods"));
    let body = node.body(&made);
    assert_eq!(body["image"], "toolchain");
    let forged = json!({"path": "/work/programs", "forge": {"repo": "programs", "commit": COMMIT}});
    assert_eq!(body["inputs"][0], forged);
    let gates = json!({"path": "/tools/gates.sh", "files": {"path": "/shared/tools/gates.sh"}});
    assert_eq!(body["inputs"][1], gates);
    assert_eq!(body["env"], json!({"FEATURE_SETS": "forge:program"}));
    let script = body["command"][2].as_str().unwrap();
    assert!(
        script.contains("cd '/work/programs'\n")
            && script.ends_with("sh /tools/gates.sh /work/programs\n")
    );
    node.answer(500, 201, json!({"pod": {"name": "duckpond-7"}}));
    let wait = node.pending();
    let every = sent("GET", "/v1/pods/duckpond-7/wait?every=63");
    assert_eq!(aimed(&wait), every);
    node.after(1_000);
    let alive = Event {
        id: wait.id,
        seq: 0,
        data: b"event: alive\ndata: {}".to_vec(),
    };
    node.node(&Egress::Events(vec![alive]));
    node.after(1_000);
    let line = "pod duckpond-7 ended: exit code 0 (Completed)";
    let ended = json!({"line": line, "exit_code": exit_code, "pod": {"name": "duckpond-7"}});
    let ended = Event {
        id: wait.id,
        seq: 1,
        data: format!("event: ended\ndata: {ended}").into_bytes(),
    };
    node.node(&Egress::Events(vec![ended]));
    let end = StreamEnd::Complete {
        status: 200,
        headers: vec![("content-type".into(), "text/event-stream".into())],
    };
    let ended = Egress::EndStream {
        id: wait.id,
        events: 2,
        end,
    };
    node.node(&ended);
    let listed = json!({"exit_code": 0, "stdout": "/out/log\n/out/summary.txt\n", "stderr": ""});
    let find = node.answer(1_000, 200, listed);
    assert_eq!(find, sent("POST", "/v1/pods/duckpond-7/exec"));
    for _ in 0..2 {
        let copied = node.answer(1_000, 200, json!({"written": "/shared/…"}));
        assert_eq!(copied, sent("POST", "/v1/pods/duckpond-7/file"));
    }
    let deleted = node.answer(1_000, 200, json!({"deleted": "duckpond-7"}));
    assert_eq!(deleted, sent("DELETE", "/v1/pods/duckpond-7"));
    assert!(node.idle());
    node
}

#[test]
fn an_offered_check_runs_in_a_pod_and_passes_when_its_steps_do() {
    let node = ran_to_its_end(json!(0));
    let log = format!("/shared/actions/programs/{COMMIT}/gates/gates-1/log");
    let line = "pod duckpond-7 ended: exit code 0 (Completed)";
    assert_eq!(
        node.reports(),
        [(1, State::Passed, line.to_owned(), Some(log))]
    );
    let row = RunRow {
        check: 1,
        pod: Some("duckpond-7".into()),
        block_ms: 2_000,
        trip_ms: 500,
        every: 63,
    };
    assert_eq!(node.run(1), Some(row));
}

#[test]
fn a_step_that_fails_fails_the_check_and_a_pod_that_broke_errs_it() {
    let failed = ran_to_its_end(json!(2));
    assert_eq!(failed.reports()[0].1, State::Failed);
    let broke = ran_to_its_end(Value::Null);
    assert_eq!(broke.reports()[0].1, State::Errored);
}

#[test]
fn a_pod_the_service_refuses_errs_the_check_and_leaves_nothing_to_delete() {
    let mut node = node();
    node.offer(1).unwrap();
    node.after(2_000);
    node.after(0);
    let refused = json!({"error": "pods \"duckpond-7\" is forbidden"});
    node.answer(500, 422, refused);
    assert!(node.idle());
    let why = "/v1/pods answered 422: pods \"duckpond-7\" is forbidden".to_owned();
    assert_eq!(node.reports(), [(1, State::Errored, why, None)]);
}

// A block of 10 ms: a request lasts 640 ms, and the pod's creation took 490 ms to land.
#[test]
fn a_wait_with_no_whole_second_to_beat_errs_the_check_and_deletes_the_pod() {
    let mut node = node();
    node.offer(1).unwrap();
    node.after(10);
    node.after(0);
    node.answer(490, 201, json!({"pod": {"name": "duckpond-7"}}));
    let deleted = node.answer(1_000, 200, json!({"deleted": "duckpond-7"}));
    assert_eq!(deleted, sent("DELETE", "/v1/pods/duckpond-7"));
    let reports: [Report; 1] = node.reports().try_into().unwrap();
    let [(1, State::Errored, why, None)] = reports else {
        panic!("one errored report");
    };
    assert!(why.contains("no whole second"), "{why}");
}

#[test]
fn a_check_another_runner_claimed_is_left_alone_and_only_its_senders_are_heard() {
    let mut node = node();
    node.offer(1).unwrap();
    node.offer(1).unwrap();
    node.after(2_000);
    node.after(0);
    let made = node.pending();
    assert_eq!(aimed(&made), sent("POST", "/v1/pods"), "one run, not two");
    let signed = Origin::Signed(OWNER.to_vec());
    let offered = node
        .chain
        .submit(signed.clone(), PROGRAM, &Op::Offer(Offer { check: 2 }))
        .unwrap_err();
    assert_eq!(offered.code, code::UNAUTHORIZED);
    let unscheduled = node
        .chain
        .submit(
            Origin::Program(PROGRAM.into()),
            PROGRAM,
            &Op::Run { check: 1 },
        )
        .unwrap_err();
    assert_eq!(unscheduled.code, code::UNAUTHORIZED);
    let event = Event {
        id: made.id,
        seq: 0,
        data: b"event: ended\ndata: {}".to_vec(),
    };
    let heard = node
        .chain
        .submit(signed, PROGRAM, &Op::Heard(event))
        .unwrap_err();
    assert_eq!(heard.code, code::UNAUTHORIZED);
}

// The wait opens after the creation's answer; 65 blocks of 1 s pass with no event, so egress
// expires it, and the pod is read.
fn outlived(node: &mut Node) {
    node.offer(1).unwrap();
    node.after(2_000);
    node.after(0);
    node.answer(500, 201, json!({"pod": {"name": "duckpond-7"}}));
    let first = node.pending();
    assert_eq!(
        aimed(&first),
        sent("GET", "/v1/pods/duckpond-7/wait?every=63")
    );
    for _ in 0..=DEFAULT_WAIT {
        node.after(1_000);
    }
}

fn run_status(state: &str, exit_code: Value) -> Value {
    let status = json!({"state": state, "exit_code": exit_code, "reason": "Error"});
    let run = json!({"name": "run", "status": status});
    json!({"pod": {"name": "duckpond-7", "phase": "Running", "containers": [run]}})
}

// What follows a pod's end: /out listed (its log alone), copied, and the pod deleted.
fn copied_and_deleted(node: &mut Node) {
    let listed = json!({"exit_code": 0, "stdout": "/out/log\n", "stderr": ""});
    let find = node.answer(1_000, 200, listed);
    assert_eq!(find, sent("POST", "/v1/pods/duckpond-7/exec"));
    let copied = node.answer(1_000, 200, json!({"written": "/shared/…"}));
    assert_eq!(copied, sent("POST", "/v1/pods/duckpond-7/file"));
    let deleted = node.answer(1_000, 200, json!({"deleted": "duckpond-7"}));
    assert_eq!(deleted, sent("DELETE", "/v1/pods/duckpond-7"));
    assert!(node.idle());
}

#[test]
fn a_pod_that_outlives_its_wait_is_read_and_waited_on_again_at_the_beat_its_blocks_measured() {
    let mut node = node();
    outlived(&mut node);
    let read = node.answer(1_000, 200, run_status("running", Value::Null));
    assert_eq!(read, sent("GET", "/v1/pods/duckpond-7"));
    let again = node.pending();
    // 66 blocks over 66 s since the first wait opened: blocks of 1000 ms, (32000 − 500) / 1000
    assert_eq!(
        aimed(&again),
        sent("GET", "/v1/pods/duckpond-7/wait?every=31")
    );
    node.after(1_000);
    let line = "pod duckpond-7 ended: exit code 0 (Completed)";
    let ended = json!({"line": line, "exit_code": 0, "pod": {"name": "duckpond-7"}});
    let ended = Event {
        id: again.id,
        seq: 0,
        data: format!("event: ended\ndata: {ended}").into_bytes(),
    };
    node.node(&Egress::Events(vec![ended]));
    let end = StreamEnd::Complete {
        status: 200,
        headers: vec![("content-type".into(), "text/event-stream".into())],
    };
    let finished = Egress::EndStream {
        id: again.id,
        events: 1,
        end,
    };
    node.node(&finished);
    copied_and_deleted(&mut node);
    let log = format!("/shared/actions/programs/{COMMIT}/gates/gates-1/log");
    assert_eq!(
        node.reports(),
        [(1, State::Passed, line.to_owned(), Some(log))]
    );
    let row = node.run(1).unwrap();
    assert_eq!((row.block_ms, row.every), (1_000, 31));
}

#[test]
fn a_pod_that_ended_while_its_wait_was_broken_reports_how_it_ended() {
    let mut node = node();
    outlived(&mut node);
    let terminated = run_status("terminated", json!(2));
    let read = node.answer(1_000, 200, terminated);
    assert_eq!(read, sent("GET", "/v1/pods/duckpond-7"));
    copied_and_deleted(&mut node);
    let line = "pod duckpond-7 ended: exit code 2 (Error)".to_owned();
    let log = format!("/shared/actions/programs/{COMMIT}/gates/gates-1/log");
    assert_eq!(node.reports(), [(1, State::Failed, line, Some(log))]);
}
