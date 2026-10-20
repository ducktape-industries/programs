//! A working value of each files op and query, as `#[tool_op]` and `#[tool_query]` name them:
//! what an agent edits rather than build one from the schema.
use std::collections::BTreeMap;

use crate::{BlobId, Change, Content, Op, PageRequest, Query};

fn id() -> BlobId {
    BlobId::Sha256([0; 32])
}

fn every() -> PageRequest {
    PageRequest {
        after: None,
        limit: None,
    }
}

const README: &str = "/shared/tools/README.md";

pub(crate) fn stage() -> Op {
    Op::Stage { bytes: Vec::new() }
}

pub(crate) fn commit() -> Op {
    let put = Change::Put {
        path: "/home/tech/notes.md".into(),
        exec: false,
        meta: BTreeMap::new(),
        content: Content::Chunks {
            size: 5,
            chunks: vec![id()],
        },
    };
    Op::Commit {
        base: None,
        message: "notes".into(),
        changes: vec![put],
    }
}

pub(crate) fn pin() -> Op {
    Op::Pin {
        name: "release-1".into(),
        snapshot: id(),
    }
}

pub(crate) fn unpin() -> Op {
    Op::Unpin {
        name: "release-1".into(),
    }
}

pub(crate) fn watch() -> Op {
    Op::Watch { deliver: vec![0] }
}

pub(crate) fn head() -> Query {
    Query::Head
}

pub(crate) fn snapshot() -> Query {
    Query::Snapshot { id: id() }
}

pub(crate) fn history() -> Query {
    Query::History {
        page: PageRequest {
            after: None,
            limit: Some(20),
        },
    }
}

pub(crate) fn stat() -> Query {
    Query::Stat {
        path: README.into(),
        at: None,
    }
}

pub(crate) fn list() -> Query {
    Query::List {
        path: "/shared/jobs".into(),
        at: None,
        page: every(),
    }
}

pub(crate) fn read() -> Query {
    Query::Read {
        path: README.into(),
        at: None,
        offset: 0,
        len: 65536,
    }
}

pub(crate) fn find() -> Query {
    Query::Find {
        prefix: "/shared/jobs/".into(),
        at: None,
        page: every(),
    }
}

pub(crate) fn grep() -> Query {
    Query::Grep {
        pattern: "warm pod".into(),
        prefix: "/shared/tools/".into(),
        at: None,
        page: every(),
    }
}

pub(crate) fn diff() -> Query {
    Query::Diff {
        from: id(),
        to: id(),
        prefix: "/shared/".into(),
    }
}

pub(crate) fn pins() -> Query {
    Query::Pins { page: every() }
}

pub(crate) fn one_pin() -> Query {
    Query::Pin {
        name: "release-1".into(),
    }
}

pub(crate) fn guard() -> Op {
    Op::Guard {
        path: "/home/tech/notes".into(),
    }
}

pub(crate) fn unguard() -> Op {
    Op::Unguard {
        path: "/home/tech/notes".into(),
    }
}

pub(crate) fn adopt() -> Op {
    Op::AdoptOwners {
        kind: "files.path".into(),
        after: None,
        limit: 100,
    }
}

pub(crate) fn vacate() -> Op {
    Op::Vacate {
        paths: vec!["/home/tech/notes".into()],
    }
}

pub(crate) fn guarded() -> Query {
    Query::Guarded {
        path: "/home/tech/notes/today.md".into(),
    }
}

pub(crate) fn owner() -> Query {
    Query::Owner {
        path: "/home/tech/notes".into(),
    }
}
