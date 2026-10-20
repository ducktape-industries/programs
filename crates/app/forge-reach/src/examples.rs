//! A working value of each forge-reach op and query, as `#[tool_op]` and `#[tool_query]` name
//! them: what an agent edits rather than build one from the schema.
use crate::{CheckState, Op, PageRequest, Query};

pub(crate) fn watched() -> Op {
    Op::Watched(forge::Watched::Previewed {
        repo: "programs".into(),
        n: 12,
        merge: "1".repeat(40),
        by: forge::Principal::Account(17),
    })
}

pub(crate) fn report() -> Op {
    Op::Report {
        repo: "programs".into(),
        commit: "0".repeat(40),
        context: "build.sh#lane".into(),
        state: CheckState::Passed,
        url: Some("duck://testnet-0a1b2c3d/files/runs/lane.log".into()),
        reach: vec![
            "Cargo.lock".into(),
            "Cargo.toml".into(),
            "crates/app/forge".into(),
        ],
        change: Some(12),
    }
}

pub(crate) fn reports() -> Query {
    Query::Reports {
        repo: "programs".into(),
        commit: "0".repeat(40),
        page: PageRequest {
            after: None,
            limit: Some(20),
        },
    }
}

pub(crate) fn verdict() -> Query {
    Query::Verdict {
        repo: "programs".into(),
        n: 12,
    }
}
