//! What awaits tells an agent of its ops and queries. Both enums are the awaits role's, defined
//! in abi, so markers carry each variant's words and a working value of it.
use ::program::tools::{Described, Tool, example};

use crate::{Call, Op, Query};

pub(crate) struct Ops;

pub(crate) struct Queries;

fn op(variant: &'static str, description: &'static str, value: Op) -> Tool {
    Tool {
        variant,
        description,
        example: Some(example(value)),
        reply: "",
    }
}

fn query(
    variant: &'static str,
    description: &'static str,
    value: Query,
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
    const ABOUT: &'static str = "The calls parked frames wait on: the kernel opens a ticket for each call a frame awaits, the called program resolves it with its answer, which resumes the frame, and the kernel closes what is left when the frame finishes.";

    fn tools() -> Vec<Tool> {
        vec![
            op(
                "Park",
                "The kernel's alone: frame of program waits on calls, a ticket opened for each, owed by its target. Refused when the frame is left awaiting nothing.",
                Op::Park {
                    frame: 1,
                    program: "actions-pods".into(),
                    calls: vec![Call {
                        ticket: 7,
                        target: "egress".into(),
                    }],
                },
            ),
            op(
                "Resolve",
                "From the ticket's target only: answer a call, with the bytes it returns or a refusal; the ticket closes and its frame resumes with the answer. A ticket that is not open is let be.",
                Op::Resolve {
                    ticket: 7,
                    answer: Ok(Vec::new()),
                },
            ),
            op(
                "Close",
                "The kernel's alone, when a frame finishes: close every ticket it still holds.",
                Op::Close { frame: 1 },
            ),
        ]
    }
}

impl Described for Queries {
    fn tools() -> Vec<Tool> {
        vec![
            query(
                "Ticket",
                "One open ticket: its frame, the program that called, the target that owes the answer and the height it opened at; null once closed, or for a ticket never opened.",
                Query::Ticket { ticket: 7 },
                "Ticket",
            ),
            query(
                "Owed",
                "The open tickets target owes, in ticket order, after a ticket and at most limit of them: what a program that answers calls reads to find its work.",
                Query::Owed {
                    target: "egress".into(),
                    after: None,
                    limit: 20,
                },
                "Tickets",
            ),
        ]
    }
}
