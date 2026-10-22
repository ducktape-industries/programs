//! Every table model-routing keeps, each keyed first by the consumer whose op wrote it: each
//! gateway's pool, each account's full episode, each conversation's pin per provider.
use store::Map;

use crate::{AccountNumber, Episode, GatewayAccount};

pub type Gateway = (AccountNumber, String, String);
pub type Account = (AccountNumber, String, String, String);
pub type Pinning = (AccountNumber, u64, String);

pub const POOLS: Map<Gateway, Vec<String>> = Map::new("p/");
pub const FULL: Map<Account, Episode> = Map::new("f/");
pub const PINS: Map<Pinning, GatewayAccount> = Map::new("c/");
