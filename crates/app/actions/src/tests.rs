// actions over forge on a MockChain: a push runs the workflows that name its branch, a change
// opened and updated runs what `on.change` names, and a check is claimed, reported and rerun.
use ::program::code;
use ::program::{Error, ExecCtx, HashKind, MockChain, Origin, Program, QueryCtx, Reply as Wanted};
use borsh::{BorshDeserialize, BorshSerialize};
use gitcore::wire::pktline;
use gitcore::{Commit, Hash, Kind, MemoryObjects, Mode, Object, Objects, Oid, Signature};
use gitcore::{Tree, TreeEntry, pack};
use store::Item;

use crate::*;

const OWNER: &[u8] = b"owner-key";
const OWNER_ACCOUNT: u64 = 11;
const STRANGER: &[u8] = b"stranger-key";
const STRANGER_ACCOUNT: u64 = 12;
const RUNNER: &str = "runner";
const RUNNER_ACCOUNT: u64 = 904;
const OFFERS: Item<Vec<u64>> = Item::new("offers");
const REFUSING: Item<bool> = Item::new("refusing");
const REPO: &str = "project";

const GATES: &str = r#"
name = "gates"

[on]
push = ["main"]

[[jobs]]
id = "gates"
runs-on = ["pods"]
image = "toolchain"
steps = ["make check"]

[jobs.env]
FEATURES = "a:b"

[[jobs.inputs]]
path = "/tools/gates.sh"
files = "/shared/tools/gates.sh"

[[jobs]]
id = "render"
runs-on = ["pods", "gpu"]
image = "toolchain"
steps = ["make render"]
"#;

const LOUD: &str = r#"
[on]
push = ["main"]

[notify]
channel = "engineering"

[[jobs]]
id = "gates"
runs-on = ["pods"]
image = "toolchain"
steps = ["make check"]
"#;

const CHANGES: &str = r#"
[on]
change = ["opened", "updated"]

[[jobs]]
id = "review"
runs-on = ["pods"]
image = "toolchain"
steps = ["make review"]
"#;

const SIXTEEN: &str = r#"
[on]
push = ["main"]

[[jobs]]
id = "gates"
runs-on = ["pods"]
image = "toolchain"
cpu = "16"
memory = "40Gi"
scratch = ["/var/tmp/target"]
steps = ["make check"]

[[jobs]]
id = "lint"
runs-on = ["pods"]
image = "toolchain"
steps = ["make lint"]
"#;

const CLAUDE: &str = r#"
[on]
change = ["opened"]

[[jobs]]
id = "review"
runs-on = ["claude"]
steps = ["Review the diff."]
"#;

#[derive(Debug, BorshSerialize, BorshDeserialize)]
enum Runs {
    Register(Vec<String>),
    Offer(Offer),
    Refuse,
}

struct Runner;

impl Program for Runner {
    const NAME: &'static str = RUNNER;
    type Op = Runs;
    type Query = ();
    type Reply = Vec<u64>;

    fn execute(ctx: &ExecCtx, op: Runs) -> Result<(), Error> {
        match op {
            Runs::Register(labels) => {
                let register = Op::Register {
                    labels,
                    deliver: vec![1],
                };
                ctx.emit(PROGRAM, &register, Wanted::None);
            }
            Runs::Refuse => REFUSING.put(ctx, &true),
            Runs::Offer(offer) => {
                ctx.env().sent_by(PROGRAM)?;
                if REFUSING.get(ctx)?.unwrap_or(false) {
                    return Err(::program::invalid("the runner is out of pods"));
                }
                let mut offers = OFFERS.get(ctx)?.unwrap_or_default();
                offers.push(offer.check);
                OFFERS.put(ctx, &offers);
            }
        }
        Ok(())
    }

