//! The `awaits` program: it opens a ticket for each call a parked frame waits on, and hands the
//! frame its answer when the called program resolves the ticket.
mod program;
mod rules;
#[cfg(test)]
mod tests;
mod tools;

use abi::role::awaits::{Answer, FrameId, Ticket};
pub use abi::role::awaits::{Call, Query, Reply, TicketRow, cancelled};
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
pub use program::Awaits;

pub const PROGRAM: &str = "awaits";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Op {
    Park {
        frame: FrameId,
        program: ::program::ProgramId,
        calls: Vec<Call>,
    },
    Resolve {
        ticket: Ticket,
        answer: Answer,
    },
    Close {
        frame: FrameId,
    },
    Migrate {
        height: u64,
        first: Option<Ticket>,
        last: Option<Ticket>,
    },
}

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let text = |n: u64| Value::Text(n.to_string());
    let (title, fields) = match op {
        Op::Park {
            frame,
            program,
            calls,
        } => (
            format!("Park · frame {frame} of {program}"),
            calls
                .iter()
                .map(|call| field(&call.target, text(call.ticket)))
                .collect(),
        ),
        Op::Resolve { ticket, answer } => (
            format!("Resolve · call {ticket}"),
            vec![field("answered", Value::Text(answer.is_ok().to_string()))],
        ),
        Op::Close { frame } => (
            format!("Close · frame {frame}"),
            vec![field("frame", text(*frame))],
        ),
        Op::Migrate {
            height,
            first,
            last,
        } => {
            let bound = |bound: &Option<Ticket>, open: &str| {
                bound.map_or_else(|| Value::Text(open.into()), text)
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
        ["Park", "Resolve", "Close", "Migrate"]
    );
}

#[test]
fn every_op_the_kernel_and_callers_send_meets_awaitss_own_at_one_tag() {
    use abi::role::awaits as role;
    let in_the_role = |op: &role::Op| match op {
        role::Op::Park { .. } | role::Op::Resolve { .. } | role::Op::Close { .. } => {}
    };
    let calls = vec![Call {
        ticket: 7,
        target: "egress".into(),
    }];
    let ops = [
        (
            role::Op::Park {
                frame: 1,
                program: "harness".into(),
                calls: calls.clone(),
            },
            Op::Park {
                frame: 1,
                program: "harness".into(),
                calls,
            },
        ),
        (
            role::Op::Resolve {
                ticket: 7,
                answer: Ok(b"pong".to_vec()),
            },
            Op::Resolve {
                ticket: 7,
                answer: Ok(b"pong".to_vec()),
            },
        ),
        (role::Op::Close { frame: 1 }, Op::Close { frame: 1 }),
    ];
    for (sent, own) in ops {
        in_the_role(&sent);
        assert_eq!(abi::decode::<Op>(&abi::encode(&sent)).unwrap(), own);
        assert_eq!(abi::decode::<role::Op>(&abi::encode(&own)).unwrap(), sent);
    }
}
