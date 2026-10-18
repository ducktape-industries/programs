//! The state Explorer keeps: where it is, and what it read. Rows are the
//! programs' own types, worded only when drawn (`ui/`).
use std::future::Future;

use serde::{Deserialize, Serialize};
use view::Paged;
use view::borsh_bytes;
use view::design::Pages;
use view::host::Page;
use view::methods::CarriedView;
use view::prelude::*;
use view::wire::manifest::Manifest;

use crate::acting::Acting;
use crate::chain::{BlockRow, Chain, TxRow};

/// Where the explorer is: a list under a tab, or one thing opened from it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Route {
    #[default]
    Overview,
    Blocks,
    Block(u64),
    Transactions(Option<String>),
    Tx {
        height: u64,
        position: u32,
    },
    Accounts,
    Account(u64),
    Programs,
    /// one program, or one view-only entry, by name
    Program(String),
}

impl Route {
    /// This page's tail after `duck://<chain>/explorer/`, which
    /// [`Route::from_path`] reads back.
    pub fn path(&self) -> String {
        match self {
            Route::Overview => String::new(),
            Route::Blocks => "blocks".into(),
            Route::Block(height) => format!("block/{height}"),
            Route::Transactions(None) => "txs".into(),
            Route::Transactions(Some(program)) => format!("txs/{program}"),
            Route::Tx { height, position } => format!("tx/{height}/{position}"),
            Route::Accounts => "accounts".into(),
            Route::Account(number) => format!("account/{number}"),
            Route::Programs => "programs".into(),
            Route::Program(name) => format!("program/{name}"),
        }
    }

    /// The page a link's tail names, if it names one.
    pub fn from_path(path: &str) -> Option<Route> {
        let parts: Vec<&str> = path.split('/').collect();
        Some(match parts.as_slice() {
            [""] => Route::Overview,
            ["blocks"] => Route::Blocks,
            ["block", height] => Route::Block(ducklink::number(height)?),
            ["txs"] => Route::Transactions(None),
            ["txs", program] if !program.is_empty() => Route::Transactions(Some((*program).into())),
            ["tx", height, position] => Route::Tx {
                height: ducklink::number(height)?,
                position: ducklink::number(position)?.try_into().ok()?,
            },
            ["accounts"] => Route::Accounts,
            ["account", number] => Route::Account(ducklink::number(number)?),
            ["programs"] => Route::Programs,
            ["program", name] if !name.is_empty() => Route::Program((*name).into()),
            _ => return None,
        })
    }

    /// A tab's own list: the root a page opened from it returns to.
    pub(crate) fn listed(&self) -> bool {
        matches!(
            self,
            Route::Overview
                | Route::Blocks
                | Route::Transactions(None)
                | Route::Accounts
                | Route::Programs
        )
    }

    /// The tab this page sits under.
    pub(crate) fn tab(&self) -> usize {
        match self {
            Route::Overview => 0,
            Route::Blocks | Route::Block(_) => 1,
            Route::Transactions(_) | Route::Tx { .. } => 2,
            Route::Accounts | Route::Account(_) => 3,
            Route::Programs | Route::Program(_) => 4,
        }
    }
}

/// A blob id as `blob.*` asks spell it: `sha256:<hex>` or `sha1:<hex>`.
pub(crate) fn blob_ref(id: &abi::BlobId) -> String {
    format!("{}:{}", blob_kind(id), abi::hex(id.digest()))
}

/// The digest a blob id names, by its name in a `blob.*` ask.
pub(crate) fn blob_kind(id: &abi::BlobId) -> &'static str {
    match id {
        abi::BlobId::Sha256(_) => "sha256",
        abi::BlobId::Sha1(_) => "sha1",
    }
}

/// A 32-byte hash as 64 hex digits, `0x` allowed.
pub(crate) fn hash_of(text: &str) -> Option<[u8; 32]> {
    let text = text.trim_start_matches("0x");
    if text.len() != 64 {
        return None;
    }
    abi::unhex(text)?.try_into().ok()
}

