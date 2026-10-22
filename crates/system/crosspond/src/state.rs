//! What crosspond keeps: this network's id, each link and the links under each name, the packets
//! owed to each linked network and their status with the low-water mark of its acks, receipts and
//! settled rows of packets taken, the ports, the names heard, and the runs whose replies settle.
use abi::Refusal;
use borsh::{BorshDeserialize, BorshSerialize};
use store::{Item, Map, Set};

#[cfg(feature = "program")]
use crate::Settled;
use crate::{AccountNumber, CertificateKind, NetworkId, Packet, Status};

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub(crate) struct Trusted {
    pub(crate) epoch: u64,
    pub(crate) validators: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub(crate) struct Accepted {
    pub(crate) epoch: u64,
    pub(crate) root: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub(crate) struct LinkRow {
    pub(crate) name: String,
    pub(crate) epoch_length: u64,
    pub(crate) kind: CertificateKind,
    pub(crate) trusted: Trusted,
    pub(crate) header: Option<Accepted>,
    pub(crate) next_in: u64,
    pub(crate) next_out: u64,
    pub(crate) refused: Option<String>,
    pub(crate) closed: bool,
    pub(crate) asked: Option<u64>,
}

pub(crate) type Taken = Option<Result<Vec<u8>, Refusal>>;

#[cfg(feature = "program")]
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub(crate) struct Parked {
    pub(crate) key: (NetworkId, u64),
    pub(crate) packet: [u8; 32],
}

pub(crate) const HOME: Item<NetworkId> = Item::new("home");
pub(crate) const LINKS: Map<NetworkId, LinkRow> = Map::new("link/");
pub(crate) const LINKED: Set<(String, NetworkId)> = Set::new("linked/");
pub(crate) const OUTBOX: Map<(NetworkId, u64), Packet> = Map::new("out/");
pub(crate) const STATUSES: Map<(NetworkId, u64), Status> = Map::new("status/");
pub(crate) const RECEIPTS: Map<(NetworkId, u64), Taken> = Map::new("receipt/");
#[cfg(feature = "program")]
pub(crate) const SETTLED: Map<(NetworkId, u64), Settled> = Map::new("settled/");
#[cfg(feature = "program")]
pub(crate) const LOWS: Set<(NetworkId, u64)> = Set::new("low/");
#[cfg(feature = "program")]
pub(crate) const PRUNED: Map<NetworkId, u64> = Map::new("pruned/");
pub(crate) const PORTS: Map<String, Vec<NetworkId>> = Map::new("port/");
pub(crate) const NAMES: Map<(NetworkId, AccountNumber), String> = Map::new("name/");
pub(crate) const NAMED: Set<(NetworkId, String, AccountNumber)> = Set::new("named/");
#[cfg(feature = "program")]
pub(crate) const REPLIES: Map<(String, u64), Parked> = Map::new("reply/");
