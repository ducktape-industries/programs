//! A working value of each wake op and query an account sends, as `#[tool_op]` and `#[tool_query]`
//! name them: what an agent edits rather than build one from the schema.
use crate::{Op, PageRequest, Query, WaitFor};

pub(crate) fn wake_on() -> Op {
    Op::WakeOn {
        event: WaitFor::Ref {
            repo: "programs".into(),
            reference: "refs/heads/dev".into(),
        },
    }
}

pub(crate) fn cancel() -> Op {
    Op::Cancel { id: 7 }
}

pub(crate) fn pod_wait() -> Op {
    Op::PodWait { id: 7 }
}

pub(crate) fn one() -> Query {
    Query::Wake { id: 7 }
}

pub(crate) fn wakes() -> Query {
    Query::Wakes {
        account: 21,
        page: PageRequest {
            after: None,
            limit: Some(20),
        },
    }
}