/// A listing of activity's frames, read a page at a time, and whether
/// rows were written above the ones it holds that it has not read
/// (`Explorer::reread_first`).
pub(crate) struct Listing {
    pub(crate) rows: Entity<Paged<activity::Frame>>,
    /// a write landed rows above those held, and the listing, read on past
    /// its first page, was not read again for it: its list offers the
    /// newest and shows no total
    pub(crate) behind: bool,
    /// the first page asked to tell whether it is behind; dropped with the
    /// listing, which cancels it
    pub(crate) checking: Option<Task<()>>,
}

impl Listing {
    /// The listing `ask` reads a page of; its first page is asked now.
    pub(crate) fn new<F>(
        ask: impl Fn(Option<Vec<u8>>) -> F + 'static,
        cx: &mut Context<Explorer>,
    ) -> Listing
    where
        F: Future<Output = Result<Page<activity::Frame>, Error>> + 'static,
    {
        Listing {
            rows: cx.new(|cx| Paged::new(ask, cx)),
            behind: false,
            checking: None,
        }
    }

    /// It holds more than its first page.
    pub(crate) fn past_first_page(&self) -> bool {
        let page = crate::queries::PAGE as usize;
        self.rows.read(|rows| rows.rows().len() > page)
    }
}

/// A listing once its first page has landed; until then its refusal, or
/// `None` while it is on its way.
pub(crate) fn landed(listing: Option<&Listing>) -> Result<&Listing, Option<Error>> {
    let listing = listing.ok_or(None)?;
    if let Some(refusal) = listing.rows.read(|rows| rows.failed().cloned()) {
        return Err(Some(refusal));
    }
    if listing.rows.read(Paged::is_loading) {
        return Err(None);
    }
    Ok(listing)
}

/// Every account identity lists, as identity answers them.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Accounts {
    #[serde(with = "borsh_bytes")]
    pub list: Vec<identity::Account>,
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Used {
    #[serde(with = "borsh_bytes")]
    pub list: Vec<activity::Used>,
}

/// What the programs role runs, lists and will change, and who may change it,
/// as it answers them at its own height.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Network {
    #[serde(with = "borsh_bytes")]
    pub programs: Vec<programs::Entry>,
    /// the view-only entries: a name and its view's code, no program behind it
    #[serde(with = "borsh_bytes")]
    pub views: Vec<programs::View>,
    /// what the programs role will change after the height it answered, by
    /// height, and whose grant made each (`None`: the vote)
    #[serde(with = "borsh_bytes")]
    pub changes: Vec<programs::Pending>,
    /// the programs the kernel tells of every frame
    pub hooks: Vec<String>,
    #[serde(with = "borsh_bytes")]
    pub grants: Vec<programs::Grant>,
    /// the programs bound to the roles the kernel calls
    #[serde(with = "borsh_bytes")]
    pub roles: Option<abi::Roles>,
}

impl Note {
    /// What a write came to: on the Programs page it sits in the page's
    /// own scroll (Expo `Outcomes`, `programs.tsx:150`).
    pub(crate) fn of_a_write(&self) -> bool {
        matches!(self, Note::Sent(_) | Note::Refused(_))
    }
}

/// The page a link named by a hash (`block/<hash>`, `tx/<hash>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Linked {
    Block,
    Tx,
}

/// The line under the bar: what a search, a link or a copy came to. Worded
/// when drawn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Note {
    /// a search that names nothing here
    NotFound(String),
    /// a hash nothing here has: searched for (`link` none), or named by a
    /// block or transaction link
    NoSuchHash {
        listed_from: Option<u64>,
        link: Option<Linked>,
    },
    /// a link whose path names no page
    Unlinked(String),
    /// what was copied: "the link", "the code id"
    Copied(String),
    /// an op the node took, in words
    Sent(String),
    /// a read refused (`design::sentence`), or a write that did not land
    /// (`design::failure`)
    Refused(String),
}

