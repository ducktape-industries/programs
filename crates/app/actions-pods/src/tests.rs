// actions-pods on a MockChain with egress, the scheduler and awaits roles and a stand-in for
// actions, the test playing egress's node: an offer run the block after, the pod made, the check
// claimed, the pod waited on as a stream, `/out` copied, the check reported and the pod deleted.
use ::program::section::{CUSTOM, MODULE};
use ::program::{
    BlobId, Error, ExecCtx, MockChain, MockHost, Origin, Program, QueryCtx, Scheme, code, invalid,
    wrong_state,
};
use abi::role::egress::{
    Answer, DEFAULT_WAIT, Event, Op as Egress, Request, RequestRow, StreamEnd, http,
};
use actions::{Asks, CheckRow, Input, Job, Need, Offer, Output, OutputsRow, PageResponse, State};
use programs::{Entry, Genesis, Programs};
use serde_json::{Value, json};
use store::Item;
use valset::{Genesis as Founding, Member, Valset};

use crate::rules::{bytes, millicores};
use crate::*;

const ADA: [u8; 32] = [1; 32];
const BO: [u8; 32] = [2; 32];
const OWNER: &[u8] = b"owner-key";
const STRANGER: &[u8] = b"stranger-key";
const COMMIT: &str = "1111111111111111111111111111111111111111";

type Report = (u64, State, String, Option<String>);

const REFUSED: u64 = 9;
const AGAIN: u64 = 3;
const SIXTEEN: u64 = 4;
// A check that needs job `build`, met by check BUILT; and one whose need, met by STRAYED, names an
// output that climbs out of its folder.
const NEEDY: u64 = 5;
const ASTRAY: u64 = 6;
const BUILT: u64 = 11;
const LOTS: u64 = 13;
const STRAYED: u64 = 12;
const QUEUE_HEAD: u64 = 14;
const QUEUE_PREVIEW: &str = "2222222222222222222222222222222222222222";
const QUEUED_CHANGE: u64 = 7;

const CLAIMED: Item<Vec<u64>> = Item::new("claimed");
const REPORTS: Item<Vec<Report>> = Item::new("reports");
const RERUN: Item<bool> = Item::new("rerun");
const OUTS: Item<Vec<(u64, Vec<Output>)>> = Item::new("outs");

struct Stand;

impl Program for Stand {
    const NAME: &'static str = actions::PROGRAM;
    type Op = actions::Op;
    type Query = actions::Query;
    type Reply = actions::Reply;

    fn execute(ctx: &ExecCtx, op: actions::Op) -> Result<(), Error> {
        match op {
            actions::Op::Register { .. } => Ok(()),
            actions::Op::Claim { check } if check == REFUSED => Err(invalid(
                "the stand-in refuses this claim for its own reasons",
            )),
            actions::Op::Claim { check } => {
                if state_of(ctx, check)? != State::Queued {
                    return Err(wrong_state(format!("check {check} is not queued")));
                }
                let mut claimed = CLAIMED.get(ctx)?.unwrap_or_default();
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
                let now = state_of(ctx, check)?;
                let moot = matches!(state, State::Errored | State::Superseded);
                let unclaimed = now == State::Queued && moot;
                if now != State::Running && !unclaimed {
                    return Err(wrong_state(format!("check {check} is not running")));
                }
                let mut reports = REPORTS.get(ctx)?.unwrap_or_default();
                reports.push((check, state, summary, log));
                REPORTS.put(ctx, &reports);
                Ok(())
            }
            actions::Op::Rerun { .. } => {
                RERUN.put(ctx, &true);
                Ok(())
            }
            actions::Op::Outputs { check, files } => {
                let mut kept = OUTS.get(ctx)?.unwrap_or_default();
                kept.push((check, files));
                OUTS.put(ctx, &kept);
                Ok(())
            }
            _ => Err(invalid("the stand-in takes registers, claims and reports")),
        }
    }

    fn query(ctx: &QueryCtx, query: actions::Query) -> Result<actions::Reply, Error> {
        match query {
            actions::Query::Check { id } => Ok(actions::Reply::Check(Some(row_now(ctx, id)?))),
            actions::Query::Checks { .. } => {
                let mut ids = vec![1];
                if RERUN.get(ctx)?.unwrap_or(false) {
                    ids.push(AGAIN);
                }
                let items = ids
                    .into_iter()
                    .map(|id| row_now(ctx, id))
                    .collect::<Result<Vec<_>, Error>>()?;
                let height = ctx.env().height;
                let page = PageResponse {
                    height,
                    items,
                    next: None,
                };
                Ok(actions::Reply::Checks(page))
            }
            actions::Query::Asks { check } => Ok(actions::Reply::Asks(asks_of(check))),
            actions::Query::Needs { check } => Ok(actions::Reply::Needs(needs_of(check))),
            actions::Query::Outputs { check } => Ok(actions::Reply::Outputs(outputs_of(check))),
            _ => Err(invalid("the stand-in answers checks")),
        }
    }
}

struct Forged;

impl Program for Forged {
    const NAME: &'static str = forge::PROGRAM;
    type Op = forge::Op;
    type Query = forge::Query;
    type Reply = forge::Reply;

    fn execute(_: &ExecCtx, _: forge::Op) -> Result<(), Error> {
        Err(invalid("the stand-in for forge takes no op"))
    }

