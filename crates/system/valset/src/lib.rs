//! The `valset` program: who validates and who resides on a network. A key enrolls as a
//! resident, by invite or through an open door; the validators' vote changes the rest. A
//! validator votes with its node key, or through the user who manages the program account its
//! node key sits on: any of that user's devices votes for it.
//! Types, rules and [`Valset`] always build; the `program` feature adds the wasm exports.
mod program;
mod rules;
#[cfg(test)]
mod tests;
mod tools;

pub use program::Valset;

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
pub use store::{PageRequest, PageResponse};

pub use abi::role::validators::{
    Genesis, Grant, INVITE_NAMESPACE, Invite, Member, Motion, Op, Voted,
};

pub const PROGRAM: &str = "valset";

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Role {
    Validator,
    Resident,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Membership {
    pub key: Vec<u8>,
    pub address: Option<String>,
    pub halo: String,
    pub role: Role,
}

impl Membership {
    pub fn member(&self) -> Member {
        Member {
            key: self.key.clone(),
            address: self.address.clone(),
            halo: self.halo.clone(),
        }
    }
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
#[tool_about(
    "The network's members and their roles, the motions waiting for votes, and whom a key votes for."
)]
#[ask(Valset)]
pub enum Query {
    #[ask(Reply::Validators(Vec<Vec<u8>>))]
    #[tool_query(description = "The seated validators' node keys, in key order.", example = crate::tools::validators)]
    Validators,
    #[ask(Reply::Members(Vec<Member>))]
    #[tool_query(description = "Every member, validator or resident, with its key, address and halo.", example = crate::tools::members)]
    Members,
    #[ask(Reply::Memberships(PageResponse<Membership>))]
    #[tool_query(description = "A page of the members, in key order, each with its role.", example = crate::tools::memberships)]
    Memberships { page: PageRequest },
    #[ask(Reply::Membership(Option<Membership>))]
    #[tool_query(description = "One key's membership: its address, halo and role; null for a key that is not a member.", example = crate::tools::membership)]
    Membership { key: Vec<u8> },
    #[ask(Reply::Motions { needed: u64, page: PageResponse<Pending> })]
    #[tool_query(description = "A page of the motions waiting for votes, in their bytes' order, each with the seated validators that voted for it, and how many votes pass one now.", example = crate::tools::motions)]
    Motions { page: PageRequest },
    #[ask(Reply::Voices(Vec<Vec<u8>>))]
    #[tool_query(description = "The validators a frame signed by key votes for: key itself when it is one, else the validators whose node keys sit on active program accounts that key's account manages. Empty: key does not vote.", example = crate::tools::voices)]
    Voices { key: Vec<u8> },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Validators(Vec<Vec<u8>>),
    Members(Vec<Member>),
    Memberships(PageResponse<Membership>),
    Membership(Option<Membership>),
    Motions {
        needed: u64,
        page: PageResponse<Pending>,
    },
    Voices(Vec<Vec<u8>>),
}

/// A motion waiting for votes, and the validators that cast one: only
/// those still seated, as the tally counts them.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Pending {
    pub motion: Motion,
    pub voters: Vec<Vec<u8>>,
}

/// The ask another program makes of valset, at the program genesis bound to
/// the validators role.
pub fn role(ctx: &::program::QueryCtx, key: &[u8]) -> Result<Option<Role>, ::program::Error> {
    let validators = &ctx.env().role(abi::role::VALIDATORS)?.clone();
    match ctx.query::<Query, Reply>(validators, &Query::Membership { key: key.to_vec() })? {
        Reply::Membership(membership) => Ok(membership.map(|membership| membership.role)),
        other => Err(::program::unexpected_reply(
            validators,
            "Membership",
            &other,
        )),
    }
}

/// An op as a person reads it: a title and its fields. The source of the
/// `ducktape.describe` module this program ships (`make wasm-describes`).
pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let (title, fields) = match op {
        Op::Enroll {
            address,
            halo,
            invite,
        } => (
            address
                .as_ref()
                .map_or("Enroll".into(), |address| format!("Enroll · {address}")),
            vec![
                field("address", Value::text(address.as_deref().unwrap_or("none"))),
                field("halo", Value::text(halo)),
                field(
                    "invite",
                    invite.as_ref().map_or(Value::text("none"), |invite| {
                        Value::Key(invite.issuer.clone())
                    }),
                ),
            ],
        ),
        Op::Leave => ("Leave".into(), vec![]),
        Op::SetHalo { halo } => (
            format!("Halo · {halo}"),
            vec![field("halo", Value::text(halo))],
        ),
        Op::Vote(motion) => {
            let key = |key: &Vec<u8>| vec![field("key", Value::Key(key.clone()))];
            let (verb, fields) = match motion {
                Motion::Promote { key: promoted } => ("promote".into(), key(promoted)),
                Motion::Demote { key: demoted } => ("demote".into(), key(demoted)),
                Motion::Remove { key: removed } => ("remove".into(), key(removed)),
                Motion::Door { open: true } => ("open the door".into(), vec![]),
                Motion::Door { open: false } => ("close the door".into(), vec![]),
                Motion::Role { role, op } => described_op(role, op),
            };
            (format!("Vote · {verb}"), fields)
        }
    };
    describe::Description { title, fields }
}

fn described_op(role: &str, op: &[u8]) -> (String, Vec<describe::Field>) {
    use describe::{Value, field};
    let described = match role {
        abi::role::PROGRAMS => abi::decode(op).ok().map(|op| programs::describe(&op)),
        abi::role::HALO => abi::decode(op).ok().map(|op| halo::describe(&op)),
        _ => None,
    };
    match described {
        Some(described) => (described.title, described.fields),
        None => (role.to_owned(), vec![field("op", Value::bytes(op))]),
    }
}

describe::export!(Op, describe);

#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        ["Enroll", "Leave", "Vote", "SetHalo"]
    );
}
