//! The program on a MockChain with identity, attribution, the scheduler, awaits, valset, egress,
//! access and files seated, and stand-ins for forge and the programs role: each kind of wake is
//! armed, heard and told once to its account through attribution, and each callback is its sender's.
use ::program::{
    BlobId, Error, ExecCtx, HashKind, MockChain, MockHost, Origin, Principal, Program, QueryCtx,
    Scheme, code, invalid, not_found,
};
use abi::role::egress::{Answer, Event, Op as Egress, Request, RequestRow, StreamEnd, http};
use abi::role::programs::Entry;
use borsh::{BorshDeserialize, BorshSerialize};
use serde_json::{Value, json};
use store::Item;
use valset::{Genesis, Member, Valset};

use crate::*;

const NODE: [u8; 32] = [1; 32];
const ADA: &[u8] = b"ada-key";
const BO: &[u8] = b"bo-key";
const POD: &str = "duckpond-7";

const REPOS: Item<Vec<String>> = Item::new("repos");
const CHANGES: Item<Vec<(String, u64, bool, forge::Revision)>> = Item::new("changes");
const WATCHING: Item<Vec<u8>> = Item::new("watching");
const RUNNING: Item<Vec<Entry>> = Item::new("running");
const PLANNED: Item<Vec<programs::Pending>> = Item::new("planned");

struct ForgeStand;

impl Program for ForgeStand {
    const NAME: &'static str = forge::PROGRAM;
    type Op = forge::Op;
    type Query = forge::Query;
    type Reply = forge::Reply;

    fn execute(ctx: &ExecCtx, op: forge::Op) -> Result<(), Error> {
        match op {
            forge::Op::Create { repo, .. } => {
                let mut repos = REPOS.get(ctx)?.unwrap_or_default();
                repos.push(repo);
                REPOS.put(ctx, &repos);
            }
            forge::Op::ChangeOpen { repo, from, .. } => {
                let mut changes = CHANGES.get(ctx)?.unwrap_or_default();
                let n = changes.iter().filter(|change| change.0 == repo).count() as u64 + 1;
                changes.push((repo, n, false, from));
                CHANGES.put(ctx, &changes);
            }
            forge::Op::ChangeClose { repo, n } => {
                let mut changes = CHANGES.get(ctx)?.unwrap_or_default();
                for change in changes.iter_mut().filter(|c| c.0 == repo && c.1 == n) {
                    change.2 = true;
                }
                CHANGES.put(ctx, &changes);
            }
            forge::Op::Watch { deliver } => WATCHING.put(ctx, &deliver),
            _ => return Err(invalid("the forge stand-in opens and closes changes")),
        }
        Ok(())
    }

    fn query(ctx: &QueryCtx, query: forge::Query) -> Result<forge::Reply, Error> {
        let height = ctx.env().height;
        match query {
            forge::Query::Repo { repo } => {
                let known = REPOS.get(ctx)?.unwrap_or_default().contains(&repo);
                if !known {
                    return Err(not_found(format!("no repository {repo}")));
                }
                let info = forge::RepoInfo {
                    name: repo,
                    repo: forge::Repo {
                        hash: HashKind::Sha1,
                        owner: Principal::Root,
                        settings: forge::Settings::default(),
                        refs_count: 0,
                        last_activity: 0,
                    },
                };
                Ok(forge::Reply::Repo {
                    height,
                    repo: info,
                    bounds: bounds(),
                })
            }
            forge::Query::Change { repo, n, .. } => {
                let changes = CHANGES.get(ctx)?.unwrap_or_default();
                let Some((_, _, closed, from)) = changes.iter().find(|c| c.0 == repo && c.1 == n)
                else {
                    return Err(not_found(format!("no change {n} in {repo}")));
                };
                let state = match closed {
                    true => forge::ChangeState::Closed,
                    false => forge::ChangeState::Open,
                };
                Ok(forge::Reply::Change {
                    height,
                    change: change_row(n, state, from.clone()),
                    source_head: None,
                    target_head: None,
                    reviews: PageResponse {
                        height,
                        items: Vec::new(),
                        next: None,
                    },
                })
            }
            _ => Err(invalid(
                "the forge stand-in answers repositories and changes",
            )),
        }
    }
}

