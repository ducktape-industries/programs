//! The `programs` program: the roster of programs a network runs, and
//! the scheduled changes to it, made by the validators' vote
//! (`Env::authority`) or by any account, whose change to a program that
//! fills no role lands at least [`NOTICE`] blocks after it is made, so the
//! vote can cancel it first.
//! It fills the kernel's programs role: each block the kernel asks
//! `Query::At(height)` and runs what it answers, and on a roster change
//! `Query::Roles`, the role table the published programs declare. A client asks
//! `Query::Roster`: what runs and is listed at the query's own height. An
//! update is a `Change::Set` of held code under a running id: the
//! kernel swaps the code and keeps the program's state and account; `init`
//! does not run again.
//!
//! The types, rules and [`Programs`] program are always built; a view links
//! them with `program` off. The `program` feature adds its wasm exports.
mod program;
mod rules;
#[cfg(test)]
mod tests;

pub use program::Programs;
pub use store::{PageRequest, PageResponse};

use ::program::{AccountNumber, ProgramId, Roles};
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

pub use abi::role::programs::{Entry, Genesis, View};

pub const PROGRAM: &str = "programs";

pub const CODE_KIND: &str = "program";

pub const NOTICE: u64 = 60;

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Pending {
    pub height: u64,
    pub change: Change,
    pub by: Option<AccountNumber>,
}

pub fn role_bound(roles: &Roles, program: &str) -> bool {
    roles.iter().any(|(_, filled)| filled == program)
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Change {
    Set(Entry),
    Remove(ProgramId),
    /// A view with no program behind it, listed under its name.
    SetView(View),
    RemoveView(ProgramId),
    SetHook(ProgramId),
    RemoveHook(ProgramId),
    Publish {
        entry: Entry,
        roles: Vec<String>,
    },
}

impl Change {
    /// What the change does, in words: `Set`, `Remove`, `Set view`, `Remove view`.
    pub fn verb(&self) -> &'static str {
        match self {
            Change::Set(_) => "Set",
            Change::Remove(_) => "Remove",
            Change::SetView(_) => "Set view",
            Change::RemoveView(_) => "Remove view",
            Change::SetHook(_) => "Set hook",
            Change::RemoveHook(_) => "Remove hook",
            Change::Publish { .. } => "Publish",
        }
    }

    /// The code a set lands; a removal has none.
    pub fn code(&self) -> Option<abi::BlobId> {
        match self {
            Change::Set(entry) | Change::Publish { entry, .. } => Some(entry.code),
            Change::SetView(view) => Some(view.view),
            Change::Remove(_)
            | Change::RemoveView(_)
            | Change::SetHook(_)
            | Change::RemoveHook(_) => None,
        }
    }

    pub fn program(&self) -> &str {
        match self {
            Change::Set(entry) | Change::Publish { entry, .. } => &entry.program,
            Change::SetView(view) => &view.name,
            Change::Remove(program)
            | Change::RemoveView(program)
            | Change::SetHook(program)
            | Change::RemoveHook(program) => program,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Scheduled {
    pub height: u64,
    pub change: Change,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Op {
    Schedule(Scheduled),
    Cancel { height: u64, program: ProgramId },
}

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Ask,
)]
#[ask(Programs)]
pub enum Query {
    #[ask(Reply::Programs(Vec<Entry>))]
    At(u64),
    #[ask(Reply::Scheduled(PageResponse<Pending>))]
    Scheduled { page: PageRequest },
    #[ask(Reply::Program { height: u64, entry: Option<Entry> })]
    Program(ProgramId),
    /// The view-only entries at a height, by name.
    #[ask(Reply::Views(Vec<View>))]
    Views(u64),
    #[ask(Reply::Roles(abi::Roles))]
    Roles,
    #[ask(Reply::Hooks(Vec<ProgramId>))]
    Hooks(u64),
    /// What runs and what is listed now: the programs and the view-only
    /// entries at the query's own height, by name, and that height. A change
    /// lands at its height whether or not an op has run here since.
    #[ask(Reply::Roster { height: u64, programs: Vec<Entry>, views: Vec<View> })]
    Roster,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Programs(Vec<Entry>),
    Scheduled(PageResponse<Pending>),
    Program {
        height: u64,
        entry: Option<Entry>,
    },
    Views(Vec<View>),
    Roles(abi::Roles),
    Hooks(Vec<ProgramId>),
    Roster {
        height: u64,
        programs: Vec<Entry>,
        views: Vec<View>,
    },
}

/// An op as a person reads it: a title and its fields. The source of the
/// `ducktape.describe` module this program ships (`make wasm-describes`).
pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let height = |height: &u64| field("height", Value::Text(height.to_string()));
    let (title, fields) = match op {
        Op::Schedule(Scheduled { height: at, change }) => {
            let mut fields = vec![
                field("change", Value::text(change.verb())),
                field("program", Value::Program(change.program().into())),
                height(at),
            ];
            fields.extend(
                change
                    .code()
                    .map(|code| field("code", Value::Hash(code.digest().to_vec()))),
            );
            match change {
                Change::Set(entry) => fields.push(field("params", Value::bytes(&entry.params))),
                Change::Publish { entry, roles } => {
                    fields.push(field("params", Value::bytes(&entry.params)));
                    fields.push(field(
                        "roles",
                        Value::List(roles.iter().map(Value::text).collect()),
                    ));
                }
                Change::Remove(_)
                | Change::SetView(_)
                | Change::RemoveView(_)
                | Change::SetHook(_)
                | Change::RemoveHook(_) => {}
            }
            (format!("Schedule · {}", change.program()), fields)
        }
        Op::Cancel {
            height: at,
            program,
        } => (
            format!("Cancel · {program}"),
            vec![
                field("program", Value::Program(program.clone())),
                height(at),
            ],
        ),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

/// Old op bytes are described with the current code (`describe`): the op
/// enum only grows at its end. Append a new variant here; never reorder.
#[test]
fn op_variants_only_append() {
    assert_eq!(describe::variants::<Op>(), ["Schedule", "Cancel"]);
}
