//! The `ownership` program: who may write each owned key a program holds. The kernel asks it once
//! per run that wrote owned keys (`Op::Guard`); it records each new key's owner and policy and
//! admits a change only where the key's policy admits the actor. `program` adds the wasm exports.
mod adopt;
mod examples;
mod program;
mod rules;
#[cfg(test)]
mod tests;

use std::fmt;

use ::program::{AccountNumber, ProgramId};
use abi::role::ownership::Guard;
pub use adopt::{Adopted, Rule, adopt, again_next_block, by_itself, owner_of, rule_of, writers_of};
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
pub use program::Ownership;
use rules::{DEFAULTS, LINKED, ONLY_ME_POLICIES, OPEN, OWNED, POLICIES, RECORDS};
pub use store::{PageRequest, PageResponse};

pub const PROGRAM: &str = "ownership";
pub const OPEN_POLICY: &str = "Open";
pub const ONLY_ME: &str = "Only me";

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

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Described,
)]
#[tool_about(
    "Who may write each owned key a program holds: every item has an owner and links to one of its owner's policies, which names who besides the owner may write it; the kernel asks once per run that wrote owned keys, and a write the policy does not admit is refused."
)]
pub enum Op {
    #[tool_op(description = "The kernel's alone, once per run that wrote owned keys: a key the run creates and claims as a kind is recorded as the actor's (the account the frame acts as, else the program) and linked to the actor's default for that kind, then program, then everything, else their built-in Only me; a write to a recorded key is refused unless its effective policy admits the actor; a recorded key the run dropped loses its record. A refusal writes nothing.", example = crate::examples::guard)]
    Guard(Guard),
    #[tool_op(description = "Make a policy the sender owns: a name, and who besides the owner may write the items linked to it (Anyone, the Owner alone, or the Listed parties). Returns its id.", example = crate::examples::create)]
    Create { name: String, write: Writers },
    #[tool_op(description = "Rename a policy the sender owns and change who may write its items. A built-in policy (Open, Only me) never changes.", example = crate::examples::edit)]
    Edit {
        policy: PolicyId,
        name: String,
        write: Writers,
    },
    #[tool_op(description = "Retire a policy the sender owns, not retired already, dropping every default of theirs that names it; its items stay under it. A built-in policy (Open, Only me) is never retired.", example = crate::examples::retire)]
    Retire { policy: PolicyId },
    #[tool_op(description = "Retire policy from and let policy to govern its items from then on (they stay linked to from: Record and Linked still name from, Effective follows the switch): two different policies, both the sender's, to not retired, from not switched before and not a built-in (Open, Only me).", example = crate::examples::switch)]
    Switch { from: PolicyId, to: PolicyId },
    #[tool_op(description = "Link an item the sender owns (program's owned key) to another of its owner's policies, not retired.", example = crate::examples::relink)]
    Relink {
        program: ProgramId,
        key: Vec<u8>,
        policy: PolicyId,
    },
    #[tool_op(description = "Hand an item the sender owns (program's owned key) to another account or program that exists; it keeps its policy.", example = crate::examples::transfer)]
    Transfer {
        program: ProgramId,
        key: Vec<u8>,
        to: Party,
    },
    #[tool_op(description = "Set the policy the sender's new items link to in scope (a Kind of item, a Program, or Global; the most specific wins, else their built-in Only me), or clear it with null. The policy is the sender's and not retired.", example = crate::examples::set_default)]
    SetDefault {
        scope: Scope,
        policy: Option<PolicyId>,
    },
    #[tool_op(description = "One-time, from a program: record owners for the owned rows it kept before the guard, each linked by its rule (Open: the owner's open policy; OwnerOnly: their built-in Only me; List: one policy per access list it converts, owned by the party it names). A row recorded already is left as it is unless the program itself holds it, so a second run changes nothing.", example = crate::examples::adopt)]
    Adopt { items: Vec<Adopted> },
    #[tool_op(description = "From a program, in its own message as its own account: move the record of one of its owned keys to a key that has none.", example = crate::examples::moved)]
    Moved { from_key: Vec<u8>, to_key: Vec<u8> },
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
#[ask(Ownership)]
pub enum Query {
    #[ask(Reply::Record(Option<crate::Record>))]
    #[reads(RECORDS)]
    #[tool_query(description = "The owner, policy and kind recorded for program's owned key; null for a key with no record.", example = crate::examples::record)]
    Record { program: ProgramId, key: Vec<u8> },
    #[ask(Reply::Policy(Option<crate::Policy>))]
    #[reads(RECORDS, POLICIES)]
    #[tool_query(description = "The policy that governs program's key now: the one it links to, or, once it was switched, the policy the switches end at; null for a key with no record.", example = crate::examples::effective)]
    Effective { program: ProgramId, key: Vec<u8> },
    #[ask(Reply::Policy(Option<crate::Policy>))]
    #[reads(POLICIES)]
    #[tool_query(description = "One policy by id: its owner, name, writers, and whether it is retired or switched; null for none.", example = crate::examples::policy)]
    Policy { id: PolicyId },
    #[ask(Reply::Policies(PageResponse<crate::Policy>))]
    #[reads(OWNED, POLICIES)]
    #[tool_query(description = "A page of an owner's policies, in id order.", example = crate::examples::policies)]
    Policies { owner: Party, page: PageRequest },
    #[ask(Reply::Linked(PageResponse<crate::Linked>))]
    #[reads(LINKED)]
    #[tool_query(description = "A page of the items linked to a policy, each by program and key.", example = crate::examples::linked)]
    Linked { policy: PolicyId, page: PageRequest },
    #[ask(Reply::Defaults(PageResponse<ScopeDefault>))]
    #[reads(DEFAULTS)]
    #[tool_query(description = "A page of an owner's defaults: each a scope and the policy their new items there link to.", example = crate::examples::defaults)]
    Defaults { owner: Party, page: PageRequest },
    #[ask(Reply::Open(Option<PolicyId>))]
    #[reads(OPEN)]
    #[tool_query(description = "An owner's built-in Open policy, which lets anyone write and never changes, made the first time one of their items is adopted open; null until then.", example = crate::examples::open)]
    Open { owner: Party },
    #[ask(Reply::OnlyMe(Option<PolicyId>))]
    #[reads(ONLY_ME_POLICIES)]
    #[tool_query(description = "An owner's built-in Only me policy, which lets only the owner write and never changes: what their new items link to when no default of theirs applies, made the first time one needs it; null until then.", example = crate::examples::only_me)]
    OnlyMe { owner: Party },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Record(Option<Record>),
    Policy(Option<Policy>),
    Policies(PageResponse<Policy>),
    Linked(PageResponse<Linked>),
    Defaults(PageResponse<ScopeDefault>),
    Open(Option<PolicyId>),
    OnlyMe(Option<PolicyId>),
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
        Op::Adopt { items } => (
            "Record owners from before the guard".into(),
            vec![field("items", Value::count(items.len() as u64))],
        ),
        Op::Moved { from_key, to_key } => (
            "Move a record to another key".into(),
            vec![
                field("from", Value::bytes(from_key)),
                field("to", Value::bytes(to_key)),
            ],
        ),
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
            "SetDefault",
            "Adopt",
            "Moved"
        ]
    );
}