fn bounds() -> forge::Bounds {
    forge::Bounds {
        max_objects: 10_000,
        max_delta_depth: 64,
        max_object_size: 1 << 20,
        push_walk: 1000,
        fetch_walk: 1000,
        merge_cost: 100_000,
        page_size: 128,
        log_walk: 1000,
        tree_walk: 256,
        diff_bytes: 4 << 20,
        blob_bytes: 1 << 20,
        record_bytes: 64 << 10,
    }
}

fn change_row(n: u64, state: forge::ChangeState, from: forge::Revision) -> forge::Change {
    forge::Change {
        n,
        from,
        into: b"refs/heads/main".to_vec(),
        title: "dev".into(),
        body: String::new(),
        author: Principal::Root,
        state,
        reviewers: Vec::new(),
        created_height: 0,
        updated_height: 0,
        created_time: 0,
        updated_time: 0,
        review_count: 0,
        comment_count: 0,
        verdicts: forge::ReviewCounts::default(),
        merge_oid: None,
        closed_by: None,
        merged_by: None,
        merged_heads: None,
        channel: String::new(),
        system_seq: 0,
    }
}

#[derive(Debug, BorshSerialize, BorshDeserialize)]
enum Plan {
    Running(Vec<Entry>),
    Scheduled(Vec<programs::Pending>),
}

struct ProgramsStand;

impl Program for ProgramsStand {
    const NAME: &'static str = programs::PROGRAM;
    type Op = Plan;
    type Query = programs::Query;
    type Reply = programs::Reply;

    fn execute(ctx: &ExecCtx, op: Plan) -> Result<(), Error> {
        match op {
            Plan::Running(entries) => RUNNING.put(ctx, &entries),
            Plan::Scheduled(pending) => PLANNED.put(ctx, &pending),
        }
        Ok(())
    }

    fn query(ctx: &QueryCtx, query: programs::Query) -> Result<programs::Reply, Error> {
        let height = ctx.env().height;
        let mut planned = PLANNED.get(ctx)?.unwrap_or_default();
        planned.sort_by_key(|pending| pending.height);
        match query {
            programs::Query::Scheduled { .. } => {
                let items = planned
                    .into_iter()
                    .filter(|pending| pending.height > height)
                    .collect();
                Ok(programs::Reply::Scheduled(PageResponse {
                    height,
                    items,
                    next: None,
                }))
            }
            programs::Query::Program(program) => {
                let mut entry = RUNNING
                    .get(ctx)?
                    .unwrap_or_default()
                    .into_iter()
                    .find(|entry| entry.program == program);
                for pending in planned.iter().filter(|pending| pending.height <= height) {
                    match &pending.change {
                        programs::Change::Set(set) if set.program == program => {
                            entry = Some(set.clone())
                        }
                        programs::Change::Remove(gone) if *gone == program => entry = None,
                        _ => {}
                    }
                }
                Ok(programs::Reply::Program { height, entry })
            }
            _ => Err(invalid(
                "the programs stand-in answers Scheduled and Program",
            )),
        }
    }
}

struct World {
    chain: MockChain,
    height: u64,
    time: u64,
    ada: AccountNumber,
    bo: AccountNumber,
}

fn signed(key: &[u8]) -> Origin {
    Origin::Signed(key.to_vec())
}

