//! A working value of each activity op and query, as `#[tool_op]` and `#[tool_query]` name them:
//! what an agent edits rather than build one from the schema.
use crate::{Op, PageRequest, Query};

pub(crate) fn migrate() -> Op {
    Op::Migrate {
        height: 300_000,
        first: Some((21, "chat".into())),
        last: None,
    }
}

fn page() -> PageRequest {
    PageRequest {
        after: None,
        limit: Some(20),
    }
}

pub(crate) fn acted() -> Query {
    Query::Acted {
        account: 21,
        page: page(),
    }
}

pub(crate) fn used() -> Query {
    Query::Used { page: page() }
}

pub(crate) fn frames() -> Query {
    Query::Frames { page: page() }
}
