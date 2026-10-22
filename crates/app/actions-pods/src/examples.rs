//! A working value of each actions-pods op and query, as `#[tool_op]` and `#[tool_query]` name
//! them: what an agent edits rather than build one from the schema.
use crate::{Event, Op, Query};

const CHECK: u64 = 42;

pub(crate) fn offer() -> Op {
    Op::Offer(actions::Offer { check: CHECK })
}

pub(crate) fn run() -> Op {
    Op::Run { check: CHECK }
}

pub(crate) fn heard() -> Op {
    Op::Heard(Event {
        id: 7,
        seq: 0,
        data: b"event: ended\ndata: {}\n".to_vec(),
    })
}

pub(crate) fn sweep() -> Op {
    Op::Sweep
}

pub(crate) fn resume() -> Op {
    Op::Resume { check: CHECK }
}

pub(crate) fn set_concurrent() -> Op {
    Op::SetConcurrent { most: 4 }
}

pub(crate) fn run_of() -> Query {
    Query::Run { check: CHECK }
}

pub(crate) fn pods() -> Query {
    Query::Pods
}

pub(crate) fn waiting() -> Query {
    Query::Waiting
}

pub(crate) fn concurrent() -> Query {
    Query::Concurrent
}

pub(crate) fn progress() -> Query {
    Query::Progress { check: CHECK }
}
