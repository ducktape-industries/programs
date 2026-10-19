//! The `fleet` program: the network's default way to run workloads. It keeps what was asked to
//! run, which machine took each run and how it ended; a machine says it is ready, claims what
//! fleet assigns it, beats while it runs and reports the end (`rules.rs`).
mod examples;
mod program;
mod rules;
mod state;
#[cfg(test)]
mod tests;

pub use ::program::{AccountNumber, BlobId, ProgramId};
pub use program::Fleet;
pub use store::{PageRequest, PageResponse};

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

use crate::state::{HOLDS, MACHINES, QUEUED, RUNS, WORKLOADS};

pub const PROGRAM: &str = "fleet";

// A machine: the account whose key runs it, and its name among that account's machines.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, BorshSerialize, BorshDeserialize, BorshSchema,
)]
pub struct MachineId {
    pub account: AccountNumber,
    pub name: String,
}

// Job: runs to its end. Service: kept running until someone stops it, so an end that nobody
// asked for queues its next attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Kind {
    Job,
    Service,
}

// What a run is placed at before its command starts, as the pods service's inputs name them.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Input {
    Forge {
        path: String,
        repo: String,
        commit: String,
    },
    Files {
        path: String,
        files: String,
        snapshot: Option<BlobId>,
    },
    Blob {
        path: String,
        blob: BlobId,
    },
    Scratch {
        path: String,
        memory: bool,
        size: Option<String>,
    },
}

// What a workload asks of its backend, as Kubernetes quantities; the backend reads them.
#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Asks {
    pub cpu: Option<String>,
    pub memory: Option<String>,
}

// Where a run may go: that one machine, or any machine holding every label. None: anywhere.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Affinity {
    Machine(MachineId),
    Labels(Vec<String>),
}

// What a run whose machine went silent becomes: queued again, or parked as lost until its
// machine reports or someone requeues it (a run that must never happen on two machines).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum OnLoss {
    Retry,
    Park,
}

// Who is told when a run ends: a chat channel, or a program as `deliver ++ borsh(Ended)`.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Notify {
    Chat {
        channel: String,
    },
    Program {
        program: ProgramId,
        deliver: Vec<u8>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Spec {
    pub kind: Kind,
    pub image: Option<String>,
    pub command: Vec<String>,
    pub env: Vec<(String, String)>,
    pub workdir: Option<String>,
    pub inputs: Vec<Input>,
    pub asks: Asks,
    pub on: Option<Affinity>,
    // the files folder the machine writes the run's log and outputs into
    pub out: Option<String>,
    pub on_loss: OnLoss,
    pub notify: Option<Notify>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Workload {
    pub id: u64,
    pub by: AccountNumber,
    pub height: u64,
    pub spec: Spec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum State {
    Queued,
    Assigned,
    Running,
    Stopping,
    Lost,
    Passed,
    Failed,
    Errored,
    Cancelled,
}

impl State {
    pub fn word(self) -> &'static str {
        match self {
            State::Queued => "queued",
            State::Assigned => "assigned",
            State::Running => "running",
            State::Stopping => "stopping",
            State::Lost => "lost",
            State::Passed => "passed",
            State::Failed => "failed",
            State::Errored => "errored",
            State::Cancelled => "cancelled",
        }
    }

    pub fn ended(self) -> bool {
        matches!(
            self,
            State::Passed | State::Failed | State::Errored | State::Cancelled
        )
    }
}

// Where a run's machine wrote its log and outputs: a files folder, at the snapshot it made.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Out {
    pub path: String,
    pub snapshot: BlobId,
}

// A workload's run: its attempt counts the assignments it has had.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct RunRow {
    pub id: u64,
    pub attempt: u32,
    pub state: State,
    pub machine: Option<MachineId>,
    pub assigned: Option<u64>,
    pub started: Option<u64>,
    pub finished: Option<u64>,
    pub summary: String,
    pub out: Option<Out>,
}

// Whose runs a machine takes.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Serves {
    Anyone,
    Listed(Vec<AccountNumber>),
}

// A machine as it says it is (labels, slots, whom it serves) and as fleet keeps it: the runs it
// holds, since when a slot of it has been free, whether it went silent, and its next lapse check.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct MachineRow {
    pub id: MachineId,
    pub labels: Vec<String>,
    pub slots: u64,
    pub serves: Serves,
    pub busy: u64,
    pub idle_since: Option<u64>,
    pub gone: bool,
    pub check_at: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct MachineView {
    pub machine: MachineRow,
    pub beat: Option<u64>,
}

