//! Exact-commit receipts and their declared read paths.
use borsh::{BorshDeserialize, BorshSerialize};
use forge::CheckState;
use store::{Map, Set};

use crate::AccountNumber;

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Kept {
    pub state: CheckState,
    pub url: Option<String>,
    pub height: u64,
}

pub type Reporting = (String, String, AccountNumber, String);

pub const REPORTS: Map<Reporting, Kept> = Map::new("r/");
pub const REACH: Set<(Reporting, String)> = Set::new("p/");
pub const PATCHES: Map<Reporting, Kept> = Map::new("s/");
