//! The `actions` program, forge's CI: it hears forge's ref moves and changes opened, reads the
//! workflows at that commit (`workflow.rs`), queues a check per job, offers each to the runners
//! whose labels cover it (`rules.rs`), and records how the runner that claimed it says it ended.
mod examples;
mod program;
mod rules;
mod state;
#[cfg(test)]
mod tests;
mod workflow;

pub use ::program::{AccountNumber, BlobId, Principal, ProgramId};
pub use program::Actions;
pub use store::{PageRequest, PageResponse};

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

use crate::state::{ASKS, BY_COMMIT, BY_EVENT, CHECKS, MET, NEEDS, OUTPUTS, QUEUED, RUNNERS, RUNS};

pub const PROGRAM: &str = "actions";
// Where a repository keeps its workflows, one TOML file each.
pub const WORKFLOWS: &str = ".forge/workflows";

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Described,
)]
#[tool_about(
    "forge's CI: on a push, a change's Preview, a dispatch or a hand trigger it reads the repository's workflows in .forge/workflows at that commit, queues a check per job, offers each to the runners whose labels cover its runs-on, and records how the runner that claimed it says it ended, as a forge commit status."
)]
pub enum Op {
    #[tool_op(description = "From forge only: one event it tells its watchers. A push starts the workflows that run on it and supersedes the branch's earlier runs; a change opened or moved has its Preview built the block after; a merge or close supersedes the change's Preview runs.", example = crate::examples::watched)]
    Watched(forge::Watched),
    #[tool_op(description = "Run every workflow of repo at commit but those whose on.trigger is false, as a Manual run by the sender.", example = crate::examples::trigger)]
    Trigger { repo: String, commit: String },
    #[tool_op(description = "Run checks: the sender takes the checks whose runs-on its labels all cover, and replaces what it registered before. A program that registers is offered each queued check it may run, now and as they queue, as deliver followed by the borsh of an Offer; an account leaves deliver empty and reads Queued. No labels takes the runner off.", example = crate::examples::register)]
    Register {
        labels: Vec<String>,
        deliver: Vec<u8>,
    },
    #[tool_op(description = "Take a queued check to run it: it turns running, with the sender as its claimant. Refused for a check that is not queued, or whose job names its runners and not the sender.", example = crate::examples::claim)]
    Claim { check: u64 },
    #[tool_op(description = "Say how a check ended: passed, failed, errored or superseded, with a summary and a log (a files path or a URL). A running check is its claimant's to report; a queued one may be reported errored or superseded by an account allowed to run it. Jobs that wait on it are queued or skipped, and a push run's failed or errored check is told in its workflow's notify room.", example = crate::examples::report)]
    Report {
        check: u64,
        state: State,
        summary: String,
        log: Option<String>,
    },
    #[tool_op(description = "Queue a check's job again as its next attempt, with what it asked, needed and named. Refused while the check is still queued or waiting.", example = crate::examples::rerun)]
    Rerun { check: u64 },
    #[tool_op(description = "Have forge build change n's Preview, the squash a merge would land, and run the workflows that run on it: what actions sends itself the block after a change opens or its head moves, so the Preview starts a frame of its own. A Preview forge refuses records an errored run saying why.", example = crate::examples::preview)]
    Preview { repo: String, n: u64 },
    #[tool_op(description = "Record the files a running check produced, each by the blob that holds it, as its claimant says; saying it again replaces them. A check that needs its job reads them with the Outputs query to place them.", example = crate::examples::outputs)]
    Outputs { check: u64, files: Vec<Output> },
}

// What forge puts before each event it tells actions: `Op::Watched`'s tag.
pub fn watching() -> Vec<u8> {
    vec![0]
}

// Errored: the runner or what it runs on broke, so the check says nothing of the code.
// Superseded: a newer push to its branch, a newer Preview of its change, or the change's merge
// or close made it moot before it ended, so it says nothing of the code either.
// Waiting: a job it needs has not passed yet, so no runner is offered it.
// Skipped: a job it needs ended other than passed, so it never ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum State {
    Queued,
    Running,
    Passed,
    Failed,
    Errored,
    Superseded,
    Waiting,
    Skipped,
}