    fn query(ctx: &QueryCtx, query: forge::Query) -> Result<forge::Reply, Error> {
        let height = ctx.env().height;
        match query {
            forge::Query::Required { .. } => {
                let required = forge::Required {
                    reporter: forge::Principal::Account(37),
                    contexts: vec!["check.toml#check".into()],
                };
                let items = vec![forge::RequiredBranch {
                    branch: b"refs/heads/dev".to_vec(),
                    required,
                }];
                let page = PageResponse {
                    height,
                    items,
                    next: None,
                };
                Ok(forge::Reply::Required { height, page })
            }
            forge::Query::Queue { .. } => {
                let items = vec![forge::QueueEntry {
                    n: QUEUED_CHANGE,
                    head: COMMIT.into(),
                    by: forge::Principal::Account(17),
                    height,
                }];
                let page = PageResponse {
                    height,
                    items,
                    next: None,
                };
                Ok(forge::Reply::Queue { height, page })
            }
            forge::Query::Preview { n, .. } => {
                let preview = (n == QUEUED_CHANGE).then(|| forge::PreviewInfo {
                    n,
                    into_head: COMMIT.into(),
                    from_head: COMMIT.into(),
                    merge: QUEUE_PREVIEW.into(),
                    height,
                });
                Ok(forge::Reply::Preview { height, preview })
            }
            _ => Err(invalid("the stand-in for forge answers merge queues")),
        }
    }
}

fn needs_of(check: u64) -> Vec<Need> {
    let met = match check {
        NEEDY => BUILT,
        ASTRAY => STRAYED,
        _ => return Vec::new(),
    };
    vec![Need {
        job: "build".into(),
        check: Some(met),
    }]
}

fn outputs_of(check: u64) -> Option<OutputsRow> {
    let path = match check {
        BUILT => "dist/app.tar.gz",
        STRAYED => "../../etc/passwd",
        _ => return None,
    };
    let files = vec![Output {
        path: path.into(),
        blob: BlobId::Sha256([0xab; 32]),
        size: 7,
    }];
    Some(OutputsRow { by: 1, files })
}

fn asks_of(check: u64) -> Option<Asks> {
    match check {
        SIXTEEN => Some(sixteen()),
        LOTS => Some(Asks {
            cpu: Some("16".into()),
            memory: Some("lots".into()),
            scratch: Vec::new(),
        }),
        _ => None,
    }
}

fn sixteen() -> Asks {
    Asks {
        cpu: Some("16".into()),
        memory: Some("40Gi".into()),
        scratch: vec!["/var/tmp/target".into()],
    }
}

fn state_of(ctx: &QueryCtx, id: u64) -> Result<State, Error> {
    let reports = REPORTS.get(ctx)?.unwrap_or_default();
    if let Some((_, state, _, _)) = reports.iter().find(|report| report.0 == id) {
        return Ok(*state);
    }
    let claimed = CLAIMED.get(ctx)?.unwrap_or_default();
    Ok(match claimed.contains(&id) {
        true => State::Running,
        false => State::Queued,
    })
}

fn row_now(ctx: &QueryCtx, id: u64) -> Result<CheckRow, Error> {
    Ok(CheckRow {
        state: state_of(ctx, id)?,
        ..check_row(id)
    })
}

