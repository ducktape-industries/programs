//! The `awaits` program: it opens a ticket for each call a parked frame waits on, and hands the
//! frame its answer when the called program resolves the ticket.
mod program;
mod rules;
#[cfg(test)]
mod tests;
mod tools;

pub use abi::role::awaits::{Call, Op, Query, Reply, TicketRow, cancelled};
pub use program::Awaits;

pub const PROGRAM: &str = "awaits";

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
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

#[test]
fn op_variants_only_append() {
    assert_eq!(describe::variants::<Op>(), ["Park", "Resolve", "Close"]);
}
