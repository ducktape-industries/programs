//! The `actions-pods` program: an `actions` runner that runs each check it claims as a pod of the
//! pods service, through egress, hears the pod end as a stream, copies its `/out` into files,
//! reports the check and deletes the pod (`rules.rs`).
mod program;
mod rules;
#[cfg(test)]
mod tests;

pub use abi::role::egress::Event;
pub use program::ActionsPods;

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

use crate::rules::RUNS;

pub const PROGRAM: &str = "actions-pods";
// The egress service the pods run through.
pub const SERVICE: &str = "pods";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Op {
    // A check actions offers this runner (`actions::Op::Register`'s deliver is this tag).
    Offer(actions::Offer),
    // The claimed check's run, the block after its offer: its own scheduled run alone.
    Run { check: u64 },
    // An event of a pod's wait, from egress.
    Heard(Event),
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

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Ask,
)]
#[ask(ActionsPods)]
pub enum Query {
    #[ask(Reply::Run(Option<RunRow>))]
    #[reads(RUNS)]
    Run { check: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Run(Option<RunRow>),
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
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

#[test]
fn op_variants_only_append() {
    assert_eq!(describe::variants::<Op>(), ["Offer", "Run", "Heard"]);
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
