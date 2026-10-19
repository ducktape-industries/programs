//! A working value of each activity query, as `#[tool_query]` names them: what an agent edits
//! rather than build one from the schema.
use crate::{PageRequest, Query};

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

pub(crate) fn signed() -> Query {
    Query::Signed {
        key: vec![0; 32],
        page: page(),
    }
}

pub(crate) fn used() -> Query {
    Query::Used { page: page() }
}

pub(crate) fn frames() -> Query {
    Query::Frames { page: page() }
}
