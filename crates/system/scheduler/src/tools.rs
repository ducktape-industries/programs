//! What the scheduler tells an agent of its ops and queries. Both enums are the scheduler role's,
//! defined in abi, so markers carry each variant's words and a working value of it.
use ::program::tools::{Described, Tool, example};
use abi::{Outcome, Receipt};

use crate::{Op, Query};

pub(crate) struct Ops;

pub(crate) struct Queries;

fn told<T: borsh::BorshSerialize>(
    variant: &'static str,
    description: &'static str,
    value: T,
    reply: &'static str,
) -> Tool {
    Tool {
        variant,
        description,
        example: Some(example(value)),
        reply,
    }
}

impl Described for Ops {
    const ABOUT: &'static str = "Runs an op at a later height: a program schedules one, may cancel it before it runs, and the kernel runs each due entry at the end of its block and keeps its receipt.";

    fn tools() -> Vec<Tool> {
        vec![
            told(
                "Schedule",
                "From a program, as itself: run payload as an op of program target at the end of block height, which comes after this one. Returns the entry's id.",
                Op::Schedule {
                    height: 300_000,
                    target: "chat".into(),
                    payload: Vec::new(),
                },
                "",
            ),
            told(
                "Cancel",
                "From the program that scheduled the entry, before it runs: take it back.",
                Op::Cancel { entry: 1 },
                "",
            ),
            told(
                "Ran",
                "The kernel's alone: an entry ran, with its receipt, which the entry keeps.",
                Op::Ran {
                    entry: 1,
                    receipt: Receipt {
                        program: "chat".into(),
                        outcome: Outcome::Applied { output: Vec::new() },
                        events: Vec::new(),
                        nested: Vec::new(),
                    },
                },
                "",
            ),
        ]
    }
}

impl Described for Queries {
    fn tools() -> Vec<Tool> {
        vec![
            told(
                "Due",
                "The entries due at a height that have not run, in id order, after an entry and at most limit of them; next is where to go on while a page comes back full.",
                Query::Due {
                    height: 300_000,
                    after: None,
                    limit: Some(20),
                },
                "Due",
            ),
            told(
                "Entry",
                "One entry: the program that scheduled it, its height, target and payload, and once it ran its receipt; null once cancelled.",
                Query::Entry { id: 1 },
                "Entry",
            ),
        ]
    }
}
