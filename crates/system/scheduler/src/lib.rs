//! The `scheduler` program, bound to the scheduler role (`abi::role::scheduler`): a program
//! schedules a run of a target for a later height, may cancel it before it runs, and the kernel
//! runs each entry at the end of its block and hands back the run's receipt, which is kept.
mod program;
mod rules;
mod state;
#[cfg(test)]
mod tests;
mod tools;

pub use abi::role::scheduler::{Entry, EntryId, Query, Reply};
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
pub use program::Scheduler;

pub const PROGRAM: &str = "scheduler";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Op {
    Schedule {
        height: u64,
        target: ::program::ProgramId,
        payload: Vec<u8>,
    },
    Cancel {
        entry: EntryId,
    },
    Ran {
        entry: EntryId,
        receipt: abi::Receipt,
    },
    Migrate {
        height: u64,
        first: Option<(u64, EntryId)>,
        last: Option<(u64, EntryId)>,
    },
}

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let text = |n: u64| Value::Text(n.to_string());
    let (title, fields) = match op {
        Op::Schedule {
            height,
            target,
            payload,
        } => (
            format!("Schedule {target} at height {height}"),
            vec![field("payload", Value::count(payload.len() as u64))],
        ),
        Op::Cancel { entry } => (
            format!("Cancel entry {entry}"),
            vec![field("entry", text(*entry))],
        ),
        Op::Ran { entry, receipt } => (
            format!("Ran entry {entry}"),
            vec![field(
                "applied",
                Value::Text(matches!(receipt.outcome, abi::Outcome::Applied { .. }).to_string()),
            )],
        ),
        Op::Migrate {
            height,
            first,
            last,
        } => {
            let bound = |bound: &Option<(u64, EntryId)>, open: &str| match bound {
                Some((height, entry)) => Value::Text(format!("entry {entry} at {height}")),
                None => Value::Text(open.into()),
            };
            (
                "Migrate".to_owned(),
                vec![
                    field("height", text(*height)),
                    field("first", bound(first, "the first")),
                    field("last", bound(last, "the last")),
                ],
            )
        }
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        ["Schedule", "Cancel", "Ran", "Migrate"]
    );
}
