//! One owner's side of the ownership program, read whole: their policies, their open policy and
//! their defaults, with the names of every account those policies list; and how many items are on
//! one policy, which only a read of every one of them says.
use std::collections::{BTreeMap, BTreeSet};

use ownership::{PageRequest, Party, Policy, PolicyId, Scope, ScopeDefault, Writers};
use serde::{Deserialize, Serialize};
use view::host::Error;
use view::prelude::*;

use crate::kind;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Holdings {
    #[serde(with = "view::borsh_bytes")]
    pub owner: Party,
    // the owner's open policy, once ownership made it (the first item that needs it does)
    pub open: Option<PolicyId>,
    #[serde(with = "view::borsh_bytes")]
    pub policies: Vec<Policy>,
    #[serde(with = "view::borsh_bytes")]
    pub defaults: Vec<ScopeDefault>,
    // every account a policy lists, by its number
    pub names: BTreeMap<u64, String>,
}

impl Holdings {
    pub fn policy(&self, id: PolicyId) -> Option<&Policy> {
        self.policies.iter().find(|policy| policy.id == id)
    }

    // The policies an item can be linked to and a default can name: every one not removed.
    pub fn live(&self) -> impl Iterator<Item = &Policy> {
        self.policies.iter().filter(|policy| !policy.retired)
    }

    // What new items in `scope` use by the owner's own choice there.
    pub fn default_of(&self, scope: &Scope) -> Option<PolicyId> {
        self.defaults
            .iter()
            .find(|default| default.scope == *scope)
            .map(|default| default.policy)
    }

    // What new items in `scope` use with no choice of its own, as the guard falls back: the next
    // wider scope's (a kind's program, then everything), else the open policy (`None` until
    // ownership makes it).
    pub fn inherited(&self, scope: &Scope) -> Option<PolicyId> {
        let wider: Vec<Scope> = match scope {
            Scope::Kind(name) => kind(name)
                .map(|kind| Scope::Program(kind.program.to_owned()))
                .into_iter()
                .chain([Scope::Global])
                .collect(),
            Scope::Program(_) => vec![Scope::Global],
            Scope::Global => Vec::new(),
        };
        wider
            .iter()
            .find_map(|scope| self.default_of(scope))
            .or(self.open)
    }

    // Who may write the items on `policy`, said to its owner (`crate::writers`).
    pub fn writers(&self, policy: &Policy) -> String {
        crate::writers(&policy.write, &self.owner, Some(&self.owner), &self.names)
    }
}

// `owner`'s side, read whole: every page of their policies and of their defaults.
pub async fn holdings(host: Host, owner: Party) -> Result<Holdings, Error> {
    let policies = host
        .query_all(|after| ownership::ask::Policies {
            owner: owner.clone(),
            page: page(after),
        })
        .await?;
    let open = host
        .query(ownership::ask::Open {
            owner: owner.clone(),
        })
        .await?;
    let defaults = host
        .query_all(|after| ownership::ask::Defaults {
            owner: owner.clone(),
            page: page(after),
        })
        .await?;
    let mut names = BTreeMap::new();
    for number in listed_accounts_once(&policies) {
        if let Some(profile) = host.query(identity::ask::Profile { number }).await? {
            names.insert(number, profile.name);
        }
    }
    Ok(Holdings {
        owner,
        open,
        policies,
        defaults,
        names,
    })
}

// How many items are on policy `id`: ownership keeps no count, so every page of them is read.
pub async fn items(host: Host, id: PolicyId) -> Result<usize, Error> {
    let linked = host
        .query_all(|after| ownership::ask::Linked {
            policy: id,
            page: page(after),
        })
        .await?;
    Ok(linked.len())
}

fn page(after: Option<Vec<u8>>) -> PageRequest {
    PageRequest { after, limit: None }
}

fn listed_accounts_once(policies: &[Policy]) -> BTreeSet<u64> {
    let mut numbers = BTreeSet::new();
    for policy in policies {
        if let Writers::Listed(parties) = &policy.write {
            numbers.extend(parties.iter().filter_map(|party| match party {
                Party::Account(number) => Some(*number),
                Party::Program(_) => None,
            }));
        }
    }
    numbers
}