fn check_row(id: u64) -> CheckRow {
    CheckRow {
        id,
        run: 1,
        repo: "programs".into(),
        commit: if id == QUEUE_HEAD {
            QUEUE_PREVIEW
        } else {
            COMMIT
        }
        .into(),
        workflow: "gates.toml".into(),
        attempt: if id == AGAIN { 2 } else { 1 },
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
        state: State::Queued,
        claimant: None,
        reporter: None,
        queued: 1,
        started: None,
        finished: None,
        summary: String::new(),
        log: None,
    }
}

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
    let genesis = Founding {
        validators: members.to_vec(),
        member_cap: 8,
        queries: abi::role::validators::QueryPolicy::Signed,
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

fn code(build: &[u8]) -> Vec<u8> {
    let name = b"ducktape.build";
    let mut body = vec![name.len() as u8];
    body.extend(name);
    body.extend(build);
    let mut code = MODULE.to_vec();
    code.extend([1, 1, 0, CUSTOM, body.len() as u8]);
    code.extend(body);
    code
}

fn listed(chain: &MockChain, build: &[u8]) {
    let entry = |program: &str, code: Vec<u8>| Entry {
        program: program.into(),
        code: chain.put_blob(programs::CODE_KIND, code).unwrap(),
        params: Vec::new(),
    };
    let genesis = Genesis {
        programs: vec![entry(PROGRAM, code(build)), entry("valset", code(b""))],
        views: Vec::new(),
        hooks: Vec::new(),
        roles: ::program::Roles::default(),
    };
    chain.init(programs::PROGRAM, &genesis).unwrap();
}

impl Node {
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

    fn sweep(&self) {
        let anyone = Origin::Signed(OWNER.to_vec());
        self.chain.submit(anyone, PROGRAM, &Op::Sweep).unwrap();
    }

    fn open(&self) -> Vec<RequestRow> {
        let asked = egress::Query::Assigned { key: ADA.to_vec() };
        let egress::Reply::Requests(rows) = self.chain.query(egress::PROGRAM, &asked).unwrap()
        else {
            panic!("assigned answers Requests");
        };
        rows
    }

    fn pending(&self) -> RequestRow {
        let [row] = self.open().try_into().expect("one request in flight");
        row
    }

    fn pending_to(&self, method: &str, target: &str) -> RequestRow {
        let rows: Vec<RequestRow> = self
            .open()
            .into_iter()
            .filter(|row| aimed(row) == sent(method, target))
            .collect();
        let [row] = rows.try_into().expect("one request so aimed");
        row
    }

    fn idle(&self) -> bool {
        self.open().is_empty()
    }

    fn node(&self, op: &Egress) {
        let ada = Origin::Signed(ADA.to_vec());
        self.chain.submit(ada, egress::PROGRAM, op).unwrap();
    }

    fn reply(&self, row: &RequestRow, status: u16, body: Value) {
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
    }

    fn answer(&mut self, ms: u64, status: u16, body: Value) -> (String, String) {
        self.after(ms);
        let row = self.pending();
        self.reply(&row, status, body);
        aimed(&row)
    }

    fn answer_to(&mut self, ms: u64, method: &str, target: &str, status: u16, body: Value) {
        self.after(ms);
        let row = self.pending_to(method, target);
        self.reply(&row, status, body);
    }

    fn ended(&mut self, wait: &RequestRow, seq: u64, exit_code: Value) {
        self.after(1_000);
        let line = "pod duckpond-7 ended: exit code 0 (Completed)";
        let ended = json!({"line": line, "exit_code": exit_code, "pod": {"name": "duckpond-7"}});
        let ended = Event {
            id: wait.id,
            seq,
            data: format!("event: ended\ndata: {ended}").into_bytes(),
        };
        self.node(&Egress::Events(vec![ended]));
        let end = StreamEnd::Complete {
            status: 200,
            headers: vec![("content-type".into(), "text/event-stream".into())],
        };
        let finished = Egress::EndStream {
            id: wait.id,
            events: seq + 1,
            end,
        };
        self.node(&finished);
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

    fn outputs(&self) -> Vec<(u64, Vec<Output>)> {
        let stand = self.chain.host(actions::PROGRAM);
        let read = stand.query(MockHost::env(actions::PROGRAM));
        OUTS.get(&read).unwrap().unwrap_or_default()
    }

    fn run(&self, check: u64) -> Option<RunRow> {
        let Reply::Run(row) = self.chain.query(PROGRAM, &Query::Run { check }).unwrap() else {
            panic!("run answers Run");
        };
        row
    }

    fn pods(&self) -> Vec<(u64, PodRow)> {
        let Reply::Pods(rows) = self.chain.query(PROGRAM, &Query::Pods).unwrap() else {
            panic!("pods answers Pods");
        };
        rows
    }

    fn held(&self) -> Vec<u64> {
        self.pods().into_iter().map(|(check, _)| check).collect()
    }

    fn concurrent(&self, most: u64) -> Result<Vec<u8>, Error> {
        let owner = Origin::Signed(OWNER.to_vec());
        self.chain
            .submit(owner, PROGRAM, &Op::SetConcurrent { most })
    }

    fn listed_by_the_vote(&mut self) {
        self.chain.seat::<Programs>(programs::PROGRAM);
        listed(&self.chain, b"0");
    }

    fn held_by_owner(&mut self) {
        self.listed_by_the_vote();
        let asked = identity::Query::OfKey {
            key: OWNER.to_vec(),
        };
        let identity::Reply::Number(Some(owner)) =
            self.chain.query(identity::PROGRAM, &asked).unwrap()
        else {
            panic!("the owner key acts as an account");
        };
        let own = programs::Op::Own {
            program: PROGRAM.into(),
            holder: programs::Holder::Account(owner),
        };
        let vote = Origin::Program(MockHost::role(abi::role::VALIDATORS));
        self.chain.submit(vote, programs::PROGRAM, &own).unwrap();
    }

    fn concurrency(&self) -> u64 {
        let Reply::Concurrent(most) = self.chain.query(PROGRAM, &Query::Concurrent).unwrap() else {
            panic!("concurrent answers Concurrent");
        };
        most
    }

    fn waiting(&self) -> Vec<u64> {
        let Reply::Waiting(checks) = self.chain.query(PROGRAM, &Query::Waiting).unwrap() else {
            panic!("waiting answers Waiting");
        };
        checks
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

fn pod(name: &str) -> Value {
    json!({"pod": {"name": name}})
}

fn waiting_on_its_pod(node: &mut Node) -> RequestRow {
    node.offer(1).unwrap();
    node.after(2_000);
    node.after(0);
    node.answer(500, 201, pod("duckpond-7"));
    let wait = node.pending();
    let every = sent("GET", "/v1/pods/duckpond-7/wait?every=64");
    assert_eq!(aimed(&wait), every);
    wait
}

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
    let env = json!({"FEATURE_SETS": "forge:program", "NEEDS": "/needs", "OUT": "/out"});
    assert_eq!(
        body["env"], env,
        "OUT and NEEDS name the folders as every runner does"
    );
    assert_eq!(body["resources"], Value::Null, "the job states no CPUs");
    let script = body["command"][2].as_str().unwrap();
    assert!(
        script.contains("cd '/work/programs'\n")
            && script.ends_with("sh /tools/gates.sh /work/programs\n")
    );
    node.answer(500, 201, pod("duckpond-7"));
    let wait = node.pending();
    let every = sent("GET", "/v1/pods/duckpond-7/wait?every=64");
    assert_eq!(aimed(&wait), every);
    assert_eq!(node.held(), [1], "the pod is kept as soon as it is named");
    node.after(1_000);
    let alive = Event {
        id: wait.id,
        seq: 0,
        data: b"event: alive\ndata: {}".to_vec(),
    };
    node.node(&Egress::Events(vec![alive]));
    node.ended(&wait, 1, exit_code);
    let listed = json!({"exit_code": 0, "stdout": "/out/log\n/out/summary.txt\n", "stderr": ""});
    let find = node.answer(1_000, 200, listed);
    assert_eq!(find, sent("POST", "/v1/pods/duckpond-7/exec"));
    let copied = node.answer(1_000, 200, json!({"written": "/shared/…"}));
    assert_eq!(copied, sent("POST", "/v1/pods/duckpond-7/file"));
    let put = node.pending();
    assert_eq!(aimed(&put), sent("POST", "/v1/pods/duckpond-7/blob"));
    let asked = json!({"paths": ["/out/summary.txt"], "kind": "output", "container": "out"});
    assert_eq!(node.body(&put), asked, "the log is not an output");
    let held = format!("sha256:{}", "ab".repeat(32));
    let summary = json!({"path": "/out/summary.txt", "blob": held, "size": 5});
    node.answer(1_000, 200, json!({"blobs": [summary], "kind": "output"}));
    let deleted = node.answer(1_000, 200, json!({"deleted": "duckpond-7"}));
    assert_eq!(deleted, sent("DELETE", "/v1/pods/duckpond-7"));
    assert!(node.idle());
    assert!(node.held().is_empty(), "a deleted pod is let go");
    let summary = Output {
        path: "summary.txt".into(),
        blob: BlobId::Sha256([0xab; 32]),
        size: 5,
    };
    assert_eq!(node.outputs(), [(1, vec![summary])]);
    node
}

#[test]
fn an_offered_check_runs_in_a_pod_and_passes_when_its_steps_do() {
    let node = ran_to_its_end(json!(0));
    let log = format!("/home/actions-pods/checks/programs/{COMMIT}/gates/gates-1/log");
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
        every: 64,
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
    assert!(node.held().is_empty() && node.waiting().is_empty());
}

#[test]
fn a_wait_with_no_whole_second_to_beat_errs_the_check_and_deletes_the_pod() {
    let mut node = node();
    node.offer(1).unwrap();
    node.after(10);
    node.after(0);
    node.answer(490, 201, pod("duckpond-7"));
    let deleted = node.answer(1_000, 200, json!({"deleted": "duckpond-7"}));
    assert_eq!(deleted, sent("DELETE", "/v1/pods/duckpond-7"));
    let reports: [Report; 1] = node.reports().try_into().unwrap();
    let [(1, State::Errored, why, None)] = reports else {
        panic!("one errored report");
    };
    assert!(why.contains("under the 2 s"), "{why}");
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
    let resumed = node
        .chain
        .submit(signed.clone(), PROGRAM, &Op::Resume { check: 1 })
        .unwrap_err();
    assert_eq!(resumed.code, code::UNAUTHORIZED);
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

fn expire_wait_and_read_pod(node: &mut Node) {
    node.offer(1).unwrap();
    node.after(2_000);
    node.after(0);
    node.answer(500, 201, pod("duckpond-7"));
    let first = node.pending();
    assert_eq!(
        aimed(&first),
        sent("GET", "/v1/pods/duckpond-7/wait?every=64")
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
    expire_wait_and_read_pod(&mut node);
    let read = node.answer(1_000, 200, run_status("running", Value::Null));
    assert_eq!(read, sent("GET", "/v1/pods/duckpond-7"));
    let again = node.pending();
    assert_eq!(
        aimed(&again),
        sent("GET", "/v1/pods/duckpond-7/wait?every=32")
    );
    node.ended(&again, 0, json!(0));
    copied_and_deleted(&mut node);
    let line = "pod duckpond-7 ended: exit code 0 (Completed)";
    let log = format!("/home/actions-pods/checks/programs/{COMMIT}/gates/gates-1/log");
    assert_eq!(
        node.reports(),
        [(1, State::Passed, line.to_owned(), Some(log))]
    );
    let row = node.run(1).unwrap();
    assert_eq!((row.block_ms, row.every), (1_000, 32));
}

#[test]
fn a_pod_that_ended_while_its_wait_was_broken_reports_how_it_ended() {
    let mut node = node();
    expire_wait_and_read_pod(&mut node);
    let terminated = run_status("terminated", json!(2));
    let read = node.answer(1_000, 200, terminated);
    assert_eq!(read, sent("GET", "/v1/pods/duckpond-7"));
    copied_and_deleted(&mut node);
    let line = "pod duckpond-7 ended: exit code 2 (Error)".to_owned();
    let log = format!("/home/actions-pods/checks/programs/{COMMIT}/gates/gates-1/log");
    assert_eq!(node.reports(), [(1, State::Failed, line, Some(log))]);
}

#[test]
fn a_claim_refused_for_any_reason_but_its_state_errs_the_check_and_one_already_taken_is_left() {
    let mut node = node();
    let taken = actions::Op::Claim { check: 1 };
    let owner = Origin::Signed(OWNER.to_vec());
    node.chain.submit(owner, actions::PROGRAM, &taken).unwrap();
    node.offer(1).unwrap();
    node.offer(REFUSED).unwrap();
    node.after(2_000);
    node.after(0);
    let made = node.pending();
    assert_eq!(aimed(&made), sent("POST", "/v1/pods"), "none for check 1");
    node.answer(500, 201, pod("duckpond-7"));
    let deleted = node.answer(1_000, 200, json!({"deleted": "duckpond-7"}));
    assert_eq!(deleted, sent("DELETE", "/v1/pods/duckpond-7"));
    assert!(node.idle());
    let why = "the claim was refused: the stand-in refuses this claim for its own reasons";
    assert_eq!(
        node.reports(),
        [(REFUSED, State::Errored, why.to_owned(), None)]
    );
    assert!(node.held().is_empty());
}

#[test]
fn a_check_superseded_while_its_pod_is_made_gives_its_room_to_the_oldest_waiting_check() {
    let mut node = node();
    node.offer(1).unwrap();
    node.offer(2).unwrap();
    node.after(2_000);
    node.after(0);
    node.pending_to("POST", "/v1/pods");
    assert_eq!(node.waiting(), [2]);
    let superseded = actions::Op::Report {
        check: 1,
        state: State::Superseded,
        summary: "superseded by a push".into(),
        log: None,
    };
    let owner = Origin::Signed(OWNER.to_vec());
    node.chain
        .submit(owner, actions::PROGRAM, &superseded)
        .unwrap();
    node.answer(500, 201, pod("duckpond-7"));
    let deleted = node.answer(1_000, 200, json!({"deleted": "duckpond-7"}));
    assert_eq!(deleted, sent("DELETE", "/v1/pods/duckpond-7"));
    assert!(node.held().is_empty());
    node.after(1_000);
    node.after(0);
    node.answer_to(500, "POST", "/v1/pods", 201, pod("duckpond-8"));
    assert_eq!(node.held(), [2], "the room check 1 left is check 2's");
    assert!(node.waiting().is_empty());
}

#[test]
fn a_check_offered_while_an_older_one_waits_runs_after_it() {
    let mut node = node();
    let wait = waiting_on_its_pod(&mut node);
    node.offer(2).unwrap();
    node.after(1_000);
    node.after(0);
    assert_eq!(node.waiting(), [2]);
    node.ended(&wait, 0, json!(0));
    let listed = json!({"exit_code": 0, "stdout": "/out/log\n", "stderr": ""});
    node.answer_to(1_000, "POST", "/v1/pods/duckpond-7/exec", 200, listed);
    let written = json!({"written": "/shared/…"});
    node.answer_to(1_000, "POST", "/v1/pods/duckpond-7/file", 200, written);
    node.after(1_000);
    node.offer(3).unwrap();
    let delete = node.pending_to("DELETE", "/v1/pods/duckpond-7");
    node.reply(&delete, 200, json!({"deleted": "duckpond-7"}));
    node.after(1_000);
    node.after(0);
    node.answer_to(500, "POST", "/v1/pods", 201, pod("duckpond-8"));
    assert_eq!(node.held(), [2], "the older check takes the pod");
    assert_eq!(node.waiting(), [3]);
}

#[test]
fn the_check_a_merge_queue_waits_on_takes_free_room_before_older_waiting_checks() {
    let mut node = node();
    node.chain.seat::<Forged>(forge::PROGRAM);
    let wait = waiting_on_its_pod(&mut node);
    node.offer(2).unwrap();
    node.offer(QUEUE_HEAD).unwrap();
    node.after(1_000);
    node.after(0);
    assert_eq!(node.waiting(), [2, QUEUE_HEAD]);
    node.ended(&wait, 0, json!(0));
    copied_and_deleted(&mut node);
    node.after(1_000);
    node.after(0);
    node.answer_to(500, "POST", "/v1/pods", 201, pod("duckpond-8"));
    assert_eq!(
        node.held(),
        [QUEUE_HEAD],
        "the check on the merge queue front's Preview takes the pod"
    );
    assert_eq!(node.waiting(), [2]);
}

#[test]
fn a_creation_slower_than_a_whole_request_still_waits_at_half_the_window() {
    let mut node = node();
    node.offer(1).unwrap();
    node.after(1_000);
    node.after(0);
    node.answer(88_951, 201, pod("duckpond-7"));
    let wait = node.pending();
    assert_eq!(
        aimed(&wait),
        sent("GET", "/v1/pods/duckpond-7/wait?every=32")
    );
}

#[test]
fn a_job_that_states_what_it_asks_gets_it_for_its_pod_and_cargo_runs_as_many_jobs_as_cpus() {
    let mut node = node();
    node.offer(SIXTEEN).unwrap();
    node.after(2_000);
    node.after(0);
    let made = node.pending();
    let body = node.body(&made);
    let requests = json!({"requests": {"cpu": "16", "memory": "40Gi"}});
    assert_eq!(body["resources"], requests);
    let env = json!({"CARGO_BUILD_JOBS": "16", "FEATURE_SETS": "forge:program", "NEEDS": "/needs", "OUT": "/out"});
    assert_eq!(body["env"], env);
    let scratch = json!({"path": "/var/tmp/target", "scratch": {"medium": "Memory"}});
    assert_eq!(body["inputs"][2], scratch);
}

const FULL: &str = "pods \"duckpond-8\" is forbidden: exceeded quota: hardcap, requested: requests.cpu=16, used: requests.cpu=40, limited: requests.cpu=48";

#[test]
fn a_quota_with_no_room_leaves_the_check_queued_until_a_pod_of_this_runner_ends() {
    let mut node = node();
    node.held_by_owner();
    node.concurrent(2).unwrap();
    let wait = waiting_on_its_pod(&mut node);
    node.offer(2).unwrap();
    node.after(1_000);
    node.after(0);
    node.answer_to(500, "POST", "/v1/pods", 403, json!({"error": FULL}));
    assert_eq!(node.waiting(), [2]);
    assert!(node.reports().is_empty(), "a full quota reports nothing");
    assert_eq!(node.held(), [1]);
    node.ended(&wait, 0, json!(0));
    copied_and_deleted(&mut node);
    assert!(node.waiting().is_empty(), "check 2 runs again");
    node.after(1_000);
    node.after(0);
    let made = node.pending();
    assert_eq!(aimed(&made), sent("POST", "/v1/pods"));
    node.answer(500, 201, pod("duckpond-9"));
    let wait = node.pending();
    assert_eq!(
        aimed(&wait),
        sent("GET", "/v1/pods/duckpond-9/wait?every=32")
    );
    assert_eq!(node.held(), [2]);
}

#[test]
fn a_check_the_quota_has_no_room_for_lets_a_later_check_take_the_free_room() {
    let mut node = node();
    node.held_by_owner();
    node.concurrent(2).unwrap();
    waiting_on_its_pod(&mut node);
    node.offer(SIXTEEN).unwrap();
    node.offer(8).unwrap();
    node.after(1_000);
    node.after(0);
    node.answer_to(500, "POST", "/v1/pods", 403, json!({"error": FULL}));
    assert_eq!(node.waiting(), [SIXTEEN], "check 8 is booked to run again");
    node.after(1_000);
    node.after(0);
    node.answer_to(500, "POST", "/v1/pods", 201, pod("duckpond-8"));
    assert_eq!(node.held(), [1, 8], "check 8 fits where check 4 did not");
    assert_eq!(node.waiting(), [SIXTEEN]);
    assert!(node.reports().is_empty(), "a full quota reports nothing");
}

#[test]
fn a_check_the_quota_has_no_room_for_is_tried_again_a_whole_egress_wait_later() {
    let mut node = node();
    node.offer(1).unwrap();
    node.after(2_000);
    node.after(0);
    node.answer(500, 403, json!({"error": FULL}));
    assert!(node.idle());
    assert_eq!(node.waiting(), [1]);
    for _ in 0..=DEFAULT_WAIT {
        node.after(1_000);
    }
    node.after(0);
    let made = node.pending();
    assert_eq!(
        aimed(&made),
        sent("POST", "/v1/pods"),
        "a Sweep tried it again"
    );
}

#[test]
fn a_full_check_offered_again_that_waits_for_room_is_tried_again_when_room_frees() {
    let mut node = node();
    node.held_by_owner();
    node.concurrent(2).unwrap();
    waiting_on_its_pod(&mut node);
    node.offer(SIXTEEN).unwrap();
    node.after(1_000);
    node.after(0);
    node.answer_to(500, "POST", "/v1/pods", 403, json!({"error": FULL}));
    node.offer(REFUSED).unwrap();
    node.offer(SIXTEEN).unwrap();
    node.after(1_000);
    node.after(0);
    node.answer_to(500, "POST", "/v1/pods", 201, pod("duckpond-8"));
    let deleted = json!({"deleted": "duckpond-8"});
    node.answer_to(1_000, "DELETE", "/v1/pods/duckpond-8", 200, deleted);
    node.after(1_000);
    node.after(0);
    let made = node.pending_to("POST", "/v1/pods");
    let memory = &node.body(&made)["resources"]["requests"]["memory"];
    assert_eq!(memory, "40Gi", "check 4 is tried again");
}

#[test]
fn a_pod_that_asks_more_than_the_whole_quota_errs_its_check() {
    let mut node = node();
    node.offer(1).unwrap();
    node.after(2_000);
    node.after(0);
    let never = "pods \"duckpond-7\" is forbidden: exceeded quota: hardcap, requested: requests.cpu=16,requests.memory=200Gi, used: requests.cpu=0,requests.memory=0, limited: requests.cpu=54840m,requests.memory=124634176Ki";
    node.answer(500, 403, json!({"error": never}));
    assert!(node.idle() && node.waiting().is_empty());
    let reports: [Report; 1] = node.reports().try_into().unwrap();
    let [(1, State::Errored, why, None)] = reports else {
        panic!("one errored report");
    };
    assert!(
        why.starts_with("the pod asks more than the quota holds"),
        "{why}"
    );
}

#[test]
fn a_run_an_update_cut_off_is_taken_up_by_a_sweep_and_reports_how_its_pod_ended() {
    let mut node = node();
    node.chain.seat::<Programs>(programs::PROGRAM);
    listed(&node.chain, b"1");
    waiting_on_its_pod(&mut node);
    let [(1, before)]: [(u64, PodRow); 1] = node.pods().try_into().unwrap() else {
        panic!("check 1's pod is held");
    };
    assert!(before.code.is_some(), "the pod names the code that made it");
    listed(&node.chain, b"2");
    node.chain.cancel_parked(PROGRAM);
    for _ in 0..=DEFAULT_WAIT {
        node.after(1_000);
    }
    assert!(node.idle() && node.reports().is_empty());
    node.sweep();
    node.sweep();
    node.after(2_000);
    node.after(0);
    let again = node.pending();
    assert_eq!(
        aimed(&again),
        sent("GET", "/v1/pods/duckpond-7/wait?every=64"),
        "one Resume, at the block the Sweep measured"
    );
    let [(1, after)]: [(u64, PodRow); 1] = node.pods().try_into().unwrap() else {
        panic!("check 1's pod is held");
    };
    assert!(after.code.is_some() && after.code != before.code);
    node.ended(&again, 0, json!(0));
    copied_and_deleted(&mut node);
    let line = "pod duckpond-7 ended: exit code 0 (Completed)";
    let log = format!("/home/actions-pods/checks/programs/{COMMIT}/gates/gates-1/log");
    assert_eq!(
        node.reports(),
        [(1, State::Passed, line.to_owned(), Some(log))]
    );
    assert!(node.held().is_empty());
}

#[test]
fn a_rerun_deletes_the_earlier_attempts_pod_and_reports_it_superseded() {
    let mut node = node();
    waiting_on_its_pod(&mut node);
    let rerun = actions::Op::Rerun { check: 1 };
    let owner = Origin::Signed(OWNER.to_vec());
    node.chain.submit(owner, actions::PROGRAM, &rerun).unwrap();
    node.offer(AGAIN).unwrap();
    node.after(1_000);
    node.after(0);
    let deleted = "/v1/pods/duckpond-7";
    node.answer_to(
        1_000,
        "DELETE",
        deleted,
        200,
        json!({"deleted": "duckpond-7"}),
    );
    node.answer_to(500, "POST", "/v1/pods", 201, pod("duckpond-8"));
    let why = "superseded by check 3 (attempt 2)".to_owned();
    assert_eq!(node.reports(), [(1, State::Superseded, why, None)]);
    assert_eq!(node.held(), [AGAIN]);
    node.pending_to("GET", "/v1/pods/duckpond-8/wait?every=32");
}

#[test]
fn a_sweep_deletes_the_pod_of_a_check_that_ended_and_leaves_a_live_run_alone() {
    let mut node = node();
    node.held_by_owner();
    node.concurrent(2).unwrap();
    waiting_on_its_pod(&mut node);
    node.offer(2).unwrap();
    node.after(1_000);
    node.after(0);
    node.answer_to(500, "POST", "/v1/pods", 201, pod("duckpond-8"));
    node.pending_to("GET", "/v1/pods/duckpond-8/wait?every=32");
    let report = actions::Op::Report {
        check: 1,
        state: State::Passed,
        summary: "a person ran it".into(),
        log: None,
    };
    let owner = Origin::Signed(OWNER.to_vec());
    node.chain.submit(owner, actions::PROGRAM, &report).unwrap();
    node.sweep();
    let deleted = "/v1/pods/duckpond-7";
    node.answer_to(
        1_000,
        "DELETE",
        deleted,
        200,
        json!({"deleted": "duckpond-7"}),
    );
    assert_eq!(node.held(), [2]);
    let deletes = node
        .open()
        .iter()
        .filter(|row| aimed(row).0 == "DELETE")
        .count();
    assert_eq!(deletes, 0, "check 2's pod is left alone");
}

#[test]
fn a_check_past_the_runners_concurrency_waits_for_one_of_its_pods_to_end() {
    let mut node = node();
    assert_eq!(node.concurrency(), 1);
    node.offer(1).unwrap();
    node.offer(2).unwrap();
    node.after(2_000);
    node.after(0);
    let made = node.pending();
    assert_eq!(aimed(&made), sent("POST", "/v1/pods"), "one pod, not two");
    assert_eq!(node.waiting(), [2]);
    node.answer(500, 201, pod("duckpond-7"));
    let wait = node.pending();
    node.ended(&wait, 0, json!(0));
    copied_and_deleted(&mut node);
    node.after(1_000);
    node.after(0);
    let next = node.pending();
    assert_eq!(
        aimed(&next),
        sent("POST", "/v1/pods"),
        "check 2 runs once the pod ends"
    );
    assert!(node.waiting().is_empty());

    let mut wide = crate::tests::node();
    wide.held_by_owner();
    wide.concurrent(2).unwrap();
    assert_eq!(wide.concurrency(), 2);
    wide.offer(1).unwrap();
    wide.offer(2).unwrap();
    wide.after(2_000);
    wide.after(0);
    let posts = wide
        .open()
        .iter()
        .filter(|row| aimed(row) == sent("POST", "/v1/pods"))
        .count();
    assert_eq!(posts, 2, "two pods at once");
    let none = wide.concurrent(0).unwrap_err();
    assert_eq!(none.code, code::INVALID_INPUT);
}

#[test]
fn only_the_vote_or_an_account_holding_the_name_sets_how_many_pods_it_holds() {
    let mut held = node();
    held.held_by_owner();
    let stranger = Origin::Signed(STRANGER.to_vec());
    let create = identity::Op::Create {
        name: "Stranger".into(),
        scheme: Scheme::Ed25519,
    };
    held.chain
        .submit(stranger.clone(), identity::PROGRAM, &create)
        .unwrap();
    let eight = Op::SetConcurrent { most: 8 };
    let refused = held.chain.submit(stranger, PROGRAM, &eight).unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED, "{}", refused.message);
    assert_eq!(held.concurrency(), 1);
    held.concurrent(2).unwrap();
    assert_eq!(held.concurrency(), 2);

    let mut voted = node();
    voted.listed_by_the_vote();
    let refused = voted.concurrent(2).unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED, "{}", refused.message);
    let vote = Origin::Program(MockHost::role(abi::role::VALIDATORS));
    let three = Op::SetConcurrent { most: 3 };
    voted.chain.submit(vote, PROGRAM, &three).unwrap();
    assert_eq!(voted.concurrency(), 3);
}

#[test]
fn a_superseded_checks_pod_is_deleted_by_the_next_run_which_takes_its_room() {
    let mut node = node();
    waiting_on_its_pod(&mut node);
    let superseded = actions::Op::Report {
        check: 1,
        state: State::Superseded,
        summary: "superseded by Preview 0123456789ab of #7".into(),
        log: None,
    };
    let owner = Origin::Signed(OWNER.to_vec());
    node.chain
        .submit(owner, actions::PROGRAM, &superseded)
        .unwrap();
    node.offer(2).unwrap();
    node.after(1_000);
    node.after(0);
    let deleted = json!({"deleted": "duckpond-7"});
    node.answer_to(1_000, "DELETE", "/v1/pods/duckpond-7", 200, deleted);
    node.answer_to(500, "POST", "/v1/pods", 201, pod("duckpond-8"));
    assert_eq!(node.held(), [2]);
    assert!(node.waiting().is_empty());
    node.pending_to("GET", "/v1/pods/duckpond-8/wait?every=32");
}

#[test]
fn a_check_that_needs_a_job_has_its_outputs_placed_in_its_one_pod() {
    let mut node = node();
    node.offer(NEEDY).unwrap();
    node.after(2_000);
    node.after(0);
    let made = node.pending();
    assert_eq!(aimed(&made), sent("POST", "/v1/pods"));
    let held = format!("sha256:{}", "ab".repeat(32));
    let placed = json!({"path": "/needs/build/dist/app.tar.gz", "blob": held});
    let inputs = node.body(&made)["inputs"].clone();
    assert_eq!(inputs[2], placed, "{inputs}");
}

#[test]
fn a_quantity_reads_as_kubernetes_writes_it() {
    let cpus = ["16", "1.5", "500m", "0.25"].map(millicores);
    assert_eq!(cpus, [Some(16_000), Some(1_500), Some(500), Some(250)]);
    let not = ["lots", "16.", "+5", "1.2345", "m", "5mm", ""].map(millicores);
    assert!(not.iter().all(Option::is_none), "{not:?}");
    let memory = ["40Gi", "512Mi", "1.5G", "1000", "124634176Ki"].map(bytes);
    let expected: [u64; 5] = [40 << 30, 512 << 20, 1_500_000_000, 1000, 124_634_176 << 10];
    assert_eq!(memory, expected.map(Some));
    assert_eq!(["lots", "40GiB", "Gi", "5m"].map(bytes), [None; 4]);
}

#[test]
fn a_job_asking_what_is_no_kubernetes_quantity_errs_its_check_naming_the_field_and_makes_no_pod() {
    let mut node = node();
    node.offer(LOTS).unwrap();
    node.after(2_000);
    node.after(0);
    assert!(node.idle(), "no pod is asked for");
    let why = "the job's memory is no Kubernetes quantity such as \"40Gi\" or \"512Mi\": it reads \"lots\"";
    assert_eq!(
        node.reports(),
        [(LOTS, State::Errored, why.to_owned(), None)]
    );
}

#[test]
fn a_needed_output_that_would_leave_its_folder_errs_the_check_and_makes_no_pod() {
    let mut node = node();
    node.offer(ASTRAY).unwrap();
    node.after(2_000);
    node.after(0);
    assert!(node.idle());
    let why = "check output \"../../etc/passwd\" of `build` would land outside /needs/build";
    assert_eq!(
        node.reports(),
        [(ASTRAY, State::Errored, why.to_owned(), None)]
    );
}

const UNMADE: &str = "the pod was not created: Error from server (InternalError): error when creating \"STDIN\": Internal error occurred: resource quota evaluation timed out";

#[test]
fn a_pod_the_cluster_did_not_make_waits_and_the_sweep_it_books_makes_it_again() {
    let mut node = node();
    node.offer(1).unwrap();
    node.after(2_000);
    node.after(0);
    node.answer(500, 502, json!({"error": UNMADE}));
    assert!(
        node.reports().is_empty(),
        "a pod not made is not the check's end"
    );
    assert_eq!(node.waiting(), [1]);
    for _ in 1..DEFAULT_WAIT {
        node.after(1_000);
    }
    assert!(node.idle(), "nothing tries again before the booked Sweep");
    node.after(1_000);
    node.after(0);
    node.answer_to(500, "POST", "/v1/pods", 502, json!({"error": UNMADE}));
    assert_eq!(
        node.waiting(),
        [1],
        "still waiting, so another Sweep is booked"
    );
    for _ in 0..DEFAULT_WAIT {
        node.after(1_000);
    }
    node.after(0);
    node.answer_to(500, "POST", "/v1/pods", 201, pod("duckpond-7"));
    assert!(node.waiting().is_empty() && node.reports().is_empty());
    assert_eq!(node.held(), [1]);
}

#[test]
fn a_waiting_check_that_ended_elsewhere_is_not_counted_as_waiting() {
    let mut node = node();
    node.held_by_owner();
    node.concurrent(2).unwrap();
    waiting_on_its_pod(&mut node);
    node.offer(2).unwrap();
    node.after(1_000);
    node.after(0);
    node.answer_to(500, "POST", "/v1/pods", 403, json!({"error": FULL}));
    assert_eq!(node.waiting(), [2]);
    let superseded = actions::Op::Report {
        check: 2,
        state: State::Superseded,
        summary: "superseded by a push".into(),
        log: None,
    };
    let owner = Origin::Signed(OWNER.to_vec());
    node.chain
        .submit(owner, actions::PROGRAM, &superseded)
        .unwrap();
    assert!(
        node.waiting().is_empty(),
        "the queue says how deep it really is"
    );
}

#[test]
fn a_newer_check_that_deferred_to_an_older_one_runs_once_that_one_drops_out_before_its_run() {
    let mut node = node();
    let _wait = waiting_on_its_pod(&mut node);
    node.offer(2).unwrap();
    node.after(1_000);
    node.after(0);
    assert_eq!(node.waiting(), [2]);
    let owner = Origin::Signed(OWNER.to_vec());
    let superseded = |check| actions::Op::Report {
        check,
        state: State::Superseded,
        summary: "superseded by a push".into(),
        log: None,
    };
    node.chain
        .submit(owner.clone(), actions::PROGRAM, &superseded(1))
        .unwrap();
    node.offer(8).unwrap();
    node.after(1_000);
    node.after(0);
    let delete = node.pending_to("DELETE", "/v1/pods/duckpond-7");
    node.reply(&delete, 200, json!({"deleted": "duckpond-7"}));
    assert!(node.held().is_empty());
    assert_eq!(node.waiting(), [8], "8 waits behind 2, whose run is booked");
    node.chain
        .submit(owner, actions::PROGRAM, &superseded(2))
        .unwrap();
    node.after(1_000);
    node.after(0);
    node.answer_to(500, "POST", "/v1/pods", 201, pod("duckpond-8"));
    assert_eq!(node.held(), [8], "the room check 2 never took is check 8's");
    assert!(node.waiting().is_empty());
}

#[test]
fn a_pod_whose_out_cannot_be_listed_says_why_its_check_kept_no_log() {
    let mut node = node();
    let wait = waiting_on_its_pod(&mut node);
    node.ended(&wait, 0, json!(255));
    let unreached = "error: unable to upgrade connection: container not found (\"out\")";
    let listed = json!({"exit_code": 1, "stdout": "", "stderr": format!("{unreached}\n")});
    let find = node.answer(1_000, 200, listed);
    assert_eq!(find, sent("POST", "/v1/pods/duckpond-7/exec"));
    let deleted = node.answer(1_000, 200, json!({"deleted": "duckpond-7"}));
    assert_eq!(deleted, sent("DELETE", "/v1/pods/duckpond-7"));
    assert!(node.idle());
    let line = format!(
        "pod duckpond-7 ended: exit code 0 (Completed); /out was not listed: find exited 1: {unreached}"
    );
    assert_eq!(node.reports(), [(1, State::Failed, line, None)]);
}
