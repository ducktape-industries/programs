//! The `activity` program: the frames each account acted in, newest first, and how many it sent
//! each program, as the kernel tells its hooks. Every frame, and every frame by signing key, is the
//! node's to list (`chain.blocks`, `/v1/activity`); views list what an account did from here.
mod examples;
mod program;
mod rules;
#[cfg(test)]
mod tests;
pub mod view;

pub use program::Activity;
pub use store::{PageRequest, PageResponse};

use ::program::{AccountNumber, ProgramId};
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

pub const PROGRAM: &str = "activity";

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Described,
)]
#[tool_about(
    "The frames each account acted in, newest first, and how many it sent each program, as the kernel tells it. Every frame, and every frame by signing key, is the node's to list (/v1/blocks, /v1/activity). Its one op follows a program rename."
)]
pub enum Op {
    #[tool_op(description = "Move each renamed program's frame counts (the programs role's Renames at height) to its new id, added to what it has counted there since, over the counts first..=last (each [account, program]; null: from the first, or to the last). Each frame's own record keeps the id it called. Anyone: it reads the renames from the programs role, never from its sender, so running it again changes nothing.", example = examples::migrate)]
    Migrate {
        height: u64,
        first: Option<(AccountNumber, ProgramId)>,
        last: Option<(AccountNumber, ProgramId)>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Frame {
    pub height: u64,
    pub position: u32,
    pub time: u64,
    pub signer: Vec<u8>,
    pub target: ProgramId,
    pub applied: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Used {
    pub account: AccountNumber,
    pub program: ProgramId,
    pub frames: u64,
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
#[ask(Activity)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum Query {
    #[ask(Reply::Frames(PageResponse<Frame>))]
    #[tool_query(description = "The frames an account acted in, newest first: height, position, time, signer, target program and whether it applied.", example = examples::acted)]
    Acted {
        account: AccountNumber,
        page: PageRequest,
    },
    #[ask(Reply::Used(PageResponse<Used>))]
    #[tool_query(description = "How many frames each account sent each program, a page at a time.", example = examples::used)]
    Used { page: PageRequest } = 2,
    #[ask(Reply::Frames(PageResponse<Frame>))]
    #[tool_query(description = "Refused: activity keeps no network-wide list. Every frame is in the node's blocks (/v1/blocks).", example = examples::frames)]
    Frames { page: PageRequest },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Frames(PageResponse<Frame>),
    Used(PageResponse<Used>),
}

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    match op {
        Op::Migrate {
            height,
            first,
            last,
        } => {
            let bound = |bound: &Option<(AccountNumber, ProgramId)>, open: &str| match bound {
                Some((account, program)) => Value::Text(format!("{account} {program}")),
                None => Value::Text(open.into()),
            };
            describe::Description {
                title: "Migrate".into(),
                fields: vec![
                    field("height", Value::Text(height.to_string())),
                    field("first", bound(first, "the first")),
                    field("last", bound(last, "the last")),
                ],
            }
        }
    }
}

describe::export!(Op, describe);

#[test]
fn op_variants_only_append() {
    assert_eq!(describe::variants::<Op>(), ["Migrate"]);
}
