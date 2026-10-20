//! Every table forge-reach keeps: each reported result by repository, commit, reporter and
//! context, and the paths its check read, one row each.
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