    fn query(ctx: &QueryCtx, (): ()) -> Result<Vec<u64>, Error> {
        Ok(OFFERS.get(ctx)?.unwrap_or_default())
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

struct World {
    chain: MockChain,
    store: MemoryObjects,
    time: i64,
    height: std::cell::Cell<u64>,
}

fn world() -> World {
    let mut chain = MockChain::default();
    chain.seat::<forge::Forge>(forge::PROGRAM);
    chain.seat::<chat::Chat>("chat");
    chain.seat::<access::Access>("access");
    chain.seat::<Actions>(PROGRAM);
    chain.seat::<Runner>(RUNNER);
    chain.seat::<::scheduler::Scheduler>(::scheduler::PROGRAM);
    chain.hold(OWNER, OWNER_ACCOUNT);
    chain.hold(STRANGER, STRANGER_ACCOUNT);
    let programs = [
        (forge::PROGRAM, 900),
        ("chat", 901),
        ("access", 902),
        (PROGRAM, 903),
        (RUNNER, RUNNER_ACCOUNT),
    ];
    for (program, number) in programs {
        chain.register(program, number);
    }
    chain.init(forge::PROGRAM, &bounds()).unwrap();
    chain.init(PROGRAM, &()).unwrap();
    let create = forge::Op::Create {
        repo: REPO.into(),
        hash: HashKind::Sha1,
    };
    chain
        .submit(Origin::Signed(OWNER.to_vec()), forge::PROGRAM, &create)
        .unwrap();
    World {
        chain,
        store: MemoryObjects::new(Hash::Sha1),
        time: 1,
        height: std::cell::Cell::new(1),
    }
}

impl World {
    fn tree(&mut self, entries: &[(&str, Mode, Oid)]) -> Oid {
        let tree = Tree {
            entries: entries
                .iter()
                .map(|(name, mode, id)| TreeEntry {
                    mode: *mode,
                    name: name.as_bytes().to_vec(),
                    id: *id,
                })
                .collect(),
        };
        self.store.put(Kind::Tree, &tree.serialize()).unwrap()
    }

    fn commit(&mut self, parents: &[Oid], workflows: &[(&str, &str)]) -> Oid {
        self.commit_files(parents, workflows, &[])
    }

    fn commit_files(
        &mut self,
        parents: &[Oid],
        workflows: &[(&str, &str)],
        extra: &[(&str, &[u8])],
    ) -> Oid {
        self.time += 1;
        let mut files = Vec::new();
        for (name, text) in workflows {
            let blob = self.store.put(Kind::Blob, text.as_bytes()).unwrap();
            files.push((*name, Mode::Regular, blob));
        }
        let workflows = self.tree(&files);
        let forge_dir = self.tree(&[("workflows", Mode::Directory, workflows)]);
        let own = format!("t{}", self.time);
        let mark = self.store.put(Kind::Blob, b"x\n").unwrap();
        let mut entries = vec![
            (".forge".to_owned(), Mode::Directory, forge_dir),
            (own, Mode::Regular, mark),
        ];
        for (path, bytes) in extra {
            let blob = self.store.put(Kind::Blob, bytes).unwrap();
            entries.push((path.to_string(), Mode::Regular, blob));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let named: Vec<(&str, Mode, Oid)> = entries
            .iter()
            .map(|(name, mode, id)| (name.as_str(), *mode, *id))
            .collect();
        let root = self.tree(&named);
        let signature = Signature {
            name: b"Ada".to_vec(),
            email: b"ada@example.com".to_vec(),
            time: self.time,
            offset_minutes: 0,
        };
        let commit = Commit {
            tree: root,
            parents: parents.to_vec(),
            author: signature.clone(),
            committer: signature,
            extra: Vec::new(),
            message: b"step\n".to_vec(),
        };
        self.store.put(Kind::Commit, &commit.serialize()).unwrap()
    }

    fn push(&self, old: Option<Oid>, new: Oid, branch: &str) {
        let objects: Vec<Object> = self
            .store
            .ids()
            .map(|id| self.store.get(id).unwrap().unwrap())
            .collect();
        let pack = pack::write(objects, Hash::Sha1).unwrap();
        let pack = self.chain.put_blob(forge::PACK_KIND, pack).unwrap();
        let old = old.unwrap_or(Hash::Sha1.zero());
        let mut request = Vec::new();
        let line = format!("{old} {new} refs/heads/{branch}\0report-status");
        pktline::push(&mut request, line.as_bytes());
        request.extend_from_slice(pktline::flush());
        let push = forge::Op::Push {
            repo: REPO.into(),
            request,
            pack: Some(pack),
        };
        self.forge(&push).unwrap();
    }

    fn forge(&self, op: &forge::Op) -> Result<Vec<u8>, Error> {
        let owner = Origin::Signed(OWNER.to_vec());
        self.chain.submit(owner, forge::PROGRAM, op)
    }

    fn act(&self, op: &Op) -> Result<Vec<u8>, Error> {
        self.chain
            .submit(Origin::Signed(OWNER.to_vec()), PROGRAM, op)
    }

    fn stranger(&self, op: &Op) -> Result<Vec<u8>, Error> {
        self.chain
            .submit(Origin::Signed(STRANGER.to_vec()), PROGRAM, op)
    }

    fn runner(&self, labels: &[&str]) {
        let labels = labels.iter().map(|label| label.to_string()).collect();
        let anyone = Origin::Signed(OWNER.to_vec());
        self.chain
            .submit(anyone, RUNNER, &Runs::Register(labels))
            .unwrap();
    }

    fn offers(&self) -> Vec<u64> {
        self.chain.query(RUNNER, &()).unwrap()
    }

    fn checks(&self, commit: Oid) -> Vec<CheckRow> {
        let asked = Query::Checks {
            repo: REPO.into(),
            commit: commit.to_hex(),
            page: PageRequest::default(),
        };
        let Reply::Checks(page) = self.chain.query(PROGRAM, &asked).unwrap() else {
            panic!("checks answer Checks");
        };
        page.items
    }

    fn check(&self, id: u64) -> CheckRow {
        let Reply::Check(Some(check)) = self.chain.query(PROGRAM, &Query::Check { id }).unwrap()
        else {
            panic!("no check {id}");
        };
        check
    }

    fn tick(&self) {
        let height = self.height.get() + 2;
        self.height.set(height);
        self.chain.at(height, height * 1000);
    }

    fn preview(&self, n: u64) -> Oid {
        self.tick();
        let asked = forge::Query::Preview {
            repo: REPO.into(),
            n,
        };
        let reply = self.chain.query(forge::PROGRAM, &asked).unwrap();
        let forge::Reply::Preview {
            preview: Some(made),
            ..
        } = reply
        else {
            panic!("change {n} has no Preview: {reply:?}");
        };
        Oid::from_hex(Hash::Sha1, &made.merge).unwrap()
    }

    fn statuses(&self, commit: Oid) -> Vec<(String, forge::CheckState)> {
        let asked = forge::Query::Statuses {
            repo: REPO.into(),
            commit: commit.to_hex(),
            page: PageRequest::first(16),
        };
        let reply = self.chain.query(forge::PROGRAM, &asked).unwrap();
        let forge::Reply::Statuses { page, .. } = reply else {
            panic!("statuses answer Statuses: {reply:?}");
        };
        let actions = forge::Principal::Account(903);
        assert!(page.items.iter().all(|status| status.reporter == actions));
        page.items
            .into_iter()
            .map(|status| (status.context, status.state))
            .collect()
    }

    fn runs(&self, event: Event, page: PageRequest) -> PageResponse<RunRow> {
        let asked = Query::Runs {
            repo: REPO.into(),
            event,
            page,
        };
        let Reply::Runs(page) = self.chain.query(PROGRAM, &asked).unwrap() else {
            panic!("runs answer Runs");
        };
        page
    }

    fn run(&self, id: u64) -> RunRow {
        let Reply::Run(Some(run)) = self.chain.query(PROGRAM, &Query::Run { id }).unwrap() else {
            panic!("no run {id}");
        };
        run
    }
}

fn jobs(checks: &[CheckRow]) -> Vec<(&str, State)> {
    checks
        .iter()
        .map(|check| (check.job.id.as_str(), check.state))
        .collect()
}

#[test]
fn a_push_to_a_branch_a_workflow_names_queues_a_check_per_job_and_offers_it() {
    let mut world = world();
    world.runner(&["pods"]);
    let root = world.commit(&[], &[("gates.toml", GATES)]);
    world.push(None, root, "main");
    let checks = world.checks(root);
    assert_eq!(
        jobs(&checks),
        [("gates", State::Queued), ("render", State::Queued)]
    );
    let gates = &checks[0];
    assert_eq!((gates.workflow.as_str(), gates.attempt), ("gates.toml", 1));
    assert_eq!(gates.job.env, [("FEATURES".to_owned(), "a:b".to_owned())]);
    assert_eq!(gates.job.inputs[0].files, "/shared/tools/gates.sh");
    let run = world.run(gates.run);
    let pushed = Event::Push {
        reference: b"refs/heads/main".to_vec(),
    };
    assert_eq!(
        (run.event, run.name, run.error),
        (pushed, Some("gates".into()), None)
    );
    assert_eq!(run.by, Some(Principal::Account(OWNER_ACCOUNT)));
    assert_eq!(world.offers(), [gates.id], "the runner lacks the gpu label");
    let other = world.commit(&[root], &[("gates.toml", GATES)]);
    world.push(None, other, "other");
    assert!(world.checks(other).is_empty(), "on.push names main alone");
}

#[test]
fn a_change_runs_what_on_change_names_on_its_preview_each_time_either_head_moves() {
    let mut world = world();
    let root = world.commit(&[], &[("changes.toml", CHANGES)]);
    world.push(None, root, "main");
    assert!(world.checks(root).is_empty());
    let feature = world.commit(&[root], &[("changes.toml", CHANGES)]);
    world.push(None, feature, "feature");
    let open = forge::Op::ChangeOpen {
        repo: REPO.into(),
        from: forge::Revision::Ref(b"refs/heads/feature".to_vec()),
        into: b"refs/heads/main".to_vec(),
        title: "Feature".into(),
        body: String::new(),
        reviewers: Vec::new(),
    };
    world.forge(&open).unwrap();
    assert!(
        world.checks(feature).is_empty(),
        "checks run on the Preview, not the head"
    );
    let first = world.preview(1);
    let opened = world.checks(first);
    assert_eq!(jobs(&opened), [("review", State::Queued)]);
    assert_eq!(world.run(opened[0].run).event, Event::Preview { n: 1 });
    let pending = ("changes.toml#review".to_owned(), forge::CheckState::Pending);
    assert_eq!(world.statuses(first), [pending]);
    let later = world.commit(&[feature], &[("changes.toml", CHANGES)]);
    world.push(Some(feature), later, "feature");
    let second = world.preview(1);
    assert_ne!(second, first, "the source moved");
    assert_eq!(jobs(&world.checks(second)), [("review", State::Queued)]);
    let moved = world.commit(&[root], &[("changes.toml", CHANGES)]);
    world.push(Some(root), moved, "main");
    let third = world.preview(1);
    assert_ne!(third, second, "the target moved");
    assert_eq!(jobs(&world.checks(third)), [("review", State::Queued)]);
    let close = forge::Op::ChangeClose {
        repo: REPO.into(),
        n: 1,
    };
    world.forge(&close).unwrap();
    let last = world.commit(&[later], &[("changes.toml", CHANGES)]);
    world.push(Some(later), last, "feature");
    assert_eq!(world.preview(1), third, "the change is closed");
}

#[test]
fn a_workflow_that_does_not_read_makes_a_run_that_says_why() {
    let mut world = world();
    let broken = "[on]\npush = [\"main\"]\nwhen = 1\n";
    let root = world.commit(&[], &[("broken.toml", broken), ("gates.toml", GATES)]);
    world.push(None, root, "main");
    let run = world.run(1);
    assert_eq!(run.workflow, "broken.toml");
    assert!(run.error.unwrap().contains("when"));
    assert_eq!(world.run(2).error, None);
    assert_eq!(world.checks(root).len(), 2, "gates.toml still runs");
}

#[test]
fn a_check_is_claimed_once_reported_once_and_rerun_as_its_next_attempt() {
    let mut world = world();
    world.runner(&["pods"]);
    let root = world.commit(&[], &[("gates.toml", GATES)]);
    world.push(None, root, "main");
    let report = |state| Op::Report {
        check: 1,
        state,
        summary: "exit code 0".into(),
        log: Some("/shared/actions/log".into()),
    };
    let unclaimed = world.act(&report(State::Passed)).unwrap_err();
    assert_eq!(unclaimed.code, code::WRONG_STATE);
    world.act(&Op::Claim { check: 1 }).unwrap();
    let twice = world.act(&Op::Claim { check: 1 }).unwrap_err();
    assert_eq!(twice.code, code::WRONG_STATE);
    assert_eq!(world.check(1).state, State::Running);
    let unended = world.act(&report(State::Running)).unwrap_err();
    assert_eq!(unended.code, code::INVALID_INPUT);
    world.act(&report(State::Passed)).unwrap();
    let told = [
        ("gates.toml#gates".to_owned(), forge::CheckState::Passed),
        ("gates.toml#render".to_owned(), forge::CheckState::Pending),
    ];
    assert_eq!(world.statuses(root), told);
    let passed = world.check(1);
    assert_eq!(passed.state, State::Passed);
    assert_eq!(
        (passed.claimant, passed.reporter),
        (Some(OWNER_ACCOUNT), Some(OWNER_ACCOUNT))
    );
    assert_eq!(
        world.act(&report(State::Failed)).unwrap_err().code,
        code::WRONG_STATE
    );
    world.act(&Op::Rerun { check: 1 }).unwrap();
    let again = world.check(3);
    assert_eq!(
        (again.job.id.as_str(), again.attempt, again.state),
        ("gates", 2, State::Queued)
    );
    assert_eq!(again.run, passed.run);
    assert_eq!(world.offers(), [1, 3]);
    let queued = world.act(&Op::Rerun { check: 3 }).unwrap_err();
    assert_eq!(queued.code, code::WRONG_STATE);
}

#[test]
fn a_job_that_states_what_its_pod_asks_keeps_it_for_every_attempt_and_a_bad_quantity_says_why() {
    let mut world = world();
    let root = world.commit(&[], &[("sixteen.toml", SIXTEEN)]);
    world.push(None, root, "main");
    let asks = |world: &World, check: u64| {
        let Reply::Asks(asks) = world.chain.query(PROGRAM, &Query::Asks { check }).unwrap() else {
            panic!("asks answers Asks");
        };
        asks
    };
    let sixteen = Asks {
        cpu: Some("16".into()),
        memory: Some("40Gi".into()),
        scratch: vec!["/var/tmp/target".into()],
    };
    assert_eq!(asks(&world, 1), Some(sixteen.clone()));
    assert_eq!(asks(&world, 2), None, "lint states nothing");
    world.act(&Op::Claim { check: 1 }).unwrap();
    world.act(&Op::Rerun { check: 1 }).unwrap();
    assert_eq!(world.check(3).attempt, 2);
    assert_eq!(asks(&world, 3), Some(sixteen), "a rerun asks the same");
    let lots = SIXTEEN.replace("memory = \"40Gi\"", "memory = \"lots\"");
    let other = world.commit(&[root], &[("sixteen.toml", lots.as_str())]);
    world.push(Some(root), other, "main");
    let error = world.run(2).error.unwrap();
    assert!(
        error.contains("memory takes a Kubernetes quantity"),
        "{error}"
    );
}

#[test]
fn a_trigger_runs_every_workflow_at_a_commit() {
    let mut world = world();
    let root = world.commit(&[], &[("changes.toml", CHANGES)]);
    world.push(None, root, "main");
    world
        .act(&Op::Trigger {
            repo: REPO.into(),
            commit: root.to_hex(),
        })
        .unwrap();
    let checks = world.checks(root);
    assert_eq!(jobs(&checks), [("review", State::Queued)]);
    assert_eq!(world.run(checks[0].run).event, Event::Manual);
}

// A branch's runs list a page at a time, oldest first, apart from the run a trigger made at the
// same commit and from another branch's.
#[test]
fn runs_list_by_event_a_page_at_a_time_oldest_first() {
    let mut world = world();
    let root = world.commit(&[], &[("gates.toml", GATES)]);
    world.push(None, root, "main");
    let trigger = Op::Trigger {
        repo: REPO.into(),
        commit: root.to_hex(),
    };
    world.act(&trigger).unwrap();
    let next = world.commit(&[root], &[("gates.toml", GATES)]);
    world.push(Some(root), next, "main");
    let ids =
        |page: &PageResponse<RunRow>| -> Vec<u64> { page.items.iter().map(|run| run.id).collect() };
    let main = Event::Push {
        reference: b"refs/heads/main".to_vec(),
    };
    let first = world.runs(main.clone(), PageRequest::first(1));
    assert_eq!(ids(&first), [1]);
    let second = world.runs(main, PageRequest::resume(first.next, 1));
    assert_eq!((ids(&second), second.next), (vec![3], None));
    assert_eq!(second.items[0].commit, next.to_hex());
    assert_eq!(ids(&world.runs(Event::Manual, PageRequest::default())), [2]);
    let other = Event::Push {
        reference: b"refs/heads/other".to_vec(),
    };
    assert!(world.runs(other, PageRequest::default()).items.is_empty());
}

#[test]
fn a_runner_that_registers_late_hears_what_is_queued_and_only_forge_says_what_moved() {
    let mut world = world();
    let root = world.commit(&[], &[("gates.toml", GATES)]);
    world.push(None, root, "main");
    world.runner(&["pods", "gpu"]);
    assert_eq!(world.offers(), [1, 2]);
    let moved = forge::Watched::Moved {
        repo: REPO.into(),
        reference: b"refs/heads/main".to_vec(),
        old: None,
        new: Some(root.to_hex()),
        by: None,
    };
    let forged = world.act(&Op::Watched(moved.clone())).unwrap_err();
    assert_eq!(forged.code, code::UNAUTHORIZED);
    assert_eq!(abi::encode(&Op::Watched(moved))[..1], watching());
}

#[test]
fn a_push_whose_check_ends_red_says_so_in_the_workflow_channel() {
    let mut world = world();
    let create = chat::Op::CreateChannel {
        channel_id: "engineering".into(),
        name: "engineering".into(),
        post_policy: chat::PostPolicy::Open,
    };
    let owner = Origin::Signed(OWNER.to_vec());
    world.chain.submit(owner, chat::PROGRAM, &create).unwrap();
    let root = world.commit(&[], &[("loud.toml", LOUD)]);
    world.push(None, root, "main");
    assert_eq!(world.run(1).notify, Some("engineering".into()));
    let report = |check, state| Op::Report {
        check,
        state,
        summary: "GATES FAIL: clippy".into(),
        log: Some("/shared/actions/log".into()),
    };
    world.act(&Op::Claim { check: 1 }).unwrap();
    world.act(&report(1, State::Failed)).unwrap();
    let told = chat::Query::MessageById {
        message_id: "actions-check-1".into(),
    };
    let chat::Reply::Message(Some(row)) = world.chain.query(chat::PROGRAM, &told).unwrap() else {
        panic!("the red check is told in engineering");
    };
    let marks: Vec<&chat::Mark> = row
        .blocks
        .iter()
        .flat_map(spans)
        .flat_map(|span| &span.marks)
        .collect();
    assert!(marks.contains(&&chat::Mark::Mention(Principal::Account(OWNER_ACCOUNT))));
    let text = chat::plain_text(&row.blocks);
    assert!(text.contains("failed on project main"), "{text}");
    assert!(text.contains("GATES FAIL: clippy"), "{text}");
    world.act(&Op::Rerun { check: 1 }).unwrap();
    world.act(&Op::Claim { check: 2 }).unwrap();
    world.act(&report(2, State::Passed)).unwrap();
    let quiet = chat::Query::MessageById {
        message_id: "actions-check-2".into(),
    };
    let reply: chat::Reply = world.chain.query(chat::PROGRAM, &quiet).unwrap();
    assert_eq!(
        reply,
        chat::Reply::Message(None),
        "a green check says nothing"
    );
}

#[test]
fn a_channel_that_refuses_the_line_leaves_the_report_standing() {
    let mut world = world();
    let root = world.commit(&[], &[("loud.toml", LOUD)]);
    world.push(None, root, "main");
    world.act(&Op::Claim { check: 1 }).unwrap();
    let errored = Op::Report {
        check: 1,
        state: State::Errored,
        summary: "the pods service answered 502".into(),
        log: None,
    };
    world.act(&errored).unwrap();
    assert_eq!(
        world.check(1).state,
        State::Errored,
        "no channel engineering exists"
    );
}

fn spans(block: &chat::Block) -> &[chat::Span] {
    match block {
        chat::Block::Paragraph(spans) | chat::Block::Quote(spans) => spans,
        _ => &[],
    }
}

#[test]
fn the_programs_gates_workflow_reads() {
    let gates = workflow::parse(include_str!("../../../../.forge/workflows/gates.toml")).unwrap();
    let dev = Event::Push {
        reference: b"refs/heads/dev".to_vec(),
    };
    assert!(gates.starts_on(&dev) && gates.starts_on(&Event::Updated { n: 1 }));
    assert!(gates.notify.is_some());
    let workflow::Planned { job, asks, .. } = &gates.jobs[0];
    assert_eq!(asks.cpu.as_deref(), Some("16"));
    assert!(asks.memory.is_some() && asks.scratch == ["/var/tmp/target"]);
    let steps = &job.steps;
    assert!(steps[1].contains("/forge/tools"), "{steps:?}");
    assert_eq!(steps[2], "sh /var/tmp/tools/gates.sh /work/programs");
    let env = |key: &str| {
        job.env
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(
        (env("GATES"), env("AGENTS_LINT")),
        (Some("check"), Some("1"))
    );
    assert!(
        job.inputs.is_empty(),
        "/shared/tools holds no scripts (J-062)"
    );
}

#[test]
fn a_change_that_does_not_preview_makes_an_errored_run_that_says_why() {
    let mut world = world();
    let changes = [("changes.toml", CHANGES)];
    let root = world.commit_files(&[], &changes, &[("a", b"1\n")]);
    world.push(None, root, "main");
    let feature = world.commit_files(&[root], &changes, &[("a", b"2\n")]);
    world.push(None, feature, "feature");
    let moved = world.commit_files(&[root], &changes, &[("a", b"3\n")]);
    world.push(Some(root), moved, "main");
    let open = forge::Op::ChangeOpen {
        repo: REPO.into(),
        from: forge::Revision::Ref(b"refs/heads/feature".to_vec()),
        into: b"refs/heads/main".to_vec(),
        title: "Feature".into(),
        body: String::new(),
        reviewers: Vec::new(),
    };
    world.forge(&open).unwrap();
    world.tick();
    let run = world.run(1);
    assert_eq!(run.event, Event::Preview { n: 1 });
    let ran = (run.commit.as_str(), run.workflow.as_str());
    assert_eq!(ran, (feature.to_hex().as_str(), "refs/changes/1/merge"));
    let error = run.error.unwrap();
    assert!(error.starts_with("change 1 does not preview: "), "{error}");
    assert!(world.checks(feature).is_empty());
}

#[test]
fn an_account_registers_as_a_runner_hears_no_offer_and_claims_and_reports_what_it_finds_queued() {
    let mut world = world();
    let root = world.commit(&[], &[("changes.toml", CLAUDE)]);
    world.push(None, root, "main");
    world
        .act(&Op::Register {
            labels: vec!["claude".into()],
            deliver: Vec::new(),
        })
        .unwrap();
    let Reply::Runners(runners) = world
        .chain
        .query(
            PROGRAM,
            &Query::Runners {
                page: PageRequest::default(),
            },
        )
        .unwrap()
    else {
        panic!("runners answer Runners");
    };
    let runner = RunnerRow {
        account: OWNER_ACCOUNT,
        labels: vec!["claude".into()],
        program: None,
        deliver: Vec::new(),
    };
    assert_eq!(runners.items, [runner]);
    world
        .act(&Op::Trigger {
            repo: REPO.into(),
            commit: root.to_hex(),
        })
        .unwrap();
    let Reply::Queued(queued) = world
        .chain
        .query(
            PROGRAM,
            &Query::Queued {
                page: PageRequest::default(),
            },
        )
        .unwrap()
    else {
        panic!("queued answers Queued");
    };
    let found: Vec<(u64, &[String])> = queued
        .items
        .iter()
        .map(|check| (check.id, check.job.runs_on.as_slice()))
        .collect();
    assert_eq!(found, [(1, ["claude".to_owned()].as_slice())]);
    assert!(world.offers().is_empty(), "an account is sent no offer");
    world.act(&Op::Claim { check: 1 }).unwrap();
    let report = Op::Report {
        check: 1,
        state: State::Failed,
        summary: "1 finding".into(),
        log: Some("/shared/actions/project/log".into()),
    };
    world.act(&report).unwrap();
    let check = world.check(1);
    assert_eq!(
        (check.state, check.claimant, check.reporter),
        (State::Failed, Some(OWNER_ACCOUNT), Some(OWNER_ACCOUNT))
    );
    let told = ("changes.toml#review".to_owned(), forge::CheckState::Failed);
    assert_eq!(world.statuses(root), [told]);
}

#[test]
fn a_change_opened_before_actions_watched_joins_on_its_first_preview() {
    let mut world = world();
    let root = world.commit(&[], &[("changes.toml", CHANGES)]);
    world.push(None, root, "main");
    let feature = world.commit(&[root], &[("changes.toml", CHANGES)]);
    world.push(None, feature, "feature");
    let open = forge::Op::ChangeOpen {
        repo: REPO.into(),
        from: forge::Revision::Ref(b"refs/heads/feature".to_vec()),
        into: b"refs/heads/main".to_vec(),
        title: "Feature".into(),
        body: String::new(),
        reviewers: Vec::new(),
    };
    world.forge(&open).unwrap();
    world.tick();
    let actions = world.chain.host(PROGRAM);
    let ctx = actions.exec(::program::MockHost::env(PROGRAM));
    crate::state::SOURCES.remove(&ctx, &(REPO.to_owned(), b"refs/heads/feature".to_vec()));
    crate::state::TARGETS.remove(&ctx, &(REPO.to_owned(), b"refs/heads/main".to_vec(), 1));
    drop(ctx);
    let previewed = forge::Op::Preview {
        repo: REPO.into(),
        n: 1,
    };
    let moved = world.commit(&[root], &[("changes.toml", CHANGES)]);
    world.push(Some(root), moved, "main");
    world.forge(&previewed).unwrap();
    let first = world.preview(1);
    let later = world.commit(&[feature], &[("changes.toml", CHANGES)]);
    world.push(Some(feature), later, "feature");
    assert_ne!(
        world.preview(1),
        first,
        "the adopted change previews again as its source moves"
    );
}

#[test]
fn a_runner_that_refuses_its_offer_errs_the_check_saying_why() {
    let mut world = world();
    world.runner(&["pods"]);
    let anyone = Origin::Signed(OWNER.to_vec());
    world.chain.submit(anyone, RUNNER, &Runs::Refuse).unwrap();
    let root = world.commit(&[], &[("gates.toml", GATES)]);
    world.push(None, root, "main");
    let checks = world.checks(root);
    assert_eq!(
        jobs(&checks),
        [("gates", State::Errored), ("render", State::Queued)],
        "render needs gpu, which no runner offers"
    );
    assert_eq!(
        checks[0].summary,
        "runner refused the offer: the runner is out of pods"
    );
    let told = world.statuses(root);
    assert!(
        told.contains(&("gates.toml#gates".to_owned(), forge::CheckState::Errored)),
        "{told:?}"
    );
}

#[test]
fn a_queued_check_ends_errored_unclaimed_but_passes_only_once_claimed() {
    let mut world = world();
    let root = world.commit(&[], &[("gates.toml", GATES)]);
    world.push(None, root, "main");
    let report = |state| Op::Report {
        check: 2,
        state,
        summary: "the claim was refused: messages nest deeper than 8".into(),
        log: None,
    };
    assert_eq!(
        world.act(&report(State::Passed)).unwrap_err().code,
        code::WRONG_STATE
    );
    world.act(&report(State::Errored)).unwrap();
    let errored = world.check(2);
    assert_eq!((errored.state, errored.claimant), (State::Errored, None));
    assert_eq!(
        world.act(&Op::Claim { check: 2 }).unwrap_err().code,
        code::WRONG_STATE
    );
}

const PAIR: &str = r#"
[on]
change = ["opened", "updated"]

[[jobs]]
id = "review"
runs-on = ["pods"]
image = "toolchain"
steps = ["make review"]

[[jobs]]
id = "lint"
runs-on = ["pods"]
image = "toolchain"
steps = ["make lint"]
"#;

fn ends(checks: &[CheckRow]) -> Vec<(State, &str)> {
    checks
        .iter()
        .map(|check| (check.state, check.summary.as_str()))
        .collect()
}

fn paired(world: &mut World) -> (Oid, Oid) {
    let root = world.commit(&[], &[("pair.toml", PAIR)]);
    world.push(None, root, "main");
    let feature = world.commit(&[root], &[("pair.toml", PAIR)]);
    world.push(None, feature, "feature");
    let open = forge::Op::ChangeOpen {
        repo: REPO.into(),
        from: forge::Revision::Ref(b"refs/heads/feature".to_vec()),
        into: b"refs/heads/main".to_vec(),
        title: "Feature".into(),
        body: String::new(),
        reviewers: Vec::new(),
    };
    world.forge(&open).unwrap();
    (root, feature)
}

#[test]
fn a_newer_preview_and_then_the_close_supersede_the_changes_open_checks() {
    let mut world = world();
    let (_, feature) = paired(&mut world);
    let first = world.preview(1);
    world.act(&Op::Claim { check: 1 }).unwrap();
    let later = world.commit(&[feature], &[("pair.toml", PAIR)]);
    world.push(Some(feature), later, "feature");
    let second = world.preview(1);
    let why = format!("superseded by Preview {} of #1", &second.to_hex()[..12]);
    assert_eq!(
        ends(&world.checks(first)),
        [(State::Superseded, why.as_str()); 2]
    );
    let errored = forge::CheckState::Errored;
    let told = world.statuses(first);
    assert!(told.iter().all(|(_, state)| *state == errored), "{told:?}");
    assert_eq!(ends(&world.checks(second)), [(State::Queued, ""); 2]);
    let close = forge::Op::ChangeClose {
        repo: REPO.into(),
        n: 1,
    };
    world.forge(&close).unwrap();
    assert_eq!(
        ends(&world.checks(second)),
        [(State::Superseded, "change #1 closed"); 2]
    );
}

#[test]
fn a_change_that_merges_supersedes_its_open_checks() {
    let mut world = world();
    let (root, feature) = paired(&mut world);
    let preview = world.preview(1);
    let merge = forge::Op::SquashMerge {
        repo: REPO.into(),
        into: b"refs/heads/main".to_vec(),
        from: forge::Revision::Ref(b"refs/heads/feature".to_vec()),
        expected_into: root.to_hex(),
        expected_from: feature.to_hex(),
        message: None,
        change: Some(1),
    };
    world.forge(&merge).unwrap();
    assert_eq!(
        ends(&world.checks(preview)),
        [(State::Superseded, "change #1 merged"); 2]
    );
}

#[test]
fn a_newer_push_to_the_branch_supersedes_its_open_checks_and_says_nothing_in_the_channel() {
    let mut world = world();
    let create = chat::Op::CreateChannel {
        channel_id: "engineering".into(),
        name: "engineering".into(),
        post_policy: chat::PostPolicy::Open,
    };
    let owner = Origin::Signed(OWNER.to_vec());
    world.chain.submit(owner, chat::PROGRAM, &create).unwrap();
    let root = world.commit(&[], &[("loud.toml", LOUD)]);
    world.push(None, root, "main");
    world.act(&Op::Claim { check: 1 }).unwrap();
    let other = world.commit(&[root], &[("loud.toml", LOUD)]);
    world.push(None, other, "other");
    assert_eq!(world.check(1).state, State::Running, "another branch moved");
    let next = world.commit(&[root], &[("loud.toml", LOUD)]);
    world.push(Some(root), next, "main");
    let check = world.check(1);
    let why = format!("superseded by a push of {} to main", &next.to_hex()[..12]);
    assert_eq!((check.state, check.summary), (State::Superseded, why));
    let told = ("loud.toml#gates".to_owned(), forge::CheckState::Errored);
    assert_eq!(world.statuses(root), [told]);
    let quiet = chat::Query::MessageById {
        message_id: "actions-check-1".into(),
    };
    let reply: chat::Reply = world.chain.query(chat::PROGRAM, &quiet).unwrap();
    assert_eq!(reply, chat::Reply::Message(None), "superseded is not red");
    let by_hand = Op::Report {
        check: 2,
        state: State::Superseded,
        summary: "swept".into(),
        log: None,
    };
    world.act(&by_hand).unwrap();
    assert_eq!(world.check(2).state, State::Superseded);
}

// A release: a publish that needs two builds, named before them.
const RELEASE: &str = r#"
[on]
push = ["main"]

[[jobs]]
id = "publish"
runs-on = ["pods"]
image = "toolchain"
needs = ["linux", "mac"]
steps = ["make publish"]

[[jobs]]
id = "linux"
runs-on = ["pods"]
image = "toolchain"
steps = ["make linux"]

[[jobs]]
id = "mac"
runs-on = ["pods"]
image = "toolchain"
steps = ["make mac"]
"#;

// Three jobs in a line, each needing the one before.
const LINE: &str = r#"
[on]
push = ["main"]

[[jobs]]
id = "build"
runs-on = ["pods"]
image = "toolchain"
steps = ["make build"]

[[jobs]]
id = "sign"
runs-on = ["pods"]
image = "toolchain"
needs = ["build"]
steps = ["make sign"]

[[jobs]]
id = "publish"
runs-on = ["pods"]
image = "toolchain"
needs = ["sign"]
steps = ["make publish"]
"#;

fn end(world: &World, check: u64, state: State) {
    world.act(&Op::Claim { check }).unwrap();
    let report = Op::Report {
        check,
        state,
        summary: String::new(),
        log: None,
    };
    world.act(&report).unwrap();
}

fn needs(world: &World, check: u64) -> Vec<Need> {
    let Reply::Needs(needs) = world.chain.query(PROGRAM, &Query::Needs { check }).unwrap() else {
        panic!("needs answers Needs");
    };
    needs
}

#[test]
fn a_job_waits_until_every_job_it_needs_has_passed_and_only_then_is_offered() {
    let mut world = world();
    world.runner(&["pods"]);
    let root = world.commit(&[], &[("release.toml", RELEASE)]);
    world.push(None, root, "main");
    let checks = world.checks(root);
    assert_eq!(
        jobs(&checks),
        [
            ("publish", State::Waiting),
            ("linux", State::Queued),
            ("mac", State::Queued)
        ]
    );
    assert_eq!(
        world.offers(),
        [2, 3],
        "no runner is offered a waiting check"
    );
    let pending = (
        "release.toml#publish".to_owned(),
        forge::CheckState::Pending,
    );
    assert!(world.statuses(root).contains(&pending));
    let early = world.act(&Op::Claim { check: 1 }).unwrap_err();
    assert_eq!(early.code, code::WRONG_STATE);
    let unmet = |job: &str| Need {
        job: job.into(),
        check: None,
    };
    assert_eq!(needs(&world, 1), [unmet("linux"), unmet("mac")]);
    end(&world, 2, State::Passed);
    assert_eq!(world.check(1).state, State::Waiting, "mac has not passed");
    end(&world, 3, State::Passed);
    assert_eq!(world.check(1).state, State::Queued);
    assert_eq!(world.offers(), [2, 3, 1]);
    let met = |job: &str, check| Need {
        job: job.into(),
        check: Some(check),
    };
    assert_eq!(needs(&world, 1), [met("linux", 2), met("mac", 3)]);
}

#[test]
fn a_need_that_fails_skips_all_that_wait_on_it_and_a_rerun_waits_on_its_newest_attempt() {
    let mut world = world();
    let root = world.commit(&[], &[("line.toml", LINE)]);
    world.push(None, root, "main");
    end(&world, 1, State::Failed);
    let checks = world.checks(root);
    assert_eq!(
        jobs(&checks),
        [
            ("build", State::Failed),
            ("sign", State::Skipped),
            ("publish", State::Skipped)
        ]
    );
    assert_eq!(
        checks[1].summary,
        "needs `build`, whose check 1 ended failed"
    );
    assert_eq!(
        checks[2].summary,
        "needs `sign`, whose check 2 ended skipped"
    );
    let skipped = ("line.toml#publish".to_owned(), forge::CheckState::Errored);
    assert!(world.statuses(root).contains(&skipped));
    world.act(&Op::Rerun { check: 1 }).unwrap();
    world.act(&Op::Rerun { check: 2 }).unwrap();
    assert_eq!(world.check(4).state, State::Queued);
    assert_eq!(
        world.check(5).state,
        State::Waiting,
        "build's newest attempt is queued"
    );
    let waiting = world.act(&Op::Rerun { check: 5 }).unwrap_err();
    assert_eq!(waiting.code, code::WRONG_STATE);
    end(&world, 4, State::Passed);
    assert_eq!(world.check(5).state, State::Queued);
}

#[test]
fn a_waiting_check_is_superseded_with_its_run_and_a_need_that_cannot_be_met_says_why() {
    let mut world = world();
    let root = world.commit(&[], &[("line.toml", LINE)]);
    world.push(None, root, "main");
    let next = world.commit(&[root], &[("line.toml", LINE)]);
    world.push(Some(root), next, "main");
    let states: Vec<State> = world.checks(root).iter().map(|check| check.state).collect();
    assert_eq!(states, [State::Superseded; 3]);
    let needs = "needs = [\"build\"]";
    let cases = [
        (
            "needs = [\"lint\"]",
            "job \"sign\" needs \"lint\", which is no job of this file",
        ),
        ("needs = [\"sign\"]", "job \"sign\" needs itself"),
        (
            "needs = [\"publish\"]",
            "jobs sign, publish need each other in a loop",
        ),
    ];
    let mut parent = next;
    for (run, (need, why)) in (3..).zip(cases) {
        let text = LINE.replace(needs, need);
        let commit = world.commit(&[parent], &[("line.toml", text.as_str())]);
        world.push(Some(parent), commit, "main");
        parent = commit;
        assert!(world.checks(commit).is_empty());
        assert_eq!(world.run(run).error.as_deref(), Some(why));
    }
}

#[test]
fn a_running_check_records_its_outputs_by_blob_and_who_said_so() {
    let mut world = world();
    let root = world.commit(&[], &[("release.toml", RELEASE)]);
    world.push(None, root, "main");
    let blob = world.chain.put_blob("chunk", b"tarball".to_vec()).unwrap();
    let files = vec![Output {
        path: "ducktape-app.tar.gz".into(),
        blob,
        size: 7,
    }];
    let said = Op::Outputs {
        check: 2,
        files: files.clone(),
    };
    assert_eq!(world.act(&said).unwrap_err().code, code::WRONG_STATE);
    world.act(&Op::Claim { check: 2 }).unwrap();
    world.act(&said).unwrap();
    let asked = Query::Outputs { check: 2 };
    let Reply::Outputs(Some(row)) = world.chain.query(PROGRAM, &asked).unwrap() else {
        panic!("outputs answers Outputs");
    };
    assert_eq!(
        row,
        OutputsRow {
            by: OWNER_ACCOUNT,
            files
        }
    );
    let Reply::Outputs(none) = world
        .chain
        .query(PROGRAM, &Query::Outputs { check: 3 })
        .unwrap()
    else {
        panic!("outputs answers Outputs");
    };
    assert_eq!(none, None);
}

// A running check's report and outputs are its claimant's alone, so no other account marks it
// passed or swaps what it built; a queued check any account may still end errored or superseded,
// as claiming it first would let it.
#[test]
fn only_the_account_that_claimed_a_check_reports_it_or_records_its_outputs() {
    let mut world = world();
    let root = world.commit(&[], &[("gates.toml", GATES)]);
    world.push(None, root, "main");
    world.act(&Op::Claim { check: 1 }).unwrap();
    let passed = Op::Report {
        check: 1,
        state: State::Passed,
        summary: "exit code 0".into(),
        log: None,
    };
    let forged = world.stranger(&passed).unwrap_err();
    assert_eq!(forged.code, code::UNAUTHORIZED);
    let blob = world.chain.put_blob("chunk", b"forged".to_vec()).unwrap();
    let swapped = Op::Outputs {
        check: 1,
        files: vec![Output {
            path: "app.tar.gz".into(),
            blob,
            size: 6,
        }],
    };
    assert_eq!(
        world.stranger(&swapped).unwrap_err().code,
        code::UNAUTHORIZED
    );
    let Reply::Outputs(none) = world
        .chain
        .query(PROGRAM, &Query::Outputs { check: 1 })
        .unwrap()
    else {
        panic!("outputs answers Outputs");
    };
    assert_eq!(none, None, "the stranger's outputs were not kept");
    assert_eq!(world.check(1).state, State::Running);
    world.act(&swapped).unwrap();
    world.act(&passed).unwrap();
    let check = world.check(1);
    assert_eq!(
        (check.state, check.reporter),
        (State::Passed, Some(OWNER_ACCOUNT))
    );
    let errored = Op::Report {
        check: 2,
        state: State::Errored,
        summary: "no runner took it".into(),
        log: None,
    };
    world.stranger(&errored).unwrap();
    let check = world.check(2);
    assert_eq!(
        (check.state, check.reporter),
        (State::Errored, Some(STRANGER_ACCOUNT))
    );
}

// A job that names its runners: only they claim it or end it unclaimed, so a stranger can neither
// run it nor end it; a rerun keeps the names, and a job that names none is open as before.
const NAMED_JOBS: &str = r#"
[on]
push = ["main"]

[[jobs]]
id = "gates"
runs-on = ["pods"]
image = "toolchain"
runners = [11]
steps = ["make check"]

[[jobs]]
id = "lint"
runs-on = ["pods"]
image = "toolchain"
steps = ["make lint"]
"#;

#[test]
fn a_job_that_names_its_runners_is_claimed_reported_and_given_outputs_by_them_alone() {
    let mut world = world();
    world.runner(&["pods"]);
    let root = world.commit(&[], &[("named.toml", NAMED_JOBS)]);
    world.push(None, root, "main");
    assert_eq!(
        world.offers(),
        [2],
        "the program runner is not named, so hears only lint"
    );
    let claimed = world.stranger(&Op::Claim { check: 1 }).unwrap_err();
    assert_eq!(claimed.code, code::UNAUTHORIZED);
    let errored = |check| Op::Report {
        check,
        state: State::Errored,
        summary: "no runner".into(),
        log: None,
    };
    assert_eq!(
        world.stranger(&errored(1)).unwrap_err().code,
        code::UNAUTHORIZED
    );
    assert_eq!(world.check(1).state, State::Queued);
    world.act(&Op::Claim { check: 1 }).unwrap();
    let passed = Op::Report {
        check: 1,
        state: State::Passed,
        summary: String::new(),
        log: None,
    };
    let blob = world.chain.put_blob("chunk", b"built".to_vec()).unwrap();
    let outputs = Op::Outputs {
        check: 1,
        files: vec![Output {
            path: "built".into(),
            blob,
            size: 5,
        }],
    };
    assert_eq!(
        world.stranger(&outputs).unwrap_err().code,
        code::UNAUTHORIZED
    );
    assert_eq!(
        world.stranger(&passed).unwrap_err().code,
        code::UNAUTHORIZED
    );
    world.act(&outputs).unwrap();
    world.act(&passed).unwrap();
    world.stranger(&Op::Rerun { check: 1 }).unwrap();
    let again = world.check(3);
    assert_eq!((again.job.id.as_str(), again.attempt), ("gates", 2));
    assert_eq!(
        world.stranger(&Op::Claim { check: 3 }).unwrap_err().code,
        code::UNAUTHORIZED,
        "a rerun keeps the names"
    );
    world.stranger(&Op::Claim { check: 2 }).unwrap();
    let lint = Op::Report {
        check: 2,
        state: State::Passed,
        summary: String::new(),
        log: None,
    };
    world.stranger(&lint).unwrap();
    assert_eq!(world.check(2).reporter, Some(STRANGER_ACCOUNT));
}

#[test]
fn a_job_that_names_no_runner_in_its_list_is_an_errored_run() {
    let mut world = world();
    let text = NAMED_JOBS.replace("runners = [11]", "runners = []");
    let root = world.commit(&[], &[("named.toml", text.as_str())]);
    world.push(None, root, "main");
    assert!(world.checks(root).is_empty());
    let why = "job \"gates\": runners names at least one account; leave it out for any runner";
    assert_eq!(world.run(1).error.as_deref(), Some(why));
}

// A rerun of a need that passes brings back what its failure skipped, one job at a time down the
// line, each as its next attempt.
#[test]
fn a_rerun_that_passes_brings_back_the_jobs_its_failure_skipped() {
    let mut world = world();
    let root = world.commit(&[], &[("line.toml", LINE)]);
    world.push(None, root, "main");
    end(&world, 1, State::Failed);
    world.act(&Op::Rerun { check: 1 }).unwrap();
    end(&world, 4, State::Passed);
    let sign = world.check(5);
    assert_eq!(
        (sign.job.id.as_str(), sign.attempt, sign.state),
        ("sign", 2, State::Queued)
    );
    let met = Need {
        job: "build".into(),
        check: Some(4),
    };
    assert_eq!(needs(&world, 5), [met]);
    assert_eq!(
        world.check(3).state,
        State::Skipped,
        "publish waits for sign to pass"
    );
    end(&world, 5, State::Passed);
    let publish = world.check(6);
    assert_eq!(
        (publish.job.id.as_str(), publish.attempt, publish.state),
        ("publish", 2, State::Queued)
    );
    end(&world, 6, State::Passed);
    assert_eq!(world.checks(root).len(), 6, "nothing more comes back");
}
