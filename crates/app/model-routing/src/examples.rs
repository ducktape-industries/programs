//! A working value of each model-routing op and query, as `#[tool_op]` and `#[tool_query]` name
//! them: what an agent edits rather than build one from the schema.
use crate::{Op, PageRequest, Query, Until};

fn first_page() -> PageRequest {
    PageRequest {
        after: None,
        limit: Some(20),
    }
}

pub(crate) fn pool() -> Op {
    Op::Pool {
        provider: "claude".into(),
        service: "fabrics-claude".into(),
        accounts: vec!["anthropic3-eph".into(), "anthropic4".into()],
    }
}

pub(crate) fn full() -> Op {
    Op::Full {
        provider: "claude".into(),
        service: "fabrics-claude".into(),
        account: "anthropic3-eph".into(),
        until: Until::Reset(1_800_000_000),
    }
}

pub(crate) fn available() -> Op {
    Op::Available {
        provider: "claude".into(),
        service: "fabrics-claude".into(),
        account: "anthropic3-eph".into(),
        read_at: 1_800_000_000,
    }
}

pub(crate) fn pin() -> Op {
    Op::Pin {
        conversation: 607,
        provider: "claude".into(),
        services: vec!["fabrics-claude".into(), "zk-claude".into()],
    }
}

pub(crate) fn forget() -> Op {
    Op::Forget { conversation: 607 }
}

pub(crate) fn resolve() -> Query {
    Query::Resolve {
        consumer: 9,
        conversation: 607,
        provider: "claude".into(),
        services: vec!["fabrics-claude".into(), "zk-claude".into()],
    }
}

pub(crate) fn pools() -> Query {
    Query::Pools {
        consumer: 9,
        page: first_page(),
    }
}

pub(crate) fn fulls() -> Query {
    Query::Fulls {
        consumer: 9,
        page: first_page(),
    }
}

pub(crate) fn pins() -> Query {
    Query::Pins {
        consumer: 9,
        page: first_page(),
    }
}
