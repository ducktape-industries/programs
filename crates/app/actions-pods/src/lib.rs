//! The `actions-pods` program: an `actions` runner that runs each check it claims as a pod of the
//! pods service, through egress, hears the pod end as a stream, keeps its `/out` as outputs,
//! reports the check and deletes the pod; `Sweep` settles the pods runs left (`rules.rs`).
mod examples;
mod program;
mod rules;
#[cfg(test)]
mod tests;

pub use abi::role::egress::Event;
pub use program::ActionsPods;

use ::program::BlobId;
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

use crate::rules::{CONCURRENT, PODS, RUNS, WAITING};

pub const PROGRAM: &str = "actions-pods";
// The egress service the pods run through.
pub const SERVICE: &str = "pods";

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Described,
)]
#[tool_about(
    "An actions runner that runs each check it is offered as a pod of the pods service, through egress: it claims the check, hears the pod end as a stream, keeps its /out as the check's outputs, reports how it ended and deletes the pod."
)]
pub enum Op {
    #[tool_op(description = "From actions only: a check offered to this runner, whose run it books for the block after (actions' Register deliver is this tag).", example = crate::examples::offer)]
    Offer(actions::Offer),
    #[tool_op(description = "This program's own scheduled run alone: run an offered check in a pod, claim it, wait for the pod to end, record its outputs, report it and delete the pod. A check past the quota or the concurrent limit, or whose pod the cluster did not make, waits.", example = crate::examples::run)]
    Run { check: u64 },
    #[tool_op(description = "From the egress role only: an event of a pod's streamed wait; the ended event is kept for the run that waits on that pod.", example = crate::examples::heard)]
    Heard(Event),
    #[tool_op(description = "Settle every pod this runner holds against its check: a pod whose check no longer runs is deleted, one an update of this program cut off is taken up again, and waiting checks are tried again. Open to anyone.", example = crate::examples::sweep)]
    Sweep,
    #[tool_op(description = "This program's own scheduled run alone, the block after a Sweep found a pod an update cut off: wait for that pod again and finish its check.", example = crate::examples::resume)]
    Resume { check: u64 },
    #[tool_op(description = "How many pods this runner holds at once (unset: 1), at least 1; a check offered past it waits until one of its pods ends. Open to anyone.", example = crate::examples::set_concurrent)]
    SetConcurrent { most: u64 },
}

// The tag actions offers checks under, and the one egress streams a wait's events under.
pub fn offered_as() -> Vec<u8> {
    vec![0]
}

pub fn heard_as() -> Vec<u8> {
    vec![2]
}

// `init`'s params: the labels this runner registers with actions.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Config {
    pub labels: Vec<String>,
}

// A check this runner ran: its pod, the block time measured between its offer and its run, the
// trip its pod's creation took to land, and the seconds between the wait's alive events.
#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct RunRow {
    pub check: u64,
    pub pod: Option<String>,
    pub block_ms: u64,
    pub trip_ms: u64,
    pub every: u64,
}

// A pod this runner holds, from the moment the pods service names it until a run deletes it, and
// the code this program ran when it made or took it up (None where the programs role answers
// nothing): a frame parked on the pod ends when that code changes.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct PodRow {
    pub pod: String,
    pub code: Option<BlobId>,
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
    "The pods this runner holds, the checks waiting for room, its concurrent limit, and how each check it ran went."
)]
#[ask(ActionsPods)]
pub enum Query {
    #[ask(Reply::Run(Option<RunRow>))]
    #[reads(RUNS)]
    #[tool_query(description = "A check this runner ran: its pod, the block time between its offer and its run, the milliseconds its pod's creation took to land, and the seconds between its wait's alive events; or null.", example = crate::examples::run_of)]
    Run { check: u64 },
    #[ask(Reply::Pods(Vec<(u64, PodRow)>))]
    #[reads(PODS)]
    #[tool_query(description = "Every pod this runner holds, by check, each with the code this program ran when it made or took it up.", example = crate::examples::pods)]
    Pods,
    #[ask(Reply::Waiting(Vec<u64>))]
    #[reads(WAITING)]
    #[tool_query(description = "The checks still queued that wait for room in the quota or the concurrent limit, or for a pod the cluster did not make.", example = crate::examples::waiting)]
    Waiting,
    #[ask(Reply::Concurrent(u64))]
    #[reads(CONCURRENT)]
    #[tool_query(description = "How many pods this runner holds at once: 1 until SetConcurrent says otherwise.", example = crate::examples::concurrent)]
    Concurrent,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Run(Option<RunRow>),
    Pods(Vec<(u64, PodRow)>),
    Waiting(Vec<u64>),
    Concurrent(u64),
}

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let (title, fields) = match op {
        Op::Offer(offer) => (
            format!("Take check {}", offer.check),
            vec![field("check", Value::count(offer.check))],
        ),
        Op::Run { check } => (
            format!("Run check {check} in a pod"),
            vec![field("check", Value::count(*check))],
        ),
        Op::Heard(event) => (
            format!("Hear request {}'s event {}", event.id, event.seq),
            vec![field("data", Value::count(event.data.len() as u64))],
        ),
        Op::Sweep => ("Settle the pods of checks".to_owned(), Vec::new()),
        Op::Resume { check } => (
            format!("Take up check {check}'s pod again"),
            vec![field("check", Value::count(*check))],
        ),
        Op::SetConcurrent { most } => (
            format!("Hold at most {most} pods at once"),
            vec![field("most", Value::count(*most))],
        ),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        ["Offer", "Run", "Heard", "Sweep", "Resume", "SetConcurrent"]
    );
}

#[test]
fn the_tags_are_the_ops_own() {
    let offer = Op::Offer(actions::Offer { check: 1 });
    let heard = Op::Heard(Event {
        id: 1,
        seq: 0,
        data: Vec::new(),
    });
    assert_eq!(abi::encode(&offer)[..1], offered_as());
    assert_eq!(abi::encode(&heard)[..1], heard_as());
}
