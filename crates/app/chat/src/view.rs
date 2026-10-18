//! The reads a view makes of chat, folded into what a principal is called:
//! the roster into [`Names`], a channel's roots into one page. Each takes
//! the asker (`|ask| host.query(ask)` in a view) so this crate links no
//! view runtime. Names are display text, not identity: "the same person"
//! is the account number.
use std::collections::BTreeMap;
use std::future::Future;

use ::program::Error;

use crate::{Kind, MsgRow, PageRequest, PageResponse, Principal, Profile, ask};

/// Every account's profile, every page of it, folded into [`Names`].
pub async fn roster<F: Future<Output = Result<PageResponse<Profile>, Error>>>(
    ask: impl Fn(ask::Accounts) -> F,
) -> Result<Names, Error> {
    let mut rows = Vec::new();
    let mut after = None;
    loop {
        let page = ask(ask::Accounts {
            page: PageRequest { after, limit: None },
        })
        .await?;
        rows.extend(page.items);
        after = page.next;
        if after.is_none() {
            return Ok(Names::from_roster(rows));
        }
    }
}

/// A channel's roots as `viewer` sees them, oldest first: pages of
/// `per_page` below the cursor `below` (or the newest) until at least
/// `want` rows are read or the channel ends, and whether older ones remain.
pub async fn roots<F: Future<Output = Result<PageResponse<MsgRow>, Error>>>(
    ask: impl Fn(ask::Roots) -> F,
    channel_id: String,
    viewer: Vec<Principal>,
    mut below: Option<Vec<u8>>,
    want: usize,
    per_page: u64,
) -> Result<(Vec<MsgRow>, bool), Error> {
    let mut rows = Vec::new();
    loop {
        let page = ask(ask::Roots {
            channel_id: channel_id.clone(),
            viewer: viewer.clone(),
            page: PageRequest {
                after: below,
                limit: Some(per_page),
            },
        })
        .await?;
        rows.extend(page.items);
        below = page.next;
        if below.is_none() || rows.len() >= want {
            break;
        }
    }
    rows.sort_by_key(|row| row.seq);
    Ok((rows, below.is_some()))
}

/// The roster as a view reads it: each account's profile, by number.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Names {
    profiles: BTreeMap<u64, Profile>,
}

impl Names {
    pub const fn empty() -> Self {
        Self {
            profiles: BTreeMap::new(),
        }
    }

    pub fn from_roster(roster: impl IntoIterator<Item = Profile>) -> Self {
        let mut names = Self::empty();
        for profile in roster {
            names.profiles.insert(profile.number, profile);
        }
        names
    }

    pub fn people(&self) -> impl Iterator<Item = u64> + '_ {
        self.profiles.keys().copied()
    }

    pub fn pickable(&self, principal: &Principal) -> bool {
        principal.account().is_some()
    }

    fn profile(&self, principal: &Principal) -> Option<&Profile> {
        self.profiles.get(&principal.account()?)
    }

    /// What the roster says a principal is; `None` past the roster.
    pub fn kind(&self, principal: &Principal) -> Option<&Kind> {
        self.profile(principal).map(|profile| &profile.kind)
    }

    /// The name the roster gives a principal: an account's own.
    pub fn name(&self, principal: &Principal) -> Option<&str> {
        self.profile(principal).map(|profile| profile.name.as_str())
    }

    /// A message's author line: the name, else what the principal is.
    pub fn author(&self, principal: &Principal) -> String {
        self.member(principal)
    }

    /// A member row or a dm peer: the name, else what the
    /// principal is; noted while the account does not act ("(suspended)").
    pub fn member(&self, principal: &Principal) -> String {
        let name = match self.name(principal) {
            Some(name) => name.to_owned(),
            None => unnamed(principal),
        };
        match self.kind(principal).and_then(Kind::note) {
            Some(note) => format!("{name} ({note})"),
            None => name,
        }
    }

    /// How a mention reads: `@name`.
    pub fn mention(&self, principal: &Principal) -> String {
        match principal {
            Principal::Account(account) => self
                .name(principal)
                .filter(|name| !name.is_empty())
                .map_or_else(|| format!("@account-{account}"), |name| format!("@{name}")),
            Principal::Root => "@system".into(),
        }
    }

    /// The program an account is the account of.
    pub fn program(&self, principal: &Principal) -> Option<&str> {
        match self.kind(principal)? {
            Kind::System(program) => Some(program),
            Kind::User | Kind::Program { .. } => None,
        }
    }

    /// What an account is, beside its name ([`Kind::badge`]): an agent and
    /// who manages it ("Agent · managed by Dev"), or the program it is
    /// ("Program · forge"). None for a user.
    pub fn badge(&self, principal: &Principal) -> Option<String> {
        self.kind(principal)?
            .badge(|manager| self.member(&Principal::Account(manager)))
    }
}

/// A principal no roster names: its account number, or the system.
pub fn unnamed(principal: &Principal) -> String {
    match principal {
        Principal::Account(number) => format!("account {number}"),
        Principal::Root => "system".into(),
    }
}