fn world() -> World {
    let mut chain = MockChain::default();
    chain.seat::<identity::Identity>(identity::PROGRAM);
    chain.seat::<Valset>("valset");
    chain.seat::<awaits::Awaits>(awaits::PROGRAM);
    chain.seat::<::scheduler::Scheduler>(::scheduler::PROGRAM);
    chain.seat::<access::Access>("access");
    chain.seat_awaiting::<egress::Egress>(egress::PROGRAM);
    chain.seat::<attribution::Attribution>(attribution::PROGRAM);
    chain.seat::<files::Files>(files::PROGRAM);
    chain.seat::<ForgeStand>(forge::PROGRAM);
    chain.seat::<ProgramsStand>(programs::PROGRAM);
    chain.seat_awaiting::<Wake>(PROGRAM);
    let genesis = Genesis {
        validators: vec![Member {
            key: NODE.to_vec(),
            address: None,
            halo: "node.example.org:9100".into(),
        }],
        member_cap: 8,
    };
    chain.init("valset", &genesis).unwrap();
    let person = |key: &[u8], name: &str| {
        let create = identity::Op::Create {
            name: name.into(),
            scheme: Scheme::Ed25519,
        };
        let made = chain
            .submit(signed(key), identity::PROGRAM, &create)
            .unwrap();
        abi::decode::<AccountNumber>(&made).unwrap()
    };
    let ada = person(ADA, "Ada");
    let bo = person(BO, "Bo");
    let register = Egress::Register {
        service: "pods".into(),
    };
    let assign = Egress::Assign {
        service: "pods".into(),
        nodes: vec![NODE.to_vec()],
    };
    for op in [register, assign] {
        chain.submit(signed(ADA), egress::PROGRAM, &op).unwrap();
    }
    let serve = Egress::Serve {
        services: vec!["pods".into()],
    };
    chain
        .submit(signed(&NODE), egress::PROGRAM, &serve)
        .unwrap();
    chain.at(2, 1_000);
    World {
        chain,
        height: 2,
        time: 1_000,
        ada,
        bo,
    }
}

impl World {
    fn after(&mut self, ms: u64) {
        self.height += 1;
        self.time += ms;
        self.chain.at(self.height, self.time);
    }

    fn to(&mut self, height: u64) {
        while self.height < height {
            self.after(1_000);
        }
    }

    fn act(&self, key: &[u8], op: &Op) -> Result<Vec<u8>, Error> {
        self.chain.submit(signed(key), PROGRAM, op)
    }

    fn wake_on(&self, key: &[u8], event: WaitFor) -> Result<u64, Error> {
        let made = self.act(key, &Op::WakeOn { event })?;
        Ok(abi::decode(&made).unwrap())
    }

    fn row(&self, id: u64) -> Option<WakeRow> {
        let Reply::Wake(row) = self.chain.query(PROGRAM, &Query::Wake { id }).unwrap() else {
            panic!("Wake answers Wake");
        };
        row
    }

    fn told(&self, account: AccountNumber) -> Vec<String> {
        let asked = attribution::Query::ChangesTo {
            recipient: account,
            page: PageRequest::default(),
        };
        let attribution::Reply::Changes(page) =
            self.chain.query(attribution::PROGRAM, &asked).unwrap()
        else {
            panic!("ChangesTo answers Changes");
        };
        page.items
            .into_iter()
            .filter(|change| change.source.program == PROGRAM)
            .map(|change| String::from_utf8(change.detail).unwrap())
            .collect()
    }

    fn forge_op(&self, op: &forge::Op) {
        self.chain.submit(signed(ADA), forge::PROGRAM, op).unwrap();
    }

    fn forged(&self, event: forge::Watched) {
        let by_forge = Origin::Program(forge::PROGRAM.into());
        self.chain
            .submit(by_forge, PROGRAM, &Op::Forged(event))
            .unwrap();
    }

    fn forge_watch(&self) -> Vec<u8> {
        let host = self.chain.host(forge::PROGRAM);
        let read = host.query(MockHost::env(forge::PROGRAM));
        WATCHING.get(&read).unwrap().unwrap_or_default()
    }

    fn files_watched(&self) -> bool {
        let host = self.chain.host(files::PROGRAM);
        host.borrow()
            .state
            .keys()
            .any(|key| key.starts_with(b"watchers/"))
    }

