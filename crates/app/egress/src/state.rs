//! Everything egress keeps: each registered service with its owner and assigned nodes, each unregistered name's last owner, which
//! node serves which service, each service's next turn in the election, every open request with
//! its ticket, wait, expiry, node and the account it was sent as, and an events request's stream.
use borsh::{BorshDeserialize, BorshSerialize};
use store::{Item, Kind, Map, Owned, Set};

use ::program::Ticket;

use crate::{AccountNumber, NodeRow, RequestRow, ServiceRow, StreamEnd};

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Stored {
    pub row: RequestRow,
    pub ticket: Ticket,
    /// The blocks the request waits for its answer, and an events request for each next
    /// event: the expiry moves this far past every event.
    pub wait: u64,
}

pub struct Service;

impl Kind for Service {
    const KIND: &'static str = "egress.service";
}

pub const SERVICES: Map<String, Owned<ServiceRow, Service>> = Map::new("service/");
pub const RETIRED: Map<String, AccountNumber> = Map::new("retired/");
pub const NODES: Map<Vec<u8>, NodeRow> = Map::new("node/");
pub const SERVING: Set<(String, Vec<u8>)> = Set::new("serving/");
pub const TURNS: Map<String, u64> = Map::new("turn/");
pub const REQUESTS: Map<u64, Stored> = Map::new("request/");
pub const ASSIGNED: Set<(Vec<u8>, u64)> = Set::new("assigned/");
pub const SENDERS: Map<u64, AccountNumber> = Map::new("sender/");
pub const COUNT: Item<u64> = Item::new("count");

#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Stream {
    pub next: u64,
    pub moved: u64,
    pub ending: Option<Ending>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Ending {
    pub events: u64,
    pub end: StreamEnd,
}

pub const STREAMS: Map<u64, Stream> = Map::new("stream/");
pub const EARLY: Map<(u64, u64), Vec<u8>> = Map::new("early/");
// The scheduled expiry of a request whose stream moved it, apart from its row, so a moving
// stream rewrites eight bytes a block and not the whole request.
pub const EXPIRIES: Map<u64, u64> = Map::new("expiry/");
