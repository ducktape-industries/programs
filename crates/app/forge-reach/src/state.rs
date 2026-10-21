//! Every table forge-reach keeps: each reported result by repository, commit, reporter and
//! context, the paths its check read, one row each, each open change's latest Preview heard,
//! and every commit an open change has had as its head or Preview.
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
pub const PREVIEWED: Map<(String, u64), (String, String)> = Map::new("v/");
pub const PREVIEWING: Set<(String, String, u64)> = Set::new("h/");
pub const HEADS: Set<(String, u64, String)> = Set::new("c/");