    fn commit(&self, key: &[u8], path: &str, text: &str) -> BlobId {
        let base = match self
            .chain
            .query(files::PROGRAM, &files::Query::Head)
            .unwrap()
        {
            files::Reply::Head(head) => head.map(|row| row.id),
            other => panic!("{other:?}"),
        };
        let chunk = self
            .chain
            .put_blob(files::CHUNK, text.as_bytes().to_vec())
            .unwrap();
        let commit = files::Op::Commit {
            base,
            message: "configure".into(),
            changes: vec![files::Change::Put {
                path: path.into(),
                exec: false,
                meta: Default::default(),
                content: files::Content::Chunks {
                    size: text.len() as u64,
                    chunks: vec![chunk],
                },
            }],
        };
        let made = self
            .chain
            .submit(signed(key), files::PROGRAM, &commit)
            .unwrap();
        abi::decode(&made).unwrap()
    }

    fn plan(&self, plan: Plan) {
        self.chain
            .submit(Origin::Root, programs::PROGRAM, &plan)
            .unwrap();
    }

    fn open(&self) -> Vec<RequestRow> {
        let asked = egress::Query::Assigned { key: NODE.to_vec() };
        let egress::Reply::Requests(rows) = self.chain.query(egress::PROGRAM, &asked).unwrap()
        else {
            panic!("Assigned answers Requests");
        };
        rows
    }

    fn pending(&self) -> RequestRow {
        let [row] = self.open().try_into().expect("one request in flight");
        row
    }

    fn node(&self, op: &Egress) {
        self.chain
            .submit(signed(&NODE), egress::PROGRAM, op)
            .unwrap();
    }

