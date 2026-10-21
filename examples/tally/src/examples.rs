//! A working value of each tally op and query, as `#[tool_op]` and `#[tool_query]` name them.
use crate::{Op, Query};

pub(crate) fn add() -> Op {
    Op::Add { by: 1 }
}

pub(crate) fn total() -> Query {
    Query::Total
}

pub(crate) fn added() -> Query {
    Query::Added { account: 1 }
}
