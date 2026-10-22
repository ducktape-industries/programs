//! The `attribution` program: which accounts each program's objects relate to (a mention,
//! an author, an owner, an assignee), every change to those relations, and the program each
//! change is handed to: the one a scripted recipient's script calls, once it has registered.
mod examples;
mod program;
mod rules;
mod state;
#[cfg(test)]
mod tests;

pub use ::program::{AccountNumber, Cause, ProgramId};
pub use program::Attribution;
pub use store::{PageRequest, PageResponse};

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

pub const PROGRAM: &str = "attribution";

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, BorshSerialize, BorshDeserialize, BorshSchema,
)]
pub struct Object {
    pub kind: String,
    pub id: String,
}

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, BorshSerialize, BorshDeserialize, BorshSchema,
)]
pub struct Source {
    pub program: ProgramId,
    pub object: Object,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Actor {
    Account(AccountNumber),
    Key(Vec<u8>),
    Program(ProgramId),
    System,
    Foreign(abi::Foreign),
}

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, BorshSerialize, BorshDeserialize, BorshSchema,
)]
pub enum Reason {
    Mention,
    Authorship,
    Ownership,
    Assignment,
    Credit,
    Result,
    Report,
    Defined(String),
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Relation {
    pub recipient: AccountNumber,
    pub reason: Reason,
    pub detail: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Transfer {
    pub reason: Reason,
    pub from: AccountNumber,
    pub to: AccountNumber,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Update {
    pub object: Object,
    pub revision: u64,
    pub actor: Actor,
    pub relations: Vec<Relation>,
    pub transfers: Vec<Transfer>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Kind {
    Added,
    Withdrawn,
    TransferredIn { from: AccountNumber },
    TransferredOut { to: AccountNumber },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Change {
    pub seq: u64,
    pub source: Source,
    pub revision: u64,
    pub recipient: AccountNumber,
    pub reason: Reason,
    pub kind: Kind,
    pub detail: Vec<u8>,
    pub actor: Actor,
    pub cause: Cause,
    pub height: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Relations {
    pub source: Source,
    pub revision: u64,
    pub relations: Vec<Relation>,
}

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Described,
)]
#[tool_about(
    "Which accounts each program's objects relate to (a mention, an author, an owner, an assignee, a credit, a result, a report, or a reason of the program's own), every change to those relations, and the program each change is handed to: the one a scripted recipient's script calls, once it has registered."
)]
pub enum Op {
    #[tool_op(description = "From a program, about one of its objects (a kind and an id, each named and with no /): replace the accounts it relates to, at a revision later than the last one reported, each account once per reason, with any transfers that match a withdrawal and an addition. Each account added or withdrawn is recorded as a change (a new detail alone records none), and each change is handed to the registered program a recipient's script calls when that recipient is an active program account.", example = crate::examples::attribute)]
    Attribute(Update),
    #[tool_op(description = "Attribute several of the sending program's objects in one op, each as Attribute does, in order; one refused refuses them all.", example = crate::examples::attribute_batch)]
    AttributeBatch { updates: Vec<Update> },
    #[tool_op(description = "From a program: be handed each change about an account whose script calls it, as deliver followed by the borsh of the Change; registering again replaces deliver.", example = crate::examples::register)]
    Register { deliver: Vec<u8> },
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
#[ask(Attribution)]
pub enum Query {
    #[ask(Reply::Relations(Option<Relations>))]
    #[reads(state::OBJECTS)]
    #[tool_query(description = "The accounts a program's object relates to at its latest revision, each with its reason and detail; null for an object never reported.", example = crate::examples::relations)]
    Relations { source: Source },
    #[ask(Reply::Changes(PageResponse<Change>))]
    #[reads(state::CHANGES)]
    #[tool_query(description = "A page of every change to relations, oldest first: each with its object, revision, recipient, reason, kind (added, withdrawn, transferred in or out), actor, cause and height.", example = crate::examples::changes)]
    Changes { page: PageRequest },
    #[ask(Reply::Changes(PageResponse<Change>))]
    #[reads(state::TO, state::CHANGES)]
    #[tool_query(description = "A page of the changes about one account, oldest first: what an account's inbox reads.", example = crate::examples::changes_to)]
    ChangesTo {
        recipient: AccountNumber,
        page: PageRequest,
    },
    #[ask(Reply::Changes(PageResponse<Change>))]
    #[reads(state::OF, state::CHANGES)]
    #[tool_query(description = "A page of the changes to one program's object, oldest first.", example = crate::examples::changes_of)]
    ChangesOf { source: Source, page: PageRequest },
    #[ask(Reply::Registered(Vec<ProgramId>))]
    #[reads(state::REGISTERED)]
    #[tool_query(description = "The programs registered to be handed changes.", example = crate::examples::registered)]
    Registered,
    #[ask(Reply::Change(Option<Change>))]
    #[reads(state::CHANGES)]
    #[tool_query(description = "One change by its number; null for none.", example = crate::examples::change)]
    Change { seq: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Relations(Option<Relations>),
    Changes(PageResponse<Change>),
    Registered(Vec<ProgramId>),
    Change(Option<Change>),
}

impl Change {
    pub fn payload(deliver: &[u8], change: &Change) -> Vec<u8> {
        let mut payload = deliver.to_vec();
        payload.extend(abi::encode(change));
        payload
    }
}

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let object = |update: &Update| {
        field(
            "object",
            Value::text(format!("{}/{}", update.object.kind, update.object.id)),
        )
    };
    let recipients = |update: &Update| {
        field(
            "recipients",
            Value::List(
                update
                    .relations
                    .iter()
                    .map(|relation| Value::Account(relation.recipient))
                    .collect(),
            ),
        )
    };
    let (title, fields) = match op {
        Op::Attribute(update) => (
            format!("Attribute revision {}", update.revision),
            vec![object(update), recipients(update)],
        ),
        Op::AttributeBatch { updates } => (
            format!("Attribute {} objects", updates.len()),
            updates.iter().map(object).collect(),
        ),
        Op::Register { .. } => (
            "Register to be handed changes about the accounts whose script calls this program"
                .to_owned(),
            Vec::new(),
        ),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        ["Attribute", "AttributeBatch", "Register"]
    );
}
