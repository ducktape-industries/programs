//! A working value of each ownership op and query, as `#[tool_op]` and `#[tool_query]` name
//! them: what an agent edits rather than build one from the schema.
use abi::role::ownership::{Guard, Write};

use crate::{Adopted, Op, PageRequest, Party, Query, Rule, Scope, Writers};

const POLICY: u64 = 3;

fn files() -> String {
    "files".into()
}

fn key() -> Vec<u8> {
    b"f/notes.md".to_vec()
}

fn me() -> Party {
    Party::Account(21)
}

pub(crate) fn guard() -> Op {
    Op::Guard(Guard {
        program: files(),
        origin: abi::Origin::System,
        sender: Some(abi::Principal::Account(21)),
        writes: vec![Write {
            key: key(),
            claimed: Some("file".into()),
            existed: false,
            kept: true,
        }],
    })
}

pub(crate) fn create() -> Op {
    Op::Create {
        name: "Team".into(),
        write: Writers::Listed(vec![Party::Account(17)]),
    }
}

pub(crate) fn edit() -> Op {
    Op::Edit {
        policy: POLICY,
        name: "Team".into(),
        write: Writers::Listed(vec![Party::Account(17), Party::Account(56)]),
    }
}

pub(crate) fn retire() -> Op {
    Op::Retire { policy: POLICY }
}

pub(crate) fn switch() -> Op {
    Op::Switch {
        from: POLICY,
        to: POLICY + 1,
    }
}

pub(crate) fn relink() -> Op {
    Op::Relink {
        program: files(),
        key: key(),
        policy: POLICY,
    }
}

pub(crate) fn transfer() -> Op {
    Op::Transfer {
        program: files(),
        key: key(),
        to: Party::Account(17),
    }
}

pub(crate) fn set_default() -> Op {
    Op::SetDefault {
        scope: Scope::Program(files()),
        policy: Some(POLICY),
    }
}

pub(crate) fn adopt() -> Op {
    Op::Adopt {
        items: vec![Adopted {
            key: key(),
            owner: me(),
            kind: Some("file".into()),
            rule: Rule::OwnerOnly,
        }],
    }
}

pub(crate) fn moved() -> Op {
    Op::Moved {
        from_key: key(),
        to_key: b"f/notes-old.md".to_vec(),
    }
}

pub(crate) fn record() -> Query {
    Query::Record {
        program: files(),
        key: key(),
    }
}

pub(crate) fn effective() -> Query {
    Query::Effective {
        program: files(),
        key: key(),
    }
}

pub(crate) fn policy() -> Query {
    Query::Policy { id: POLICY }
}

pub(crate) fn policies() -> Query {
    Query::Policies {
        owner: me(),
        page: PageRequest::default(),
    }
}

pub(crate) fn linked() -> Query {
    Query::Linked {
        policy: POLICY,
        page: PageRequest::default(),
    }
}

pub(crate) fn defaults() -> Query {
    Query::Defaults {
        owner: me(),
        page: PageRequest::default(),
    }
}

pub(crate) fn open() -> Query {
    Query::Open { owner: me() }
}

pub(crate) fn only_me() -> Query {
    Query::OnlyMe { owner: me() }
}
