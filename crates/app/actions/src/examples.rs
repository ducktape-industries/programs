//! A working value of each actions op and query, as `#[tool_op]` and `#[tool_query]` name them:
//! what an agent edits rather than build one from the schema.
use crate::{BlobId, Event, Op, Output, PageRequest, Principal, Query, State};

const CHECK: u64 = 42;

fn repo() -> String {
    "programs".into()
}

fn commit() -> String {
    "8dc7009fffe2a2b14b709c152f8649d1b42d295f".into()
}

pub(crate) fn watched() -> Op {
    Op::Watched(forge::Watched::Closed {
        repo: repo(),
        n: 7,
        by: Principal::Account(21),
    })
}

pub(crate) fn trigger() -> Op {
    Op::Trigger {
        repo: repo(),
        commit: commit(),
    }
}

pub(crate) fn register() -> Op {
    Op::Register {
        labels: vec!["linux".into()],
        deliver: Vec::new(),
    }
}

pub(crate) fn claim() -> Op {
    Op::Claim { check: CHECK }
}

pub(crate) fn report() -> Op {
    Op::Report {
        check: CHECK,
        state: State::Passed,
        summary: "12 tests passed".into(),
        log: Some("/shared/ci/42.log".into()),
    }
}

pub(crate) fn rerun() -> Op {
    Op::Rerun { check: CHECK }
}

pub(crate) fn preview() -> Op {
    Op::Preview { repo: repo(), n: 7 }
}

pub(crate) fn outputs() -> Op {
    Op::Outputs {
        check: CHECK,
        files: vec![Output {
            path: "target/app.wasm".into(),
            blob: BlobId::Sha256([0; 32]),
            size: 1024,
        }],
    }
}

pub(crate) fn check() -> Query {
    Query::Check { id: CHECK }
}

pub(crate) fn checks() -> Query {
    Query::Checks {
        repo: repo(),
        commit: commit(),
        page: PageRequest::default(),
    }
}

pub(crate) fn queued() -> Query {
    Query::Queued {
        page: PageRequest::default(),
    }
}

pub(crate) fn run() -> Query {
    Query::Run { id: 9 }
}

pub(crate) fn runners() -> Query {
    Query::Runners {
        page: PageRequest::default(),
    }
}

pub(crate) fn asks() -> Query {
    Query::Asks { check: CHECK }
}

pub(crate) fn needs() -> Query {
    Query::Needs { check: CHECK }
}

pub(crate) fn outputs_of() -> Query {
    Query::Outputs { check: CHECK }
}

pub(crate) fn runs() -> Query {
    Query::Runs {
        repo: repo(),
        event: Event::Push {
            reference: b"refs/heads/dev".to_vec(),
        },
        page: PageRequest::default(),
    }
}

pub(crate) fn time_limit() -> Query {
    Query::TimeLimit { check: CHECK }
}
