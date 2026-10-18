//! Where a program's code was built from: the repository and commit a packer stamps into the
//! code as its [`SECTION`] custom section, the borsh of a [`Source`].
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

pub const SECTION: &str = "ducktape.source";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Source {
    pub repo: String,
    pub commit: String,
}
