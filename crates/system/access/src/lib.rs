//! The `access` program: who may write each key another program names. A program answers the
//! kernel what an op needs and who owns a key; this program keeps each key's write list, and the
//! kernel asks it, before every op runs, whether the op's actor may. Types, rules and [`Access`]
//! always build; `program` adds the wasm exports.
mod examples;
mod program;
mod rules;
#[cfg(test)]
mod tests;

use ::program::{Need, Ownership, Party, ProgramId, Resource, Writers};
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
pub use program::Access;
pub use store::{PageRequest, PageResponse};

pub const PROGRAM: &str = "access";

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Described,
)]
#[tool_about(
    "Who may write each key another program names: each key's write list. A key's writers are the nearest list at or above it, else its program's default for its owner, else anyone."
)]
pub enum Op {
    #[tool_op(description = "The kernel alone: judges each need of an op before it runs and refuses the first the actor may not.", example = examples::check)]
    Check {
        program: ProgramId,
        actor: Vec<Party>,
        needs: Vec<Need>,
    },
    #[tool_op(description = "Set a key's write list (Owner: its owner alone; Listed: its owner and the accounts and programs listed; Anyone), or clear it with null. The key's owner, or the program itself for its own keys. Each listed account and program must exist.", example = examples::set)]
    Set {
        program: ProgramId,
        key: String,
        list: Option<Ownership>,
    },
    #[tool_op(description = "The program alone: move every list at and below from to the same place under to, as when a path is renamed.", example = examples::moved)]
    Move {
        program: ProgramId,
        from: String,
        to: String,
    },
    #[tool_op(description = "Rewrite the program ids access stores by the programs role's Renames at height: each list a renamed program's keys carry moves to its new id (a list already set there is kept), and each renamed program a list names follows, over the lists first..=last (each [program, key]; null: from the first, or to the last). Anyone: it reads the renames from the programs role, never from its sender, so running it again changes nothing.", example = examples::migrate)]
    Migrate {
        height: u64,
        first: Option<(ProgramId, String)>,
        last: Option<(ProgramId, String)>,
    },
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    ::program::Ask,
    ::program::Described,
)]
#[ask(Access)]
pub enum Query {
    #[ask(Reply::List(Option<Ownership>))]
    #[tool_query(description = "The list set on one key, or null when none is.", example = examples::list)]
    List { program: ProgramId, key: String },
    #[ask(Reply::Resource(Resource))]
    #[tool_query(description = "What the program answers for a key: its owner and its default list.", example = examples::resource)]
    Resource { program: ProgramId, key: String },
    #[ask(Reply::Writers(Writers))]
    #[tool_query(description = "Who writes a key: anyone, or the list that decides it, where it is set and whose key it is.", example = examples::writers)]
    Writers { program: ProgramId, key: String },
    #[ask(Reply::Lists(PageResponse<Listed>))]
    #[tool_query(description = "Every list a program's keys carry, a page at a time.", example = examples::lists)]
    Lists {
        program: ProgramId,
        page: PageRequest,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Listed {
    pub key: String,
    pub list: Ownership,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    List(Option<Ownership>),
    Resource(Resource),
    Writers(Writers),
    Lists(PageResponse<Listed>),
}

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let (title, fields) = match op {
        Op::Check { program, needs, .. } => (
            format!("Check · {program}"),
            vec![field("needs", Value::count(needs.len() as u64))],
        ),
        Op::Set { program, key, list } => (
            format!("Write list · {program} {key}"),
            vec![
                field("program", Value::Program(program.clone())),
                field("key", Value::text(key)),
                field("list", listed(list.as_ref())),
            ],
        ),
        Op::Move { program, from, to } => (
            format!("Move lists · {program} {from}"),
            vec![
                field("program", Value::Program(program.clone())),
                field("from", Value::text(from)),
                field("to", Value::text(to)),
            ],
        ),
        Op::Migrate {
            height,
            first,
            last,
        } => {
            let bound = |bound: &Option<(ProgramId, String)>, open: &str| match bound {
                Some((program, key)) => Value::Text(format!("{program} {key}")),
                None => Value::Text(open.into()),
            };
            (
                "Migrate".into(),
                vec![
                    field("height", Value::Text(height.to_string())),
                    field("first", bound(first, "the first")),
                    field("last", bound(last, "the last")),
                ],
            )
        }
    };
    describe::Description { title, fields }
}

fn listed(list: Option<&Ownership>) -> describe::Value {
    use describe::Value;
    match list {
        None => Value::text("cleared"),
        Some(Ownership::Owner) => Value::text("its owner"),
        Some(Ownership::Anyone) => Value::text("anyone"),
        Some(Ownership::Listed(parties)) => Value::List(
            std::iter::once(Value::text("its owner"))
                .chain(parties.iter().map(|party| match party {
                    Party::Account(number) => Value::Account(*number),
                    Party::Program(program) => Value::Program(program.clone()),
                }))
                .collect(),
        ),
    }
}

describe::export!(Op, describe);

#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        ["Check", "Set", "Move", "Migrate"]
    );
}
