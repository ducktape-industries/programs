//! The `actions` program, forge's CI: it hears forge's ref moves and changes opened, reads the
//! workflows at that commit (`workflow.rs`), queues a check per job, offers each to the runners
//! whose labels cover it (`rules.rs`), and records how the runner that claimed it says it ended.
mod program;
mod rules;
mod state;
#[cfg(test)]
mod tests;
mod workflow;

pub use ::program::{AccountNumber, Principal, ProgramId};
pub use program::Actions;
pub use store::{PageRequest, PageResponse};

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

use crate::state::{BY_COMMIT, CHECKS, QUEUED, RUNNERS, RUNS};

pub const PROGRAM: &str = "actions";
// Where a repository keeps its workflows, one TOML file each.
pub const WORKFLOWS: &str = ".forge/workflows";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Op {
    // What forge tells a watcher (`forge::Op::Watch`); forge alone sends it.
    Watched(forge::Watched),
    // Every workflow at `commit`, run whatever events it names.
    Trigger {
        repo: String,
        commit: String,
    },
    // The sender runs the checks whose `runs-on` its labels cover. A program that registers
    // hears each as `deliver ++ borsh(Offer)`; no labels takes the runner off.
    Register {
        labels: Vec<String>,
        deliver: Vec<u8>,
    },
    Claim {
        check: u64,
    },
    Report {
        check: u64,
        state: State,
        summary: String,
        log: Option<String>,
    },
    Rerun {
        check: u64,
    },
}

// What forge puts before each event it tells actions: `Op::Watched`'s tag.
pub fn watching() -> Vec<u8> {
    vec![0]
}

// Errored: the runner or what it runs on broke, so the check says nothing of the code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum State {
    Queued,
    Running,
    Passed,
    Failed,
    Errored,
}

impl State {
    pub fn word(self) -> &'static str {
        match self {
            State::Queued => "queued",
            State::Running => "running",
            State::Passed => "passed",
            State::Failed => "failed",
            State::Errored => "errored",
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

// A file of files placed at `path` before the steps run.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Input {
    pub path: String,
    pub files: String,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Event {
    Push { reference: Vec<u8> },
    Opened { n: u64 },
    Updated { n: u64 },
    Manual,
    // change `n`'s Preview: the squash a merge would land, built by forge
    Preview { n: u64 },
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
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Ask,
)]
#[ask(Actions)]
pub enum Query {
    #[ask(Reply::Check(Option<CheckRow>))]
    #[reads(CHECKS)]
    Check { id: u64 },
    #[ask(Reply::Checks(PageResponse<CheckRow>))]
    #[reads(CHECKS, BY_COMMIT)]
    Checks {
        repo: String,
        commit: String,
        page: PageRequest,
    },
    #[ask(Reply::Queued(PageResponse<CheckRow>))]
    #[reads(CHECKS, QUEUED)]
    Queued { page: PageRequest },
    #[ask(Reply::Run(Option<RunRow>))]
    #[reads(RUNS)]
    Run { id: u64 },
    #[ask(Reply::Runners(PageResponse<RunnerRow>))]
    #[reads(RUNNERS)]
    Runners { page: PageRequest },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Check(Option<CheckRow>),
    Checks(PageResponse<CheckRow>),
    Queued(PageResponse<CheckRow>),
    Run(Option<RunRow>),
    Runners(PageResponse<RunnerRow>),
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
        Op::Watched(forge::Watched::Previewed { repo, n, .. }) => {
            (format!("Hear change {repo}#{n} previewed"), Vec::new())
        }
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
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        ["Watched", "Trigger", "Register", "Claim", "Report", "Rerun"]
    );
}
