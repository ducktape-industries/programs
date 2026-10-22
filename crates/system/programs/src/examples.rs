//! A working value of each programs op and query, as `#[tool_op]` and `#[tool_query]` name them:
//! what an agent edits rather than build one from the schema.
use crate::{Change, Entry, Holder, Op, PageRequest, Query, Scheduled};

const AT: u64 = 300_000;

fn chat() -> String {
    "chat".into()
}

fn set_chat() -> Scheduled {
    Scheduled {
        height: AT,
        change: Change::Set(Entry {
            program: chat(),
            code: abi::BlobId::Sha256([0; 32]),
            params: Vec::new(),
        }),
    }
}

pub(crate) fn schedule_over() -> Op {
    Op::ScheduleOver {
        scheduled: set_chat(),
        expected: Some(abi::BlobId::Sha256([1; 32])),
    }
}

pub(crate) fn cancel() -> Op {
    Op::Cancel {
        height: AT,
        program: chat(),
    }
}

pub(crate) fn own() -> Op {
    Op::Own {
        program: chat(),
        holder: Holder::Accounts(vec![17, 21]),
    }
}

pub(crate) fn at() -> Query {
    Query::At(AT)
}

pub(crate) fn scheduled() -> Query {
    Query::Scheduled {
        page: PageRequest {
            after: None,
            limit: Some(20),
        },
    }
}

pub(crate) fn program() -> Query {
    Query::Program(chat())
}

pub(crate) fn roles() -> Query {
    Query::Roles
}

pub(crate) fn hooks() -> Query {
    Query::Hooks(AT)
}

pub(crate) fn roster() -> Query {
    Query::Roster
}

pub(crate) fn sources() -> Query {
    Query::Sources
}

pub(crate) fn owner() -> Query {
    Query::Owner(chat())
}

pub(crate) fn programs_at() -> Query {
    Query::ProgramsAt { at_height: AT }
}

pub(crate) fn views_at() -> Query {
    Query::ViewsAt { at_height: AT }
}

pub(crate) fn hooks_at() -> Query {
    Query::HooksAt { at_height: AT }
}

pub(crate) fn renames() -> Query {
    Query::Renames(AT)
}
