//! The `ownership` program: who may write each owned key a program holds. The kernel asks it once
//! per run that wrote owned keys (`Op::Guard`); it records each new key's owner and policy and
//! admits a change only where the key's policy admits the actor. `program` adds the wasm exports.
mod program;
mod rules;
#[cfg(test)]
mod tests;

use std::fmt;

use ::program::{AccountNumber, ProgramId};
use abi::role::ownership::Guard;
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
pub use program::Ownership;
use rules::{DEFAULTS, LINKED, OPEN, OWNED, POLICIES, RECORDS};
pub use store::{PageRequest, PageResponse};

pub const PROGRAM: &str = "ownership";
// what an owner's open policy, the one every item links to until they choose, is named
pub const OPEN_POLICY: &str = "Open";

pub type PolicyId = u64;

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
)]
pub enum Party {
    Account(AccountNumber),
    Program(ProgramId),
}

impl fmt::Display for Party {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Party::Account(number) => write!(f, "account {number}"),
            Party::Program(program) => write!(f, "program {program}"),
        }
    }
}

// Who a policy lets write the items linked to it, beside their owner, who always may.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Writers {
    Anyone,
    Owner,
    Listed(Vec<Party>),
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Policy {
    pub id: PolicyId,
    pub owner: Party,
    pub name: String,
    pub write: Writers,
    pub retired: bool,
    // the policy a switch moved this one's items to, which governs them from then on
    pub switched_to: Option<PolicyId>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Record {
    pub owner: Party,
    pub policy: PolicyId,
    pub kind: Option<String>,
}

// Where a default applies, most specific first: a kind of item, a program, everything.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Scope {
    Kind(String),
    Program(ProgramId),
    Global,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Linked {
    pub program: ProgramId,
    pub key: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct ScopeDefault {
    pub scope: Scope,
    pub policy: PolicyId,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Op {
    // the kernel's, `abi::role::ownership::Op::Guard`'s bytes
    Guard(Guard),
    Create {
        name: String,
        write: Writers,
    },
    Edit {
        policy: PolicyId,
        name: String,
        write: Writers,
    },
    Retire {
        policy: PolicyId,
    },
    Switch {
        from: PolicyId,
        to: PolicyId,
    },
    Relink {
        program: ProgramId,
        key: Vec<u8>,
        policy: PolicyId,
    },
    Transfer {
        program: ProgramId,
        key: Vec<u8>,
        to: Party,
    },
    SetDefault {
        scope: Scope,
        policy: Option<PolicyId>,
    },
}

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Ask,
)]
#[ask(Ownership)]
pub enum Query {
    #[ask(Reply::Record(Option<crate::Record>))]
    #[reads(RECORDS)]
    Record { program: ProgramId, key: Vec<u8> },
    #[ask(Reply::Policy(Option<crate::Policy>))]
    #[reads(RECORDS, POLICIES)]
    Effective { program: ProgramId, key: Vec<u8> },
    #[ask(Reply::Policy(Option<crate::Policy>))]
    #[reads(POLICIES)]
    Policy { id: PolicyId },
    #[ask(Reply::Policies(PageResponse<crate::Policy>))]
    #[reads(OWNED, POLICIES)]
    Policies { owner: Party, page: PageRequest },
    #[ask(Reply::Linked(PageResponse<crate::Linked>))]
    #[reads(LINKED)]
    Linked { policy: PolicyId, page: PageRequest },
    #[ask(Reply::Defaults(PageResponse<ScopeDefault>))]
    #[reads(DEFAULTS)]
    Defaults { owner: Party, page: PageRequest },
    #[ask(Reply::Open(Option<PolicyId>))]
    #[reads(OPEN)]
    Open { owner: Party },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Record(Option<Record>),
    Policy(Option<Policy>),
    Policies(PageResponse<Policy>),
    Linked(PageResponse<Linked>),
    Defaults(PageResponse<ScopeDefault>),
    Open(Option<PolicyId>),
}

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let policy = |id: &PolicyId| field("policy", Value::text(format!("#{id}")));
    let (title, fields) = match op {
        Op::Guard(guard) => (
            format!("Guard · {}", guard.program),
            vec![field("writes", Value::count(guard.writes.len() as u64))],
        ),
        Op::Create { name, write } => (
            format!("New policy · {name}"),
            vec![field("write", writers(write))],
        ),
        Op::Edit {
            policy: id,
            name,
            write,
        } => (
            format!("Edit policy · {name}"),
            vec![policy(id), field("write", writers(write))],
        ),
        Op::Retire { policy: id } => ("Remove policy".into(), vec![policy(id)]),
        Op::Switch { from, to } => (
            "Switch a policy's items".into(),
            vec![
                field("from", Value::text(format!("#{from}"))),
                field("to", Value::text(format!("#{to}"))),
            ],
        ),
        Op::Relink {
            program,
            key,
            policy: id,
        } => (
            format!("Relink · {program}"),
            vec![field("key", Value::bytes(key)), policy(id)],
        ),
        Op::Transfer { program, key, to } => (
            format!("Transfer · {program}"),
            vec![field("key", Value::bytes(key)), field("to", party(to))],
        ),
        Op::SetDefault { scope, policy: id } => {
            let scope = match scope {
                Scope::Kind(kind) => format!("kind {kind}"),
                Scope::Program(program) => format!("program {program}"),
                Scope::Global => "everything".into(),
            };
            let chosen = match id {
                Some(id) => Value::text(format!("#{id}")),
                None => Value::text("cleared"),
            };
            (
                format!("Default policy · {scope}"),
                vec![field("policy", chosen)],
            )
        }
    };
    describe::Description { title, fields }
}

fn party(party: &Party) -> describe::Value {
    match party {
        Party::Account(number) => describe::Value::Account(*number),
        Party::Program(program) => describe::Value::Program(program.clone()),
    }
}

fn writers(write: &Writers) -> describe::Value {
    use describe::Value;
    match write {
        Writers::Anyone => Value::text("anyone"),
        Writers::Owner => Value::text("its owner"),
        Writers::Listed(parties) => Value::List(
            std::iter::once(Value::text("its owner"))
                .chain(parties.iter().map(party))
                .collect(),
        ),
    }
}

describe::export!(Op, describe);

#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        [
            "Guard",
            "Create",
            "Edit",
            "Retire",
            "Switch",
            "Relink",
            "Transfer",
            "SetDefault"
        ]
    );
}
