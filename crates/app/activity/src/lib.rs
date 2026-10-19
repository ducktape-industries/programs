//! The `activity` program: where each frame a block carried sits on the chain and who signed it,
//! kept newest first network-wide, under the key that signed it and under the account it acted as,
//! as the kernel tells its hooks. Views list the network's transactions and what an account did from it.
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

pub type Op = ();

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
#[tool_about(
    "Where each frame sits on the chain and who signed it, kept newest first network-wide, by signing key and by the account it acted as, as the kernel tells it. It takes no op."
)]
#[ask(Activity)]
pub enum Query {
    #[ask(Reply::Frames(PageResponse<Frame>))]
    #[tool_query(description = "The frames an account acted in, newest first: height, position, time, signer, target program and whether it applied.", example = examples::acted)]
    Acted {
        account: AccountNumber,
        page: PageRequest,
    },
    #[ask(Reply::Frames(PageResponse<Frame>))]
    #[tool_query(description = "The frames a key signed, newest first.", example = examples::signed)]
    Signed { key: Vec<u8>, page: PageRequest },
    #[ask(Reply::Used(PageResponse<Used>))]
    #[tool_query(description = "How many frames each account sent each program, a page at a time.", example = examples::used)]
    Used { page: PageRequest },
    #[ask(Reply::Frames(PageResponse<Frame>))]
    #[tool_query(description = "Every frame on the network, newest first.", example = examples::frames)]
    Frames { page: PageRequest },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Frames(PageResponse<Frame>),
    Used(PageResponse<Used>),
}

pub fn describe(_op: &Op) -> describe::Description {
    describe::Description {
        title: "Nothing: activity takes no op".into(),
        fields: Vec::new(),
    }
}

describe::export!(Op, describe);
