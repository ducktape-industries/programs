//! Everything egress keeps: each registered service with its owner and assigned nodes, which
//! node serves which service, each service's next turn in the election, every open request with
//! its ticket, wait and expiry by its node, and an events request's place in its stream.
use borsh::{BorshDeserialize, BorshSerialize};
use store::{Item, Kind, Map, Owned, Set};

use ::program::Ticket;

use crate::{NodeRow, RequestRow, ServiceRow, StreamEnd};

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Stored {
    pub row: RequestRow,
    pub ticket: Ticket,
    /// The blocks the request waits for its answer, and an events request for each next
    /// event: the expiry moves this far past every event.
    pub wait: u64,
}

// The kind of a registered service (`egress.service`): each is its registrant's. Only `Register`
// creates one and only `Assign` rewrites it; no op deletes one.
pub struct Service;

impl Kind for Service {
    const KIND: &'static str = "egress.service";
}

pub const SERVICES: Map<String, Owned<ServiceRow, Service>> = Map::new("service/");
pub const NODES: Map<Vec<u8>, NodeRow> = Map::new("node/").shared();
pub const SERVING: Set<(String, Vec<u8>)> = Set::new("serving/").shared();
pub const TURNS: Map<String, u64> = Map::new("turn/").shared();
pub const REQUESTS: Map<u64, Stored> = Map::new("request/").shared();
pub const ASSIGNED: Set<(Vec<u8>, u64)> = Set::new("assigned/").shared();
pub const COUNT: Item<u64> = Item::new("count").shared();

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

pub const STREAMS: Map<u64, Stream> = Map::new("stream/").shared();
pub const EARLY: Map<(u64, u64), Vec<u8>> = Map::new("early/").shared();

// Every table here but SERVICES is general data: only this program's code governs it.
pub(crate) const TABLES: &[::program::Table] = &[
    SERVICES.table(),
    NODES.table(),
    SERVING.table(),
    TURNS.table(),
    REQUESTS.table(),
    ASSIGNED.table(),
    COUNT.table(),
    STREAMS.table(),
    EARLY.table(),
];
