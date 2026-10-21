//! A working value of each attribution op and query, as `#[tool_op]` and `#[tool_query]` name
//! them: what an agent edits rather than build one from the schema.
use crate::{Actor, Object, Op, PageRequest, Query, Reason, Relation, Source, Update};

fn object() -> Object {
    Object {
        kind: "message".into(),
        id: "general-176".into(),
    }
}

fn source() -> Source {
    Source {
        program: "chat".into(),
        object: object(),
    }
}

fn update() -> Update {
    Update {
        object: object(),
        revision: 1,
        actor: Actor::Account(21),
        relations: vec![Relation {
            recipient: 17,
            reason: Reason::Mention,
            detail: Vec::new(),
        }],
        transfers: Vec::new(),
    }
}

pub(crate) fn attribute() -> Op {
    Op::Attribute(update())
}

pub(crate) fn attribute_batch() -> Op {
    Op::AttributeBatch {
        updates: vec![update()],
    }
}

pub(crate) fn register() -> Op {
    Op::Register { deliver: vec![0] }
}

pub(crate) fn relations() -> Query {
    Query::Relations { source: source() }
}

pub(crate) fn changes() -> Query {
    Query::Changes {
        page: PageRequest::default(),
    }
}

pub(crate) fn changes_to() -> Query {
    Query::ChangesTo {
        recipient: 17,
        page: PageRequest::default(),
    }
}

pub(crate) fn changes_of() -> Query {
    Query::ChangesOf {
        source: source(),
        page: PageRequest::default(),
    }
}

pub(crate) fn registered() -> Query {
    Query::Registered
}

pub(crate) fn change() -> Query {
    Query::Change { seq: 1 }
}
