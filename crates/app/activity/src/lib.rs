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
    "The frames each account acted in, newest first, and how many it sent each program, as the kernel tells it. Every frame, and every frame by signing key, is the node's to list (/v1/blocks, /v1/activity). It takes no op."
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

pub fn describe(_op: &Op) -> describe::Description {
    describe::Description {
        title: "Nothing: activity takes no op".into(),
        fields: Vec::new(),
    }
}

describe::export!(Op, describe);