impl State {
    pub fn word(self) -> &'static str {
        match self {
            State::Queued => "queued",
            State::Running => "running",
            State::Passed => "passed",
            State::Failed => "failed",
            State::Errored => "errored",
            State::Superseded => "superseded",
            State::Waiting => "waiting",
            State::Skipped => "skipped",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Job {
    pub id: String,
    pub runs_on: Vec<String>,
    pub image: String,
    pub steps: Vec<String>,
    pub env: Vec<(String, String)>,
    pub inputs: Vec<Input>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Asks {
    pub cpu: Option<String>,
    pub memory: Option<String>,
    pub scratch: Vec<String>,
}

// A file of files placed at `path` before the steps run.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Input {
    pub path: String,
    pub files: String,
}

// A job a check needs, and the passed check of it that met the need once all of them have passed:
// whose outputs a runner places for the check.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Need {
    pub job: String,
    pub check: Option<u64>,
}

// A file a check produced, at `path` under its outputs, by the blob that holds its bytes.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Output {
    pub path: String,
    pub blob: BlobId,
    pub size: u64,
}

// What a check produced, and the account that said so.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct OutputsRow {
    pub by: AccountNumber,
    pub files: Vec<Output>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Event {
    Push { reference: Vec<u8> },
    Opened { n: u64 },
    Updated { n: u64 },
    Manual,
    // change `n`'s Preview: the squash a merge would land, built by forge
    Preview { n: u64 },
    Dispatch { event_type: String },
}

// One workflow file run for one event; `error` says why it ran no job, and `notify` is the chat
// channel told when a check of a push run ends failed or errored.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct RunRow {
    pub id: u64,
    pub repo: String,
    pub commit: String,
    pub workflow: String,
    pub name: Option<String>,
    pub event: Event,
    pub by: Option<Principal>,
    pub height: u64,
    pub error: Option<String>,
    pub notify: Option<String>,
}

// One job of a run, one attempt; a rerun is a check of its own with the next attempt.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct CheckRow {
    pub id: u64,
    pub run: u64,
    pub repo: String,
    pub commit: String,
    pub workflow: String,
    pub attempt: u32,
    pub job: Job,
    pub state: State,
    pub claimant: Option<AccountNumber>,
    pub reporter: Option<AccountNumber>,
    pub queued: u64,
    pub started: Option<u64>,
    pub finished: Option<u64>,
    pub summary: String,
    pub log: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct RunnerRow {
    pub account: AccountNumber,
    pub labels: Vec<String>,
    pub program: Option<ProgramId>,
    pub deliver: Vec<u8>,
}