    fn answer(&mut self, ms: u64, status: u16, body: Value) -> String {
        self.after(ms);
        let row = self.pending();
        self.reply(&row, status, body);
        target(&row)
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

    fn streams(&self) -> Vec<RequestRow> {
        let open = self.open().into_iter();
        open.filter(|row| target(row).contains("/wait?")).collect()
    }

    fn reads(&self) -> Vec<RequestRow> {
        let read = format!("/v1/pods/{POD}");
        let open = self.open().into_iter();
        open.filter(|row| target(row) == read).collect()
    }

    fn end_stream(&mut self, stream: &RequestRow, events: u64) {
        let end = StreamEnd::Complete {
            status: 200,
            headers: vec![("content-type".into(), "text/event-stream".into())],
        };
        let finished = Egress::EndStream {
            id: stream.id,
            events,
            end,
        };
        self.node(&finished);
    }
}

fn target(row: &RequestRow) -> String {
    let Request::Http { head, .. } = &row.request else {
        panic!("an http request");
    };
    head.target.clone()
}

fn entry(program: &str, code: u8) -> Entry {
    Entry {
        program: program.into(),
        code: BlobId::Sha256([code; 32]),
        params: Vec::new(),
    }
}

fn set_at(height: u64, set: Entry) -> programs::Pending {
    programs::Pending {
        height,
        change: programs::Change::Set(set),
        by: None,
    }
}

#[test]
fn a_height_wake_is_told_once_at_its_block_through_attribution() {
    let mut world = world();
    let id = world.wake_on(ADA, WaitFor::Height { height: 5 }).unwrap();
    world.to(5);
    assert!(world.told(world.ada).is_empty(), "block 5 has not ended");
    world.to(6);
    assert_eq!(world.told(world.ada), ["Wake 1 is over: block 5 has come."]);
    let asked = attribution::Query::ChangesTo {
        recipient: world.ada,
        page: PageRequest::default(),
    };
    let attribution::Reply::Changes(page) =
        world.chain.query(attribution::PROGRAM, &asked).unwrap()
    else {
        panic!("ChangesTo answers Changes");
    };
    let [change] = page.items.try_into().expect("one change");
    let object = attribution::Object {
        kind: KIND.into(),
        id: id.to_string(),
    };
    assert_eq!(change.source.object, object);
    assert_eq!(change.reason, attribution::Reason::Result);
    assert_eq!(change.actor, attribution::Actor::Program(PROGRAM.into()));
    let ended = world.row(id).unwrap().ended.unwrap();
    assert_eq!((ended.height, ended.text.as_str()), (5, "block 5 has come"));
    world.to(9);
    assert_eq!(world.told(world.ada).len(), 1, "it is told once");
    assert!(world.told(world.bo).is_empty());
}

#[test]
fn only_its_account_cancels_a_wake_and_a_cancelled_height_ends_nothing() {
    let mut world = world();
    let id = world.wake_on(ADA, WaitFor::Height { height: 6 }).unwrap();
    let theirs = world.act(BO, &Op::Cancel { id }).unwrap_err();
    assert_eq!(theirs.code, code::UNAUTHORIZED);
    world.act(ADA, &Op::Cancel { id }).unwrap();
    assert_eq!(world.row(id), None, "a cancelled wake is gone");
    world.to(9);
    assert!(world.told(world.ada).is_empty());
    let gone = world.act(ADA, &Op::Cancel { id }).unwrap_err();
    assert_eq!(gone.code, code::NOT_FOUND);
    let asked = Query::Wakes {
        account: world.ada,
        page: PageRequest::default(),
    };
    let Reply::Wakes(page) = world.chain.query(PROGRAM, &asked).unwrap() else {
        panic!("Wakes answers Wakes");
    };
    assert!(page.items.is_empty());
}

#[test]
fn a_chat_wake_is_refused_saying_attribution_already_tells_it() {
    let world = world();
    let message = WaitFor::Message {
        channel: "general".into(),
        from: None,
        mentions_me: false,
    };
    let refused = world.wake_on(ADA, message).unwrap_err();
    assert_eq!(refused.code, code::INVALID_INPUT);
    assert!(
        refused.message.contains("attribution already tells"),
        "{refused:?}"
    );
}

#[test]
fn each_callback_is_refused_from_anyone_but_its_sender() {
    let world = world();
    let commit = world.commit(ADA, "/shared/a.md", "a");
    let files::Reply::Head(Some(row)) = world
        .chain
        .query(files::PROGRAM, &files::Query::Head)
        .unwrap()
    else {
        panic!("files holds a head");
    };
    assert_eq!(row.id, commit);
    let event = Event {
        id: 1,
        seq: 0,
        data: b"event: ended\ndata: {}".to_vec(),
    };
    let merged = forge::Watched::Merged {
        repo: "notes".into(),
        n: 1,
        oid: "abc123".into(),
        by: Principal::Account(world.ada),
    };
    let firing = Firing {
        key: Key::Path {
            path: "/shared/a.md".into(),
        },
        fired: Fired::Filed(row.clone()),
        lo: 0,
        hi: 9,
    };
    let ops = [
        Op::Due,
        Op::PodEvent { id: 1, event },
        Op::Forged(merged),
        Op::Filed(row),
        Op::Fan { firing },
    ];
    for op in &ops {
        let refused = world.act(ADA, op).unwrap_err();
        assert_eq!(refused.code, code::UNAUTHORIZED, "{op:?}");
    }
    for op in [&ops[0], &ops[4]] {
        let unscheduled = world
            .chain
            .submit(Origin::Program(PROGRAM.into()), PROGRAM, op)
            .unwrap_err();
        assert_eq!(unscheduled.code, code::UNAUTHORIZED, "{op:?}");
    }
}

#[test]
fn a_file_wake_is_told_on_a_commit_to_its_path_and_files_is_heard_only_while_one_is_open() {
    let mut world = world();
    world.commit(ADA, "/shared/tools/warm-pod", "duckpond-1\n");
    assert!(!world.files_watched());
    let file = WaitFor::File {
        path: "/shared/tools/warm-pod".into(),
    };
    let id = world.wake_on(ADA, file).unwrap();
    assert!(
        world.files_watched(),
        "files is heard while a file wake is open"
    );
    world.after(1_000);
    world.commit(BO, "/shared/tools/other", "x");
    assert!(
        world.told(world.ada).is_empty(),
        "a commit elsewhere tells nothing"
    );
    let snapshot = world.commit(BO, "/shared/tools/warm-pod", "duckpond-2\n");
    let words = format!(
        "Wake {id} is over: /shared/tools/warm-pod changed in files snapshot {} by account {} (\"configure\").",
        abi::hex(snapshot.digest()),
        world.bo
    );
    assert_eq!(world.told(world.ada), [words]);
    assert!(
        !world.files_watched(),
        "the last file wake over, files is heard no more"
    );
    world.commit(BO, "/shared/tools/warm-pod", "duckpond-3\n");
    assert_eq!(world.told(world.ada).len(), 1);
}

#[test]
fn a_ref_wake_is_told_when_its_ref_moves_and_forge_is_heard_only_while_one_is_open() {
    let world = world();
    let create = forge::Op::Create {
        repo: "notes".into(),
        hash: HashKind::Sha1,
    };
    world.forge_op(&create);
    let main = WaitFor::Ref {
        repo: "notes".into(),
        reference: "main".into(),
    };
    let id = world.wake_on(ADA, main).unwrap();
    assert_eq!(world.forge_watch(), forged_as());
    let moved = |reference: &str| forge::Watched::Moved {
        repo: "notes".into(),
        reference: reference.as_bytes().to_vec(),
        old: Some("a1".into()),
        new: Some("b2".into()),
        by: Some(Principal::Account(world.bo)),
    };
    world.forged(moved("refs/heads/dev"));
    assert!(
        world.told(world.ada).is_empty(),
        "another ref tells nothing"
    );
    world.forged(moved("refs/heads/main"));
    let words = format!(
        "Wake {id} is over: refs/heads/main in notes moved from a1 to b2 by account {}.",
        world.bo
    );
    assert_eq!(world.told(world.ada), [words]);
    assert!(
        world.forge_watch().is_empty(),
        "the last forge wake over, forge is heard no more"
    );
    let unknown = WaitFor::Ref {
        repo: "nowhere".into(),
        reference: "main".into(),
    };
    let refused = world.wake_on(ADA, unknown).unwrap_err();
    assert_eq!(refused.code, code::NOT_FOUND);
}

#[test]
fn change_wakes_are_told_with_harness_words_and_a_settled_wake_says_how_it_cannot_end() {
    let mut world = world();
    let create = forge::Op::Create {
        repo: "notes".into(),
        hash: HashKind::Sha1,
    };
    world.forge_op(&create);
    for branch in ["dev", "topic"] {
        let open = forge::Op::ChangeOpen {
            repo: "notes".into(),
            from: forge::Revision::Ref(format!("refs/heads/{branch}").into_bytes()),
            into: b"refs/heads/main".to_vec(),
            title: "dev".into(),
            body: String::new(),
            reviewers: Vec::new(),
        };
        world.forge_op(&open);
    }
    let change = |n| WaitFor::Change {
        repo: "notes".into(),
        n,
    };
    let source = world.wake_on(ADA, change(1)).unwrap();
    let merged = world.wake_on(ADA, change(2)).unwrap();
    let settled = WaitFor::Settled {
        repo: "notes".into(),
        n: 2,
        until: Until::Closed,
    };
    let not_closed = world.wake_on(BO, settled).unwrap();
    let pushed = forge::Watched::Moved {
        repo: "notes".into(),
        reference: b"refs/heads/dev".to_vec(),
        old: Some("a1".into()),
        new: Some("b2".into()),
        by: Some(Principal::Account(world.bo)),
    };
    world.forged(pushed);
    let merge = forge::Watched::Merged {
        repo: "notes".into(),
        n: 2,
        oid: "abc123".into(),
        by: Principal::Account(world.ada),
    };
    world.forged(merge);
    world.after(1_000);
    world.after(0);
    let (ada, bo) = (world.ada, world.bo);
    let told = [
        format!(
            "Wake {source} is over: change 1 in notes: its source refs/heads/dev moved from a1 to b2 by account {bo}."
        ),
        format!("Wake {merged} is over: change 2 in notes was merged by account {ada} as abc123."),
    ];
    assert_eq!(world.told(ada), told);
    let settled = format!(
        "Wake {not_closed} is over: change 2 in notes was merged by account {ada} as abc123, so it will not be closed."
    );
    assert_eq!(world.told(bo), [settled]);
    let close = forge::Op::ChangeClose {
        repo: "notes".into(),
        n: 1,
    };
    world.forge_op(&close);
    let closed = world.wake_on(ADA, change(1)).unwrap_err();
    assert_eq!(closed.code, code::WRONG_STATE);
    assert!(
        closed
            .message
            .contains("change 1 in notes is already closed"),
        "{closed:?}"
    );
}

#[test]
fn a_program_wake_is_refused_with_nothing_scheduled_and_told_when_the_code_lands() {
    let mut world = world();
    let fresh = || WaitFor::Program {
        program: "fresh".into(),
    };
    let nothing = world.wake_on(ADA, fresh()).unwrap_err();
    assert_eq!(nothing.code, code::WRONG_STATE);
    world.plan(Plan::Running(vec![entry("fresh", 1)]));
    world.plan(Plan::Scheduled(vec![set_at(8, entry("fresh", 2))]));
    let id = world.wake_on(ADA, fresh()).unwrap();
    world.to(8);
    assert!(world.told(world.ada).is_empty());
    world.to(9);
    let words = format!(
        "Wake {id} is over: program fresh's code changed at block 8 from {} to {}.",
        "01".repeat(32),
        "02".repeat(32)
    );
    assert_eq!(world.told(world.ada), [words]);
}

#[test]
fn a_program_wake_whose_change_was_cancelled_follows_the_next_or_says_none_is_left() {
    let mut world = world();
    world.plan(Plan::Running(vec![entry("fresh", 1)]));
    let both = vec![set_at(8, entry("fresh", 2)), set_at(12, entry("fresh", 3))];
    world.plan(Plan::Scheduled(both));
    let fresh = WaitFor::Program {
        program: "fresh".into(),
    };
    let id = world.wake_on(ADA, fresh.clone()).unwrap();
    world.plan(Plan::Scheduled(vec![set_at(12, entry("fresh", 3))]));
    world.to(9);
    assert!(
        world.told(world.ada).is_empty(),
        "the change at 8 did not land"
    );
    world.to(13);
    let words = format!(
        "Wake {id} is over: program fresh's code changed at block 12 from {} to {}.",
        "01".repeat(32),
        "03".repeat(32)
    );
    assert_eq!(world.told(world.ada), [words]);

    world.plan(Plan::Scheduled(vec![set_at(20, entry("fresh", 4))]));
    let cancelled = world.wake_on(BO, fresh).unwrap();
    world.plan(Plan::Scheduled(Vec::new()));
    world.to(21);
    let words = format!(
        "Wake {cancelled} is over: program fresh's change for block 20 did not land, and none is scheduled now."
    );
    assert_eq!(world.told(world.bo), [words]);
}

fn armed_on_pod(world: &mut World) -> u64 {
    let pod = WaitFor::Pod { name: POD.into() };
    let id = world.wake_on(ADA, pod).unwrap();
    world.after(2_000);
    world.after(0);
    id
}

#[test]
fn a_pod_wake_is_told_the_ended_line_of_its_pods_wait_stream() {
    let mut world = world();
    let id = armed_on_pod(&mut world);
    let read = world.answer(500, 200, json!({"pod": {"name": POD}}));
    assert_eq!(read, format!("/v1/pods/{POD}"));
    let stream = world.pending();
    assert_eq!(target(&stream), format!("/v1/pods/{POD}/wait?every=64"));
    world.after(1_000);
    let alive = Event {
        id: stream.id,
        seq: 0,
        data: b"event: alive\ndata: {}".to_vec(),
    };
    world.node(&Egress::Events(vec![alive]));
    assert!(
        world.told(world.ada).is_empty(),
        "an alive beat tells nothing"
    );
    let line = format!("pod {POD} ended: exit code 0 (Completed)");
    let ended = json!({"line": line, "exit_code": 0, "pod": {"name": POD}});
    let ended = Event {
        id: stream.id,
        seq: 1,
        data: format!("event: ended\ndata: {ended}").into_bytes(),
    };
    world.node(&Egress::Events(vec![ended]));
    world.end_stream(&stream, 2);
    assert_eq!(
        world.told(world.ada),
        [format!("Wake {id} is over: {line}.")]
    );
    assert!(world.open().is_empty());
}

#[test]
fn a_pod_the_service_does_not_know_ends_its_wake_saying_so() {
    let mut world = world();
    let id = armed_on_pod(&mut world);
    world.answer(500, 404, json!({"error": "no such pod"}));
    let words = format!("Wake {id} is over: the pods service knows no pod {POD}.");
    assert_eq!(world.told(world.ada), [words]);
    assert!(world.open().is_empty());
}

#[test]
fn a_pod_stream_cut_before_its_end_is_followed_again() {
    let mut world = world();
    let id = armed_on_pod(&mut world);
    world.answer(500, 200, json!({"pod": {"name": POD}}));
    let stream = world.pending();
    world.after(1_000);
    world.end_stream(&stream, 0);
    assert!(world.told(world.ada).is_empty());
    assert!(world.open().is_empty());
    world.after(1_000);
    world.after(0);
    let again = world.pending();
    assert_eq!(target(&again), format!("/v1/pods/{POD}"));
    assert!(world.row(id).unwrap().ended.is_none());
}

#[test]
fn an_outsiders_pod_wait_leaves_one_stream_however_often_it_is_cut() {
    let mut world = world();
    let id = armed_on_pod(&mut world);
    let pod = json!({"pod": {"name": POD}});
    world.answer(500, 200, pod.clone());
    assert_eq!(world.streams().len(), 1);
    world.act(BO, &Op::PodWait { id }).unwrap();
    world.after(1_000);
    world.after(0);
    let [read]: [RequestRow; 1] = world.reads().try_into().expect("the outsider's run reads");
    world.after(500);
    world.reply(&read, 200, pod.clone());
    assert_eq!(world.streams().len(), 2, "the first stream is still open");
    for _ in 0..3 {
        let streams = world.streams();
        world.after(1_000);
        for stream in &streams {
            world.end_stream(stream, 0);
        }
        world.after(1_000);
        world.after(0);
        let [read]: [RequestRow; 1] = world.reads().try_into().expect("one run reads again");
        world.after(500);
        world.reply(&read, 200, pod.clone());
        assert_eq!(
            world.streams().len(),
            1,
            "only the current stream follows on"
        );
    }
    assert!(world.row(id).unwrap().ended.is_none());
}

#[test]
fn a_thousand_wakes_on_one_ref_end_one_frame_each_and_every_one_is_told_once() {
    let mut world = world();
    let create = forge::Op::Create {
        repo: "notes".into(),
        hash: HashKind::Sha1,
    };
    world.forge_op(&create);
    let main = WaitFor::Ref {
        repo: "notes".into(),
        reference: "main".into(),
    };
    let many = 1_000;
    for _ in 0..many {
        world.wake_on(ADA, main.clone()).unwrap();
    }
    let moved = forge::Watched::Moved {
        repo: "notes".into(),
        reference: b"refs/heads/main".to_vec(),
        old: Some("a1".into()),
        new: Some("b2".into()),
        by: Some(Principal::Account(world.bo)),
    };
    world.forged(moved);
    let later = world.wake_on(BO, main).unwrap();
    let mut told = world.told(world.ada).len();
    assert_eq!(told, 1, "the event's own op ends one wake");
    let mut blocks = 0;
    while told < many {
        world.after(1_000);
        blocks += 1;
        let now = world.told(world.ada).len();
        assert!(
            now <= 2 * told + 1,
            "a block's frames each end one wake: {told} became {now}"
        );
        assert!(now > told || blocks == 1, "the fan stalled at {told}");
        told = now;
    }
    assert!(blocks <= 64, "{blocks} blocks");
    let every: std::collections::BTreeSet<String> = world.told(world.ada).into_iter().collect();
    assert_eq!(every.len(), many, "each wake is told once");
    world.after(1_000);
    world.after(0);
    assert_eq!(world.told(world.ada).len(), many);
    assert!(
        world.told(world.bo).is_empty(),
        "a wake armed after the move is not ended by it"
    );
    assert!(world.row(later).unwrap().ended.is_none());
}
