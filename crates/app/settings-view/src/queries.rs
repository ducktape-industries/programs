//! Typed reads: who the seated key is (identity) and whom each of its
//! agents answers (access), the account a typed name or number is, and
//! whether each of its keys validates (valset). Rows keep what the programs said; they are
//! worded only when drawn.
use identity::{Kind, Ownership, PageRequest, Party, Reference, Standing};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use view::host::malformed;
use view::prelude::*;

/// Who the seated key is.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub enum Seat {
    /// a key no account holds
    Bare(Key),
    /// a key held by an agent that does not act: the agent's name and why
    Stopped {
        key: Key,
        name: String,
        note: String,
    },
    Account(Account),
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub number: u64,
    pub name: String,
    /// what identity says it is: a person, an agent, a program
    #[serde(with = "view::borsh_bytes")]
    pub kind: Kind,
    pub keys: Vec<Key>,
    pub agents: Vec<Agent>,
    // the card's picture and bio: Change photo keeps the bio as it is
    #[serde(with = "view::borsh_bytes")]
    pub avatar: Option<abi::BlobId>,
    pub bio: Option<String>,
}

/// An agent the account manages.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Agent {
    pub number: u64,
    pub name: String,
    pub keys: usize,
    /// what identity says it is: always `Kind::Program` (checked on read)
    #[serde(with = "view::borsh_bytes")]
    pub kind: Kind,
    #[serde(with = "view::borsh_bytes")]
    pub ownership: Ownership,
    pub names: BTreeMap<u64, String>,
}

impl Agent {
    pub fn standing(&self) -> Standing {
        match self.kind {
            Kind::Program { standing, .. } => standing,
            Kind::User | Kind::System(_) => Standing::Active,
        }
    }

    /// What its manager can do to its standing short of revoking it: a
    /// suspended agent resumes, any other is suspended. The control's slug,
    /// its word, and the op it sends.
    pub fn standing_toggle(&self) -> (&'static str, &'static str, identity::Op) {
        let account = self.number;
        match self.standing() {
            Standing::Suspended => ("resume", "Resume", identity::Op::Resume { account }),
            Standing::Active | Standing::Revoked => {
                ("suspend", "Suspend", identity::Op::Suspend { account })
            }
        }
    }

    pub fn party_name(&self, party: &Party) -> String {
        match party {
            Party::Account(person) => match self.names.get(person) {
                Some(name) => name.clone(),
                None => format!("#{person}"),
            },
            Party::Program(program) => program.clone(),
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Key {
    pub label: Option<String>,
    pub key: Vec<u8>,
    pub validator: bool,
}

/// Who the seated `signer` (hex) is: its account as the host resolved it
/// (`number`), with every key on it, or the key alone while it holds none.
pub(crate) async fn account(
    host: Host,
    signer: String,
    number: Option<u64>,
) -> Result<Option<Seat>, Error> {
    if signer.is_empty() {
        return Ok(None);
    }
    let key = abi::unhex(&signer)
        .ok_or_else(|| malformed("the session's signer is not hexadecimal".into()))?;
    let Some(number) = number else {
        // the host resolves no account for a suspended agent's key, which
        // identity still holds (and so refuses to create an account with)
        let held = held_by(&host, &key).await?;
        let key = read_key(&host, key, None).await?;
        return Ok(Some(match held {
            Some((name, note)) => Seat::Stopped {
                key,
                note: format!("This key belongs to {name}, {note} by its manager."),
                name,
            },
            None => Seat::Bare(key),
        }));
    };
    let Some(account) = host.query(identity::ask::Get { number }).await? else {
        return Ok(None);
    };
    let mut keys = Vec::new();
    for key in account.keys() {
        keys.push(read_key(&host, key.key.clone(), key.label.clone()).await?);
    }
    let agents = agents(&host, number).await?;
    Ok(Some(Seat::Account(Account {
        number,
        kind: account.kind(),
        name: account.card.name,
        avatar: account.card.avatar,
        bio: account.card.bio,
        keys,
        agents,
    })))
}

/// The account holding `key` that does not act, by its name and why not
/// (`Kind::note`); `None` for a key no account holds.
async fn held_by(host: &Host, key: &[u8]) -> Result<Option<(String, &'static str)>, Error> {
    let references = vec![Reference::Key(key.to_vec())];
    let resolved = host.query(identity::ask::Resolve { references }).await?;
    let Some(number) = resolved.into_iter().next().flatten() else {
        return Ok(None);
    };
    let profile = host.query(identity::ask::Profile { number }).await?;
    Ok(profile.and_then(|p| Some((p.name, p.kind.note()?))))
}

/// Every agent `manager` manages.
async fn agents(host: &Host, manager: u64) -> Result<Vec<Agent>, Error> {
    let listed = host
        .query_all(|after| identity::ask::Managed {
            by: manager,
            page: PageRequest { after, limit: None },
        })
        .await?;
    let mut agents = Vec::new();
    for agent in listed {
        let kind @ Kind::Program { .. } = agent.kind() else {
            return Err(malformed(format!(
                "identity lists account {} as managed, and it is not",
                agent.number
            )));
        };
        let number = agent.number;
        let ownership = host
            .query(access::ask::List {
                program: harness::PROGRAM.into(),
                key: harness::agent_key(number),
            })
            .await?
            .unwrap_or(Ownership::Owner);
        let names = names(host, &ownership).await?;
        agents.push(Agent {
            number,
            keys: agent.keys().len(),
            name: agent.card.name,
            kind,
            ownership,
            names,
        });
    }
    Ok(agents)
}

async fn names(host: &Host, ownership: &Ownership) -> Result<BTreeMap<u64, String>, Error> {
    let mut names = BTreeMap::new();
    for number in ownership.accounts() {
        if let Some(profile) = host.query(identity::ask::Profile { number }).await? {
            names.insert(number, profile.name);
        }
    }
    Ok(names)
}

pub(crate) enum Found {
    Party(Party),
    Nobody,
    Several,
}

pub(crate) async fn find(host: Host, typed: String) -> Result<Found, Error> {
    let numbered = typed.strip_prefix('#').unwrap_or(&typed).parse::<u64>();
    if let Ok(number) = numbered {
        let references = vec![Reference::Account(number)];
        let resolved = host.query(identity::ask::Resolve { references }).await?;
        return Ok(match resolved.into_iter().next().flatten() {
            Some(number) => Found::Party(Party::Account(number)),
            None => Found::Nobody,
        });
    }
    let wanted = typed.to_lowercase();
    let mut matched = Vec::new();
    let mut after = None;
    loop {
        let (profiles, next) = host
            .query(identity::ask::Profiles { after, limit: 100 })
            .await?;
        let named = profiles
            .into_iter()
            .filter(|profile| profile.name.to_lowercase() == wanted)
            .map(|profile| match profile.kind {
                Kind::System(program) => Party::Program(program),
                Kind::User | Kind::Program { .. } => Party::Account(profile.number),
            });
        matched.extend(named);
        let Some(next) = next else { break };
        after = Some(next);
    }
    Ok(match matched.as_slice() {
        [party] => Found::Party(party.clone()),
        [] => Found::Nobody,
        _ => Found::Several,
    })
}

async fn read_key(host: &Host, key: Vec<u8>, label: Option<String>) -> Result<Key, Error> {
    let membership = host
        .query(valset::ask::Membership { key: key.clone() })
        .await?;
    Ok(Key {
        label,
        key,
        validator: membership.is_some_and(|m| m.role == valset::Role::Validator),
    })
}