// What a `Notify::Program` hears when a run ends or is lost.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Ended {
    pub run: u64,
    pub attempt: u32,
    pub state: State,
}

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Described,
)]
#[tool_about(
    "The network's default way to run workloads: anyone asks for a run, a machine that is ready takes it (whatever's free, unless the run is pinned to a machine or a label set), beats while it runs and reports how it ended. Logs and outputs are files the machine writes; fleet keeps where."
)]
pub enum Op {
    #[tool_op(description = "Ask for a workload to run. Left without `on`, it runs on whichever ready machine has been free longest; `on` pins it to one machine or to machines holding every label named. It is placed the block after. Returns the run's id.", returns = u64, example = crate::examples::ask)]
    Ask { spec: Spec },
    #[tool_op(description = "As a machine of your account, named `name`: say which labels you hold, how many runs you take at once (0 takes none) and whose runs you serve. Queued runs you cover are assigned to you at once.", example = crate::examples::ready)]
    Ready {
        name: String,
        labels: Vec<String>,
        slots: u64,
        serves: Serves,
    },
    #[tool_op(description = "Claim a run assigned to your machine, at the attempt it was assigned with. The run's claim row is its asker's, so their policy decides whether you may.", example = crate::examples::claim)]
    Claim { run: u64, attempt: u32 },
    #[tool_op(description = "Say your machine is still there, for every run it holds. A machine silent for the egress wait (DEFAULT_WAIT blocks) loses its runs.", example = crate::examples::beat)]
    Beat { name: String },
    #[tool_op(description = "Report how a run your machine claimed ended: passed, failed, errored or cancelled, a one-line summary, and the files folder and snapshot its outputs went to.", example = crate::examples::report)]
    Report {
        run: u64,
        attempt: u32,
        state: State,
        summary: String,
        out: Option<Out>,
    },
    #[tool_op(description = "Give back a run assigned to your machine that it will not run, saying why; it is not assigned to this machine again.", example = crate::examples::decline)]
    Decline { run: u64, attempt: u32, why: String },
    #[tool_op(description = "Cancel a run: a queued or assigned one ends at once, a running one is stopped by its machine. Whoever the run's claim policy admits; its asker always.", example = crate::examples::cancel)]
    Cancel { run: u64 },
    #[tool_op(description = "Queue a lost or ended run again as its next attempt. Whoever the run's claim policy admits; its asker always.", example = crate::examples::requeue)]
    Requeue { run: u64 },
    #[tool_op(description = "Check whether a machine went silent: one that has not beaten for the egress wait loses every run it holds. fleet books this itself; anyone may send it, and it acts only on what the chain shows.", example = crate::examples::lapse)]
    Lapse { machine: MachineId },
    #[tool_op(description = "Check whether an assignment went unclaimed for the egress wait: its machine is taken as gone and the run goes elsewhere. fleet books this itself; anyone may send it.", example = crate::examples::unclaimed)]
    Unclaimed { run: u64, attempt: u32 },
    #[tool_op(description = "Place a queued run on the machine free longest that may take it. fleet books this for the block after an Ask, once the run's claim row has its owner's policy; anyone may send it.", example = crate::examples::place)]
    Place { run: u64 },
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
    "What fleet holds: each workload as asked, each run's state, the queue and the machines."
)]
#[ask(Fleet)]
pub enum Query {
    #[ask(Reply::Workload(Option<Workload>))]
    #[reads(WORKLOADS)]
    #[tool_query(description = "A workload as it was asked: who, when and its spec.", example = crate::examples::workload)]
    Workload { id: u64 },
    #[ask(Reply::Run(Option<RunRow>))]
    #[reads(RUNS)]
    #[tool_query(description = "A run: its attempt, state, machine, heights and result.", example = crate::examples::run)]
    Run { id: u64 },
    #[ask(Reply::Runs(PageResponse<RunRow>))]
    #[reads(QUEUED, RUNS)]
    #[tool_query(description = "The runs no machine holds yet, oldest first.", example = crate::examples::queued)]
    Queued { page: PageRequest },
    #[ask(Reply::Machine(Option<MachineView>))]
    #[reads(MACHINES)]
    #[tool_query(description = "A machine: its labels, slots, whom it serves, what it holds, and its last beat.", example = crate::examples::machine)]
    Machine { machine: MachineId },
    #[ask(Reply::Machines(PageResponse<MachineView>))]
    #[reads(MACHINES)]
    #[tool_query(description = "Every machine, by account and name.", example = crate::examples::machines)]
    Machines { page: PageRequest },
    #[ask(Reply::Runs(PageResponse<RunRow>))]
    #[reads(HOLDS, RUNS)]
    #[tool_query(description = "The runs a machine holds: assigned, running or stopping.", example = crate::examples::holds)]
    Holds {
        machine: MachineId,
        page: PageRequest,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Workload(Option<Workload>),
    Run(Option<RunRow>),
    Runs(PageResponse<RunRow>),
    Machine(Option<MachineView>),
    Machines(PageResponse<MachineView>),
}

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let run = |id: &u64| field("run", Value::count(*id));
    let (title, fields) = match op {
        Op::Ask { spec } => (
            "Ask for a run".to_owned(),
            vec![field("command", Value::text(spec.command.join(" ")))],
        ),
        Op::Ready { name, slots, .. } if *slots == 0 => {
            (format!("Machine {name} takes no runs"), Vec::new())
        }
        Op::Ready {
            name,
            labels,
            slots,
            ..
        } => (
            format!("Machine {name} is ready"),
            vec![
                field(
                    "labels",
                    Value::List(labels.iter().map(Value::text).collect()),
                ),
                field("slots", Value::count(*slots)),
            ],
        ),
        Op::Claim { run: id, .. } => (format!("Claim run {id}"), vec![run(id)]),
        Op::Beat { name } => (format!("Machine {name} beats"), Vec::new()),
        Op::Report {
            run: id,
            state,
            summary,
            ..
        } => (
            format!("Report run {id} {}", state.word()),
            vec![run(id), field("summary", Value::text(summary))],
        ),
        Op::Decline { run: id, why, .. } => (
            format!("Decline run {id}"),
            vec![run(id), field("why", Value::text(why))],
        ),
        Op::Cancel { run: id } => (format!("Cancel run {id}"), vec![run(id)]),
        Op::Requeue { run: id } => (format!("Queue run {id} again"), vec![run(id)]),
        Op::Lapse { machine } => (format!("Check machine {}", machine.name), Vec::new()),
        Op::Unclaimed { run: id, .. } => (format!("Check run {id} was claimed"), vec![run(id)]),
        Op::Place { run: id } => (format!("Place run {id}"), vec![run(id)]),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        [
            "Ask",
            "Ready",
            "Claim",
            "Beat",
            "Report",
            "Decline",
            "Cancel",
            "Requeue",
            "Lapse",
            "Unclaimed",
            "Place"
        ]
    );
}
