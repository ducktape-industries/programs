//! Typed reads of valset, folded into one row per key; who operates each
//! node, as identity says; and what the vote needs of the programs role, with
//! the names of the accounts it shows.
use std::collections::BTreeMap;

use identity::Kind;
use programs::Entry;
use serde::{Deserialize, Serialize};
use valset::{Membership, PageRequest, Pending, Role, ask};
use view::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub key: Vec<u8>,
    pub address: Option<String>,
    pub validator: bool,
    pub seat: Option<u32>,
    /// "operated by <manager> (<agent>)" for a key on an agent, or why
    /// identity would not say
    pub operator: Option<String>,
}

impl Node {
    /// Its address, or the dash where valset holds none.
    pub fn shown_address(&self) -> &str {
        self.address.as_deref().unwrap_or(crate::words::NO_VALUE)
    }
}

/// The consensus keys and the memberships, as one list: validators in seat
/// order, then residents in the order valset keeps them. Each key once.
pub fn fold(validators: &[Vec<u8>], members: Vec<Membership>) -> Vec<Node> {
    let seat = |key: &[u8]| {
        validators
            .iter()
            .position(|seated| seated == key)
            .map(|at| at as u32)
    };
    let mut nodes: Vec<Node> = Vec::with_capacity(members.len());
    for member in members {
        if nodes.iter().any(|node| node.key == member.key) {
            continue;
        }
        nodes.push(Node {
            seat: seat(&member.key),
            validator: member.role == Role::Validator,
            key: member.key,
            address: member.address,
            operator: None,
        });
    }
    for key in validators {
        if !nodes.iter().any(|node| &node.key == key) {
            nodes.push(Node {
                key: key.clone(),
                address: None,
                validator: true,
                seat: seat(key),
                operator: None,
            });
        }
    }
    nodes.sort_by_key(|node| (!node.validator, node.seat.unwrap_or(u32::MAX)));
    nodes
}

pub(crate) async fn nodes(host: Host) -> Result<Vec<Node>, Error> {
    let validators = host.query(ask::Validators).await?;
    let members = host
        .query_all(|after| ask::Memberships {
            page: PageRequest { after, limit: None },
        })
        .await?;
    let mut nodes = fold(&validators, members);
    for node in &mut nodes {
        node.operator = operator(&host, &node.key).await.unwrap_or_else(|refusal| {
            Some(format!(
                "operator unreadable: {}",
                design::sentence(&refusal)
            ))
        });
    }
    Ok(nodes)
}

pub(crate) async fn operator(host: &Host, key: &[u8]) -> Result<Option<String>, Error> {
    let account = match host.query(identity::ask::OfKey { key: key.to_vec() }).await {
        Ok(account) => account,
        Err(refusal) if refusal.code == abi::reason::UNAUTHORIZED => return Ok(None),
        Err(refusal) => return Err(refusal),
    };
    let Some(number) = account else {
        return Ok(None);
    };
    let Some(agent) = host.query(identity::ask::Profile { number }).await? else {
        return Ok(None);
    };
    let Kind::Program { manager, .. } = agent.kind else {
        return Ok(None);
    };
    let manager = host
        .query(identity::ask::Profile { number: manager })
        .await?
        .map_or_else(|| format!("account {manager}"), |profile| profile.name);
    Ok(Some(format!("operated by {manager} ({})", agent.name)))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Motions {
    pub(crate) needed: u64,
    pub(crate) pending: Vec<Pending>,
}

pub(crate) type Names = BTreeMap<u64, String>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Schedule {
    pub(crate) changes: Vec<programs::Pending>,
    pub(crate) names: Names,
}

async fn names(host: &Host, accounts: impl IntoIterator<Item = u64>) -> Result<Names, Error> {
    let mut names = Names::new();
    for number in accounts {
        if names.contains_key(&number) {
            continue;
        }
        if let Some(profile) = host.query(identity::ask::Profile { number }).await? {
            names.insert(number, profile.name);
        }
    }
    Ok(names)
}

pub(crate) async fn name(host: Host, number: u64) -> Result<Option<String>, Error> {
    Ok(names(&host, [number]).await?.remove(&number))
}

pub(crate) async fn voices(host: Host, key: Vec<u8>) -> Result<Vec<Vec<u8>>, Error> {
    host.query(ask::Voices { key }).await
}

pub(crate) async fn motions(host: Host) -> Result<Motions, Error> {
    let (needed, page) = host
        .query(ask::Motions {
            page: PageRequest {
                after: None,
                limit: None,
            },
        })
        .await?;
    Ok(Motions {
        needed,
        pending: page.items,
    })
}

pub(crate) async fn roster(host: Host) -> Result<Vec<Entry>, Error> {
    let (height, _) = host
        .query(programs::ask::Program(programs::PROGRAM.into()))
        .await?;
    host.query(programs::ask::At(height)).await
}

pub(crate) async fn scheduled(host: Host) -> Result<Schedule, Error> {
    let changes: Vec<programs::Pending> = host
        .query_all(|after| programs::ask::Scheduled {
            page: PageRequest { after, limit: None },
        })
        .await?;
    let names = names(&host, changes.iter().filter_map(|change| change.by)).await?;
    Ok(Schedule { changes, names })
}