#[derive(Serialize, Deserialize, Default)]
pub struct Explorer {
    /// The view's one route model in both designs (Expo's `Stack`): the
    /// tab's list at the root, each page opened from it pushed over it, a
    /// block's steps replacing the top. The phone draws the top alone, Back
    /// popping one; the desktop, with no Back, draws the top under its tab
    /// bar and keeps no page between it and the root (`go`).
    pub(crate) pages: Pages<Route>,
    /// the search field, as typed
    pub(crate) search: TextField,
    pub(crate) note: Option<Note>,
    /// the session's chain id (`<label>#<salt>`), which links are minted in
    pub(crate) session_chain: String,
    pub(crate) status: Loadable<NodeStatus>,
    pub(crate) chain: Chain,
    pub(crate) accounts: Loadable<Accounts>,
    pub(crate) validators: Loadable<Vec<Vec<u8>>>,
    pub(crate) network: Loadable<Network>,
    pub(crate) used: Loadable<Used>,
    /// every transaction, newest first, a page at a time (`read_frames`):
    /// read again on a restore, so a snapshot carries no frame
    #[serde(skip)]
    pub(crate) frames: Option<Listing>,
    /// how many rows of `frames` a program's transactions read back by
    /// themselves (`LOOK_BACK`, and that more at each Look further back);
    /// `LOOK_BACK` again whenever `frames` starts over
    #[serde(skip)]
    pub(crate) reach: usize,
    /// what the account page on screen did, newest first, a page at a
    /// time (`read_acted`), and whose it is
    #[serde(skip)]
    pub(crate) acted: Option<(u64, Listing)>,
    /// a block opened outside the window
    pub(crate) opened: Loadable<Option<(BlockRow, Vec<TxRow>)>>,
    /// what the code of the program page on screen declares (`blob.manifest`),
    /// and the id of the code it was asked of; read again when the page
    /// names other code (`read_manifest`)
    #[serde(skip)]
    pub(crate) manifest: Loadable<Option<Manifested>>,
    #[serde(skip)]
    pub(crate) manifest_of: Option<String>,
    pub(crate) located: Loadable<Option<TxRow>>,
    /// a phone's Activity rows: the transactions behind the account page's
    /// newest frames, read by their place (`read_rows`)
    #[serde(skip)]
    pub(crate) acted_rows: Loadable<Vec<TxRow>>,
    /// the account and the places of the frames `acted_rows` was read for
    #[serde(skip)]
    pub(crate) rows_of: Option<(u64, Vec<(u64, u32)>)>,
    /// a `chain.blocks` page is in flight
    #[serde(skip)]
    pub(crate) pulling: bool,
    /// the newest height `chain.heads` announced; the status moves to it
    /// when the page that reaches it lands (`pull`), not before
    #[serde(skip)]
    pub(crate) head: u64,
    /// the status read again while the one on screen stays
    #[serde(skip)]
    pub(crate) rereading_status: Option<Task<()>>,
    /// the pane's width, read from the window each render
    #[serde(skip)]
    pub(crate) width: f32,
    /// the device's pointer is a finger, read from the window each render:
    /// the phone's pages stack under Back, the desktop's do not
    #[serde(skip)]
    pub(crate) touch: bool,
    /// the list the arrows are in, and the row they are on
    #[serde(skip)]
    pub(crate) cursor: Option<(&'static str, usize)>,
    /// the scroll of the list drawn as a window (`ui::Rows::shown`): one
    /// list at a time, as `cursor`; a page opens at its top (`go`)
    #[serde(skip)]
    pub(crate) scroll: UniformListScrollHandle,
    /// the page each tab showed last, which an arrow onto the tab opens
    /// again (→ then ← comes back to the block it left); `None`, the tab's
    /// own list. Kept with `route`, so a restored view comes back the same.
    pub(crate) left: [Option<Route>; 5],
    /// who reads and what they change, read again on every open
    #[serde(skip)]
    pub(crate) acting: Acting,
    /// the phone's root title row is the search field (the Expo header's
    /// search), not the title
    #[serde(skip)]
    pub(crate) searching: bool,
    /// the phone's sheet over the page, if one is open
    #[serde(skip)]
    pub(crate) sheet: Option<Sheet>,
}

/// A sheet the phone opens over the Programs page (Expo `programs.tsx`);
/// a form is [`Acting::form`], as on the desktop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Sheet {
    /// Add: add a program, propose a grant
    Add,
    /// one program's or view-only entry's sheet: its page, its
    /// transactions, the reader's presses on it
    Program(String),
}

