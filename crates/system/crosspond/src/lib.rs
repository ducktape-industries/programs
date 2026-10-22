//! The `crosspond` program, bound to the crosspond role (`abi::role::crosspond`): links to other
//! networks, the packets this network's accounts send over them and the relay that carries them,
//! and the light client that checks each packet another network delivers before it runs.
#[cfg(feature = "program")]
mod light;
mod program;
mod queries;
#[cfg(feature = "program")]
mod rules;
mod state;
#[cfg(test)]
mod tests;
mod tools;

pub use abi::role::crosspond::{
    Ack, Acked, CertificateKind, Header, Link, Op, Packet, Proven, ProvenAck, ProvenLow, Query,
    RELAY_FRAME_ROOM, Receipt, Reply, Settled, StateProof, Status, commitment_key, low_key,
    max_packet, settled_key,
};
pub use abi::role::identity::AccountNumber;
pub use abi::{Foreign, NetworkId};
pub use program::Crosspond;

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

pub const PROGRAM: &str = "crosspond";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Changed {
    pub to: NetworkId,
    pub seq: u64,
    pub status: Status,
}

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let network = |id: &NetworkId| field("network", Value::Hash(id.to_vec()));
    let deadline =
        |deadline: &Option<u64>| deadline.map(|time| field("deadline", Value::Time(time)));
    let kind = |kind: &CertificateKind| field("certificate kind", Value::Text(format!("{kind:?}")));
    let (title, fields) = match op {
        Op::Open {
            network: id,
            name,
            epoch_length,
            epoch,
            kind: signing,
            validators,
        } => (
            format!("Open a link to {name}"),
            vec![
                network(id),
                field("epoch length", Value::count(*epoch_length)),
                field("epoch", Value::count(*epoch)),
                kind(signing),
                field("validators", Value::count(validators.len() as u64)),
            ],
        ),
        Op::Repin {
            network: id,
            epoch,
            kind: signing,
            validators,
        } => (
            "Repin the validators a link trusts".to_owned(),
            vec![
                network(id),
                field("epoch", Value::count(*epoch)),
                kind(signing),
                field("validators", Value::count(validators.len() as u64)),
            ],
        ),
        Op::Close { network: id } => ("Close a link".to_owned(), vec![network(id)]),
        Op::Port { role, from } => {
            let networks = from.iter().map(|id| Value::Hash(id.to_vec())).collect();
            let title = match from.is_empty() {
                true => format!("Close the {role} port"),
                false => format!("Open the {role} port"),
            };
            (title, vec![field("from", Value::List(networks))])
        }
        Op::Send {
            to,
            port,
            op,
            deadline: until,
        } => (
            format!("Send to {port} on another network"),
            [
                Some(network(to)),
                Some(field("op", Value::bytes(op))),
                deadline(until),
            ]
            .into_iter()
            .flatten()
            .collect(),
        ),
        Op::SendFor {
            to,
            port,
            op,
            deadline: until,
            account,
        } => (
            format!("Send to {port} on another network for account {account}"),
            [
                Some(network(to)),
                Some(field("account", Value::Account(*account))),
                Some(field("op", Value::bytes(op))),
                deadline(until),
            ]
            .into_iter()
            .flatten()
            .collect(),
        ),
        Op::Pump { to } => ("Ask for a relay".to_owned(), vec![network(to)]),
        Op::Header { from, header } => (
            "Take another network's header".to_owned(),
            vec![
                network(from),
                kind(&header.kind),
                field("block", Value::bytes(&header.block)),
                field("validators", Value::count(header.validators.len() as u64)),
            ],
        ),
        Op::DeliverProven {
            from,
            epoch,
            low,
            packets,
        } => (
            format!("Deliver {} packets", packets.len()),
            [
                Some(network(from)),
                Some(field("epoch", Value::count(*epoch))),
                low.as_ref()
                    .map(|low| field("acked below", Value::count(low.n))),
            ]
            .into_iter()
            .flatten()
            .collect(),
        ),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);
