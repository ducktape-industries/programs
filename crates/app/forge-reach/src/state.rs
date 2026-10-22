//! Every table forge-reach keeps: each reported result by repository, commit, reporter and
//! context, the paths its check read, one row each, each result of a change's patch, each open
//! change's latest Preview heard, every commit it has had as head or Preview, each result asked again.
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
pub const PREVIEWED: Map<(String, u64), (String, String)> = Map::new("v/");
pub const PREVIEWING: Set<(String, String, u64)> = Set::new("h/");
pub const HEADS: Set<(String, u64, String)> = Set::new("c/");
pub const WANTED: Map<(AccountNumber, String, String, String), u64> = Map::new("w/");
pub const WANTING: Set<(String, String, AccountNumber, String)> = Set::new("u/");