/// What the code behind a program page declares, as far as it has been
/// read (`blob.manifest`).
pub(crate) enum Declared<'a> {
    Reading,
    /// the node refused the read
    Refused(&'a Error),
    /// no capability to list, and why
    Nothing(Unlisted),
    /// what its view asks of the host
    Asks(&'a Manifest),
}

/// Why a program page lists no capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unlisted {
    /// the node does not hold the code
    NoCode,
    /// a program whose code carries no view
    NoView,
    /// a view-only entry whose blob carries no view
    NoListedView,
    /// a view whose manifest this app cannot read
    Unreadable,
    /// a view that asks for nothing
    AsksNothing,
}

impl Unlisted {
    pub(crate) fn sentence(self) -> &'static str {
        match self {
            Unlisted::NoCode => "The node does not hold this code yet.",
            Unlisted::NoView => {
                "This program draws nothing: no view, so nothing is asked of the host."
            }
            Unlisted::NoListedView => "This app cannot read this view's manifest.",
            Unlisted::Unreadable => {
                "Its view carries a manifest this app cannot read: built for another wire, or asking a capability this app does not have."
            }
            Unlisted::AsksNothing => "Its view asks nothing of the host.",
        }
    }
}

impl Explorer {
    /// What the code of the program page on screen declares; `listed`, a
    /// view-only entry's.
    pub(crate) fn declared(&self, listed: bool) -> Declared<'_> {
        let blob = match &self.manifest {
            Loadable::Idle | Loadable::Loading(_) => return Declared::Reading,
            Loadable::Failed(refusal) => return Declared::Refused(refusal),
            Loadable::Ready(blob) | Loadable::Reloading(blob, _) => blob,
        };
        let manifest = match blob.as_ref().map(|blob| &blob.view) {
            None => return Declared::Nothing(Unlisted::NoCode),
            Some(None) if listed => return Declared::Nothing(Unlisted::NoListedView),
            Some(None) => return Declared::Nothing(Unlisted::NoView),
            Some(Some(CarriedView { manifest: None, .. })) => {
                return Declared::Nothing(Unlisted::Unreadable);
            }
            Some(Some(CarriedView {
                manifest: Some(manifest),
                ..
            })) => manifest,
        };
        if manifest.capabilities.is_empty() {
            Declared::Nothing(Unlisted::AsksNothing)
        } else {
            Declared::Asks(manifest)
        }
    }

    /// Every account identity listed, or none yet.
    pub(crate) fn account_list(&self) -> &[identity::Account] {
        self.accounts
            .ready()
            .map_or(&[][..], |accounts| accounts.list.as_slice())
    }

    pub(crate) fn account(&self, number: u64) -> Option<&identity::Account> {
        self.account_list()
            .iter()
            .find(|account| account.number == number)
    }

    /// An account's name, or `account 7` where identity lists no such
    /// account.
    pub(crate) fn name_of(&self, number: u64) -> String {
        self.account(number).map_or_else(
            || format!("account {number}"),
            |account| account.card.name.clone(),
        )
    }

    /// Whether account `number` is an agent: a program account its manager
    /// declared one.
    pub(crate) fn is_agent(&self, number: u64) -> bool {
        self.account(number).is_some_and(|account| {
            matches!(
                account.kind(),
                identity::Kind::Program {
                    category: identity::Category::Agent,
                    ..
                }
            )
        })
    }

    /// The network's label: the session's chain id before its salt
    /// (`<label>#<salt>`); none until a session names one.
    pub(crate) fn network_label(&self) -> Option<&str> {
        let chain = self.session_chain.as_str();
        let label = chain.split_once('#').map_or(chain, |(label, _)| label);
        (!label.is_empty()).then_some(label)
    }

    /// Blocks left in the epoch, while the node counts epochs.
    pub(crate) fn epoch_left(&self) -> Option<u64> {
        let status = self
            .status
            .ready()
            .filter(|status| status.epoch_length > 0)?;
        let closes = (status.epoch + 1) * status.epoch_length - 1;
        Some(closes.saturating_sub(status.height))
    }

    /// What `account` is, its manager named from the list: "User", "Agent ·
    /// managed by Dev · suspended", "Program · forge" (identity's own wording,
    /// [`identity::view::kind`], over `Kind::badge` and `Kind::note`).
    pub(crate) fn kind_of(&self, account: &identity::Account) -> String {
        identity::view::kind(&account.kind(), |manager| {
            self.account(manager)
                .map(|manager| manager.card.name.clone())
        })
    }

    /// The account holding `key`, and which of its keys it is.
    pub(crate) fn holder(&self, key: &[u8]) -> Option<(&identity::Account, usize)> {
        self.account_list().iter().find_map(|account| {
            let at = account.keys().iter().position(|held| held.key == key)?;
            Some((account, at))
        })
    }

    pub(crate) fn used_by(&self, number: u64) -> Option<Vec<&activity::Used>> {
        let used = self.used.ready()?;
        Some(
            used.list
                .iter()
                .filter(|used| used.account == number)
                .collect(),
        )
    }

    /// The transactions activity recorded for account `number`, once read.
    pub(crate) fn sent_by(&self, number: u64) -> Option<u64> {
        let used = self.used_by(number)?;
        Some(used.iter().map(|used| used.frames).sum())
    }

    /// The programs account `number` used and how often, most first, then
    /// by name, once read.
    pub(crate) fn programs_used(&self, number: u64) -> Option<Vec<(&str, u64)>> {
        let mut used: Vec<(&str, u64)> = self
            .used_by(number)?
            .into_iter()
            .map(|used| (used.program.as_str(), used.frames))
            .collect();
        used.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        Some(used)
    }

    /// How many of the window's transactions went to `name`.
    pub(crate) fn sent_to(&self, name: &str) -> u64 {
        self.chain.txs.iter().filter(|tx| tx.target == name).count() as u64
    }

    /// What account `number` did, as far as it is read; `None` while no
    /// page of it is asked.
    pub(crate) fn acted(&self, number: u64) -> Option<&Listing> {
        let (account, acted) = self.acted.as_ref()?;
        (*account == number).then_some(acted)
    }

    /// When account `number` last signed with `key`, among the frames of it
    /// read: its newest page, or as far as its page was read on. A key
    /// last used further back reads as not used lately.
    pub(crate) fn last_signed(&self, number: u64, key: &[u8]) -> Option<u64> {
        self.acted(number)?.rows.read(|acted| {
            let frame = acted.rows().iter().find(|frame| frame.signer == key);
            frame.map(|frame| frame.time)
        })
    }

    /// Whether the programs role runs `program`.
    pub(crate) fn runs(&self, program: &str) -> Option<&programs::Entry> {
        self.network
            .ready()?
            .programs
            .iter()
            .find(|entry| entry.program == program)
    }

    /// Whether the programs role lists `name` as a view with no program.
    pub(crate) fn lists(&self, name: &str) -> Option<&programs::View> {
        self.network
            .ready()?
            .views
            .iter()
            .find(|view| view.name == name)
    }

    /// The code behind `name`: the program's, or the view-only entry's.
    pub(crate) fn code_of(&self, name: &str) -> Option<abi::BlobId> {
        self.runs(name)
            .map(|entry| entry.code)
            .or_else(|| self.lists(name).map(|view| view.view))
    }
}
