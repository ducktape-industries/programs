//! Every table forge-review-chat keeps: the network's rules, owned by their first writer, the room
//! each open change is mirrored into, and the count that names each notice it posts.
use store::{Item, Kind, Map, Owned};

use crate::Rule;

pub struct RulesKind;

impl Kind for RulesKind {
    const KIND: &'static str = "forge-review-chat.rules";
}

pub const RULES: Item<Owned<Vec<Rule>, RulesKind>> = Item::new("rules");
pub const ROOMS: Map<(String, u64), String> = Map::new("room/");
pub const POSTED: Item<u64> = Item::new("posted");