// What a program runner hears for each check it may claim.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Offer {
    pub check: u64,
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    ::program::Ask,
    ::program::Described,
)]
#[tool_about(
    "The checks, runs and runners of forge's CI: a check by id, a commit's checks, the queue, a repository's runs of one event, and what a check asks of its pod, needs and produced."
)]
#[ask(Actions)]
pub enum Query {
    #[ask(Reply::Check(Option<CheckRow>))]
    #[reads(CHECKS)]
    #[tool_query(description = "One check: its job, attempt, state, claimant and reporter, the heights it queued, started and finished at, its summary and log; or null.", example = crate::examples::check)]
    Check { id: u64 },
    #[ask(Reply::Checks(PageResponse<CheckRow>))]
    #[reads(CHECKS, BY_COMMIT)]
    #[tool_query(description = "A page of the checks run on commit of repo, every attempt of every workflow's jobs.", example = crate::examples::checks)]
    Checks {
        repo: String,
        commit: String,
        page: PageRequest,
    },
    #[ask(Reply::Queued(PageResponse<CheckRow>))]
    #[reads(CHECKS, QUEUED)]
    #[tool_query(description = "A page of the checks queued for a runner to claim, oldest first: what an account runner reads to find work.", example = crate::examples::queued)]
    Queued { page: PageRequest },
    #[ask(Reply::Run(Option<RunRow>))]
    #[reads(RUNS)]
    #[tool_query(description = "One run, a workflow file run for one event: its repository, commit, event, who started it, the chat room it notifies, and why it ran no job if it did not; or null.", example = crate::examples::run)]
    Run { id: u64 },
    #[ask(Reply::Runners(PageResponse<RunnerRow>))]
    #[reads(RUNNERS)]
    #[tool_query(description = "A page of the registered runners, by account: each one's labels, and the program and deliver prefix a program runner is offered checks with.", example = crate::examples::runners)]
    Runners { page: PageRequest },
    #[ask(Reply::Asks(Option<Asks>))]
    #[reads(ASKS)]
    #[tool_query(description = "What a check's job asks of the runner that takes it, its cpu, memory and scratch kept as the workflow writes them (the runner reads them); null for a job that states nothing.", example = crate::examples::asks)]
    Asks { check: u64 },
    #[ask(Reply::Needs(Vec<Need>))]
    #[reads(NEEDS, MET)]
    #[tool_query(description = "The jobs a check needs, each with the passed check that met it once all of them have passed: whose outputs a runner places for it.", example = crate::examples::needs)]
    Needs { check: u64 },
    #[ask(Reply::Outputs(Option<OutputsRow>))]
    #[reads(OUTPUTS)]
    #[tool_query(description = "The files a check produced and the account that said so, once its claimant recorded them; or null.", example = crate::examples::outputs_of)]
    Outputs { check: u64 },
    #[ask(Reply::Runs(PageResponse<RunRow>))]
    #[reads(RUNS, BY_EVENT)]
    #[tool_query(description = "A page of a repository's runs of one event, oldest first: a branch's pushes (Push with its full ref), a change's Previews (Preview), the runs triggered by hand (Manual), or a dispatch's (Dispatch).", example = crate::examples::runs)]
    Runs {
        repo: String,
        event: Event,
        page: PageRequest,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Check(Option<CheckRow>),
    Checks(PageResponse<CheckRow>),
    Queued(PageResponse<CheckRow>),
    Run(Option<RunRow>),
    Runners(PageResponse<RunnerRow>),
    Asks(Option<Asks>),
    Needs(Vec<Need>),
    Outputs(Option<OutputsRow>),
    Runs(PageResponse<RunRow>),
}

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let check = |id: &u64| field("check", Value::count(*id));
    let (title, fields) = match op {
        Op::Watched(forge::Watched::Moved {
            repo, reference, ..
        }) => (
            format!("Hear {} move in {repo}", String::from_utf8_lossy(reference)),
            Vec::new(),
        ),
        Op::Watched(forge::Watched::Opened { repo, n, .. }) => {
            (format!("Hear change {repo}#{n} open"), Vec::new())
        }
        Op::Watched(forge::Watched::Closed { repo, n, .. }) => {
            (format!("Hear change {repo}#{n} close"), Vec::new())
        }
        Op::Watched(forge::Watched::Merged { repo, n, .. }) => {
            (format!("Hear change {repo}#{n} merge"), Vec::new())
        }
        Op::Watched(forge::Watched::Previewed { repo, n, .. }) => {
            (format!("Hear change {repo}#{n} previewed"), Vec::new())
        }
        Op::Watched(forge::Watched::Dispatched {
            repo, event_type, ..
        }) => (
            format!("Hear {event_type} dispatched in {repo}"),
            Vec::new(),
        ),
        Op::Watched(forge::Watched::Reviewed { repo, n, .. }) => {
            (format!("Hear change {repo}#{n} reviewed"), Vec::new())
        }
        Op::Watched(forge::Watched::LeftQueue { repo, n, .. }) => (
            format!("Hear change {repo}#{n} leave its merge queue"),
            Vec::new(),
        ),
        Op::Trigger { repo, commit } => (
            format!("Run the workflows of {repo}"),
            vec![field("commit", Value::text(commit))],
        ),
        Op::Register { labels, .. } if labels.is_empty() => {
            ("Stop running checks".to_owned(), Vec::new())
        }
        Op::Register { labels, .. } => (
            "Run checks".to_owned(),
            vec![field(
                "labels",
                Value::List(labels.iter().map(Value::text).collect()),
            )],
        ),
        Op::Claim { check: id } => (format!("Claim check {id}"), vec![check(id)]),
        Op::Report {
            check: id,
            state,
            summary,
            ..
        } => (
            format!("Report check {id} {}", state.word()),
            vec![check(id), field("summary", Value::text(summary))],
        ),
        Op::Rerun { check: id } => (format!("Run check {id} again"), vec![check(id)]),
        Op::Preview { repo, n } => (format!("Preview change {repo}#{n}"), Vec::new()),
        Op::Outputs { check: id, files } => (
            format!("Record check {id}'s outputs"),
            vec![check(id), field("files", Value::count(files.len() as u64))],
        ),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        [
            "Watched", "Trigger", "Register", "Claim", "Report", "Rerun", "Preview", "Outputs"
        ]
    );
}
