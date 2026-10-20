//! The `attribution` program: which accounts each program's objects relate to (a mention,
//! an author, an owner, an assignee), every change to those relations, and the program each
//! change is handed to: the one a scripted recipient's script calls, once it has registered.
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

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Op {
    Attribute(Update),
    AttributeBatch { updates: Vec<Update> },
    Register { deliver: Vec<u8> },
}

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Ask,
)]
#[ask(Attribution)]
pub enum Query {
    #[ask(Reply::Relations(Option<Relations>))]
    #[reads(state::OBJECTS)]
    Relations { source: Source },
    #[ask(Reply::Changes(PageResponse<Change>))]
    #[reads(state::CHANGES)]
    Changes { page: PageRequest },
    #[ask(Reply::Changes(PageResponse<Change>))]
    #[reads(state::TO, state::CHANGES)]
    ChangesTo {
        recipient: AccountNumber,
        page: PageRequest,
    },
    #[ask(Reply::Changes(PageResponse<Change>))]
    #[reads(state::OF, state::CHANGES)]
    ChangesOf { source: Source, page: PageRequest },
    #[ask(Reply::Registered(Vec<ProgramId>))]
    #[reads(state::REGISTERED)]
    Registered,
    #[ask(Reply::Change(Option<Change>))]
    #[reads(state::CHANGES)]
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
