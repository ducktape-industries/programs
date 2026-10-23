//! The reads a view makes of chat: the roster and the users of other networks into [`Names`], a
//! channel's roots into one page. Each takes the asker (`|ask| host.query(ask)` in a view), so
//! this crate links no view runtime. Names are display text, not identity.
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;

use ::program::Error;

use store::KeyCodec;

use crate::tables::{CHANNEL_META, CHANNELS, HEADS, MESSAGES, REACTIONS, REPLIES, ROOTS};
use crate::{
    Foreign, ForeignName, Kind, MsgRow, PageRequest, PageResponse, Principal, Profile, ask,
};

pub async fn roster<F, G>(
    accounts: impl Fn(ask::Accounts) -> F,
    foreigners: impl Fn(ask::Foreigners) -> G,
) -> Result<Names, Error>
where
    F: Future<Output = Result<PageResponse<Profile>, Error>>,
    G: Future<Output = Result<PageResponse<ForeignName>, Error>>,
{
    let mut rows = Vec::new();
    let mut after = None;
    loop {
        let page = accounts(ask::Accounts {
            page: PageRequest { after, limit: None },
        })
        .await?;
        rows.extend(page.items);
        after = page.next;
        if after.is_none() {
            break;
        }
    }
    let mut met = Vec::new();
    loop {
        let page = foreigners(ask::Foreigners {
            page: PageRequest { after, limit: None },
        })
        .await?;
        met.extend(page.items);
        after = page.next;
        if after.is_none() {
            return Ok(Names::from_roster(rows).with_foreigners(met));
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

/// The roster as a view reads it: each account's profile by number, and each user of another
/// network by what crosspond heard.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Names {
    profiles: BTreeMap<u64, Profile>,
    foreign: BTreeMap<Foreign, String>,
}

impl Names {
    pub const fn empty() -> Self {
        Self {
            profiles: BTreeMap::new(),
            foreign: BTreeMap::new(),
        }
    }

    pub fn with_foreigners(mut self, met: impl IntoIterator<Item = ForeignName>) -> Self {
        for named in met {
            self.foreign.insert(named.foreign, named.handle());
        }
        self
    }

    pub fn from_roster(roster: impl IntoIterator<Item = Profile>) -> Self {
        let mut names = Self::empty();
        for profile in roster {
            names.profiles.insert(profile.number, profile);
        }
        names
    }

    pub fn people(&self) -> impl Iterator<Item = u64> + '_ {
        self.profiles
            .values()
            .filter(|profile| identity::view::listed(&profile.kind))
            .map(|profile| profile.number)
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
        match principal {
            Principal::Foreign(foreign) => self.foreign.get(foreign).map(String::as_str),
            Principal::Account(_) | Principal::Root => {
                self.profile(principal).map(|profile| profile.name.as_str())
            }
        }
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
        match self.kind(principal).and_then(design::note) {
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
            Principal::Foreign(_) => format!(
                "@{}",
                self.name(principal)
                    .map_or_else(|| unnamed(principal), str::to_owned)
            ),
        }
    }

    /// The program an account is the account of.
    pub fn program(&self, principal: &Principal) -> Option<&str> {
        match self.kind(principal)? {
            Kind::System(program) => Some(program),
            Kind::User | Kind::Program { .. } => None,
        }
    }

    // What an account is, beside its name (`design::badge`): an agent and
    // who manages it ("Agent · managed by Dev"), or the program it is
    // ("Program · forge"). None for a user.
    pub fn badge(&self, principal: &Principal) -> Option<String> {
        let kind = self.kind(principal)?;
        design::badge(kind, |manager| self.member(&Principal::Account(manager)))
    }
}

/// A principal no roster names: its account number, or the system.
pub fn unnamed(principal: &Principal) -> String {
    match principal {
        Principal::Account(number) => format!("account {number}"),
        Principal::Root => "system".into(),
        Principal::Foreign(foreign) => ForeignName {
            foreign: *foreign,
            name: None,
            network: None,
        }
        .handle(),
    }
}

// The channels `keys` wrote the row, head or meta of: what the channel list shows of each, which
// `ask::Channel` reads again alone.
pub fn channels_written<'a>(keys: impl IntoIterator<Item = &'a Vec<u8>>) -> BTreeSet<String> {
    let tables = [CHANNELS.key(&()), HEADS.key(&()), CHANNEL_META.key(&())];
    keys.into_iter()
        .filter_map(|key| {
            tables.iter().find_map(|table| {
                let mut rest = key.strip_prefix(table.as_slice())?;
                String::decode_key(&mut rest).filter(|_| rest.is_empty())
            })
        })
        .collect()
}

// What a block wrote of one channel's messages ([`messages_written`]): the seqs whose row or
// reactions it wrote, whether it wrote the timeline's roots, and each reply it added to a
// thread, as (root, reply). A view reads again only these.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Written {
    pub seqs: BTreeSet<u64>,
    pub roots: bool,
    pub replies: BTreeSet<(u64, u64)>,
}

impl Written {
    pub fn merge(&mut self, other: Written) {
        self.seqs.extend(other.seqs);
        self.roots |= other.roots;
        self.replies.extend(other.replies);
    }
}

// What `keys` wrote of channel `channel_id`'s messages.
pub fn messages_written<'a>(
    keys: impl IntoIterator<Item = &'a Vec<u8>>,
    channel_id: &str,
) -> Written {
    let channel = channel_id.to_owned();
    let rows = [MESSAGES.key(&channel), REACTIONS.key(&channel)];
    let (roots, replies) = (ROOTS.key(&channel), REPLIES.key(&channel));
    let mut written = Written::default();
    for key in keys {
        let row = rows
            .iter()
            .find_map(|table| key.strip_prefix(table.as_slice()));
        if let Some(mut rest) = row {
            written.seqs.extend(u64::decode_key(&mut rest));
        } else if key.starts_with(&roots) {
            written.roots = true;
        } else if let Some(mut rest) = key.strip_prefix(replies.as_slice()) {
            written.replies.extend(<(u64, u64)>::decode_key(&mut rest));
        }
    }
    written
}
