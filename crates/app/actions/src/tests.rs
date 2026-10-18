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
const RUNNER: &str = "runner";
const RUNNER_ACCOUNT: u64 = 904;
const OFFERS: Item<Vec<u64>> = Item::new("offers");
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

// A program runner: it registers with actions and keeps each check it is offered.
#[derive(Debug, BorshSerialize, BorshDeserialize)]
enum Runs {
    Register(Vec<String>),
    Offer(Offer),
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
            Runs::Offer(offer) => {
                ctx.env().sent_by(PROGRAM)?;
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
}

fn world() -> World {
    let mut chain = MockChain::default();
    chain.seat::<forge::Forge>(forge::PROGRAM);
    chain.seat::<chat::Chat>("chat");
    chain.seat::<access::Access>("access");
    chain.seat::<Actions>(PROGRAM);
    chain.seat::<Runner>(RUNNER);
    chain.hold(OWNER, OWNER_ACCOUNT);
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

    // A commit whose tree holds `.forge/workflows/<name>` for each workflow, and a file of its
    // own (`t<time>`), so two branches' commits merge cleanly.
    fn commit(&mut self, parents: &[Oid], workflows: &[(&str, &str)]) -> Oid {
        self.commit_files(parents, workflows, &[])
    }

    // `commit`'s tree with each of `extra` at its path in the root.
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

    // The commit change `n`'s latest Preview made.
    fn preview(&self, n: u64) -> Oid {
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

    // What forge holds of a commit's statuses: each context and its state.
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
    assert_eq!(gates.jobs[0].steps, ["sh /tools/gates.sh /work/programs"]);
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
    let run = world.run(1);
    assert_eq!(run.event, Event::Preview { n: 1 });
    let ran = (run.commit.as_str(), run.workflow.as_str());
    assert_eq!(ran, (feature.to_hex().as_str(), "refs/changes/1/merge"));
    let error = run.error.unwrap();
    assert!(error.starts_with("change 1 does not preview: "), "{error}");
    assert!(world.checks(feature).is_empty());
}
