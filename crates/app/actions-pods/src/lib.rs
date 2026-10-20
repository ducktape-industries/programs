//! The `actions-pods` program: an `actions` runner that runs each check it claims as a pod of the
//! pods service, through egress, hears the pod end as a stream, keeps its `/out` as outputs,
//! reports the check and deletes the pod; `Sweep` settles the pods runs left (`rules.rs`).
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

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Op {
    // A check actions offers this runner (`actions::Op::Register`'s deliver is this tag).
    Offer(actions::Offer),
    // The claimed check's run, the block after its offer: its own scheduled run alone.
    Run { check: u64 },
    Heard(Event),
    // Every pod this runner holds, settled against its check; open to anyone.
    Sweep,
    // A pod whose run an update of this program cut off, taken up again: its own scheduled run
    // alone, the block after a Sweep found it.
    Resume { check: u64 },
    // How many pods this runner holds at once (unset: 1); open to anyone. A check offered past it
    // waits, as one the quota had no room for does, until one of this runner's pods ends.
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
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Ask,
)]
#[ask(ActionsPods)]
pub enum Query {
    #[ask(Reply::Run(Option<RunRow>))]
    #[reads(RUNS)]
    Run { check: u64 },
    #[ask(Reply::Pods(Vec<(u64, PodRow)>))]
    #[reads(PODS)]
    Pods,
    // The checks still queued that wait for room in the quota or for a pod the cluster did not make.
    #[ask(Reply::Waiting(Vec<u64>))]
    #[reads(WAITING)]
    Waiting,
    #[ask(Reply::Concurrent(u64))]
    #[reads(CONCURRENT)]
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
