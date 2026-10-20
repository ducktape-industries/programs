//! A working value of each op and query, as `#[tool_op]` and `#[tool_query]` name them: what an
//! agent edits rather than build one from the schema.
use ::program::Principal;

use crate::{Op, Query, Rule};

pub(crate) fn watched() -> Op {
    Op::Watched(forge::Watched::Closed {
        repo: "project".into(),
        n: 7,
        by: Principal::Account(21),
    })
}

pub(crate) fn set_rules() -> Op {
    Op::SetRules {
        rules: vec![Rule {
            branch: "issue-{}/".into(),
            room: "issue-{}".into(),
        }],
    }
}

pub(crate) fn rules() -> Query {
    Query::Rules
}

pub(crate) fn mirrored() -> Query {
    Query::Mirrored {
        page: store::PageRequest::default(),
    }
}

pub(crate) fn room() -> Query {
    Query::Room {
        repo: "project".into(),
        n: 7,
    }
}
