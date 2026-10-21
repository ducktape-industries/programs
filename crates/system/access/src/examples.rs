//! A working value of each access op and query, as `#[tool_op]` and `#[tool_query]` name them:
//! what an agent edits rather than build one from the schema.
use ::program::{Need, Ownership, Party};

use crate::{Op, PageRequest, Query};

const PATH: &str = "/home/tech/notes";

fn files() -> String {
    "files".into()
}

pub(crate) fn check() -> Op {
    Op::Check {
        program: files(),
        actor: vec![Party::Account(21)],
        needs: vec![Need::write(PATH.to_owned())],
    }
}

pub(crate) fn set() -> Op {
    Op::Set {
        program: files(),
        key: PATH.into(),
        list: Some(Ownership::Listed(vec![Party::Account(17)])),
    }
}

pub(crate) fn moved() -> Op {
    Op::Move {
        program: files(),
        from: PATH.into(),
        to: "/home/tech/archive".into(),
    }
}

pub(crate) fn migrate() -> Op {
    Op::Migrate {
        height: 300_000,
        first: Some((files(), "/home".into())),
        last: Some((files(), "/shared".into())),
    }
}

pub(crate) fn list() -> Query {
    Query::List {
        program: files(),
        key: PATH.into(),
    }
}

pub(crate) fn resource() -> Query {
    Query::Resource {
        program: files(),
        key: PATH.into(),
    }
}

pub(crate) fn writers() -> Query {
    Query::Writers {
        program: files(),
        key: format!("{PATH}/a.md"),
    }
}

pub(crate) fn lists() -> Query {
    Query::Lists {
        program: files(),
        page: PageRequest {
            after: None,
            limit: Some(20),
        },
    }
}
